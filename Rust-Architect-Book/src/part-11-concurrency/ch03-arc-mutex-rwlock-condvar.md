# Chapter 11.3 — Arc, Mutex, RwLock, and Condvar

> **Where this sits:** Part XI · Concurrency · chapter 3 of 7
> **Prerequisites:** Chapters 11.1–11.2, Chapter 3.5 (RAII, temporaries), Chapter 8.3 (panics, poisoning first seen),
> Chapter 1.3 (the TOCTOU and deadlock demos).
> **After this chapter you can:** use `Arc<Mutex<T>>` and `RwLock` as run-time versions of `&mut` and `&`; read
> `Mutex::lock` down to its `lock cmpxchg` and futex call; choose a poisoning policy per lock and defend it; build a
> blocking bounded queue with `Condvar`; and prevent the four classic lock bugs: wrong order, reentrancy, a guard that
> lives too long, and slow work under a lock.

---

## Pass 1 · User level — *Shared mutable state, with the lock around the data*

### 1. Problem

Sometimes threads really do need the same mutable data: a session table, a connection pool's free list, a queue between
pipeline stages. The borrow checker can't prove exclusive access across threads at compile time, because which thread
runs when is a run-time fact. So Rust moves the check to run time, in a way that keeps the compile-time shape:

- `Arc<T>` gives many threads shared **ownership**, with the data freed when the last owner drops.
- `Mutex<T>` gives one thread at a time **exclusive access**, a run-time `&mut T`.
- `RwLock<T>` gives **many readers or one writer**, a run-time version of aliasing XOR mutation.
- `Condvar` lets a thread **wait until a condition on the locked data** becomes true.

### 2. Mental model

The compile-time rules from Chapter 3.3, now enforced while the program runs:

```text
 compile time (one thread)                    run time (many threads)
 ───────────────────────────                  ──────────────────────────────────────────────────
 let r = &mut data;   exclusive loan          let g = mutex.lock().unwrap();   MutexGuard<T>: DerefMut
   ...use r...                                  ...use g (*g is the data)...
 } loan ends at last use                      } guard dropped → UNLOCK (RAII, Chapter 3.5)

 many &data, no &mut                          rwlock.read()  → RwLockReadGuard  (any number at once)
 one &mut data, no &                          rwlock.write() → RwLockWriteGuard (alone)

 checked by: the borrow checker               checked by: the lock (waiting instead of an error)
```

Two properties set Rust's locks apart from most languages':

1. **The lock owns the data.** `Mutex<T>` *contains* the `T`. The only path to it is `lock()`, so you can't touch
   the data without the lock, or lock the wrong mutex for it. In Java, `synchronized (lockA)` guarding `mapB` is a
   convention.
2. **Unlocking is the guard's `Drop`.** You can't forget to unlock, and an early return or a panic unlocks too. What
   you *can* do is keep a guard alive longer than you meant to (§5 and §10).

**Poisoning.** If a thread panics while holding a guard, the mutex is marked **poisoned** [LIB]: every later `lock()`
returns `Err(PoisonError)`, which carries the guard, so the caller can still get at the data. Poisoning doesn't
protect memory safety (the data is still valid memory). It reports a **possibly broken invariant**: the panicking
thread may have been halfway through an update.

### 3. Rust code

**The basic shape** (listing `ch03-01-mutex-owns-data.rs`):

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

fn main() {
    let hits: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));

    let workers: Vec<_> = (0..4)
        .map(|w| {
            let hits = Arc::clone(&hits);
            thread::spawn(move || {
                for i in 0..1_000 {
                    let route = if i % 4 == w { "/checkout" } else { "/health" };
                    // Keep the critical section tiny: build the key before locking.
                    let key = route.to_string();
                    let mut map = hits.lock().unwrap(); // MutexGuard<HashMap<..>>: DerefMut to the map
                    *map.entry(key).or_insert(0) += 1;
                } // guard dropped at the end of each iteration: unlocked
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }

    // Sole owner again: no locking needed to get the data out.
    let map = Arc::try_unwrap(hits).expect("all workers joined").into_inner().unwrap();
    let mut rows: Vec<_> = map.into_iter().collect();
    rows.sort();
    println!("{rows:?}");
}
```

```text
[("/checkout", 1000), ("/health", 3000)]
```

Note the last two lines. Once every other owner is gone, `Arc::try_unwrap` gives the `Mutex` back by value, and
`into_inner` takes the data out with **no locking at all**. Ownership proves that nobody else can be holding the lock.
The same reasoning gives you `Mutex::get_mut(&mut self)`: exclusive access to the mutex means exclusive access to the
data, so no lock is needed.

**Poisoning, and the three policies** (listing `ch03-02-poisoning.rs`, the core; the full program is in the listing). A
thread panics halfway through an update whose invariant is "entries sum to `total`":

```rust,ignore
let _ = thread::spawn(move || {
    let mut g = l.lock().unwrap();
    g.entries.push(-30); // first half of the update...
    panic!("fee service timed out"); // ...and the second half (total -= 30) never happens
})
.join();

// Policy 1: propagate. `lock().unwrap()` turns the poison into a panic in THIS thread too.
let r = panic::catch_unwind(|| ledger.lock().unwrap().total);

// Policy 2: ignore. Take the guard anyway, and trust that no invariant can be broken.
let g = ledger.lock().unwrap_or_else(PoisonError::into_inner);

// Policy 3: repair. Restore the invariant, then clear the poison flag (stable since 1.77).
match ledger.lock() {
    Ok(_) => unreachable!("still poisoned"),
    Err(poisoned) => {
        let mut g = poisoned.into_inner();
        g.total = g.entries.iter().sum(); // or roll back: g.entries.pop()
    }
}
ledger.clear_poison();
```

```text
  [panic] fee service timed out
is_poisoned = true
  [panic] called `Result::unwrap()` on an `Err` value: PoisonError { .. }
policy 1 (unwrap):  this thread panicked as well
policy 2 (ignore):  Ledger { entries: [100, -30], total: 100 }, consistent = false
policy 3 (repair):  Ledger { entries: [100, -30], total: 70 }, consistent = true
after clear_poison: is_poisoned = false, lock() is Ok = true
```

Policy 2's output is the reason poisoning exists: `consistent = false`. Ignoring the poison handed out a ledger whose
entries don't match its total. §7 turns these into a per-lock decision.

**Waiting for a condition: `Condvar`.** A bounded blocking queue is the canonical example. It's the monitor pattern,
and the heart of Project L3's worker pool (listing `ch03-05-condvar-queue.rs`, excerpt):

```rust,ignore
/// Blocks while the queue is full.
pub fn push(&self, item: T) -> Result<(), PushError<T>> {
    let mut s = self.state.lock().unwrap();
    if s.items.len() == self.capacity && !s.closed {
        self.producer_waits.fetch_add(1, Ordering::Relaxed);
    }
    // wait_while re-checks the condition after every wakeup, spurious ones included.
    s = self.not_full.wait_while(s, |s| s.items.len() == self.capacity && !s.closed).unwrap();
    if s.closed {
        return Err(PushError::Closed(item));
    }
    s.items.push_back(item);
    drop(s); // unlock before notifying: the woken consumer can take the lock immediately
    self.not_empty.notify_one();
    Ok(())
}

/// Blocks while empty; None once the queue is closed AND drained.
pub fn pop(&self) -> Option<T> {
    let mut s = self.state.lock().unwrap();
    s = self.not_empty.wait_while(s, |s| s.items.is_empty() && !s.closed).unwrap();
    let item = s.items.pop_front();
    drop(s);
    if item.is_some() {
        self.not_full.notify_one();
    }
    item
}
```

```text
consumed 20000 items, sum ok = true
a producer ever found the queue full (backpressure): true
try_push into empty: Ok(())
try_push into full:  Err(Full("b"))
push after close:    Err(Closed("c"))
pop after close:     Some("a"), then None
```

Look at the signature of `wait_while`: it **takes the guard by value and returns it**. Waiting releases the lock and
reacquiring gives it back, and the type system spells that out. You can't wait on a condition variable without holding
the lock, which is the rule Java's `Object.wait` enforces at run time with `IllegalMonitorStateException`.

---

## Pass 2 · Systems level — *Futexes, a poison flag, and one cmpxchg*

### 4. Under the hood

**std's `Mutex` on Linux is a futex** [LIB] [VERSION]. Since Rust 1.62, `Mutex`, `RwLock`, and `Condvar` on Linux are
implemented directly on `futex(2)` rather than wrapping `pthread_mutex_t`. The lock word is an `AtomicU32` with three
states: 0 = unlocked, 1 = locked, 2 = locked *with waiters*. Here is the release assembly of lock, increment, unlock
(listing `ch03-07-mutex-asm.rs`: `*m.lock().unwrap() += 1` on a `&Mutex<u64>`; trimmed, comments added):

```text
playground::bump:
	mov	ecx, 1
	xor	eax, eax
	lock cmpxchg	dword ptr [rdi], ecx          ; state: 0 -> 1 ? (acquire the lock in one atomic op)
	jne	.LBB1_1                                ; somebody holds it -> lock_contended (spin, then futex wait)
.LBB1_2:
	mov	rbx, qword ptr [rip + ...GLOBAL_PANIC_COUNT@GOTPCREL]
	mov	rax, qword ptr [rbx]                   ; is this thread panicking right now? (for poisoning)
	...
	movzx	ecx, byte ptr [rdi + 4]              ; the POISON flag, a bool right after the lock word
	test	cl, cl
	jne	.LBB1_6                                ; poisoned -> the unwrap() fails
.LBB1_11:
	inc	qword ptr [rdi + 8]                    ; *guard += 1   (the u64 lives at offset 8)
	...                                            ; if a panic began while we held it: set poison
.LBB1_15:
	xor	eax, eax
	xchg	dword ptr [rdi], eax                 ; UNLOCK: state -> 0, fetching the old state
	cmp	eax, 2                                 ; were there waiters?
	je	.LBB1_17                               ; yes -> jmp futex::Mutex::wake (a FUTEX_WAKE syscall)
	ret
.LBB1_1:
	call	qword ptr [rip + <std::sys::sync::mutex::futex::Mutex>::lock_contended@GOTPCREL]
```

The layout falls out of the offsets: the futex word at 0, the poison `bool` at 4, and the data at 8. That's why
`Mutex<u64>` is 16 bytes (Chapter 11.4's table). In the **uncontended** case, a lock/unlock pair is **one
`lock cmpxchg` and one `xchg`**, both atomic read-modify-writes on a line this core probably already owns, with no
system call. Only contention takes the slow path: `lock_contended` spins briefly, then sets the state to 2 and calls
`futex(FUTEX_WAIT)` to sleep in the kernel. Unlock sees the 2 and calls `futex(FUTEX_WAKE)` for one waiter [OS]. That's
the "fast userspace mutex" idea: the kernel is involved only when threads actually have to wait.

**Where the poison bit is set** is visible too. On unlock, the guard checks whether the thread started panicking
*while it held the lock* (the `GLOBAL_PANIC_COUNT` test at the top records the state at lock time) and, if so, stores
1 into `[rdi + 4]` before releasing. The cost of poisoning in the non-panicking case is two loads and two
well-predicted branches.

**`Condvar`** is a futex on a sequence counter [LIB]. `wait` reads the counter, unlocks the mutex, sleeps in
`FUTEX_WAIT` until the counter changes, then relocks. `notify_one` increments the counter and wakes one sleeper.
**Spurious wakeups are allowed** by the API [LIB]: a waiter can return without a matching notify. That's why the
listing uses `wait_while` (which loops on the predicate) and never a bare `wait` in an `if`.

**`RwLock`** is a futex-based state word holding a reader count plus "write-locked", "readers waiting", and "writers
waiting" bits [LIB]. std's documentation deliberately leaves the **priority policy** unspecified ("dependent on the
underlying operating system"). On Linux, the futex implementation **prefers writers**: once a writer is waiting, new
readers block, which prevents writer starvation. Verified: a reader holds the lock, a writer queues behind it, and then
another thread, and even the *same* reader thread, try to read-lock (listing `ch03-04-rwlock-writer-waiting.rs`):

```text
new reader while writer waits (std):   try_read ok = false
reader 1 re-reads while writer waits: try_read ok = false
writer got the lock
final routes: ["/a", "/b"]
new reader while writer waits (parking_lot): try_read ok = false
same thread re-reads (parking_lot):          try_read ok = false
same thread re-reads, read_recursive:         ok = true
parking_lot value after writer: 1
```

The second line is the one that matters for production. With `read()` instead of `try_read()`, reader 1 would now wait
for the writer, which is waiting for reader 1: **a deadlock caused by a recursive read lock**. §10 is that incident.
`parking_lot`'s `RwLock` behaves the same (it's task-fair), and offers `read_recursive()` for code that knowingly
re-enters.

### 5. Memory

`Arc<Mutex<T>>` is **one heap allocation**: the `ArcInner` holds the two atomic counts, then the `Mutex`, which holds
the lock word, the poison flag, and `T`, inline. Nothing extra is allocated per lock or unlock. A `MutexGuard` is a
reference to the mutex plus the "was panicking" bit, and lives on the stack.

**Guard lifetime is temporary lifetime.** A guard produced inside an expression is a temporary, and Chapter 3.5's
temporary rules decide when it's dropped. That decides whether the next `lock()` succeeds or deadlocks. The listing
uses `try_lock` as a stand-in for `lock` so it can report instead of hang, and runs under two editions (listing
`ch03-03-guard-lifetime.rs`):

```rust
use std::collections::HashMap;
use std::sync::{Mutex, TryLockError};

fn second_lock(cache: &Mutex<HashMap<u32, &'static str>>) -> &'static str {
    match cache.try_lock() {
        Ok(_) => "acquired",
        Err(TryLockError::WouldBlock) => "WouldBlock (lock() would deadlock here)",
        Err(TryLockError::Poisoned(_)) => "poisoned",
    }
}

fn main() {
    let cache = Mutex::new(HashMap::from([(1, "cached")]));

    // `if let ... else`: in edition 2024 the scrutinee's temporaries (the guard) are dropped before `else`.
    if let Some(v) = cache.lock().unwrap().get(&2) {
        println!("hit {v}");
    } else {
        println!("if-let else branch:  second lock {}", second_lock(&cache));
    }

    // `match`: the scrutinee's temporaries live until the end of the whole match, in every edition.
    match cache.lock().unwrap().get(&1) {
        Some(_) => println!("match arm:           second lock {}", second_lock(&cache)),
        None => {}
    }

    // Bind the value you need, and let the guard die at the end of the `let` statement.
    let v = cache.lock().unwrap().get(&1).copied();
    println!("after let-binding:   second lock {} (value {v:?})", second_lock(&cache));
}
```

```text
edition 2024:                                         edition 2021:
if-let else branch:  second lock acquired             if-let else branch:  second lock WouldBlock (lock() would deadlock here)
match arm:           second lock WouldBlock (...)     match arm:           second lock WouldBlock (lock() would deadlock here)
after let-binding:   second lock acquired (...)       after let-binding:   second lock acquired (value Some("cached"))
```

The classic "look up in the cache, insert on a miss" code (`if let Some(v) = map.lock()...get(k) { .. } else {
map.lock()...insert(..) }`) **deadlocked in edition 2021 and works in 2024** [VERSION]. A `match` on a locked lookup
still holds the lock through every arm in all editions. The robust habit doesn't depend on edition rules: **bind what
you need with `let`, and lock again only after that statement**.

### 6. CPU / OS

Uncontended, a lock/unlock pair is two atomic RMWs, roughly tens of cycles on a line already in this core's cache
(order of magnitude) [CPU]. Contended, everything changes:

- **The lock word's cache line moves between cores** on every acquire, the same bouncing as Chapter 11.2's `Arc`
  counter, now with two RMWs per critical section.
- **Waiters sleep in the kernel.** A futex wait plus a wake costs system calls and context switches, microseconds
  rather than nanoseconds [OS].
- **Convoys form.** When the lock holder is descheduled, or does anything slow, every waiter queues behind it, and the
  critical section's duration becomes everyone's latency.

Chapter 11.7 measures a hot shared counter behind a `Mutex`: 38–181 ns per increment with 4 threads contending
(five runs), against 9–19 ns for one shared atomic and under 1 ns with no sharing. It also measures what happens when I/O runs inside a
critical section (throughput drops about 4×). The mechanism is simple: **a lock turns parallel work into serial work for
exactly as long as it's held.** Everything in §7 follows from that sentence.

---

## Pass 3 · Architect level — *Which lock, which policy, which order*

### 7. Trade-offs

**Mutex vs RwLock** (the SPEC's twelve criteria):

| Criterion | `Mutex<T>` | `RwLock<T>` |
|---|---|---|
| **Memory** | lock word + poison + T (`Mutex<u64>` = 16 B) | two words + poison + T (`RwLock<u64>` = 24 B) |
| **CPU** | 2 atomic RMWs per critical section | readers: 2 RMWs **on the shared state word**; writers: the same as Mutex |
| **Latency** | predictable for short sections | readers can run in parallel; writers wait for all readers |
| **Throughput** | serializes everything | scales reads only if critical sections are long enough to amortize the state-word traffic |
| **Contention** | one line bounces on every access | the state word still bounces for **readers** (each read lock writes the count) |
| **Cache behavior** | data + lock on one line: good for tiny T | same, plus reader-count traffic |
| **Allocation** | none | none |
| **Complexity** | simplest | writer preference, recursive-read deadlocks, upgrade patterns |
| **Safety** | `T: Send` suffices for `Sync` | needs `T: Send + Sync` (readers share `&T`) |
| **Maintainability** | clear | readers that call other readers hide deadlocks (§10) |
| **Failure modes** | convoys, deadlock by lock order | the same, plus writer starvation (on some platforms) or reader blocking behind writers (Linux std) |
| **Operational implications** | contention visible as lock-wait time | read-heavy wins are workload-dependent: measure |

The surprise is the "Contention" row. A read lock is not a read: it's a write to the lock's reader count. With many
cores taking short read locks, `RwLock` can be **slower** than `Mutex`. Chapter 11.4 measured exactly that for a
read-mostly table (259 ns vs 219 ns per read, each including an `Arc` clone), and Chapter 11.7 shows `RwLock` winning
over a single `Mutex` on a 95%
read map (92–154 vs 160–272 ns per operation across five runs), with both beaten by **sharding** (74–81 ns). Decide with a measurement. Structurally, the
better answers for read-mostly data are usually `ArcSwap` (11.4) or sharding (11.7).

**Poisoning policy, one lock at a time** (the Part VIII promise). The question is always *can a panic leave this lock's
data breaking an invariant?*

| Lock guards... | Policy | Why |
|---|---|---|
| Data with a cross-field invariant (ledger entries vs total, two linked maps) | **Propagate** (`lock().unwrap()`), or repair then `clear_poison` | Serving inconsistent data is worse than failing |
| Independent entries updated by single std calls (a cache map, a counter set) | **Recover** (`unwrap_or_else(PoisonError::into_inner)`), count it, alert | A map seen after a panic is still a valid map; availability matters more |
| A resource pool's free list | Recover, and **never `unwrap` a lock inside `Drop`** | Chapter 8.3's double-panic abort came from exactly that |
| Anything, in a `panic = "abort"` binary | N/A | No unwinding means no poisoning |

Ferrite v1 (Project L4) takes the second row for its shard maps and writes the reasoning down next to the code, which
is the real deliverable: a poisoning policy is a statement about invariants, and it belongs next to the lock.

**The four lock bugs, and their structural fixes:**

1. **Lock-order deadlock** (Chapter 1.3's demo). Fix: a global order, for example by account id. Listing
   `ch03-06-lock-ordering.rs` runs 8 threads transferring in opposite directions and finishes: `finished without
   deadlock; transfers succeeded: true; balances 1994 + 6 = 2000` (the split varies from run to run; the sum is always
   2000). It also refuses a self-transfer, which would lock the same mutex twice. Checking and acting under the same two
   guards is also the TOCTOU fix from Chapter 1.3, extended to two resources.
2. **Reentrancy** (§8). std's `Mutex` is not reentrant.
3. **A guard that lives too long** (§5, and §10's recursive read).
4. **Slow work under the lock**: I/O, logging to a slow sink, calling callbacks or code you don't own. Chapter 11.7
   measures it.

### 8. Java comparison

| Java | Rust | The difference that matters |
|---|---|---|
| `synchronized (obj) { ... }` | `let g = m.lock().unwrap(); ...` | Rust's lock *contains* the data; Java's guards whatever the programmer remembers |
| `lock.lock(); try { ... } finally { lock.unlock(); }` | the guard's `Drop` | You can't forget the `finally` |
| Monitors are **reentrant** | `Mutex` is **not** reentrant | Porting nested `synchronized` methods deadlocks (below) |
| `obj.wait()` / `notifyAll()`; `Condition.await()` | `Condvar::wait_while(guard, pred)` | Waiting consumes and returns the guard: holding the lock is enforced by types |
| `ReentrantLock(true)` (fair), `tryLock(timeout)` | std: no fairness option, `try_lock` only; `parking_lot`: `try_lock_for`, eventual fairness | Timeouts on locks are a smell either way |
| `ReentrantReadWriteLock` | `RwLock` | Java's allows reentrant reads while a writer waits; Rust's std (Linux) blocks them (§4, §10) |
| `StampedLock` optimistic reads | seqlocks (Part XIV), `ArcSwap` (11.4) | |
| An exception inside `synchronized` releases the monitor **silently** | A panic poisons the mutex | Java has no poisoning: the next thread sees half-updated state without warning |
| `ConcurrentHashMap` | sharded `Mutex`/`RwLock` maps, or crates (`dashmap`, not on the Playground) | 11.7 measures the options |

The reentrancy row catches every Java port once. A "synchronized method that calls another synchronized method" waits
for itself in Rust (listing `ch03-09-not-reentrant.rs`, where `try_lock` again stands in for `lock`):

```text
flush: WouldBlock: this thread already holds the lock; lock() would never return
flushed [1, 2, 3] (outside the lock)
```

The fix in the listing is the general one: **decide under the lock, act after releasing it.** Take what you need out
with `mem::take`, drop the guard, then call the method that locks again (or that does I/O).

The shared-map row from Chapter 1.2 closes the loop. Java 7's `HashMap` could turn a concurrent resize into an
infinite loop. Go's runtime detects concurrent map writes and kills the process with `fatal error: concurrent map
writes`. Rust rejects the program (listing `ch03-08-shared-map-rejected.rs`):

```rust,compile_fail
use std::collections::HashMap;
use std::thread;

fn main() {
    let mut routes: HashMap<String, u32> = HashMap::new();
    thread::scope(|s| {
        s.spawn(|| routes.insert("/checkout".to_string(), 1));
        s.spawn(|| routes.insert("/refunds".to_string(), 2));
    });
    println!("{}", routes.len());
}
```

```text
error[E0499]: cannot borrow `routes` as mutable more than once at a time
10 |         s.spawn(|| routes.insert("/checkout".to_string(), 1));
   |                 -- ------ first borrow occurs due to use of `routes` in closure
   |                 first mutable borrow occurs here
11 |         s.spawn(|| routes.insert("/refunds".to_string(), 2));
   |                 ^^ ------ second borrow occurs due to use of `routes` in closure
   |                 second mutable borrow occurs here
```

It's the same E0499 as two `&mut` in one function (Chapter 3.3). The borrow checker doesn't know about threads at all.
It knows that two closures each hold `&mut routes` for the same region.

> **Analogy limit.** `Mutex<T>` *looks* like "a `synchronized` block with the lock attached to the data". The
> attachment is the whole point: in Java you can read `balances` outside `synchronized` and nothing complains, which is
> how most Java data races happen. And the guard isn't a scope. It's a value, which can be returned, stored in a struct,
> or kept alive by a temporary rule (§5). Java's `synchronized` block ends at its closing brace. A Rust guard ends when
> the value is dropped.

### 9. Production scenario

**Meridian's ingestion pipeline gets backpressure.** The pipeline from Chapter 3.2 (decode → validate → enrich →
publish) originally connected its stages with growable `Vec` batches. Chapter 9.1's incident followed: a replayed
backlog produced one 2.1-million-event batch and OOM kills. The redesign put a **bounded blocking queue** between each
pair of stages, built exactly like listing `ch03-05-condvar-queue.rs`:

- **Capacity in events**, sized from each stage's throughput and a latency budget (Little's law: 50,000 events/s × 0.2 s
  of buffering is 10,000 slots).
- **`push` blocks when full.** When `publish` slows down (a broker hiccup), `enrich` blocks, then `validate`, then
  `decode`. The Kafka consumer stops polling, and the broker holds the backlog, which is where it belongs, instead of
  in the process heap.
- **`close()` drives shutdown.** On deploy, `decode` stops, closes its output queue, and each downstream stage drains
  and closes its own. `pop` returning `None` is the "no more work" signal, so nothing needs a poison pill or an
  interrupt.
- **The metrics come from the queue**: `producer_waits` (how often a stage hit backpressure) and queue depth per stage.
  The slowest stage is simply the one whose *input* queue is full.

Every lock here is held for a `VecDeque` push or pop and nothing else. The only waiting happens in `wait_while`, with
the lock released.

### 10. Failure scenario

**The balance cache that deadlocked every few hours.** Meridian's account service keeps a read-mostly balance cache
refreshed by a background writer. The Java version used `ReentrantReadWriteLock`, and one read path called a helper
that also took the read lock (a "read-locked method calling a read-locked method"). Java's lock let the nested read
through, because the thread already held a read lock.

The Rust port kept the shape with `std::sync::RwLock`:

```rust,ignore
fn available(&self, acct: AccountId) -> Money {
    let cache = self.balances.read().unwrap();      // read lock #1
    let held = self.pending_holds(acct);            // calls self.balances.read() again: read lock #2
    cache[&acct].balance - held
}
```

It passed every test and ran fine for hours. Then, under refresh load, a thread would take read lock #1, the refresher
would call `write()` and start waiting, and read lock #2 would **block behind the waiting writer**, which was waiting for
read lock #1. That's exactly the verified `reader 1 re-reads while writer waits: try_read ok = false`. All request
threads piled up behind it within seconds.

Diagnosis took a stack dump: every thread was parked in `futex_wait`, one inside `write`, the rest inside `read`, and
one thread had two frames of `available` → `pending_holds`, both inside `read`. The fixes, in order of preference:

1. **Don't re-lock.** Pass the guard's data down: `fn pending_holds_in(cache: &Balances, acct)`. Functions that need
   locked data take a reference to the data, not the lock.
2. **Restructure the data**: publish immutable snapshots with `ArcSwap` (Chapter 11.4). Readers take no lock, so
   there's nothing to nest.
3. If a nested read truly can't be avoided: `parking_lot::RwLock::read_recursive`, documented as an exception, with the
   writer-starvation trade-off accepted explicitly.

The review rule that came out of it: **a lock guard is never acquired in a function that receives `&self` of a type
that is already locked by its caller.** The practical form is that lock acquisition lives at API boundaries, and
internal helpers take `&T` or `&mut T`.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XI).*

1. What does it mean that Rust's `Mutex` "owns" its data? Which class of bugs does that remove?
2. Walk through the uncontended `lock` and `unlock` in the `bump` assembly. Which instruction acquires, which releases,
   and when does a system call happen?
3. What is a futex, and why is it "fast"?
4. What does poisoning protect against, and what doesn't it protect against? When is recovering from a poisoned lock
   correct?
5. Why must a `Condvar` wait be in a loop (or use `wait_while`)? What is a spurious wakeup?
6. Why does `RwLock<T>` need `T: Sync` while `Mutex<T>` doesn't?
7. Why can `RwLock` be slower than `Mutex` for short read-mostly critical sections?
8. Explain the recursive-read deadlock with a waiting writer. Why doesn't Java's `ReentrantReadWriteLock` have it?
9. How did edition 2024 change the "lock, look up, insert on miss" pattern? Which construct still holds the guard?
10. Name four ways to deadlock or stall with locks, and the structural fix for each.

### 12. Exercises

- **Beginner.** Wrap a `Vec<String>` in `Arc<Mutex<_>>`, and let four threads each push ten lines. Print the result
  sorted. Then remove the `Arc` by using `thread::scope`. What changed, and why is it allowed?
- **Intermediate.** Extend the bounded queue (`ch03-05`) with `pop_timeout(Duration)` using
  `Condvar::wait_timeout_while`. What should it return when the timeout fires, and how does the caller tell it apart
  from "closed"?
- **Advanced.** Implement `transfer_all(accounts: &[Account], moves: &[(usize, usize, i64)])` that performs many
  transfers concurrently without deadlocking, using the id-ordering rule. Then add a version that locks at most two
  accounts at a time and prove it can't deadlock.
- **Systems.** Locally, run a contended `Mutex` benchmark under `strace -f -c -e futex` and `perf stat -e
  context-switches`. How many futex calls per contended acquisition? Compare with `parking_lot::Mutex`.
- **Architecture.** Your service has three locks: `sessions`, `rate_limits`, and `audit_log`. Some paths need two of
  them. Write the lock-ordering document, the poisoning policy for each, and the one rule you'd enforce in code review
  to keep I/O out of critical sections.

### 13. Debugging exercise

```rust,ignore
fn get_or_load(cache: &Mutex<HashMap<u64, Profile>>, id: u64) -> Profile {
    match cache.lock().unwrap().get(&id) {
        Some(p) => p.clone(),
        None => {
            let p = load_profile_from_db(id); // ~5 ms
            cache.lock().unwrap().insert(id, p.clone());
            p
        }
    }
}
```

1. What happens on the first cache miss, and why? (Which temporary is alive in the `None` arm?)
2. Would rewriting it as `if let ... else` fix it? On which edition?
3. Even once it no longer deadlocks, there's a second problem: two threads missing on the same `id` both load it. Is
   that a bug or a trade-off? Sketch a version that loads each id once without holding the map lock during the 5 ms load
   (hint: store a per-key `Arc<OnceLock<Profile>>`, Chapter 11.4).

### 14. Design exercise

**A connection pool for Meridian's ledger client.** 64 connections to the database, 400 worker threads borrowing them.
Design the pool:

- the data structure behind the lock (free list? all connections plus a state per slot?);
- `get()` blocking with a timeout (`Condvar`), and what the caller gets on timeout;
- the guard type that returns a connection on `Drop`, including when the connection is broken or the borrower panicked
  (and how you avoid Chapter 8.3's double panic);
- the poisoning policy, with one sentence about the invariant that justifies it;
- which metrics you expose (wait time, in-use count, timeouts).

Fill in the twelve criteria for "one `Mutex` around the pool" vs "a `Mutex` per slot + an atomic free count".
