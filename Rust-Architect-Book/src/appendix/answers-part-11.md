# Appendix A — Answer Key: Part XI

> Model answers. Write yours first. Where several answers are defensible, the key says so. Measured numbers quoted here
> are the chapters' one-run Playground figures (or ranges over several runs), and carry the same "noisy" label.

---

## Chapter 11.1 — Threads and the OS

### Interview & architecture questions

**1. What `spawn` creates.** On Linux, a new **kernel task** in the same thread group. std calls `pthread_create`, and
glibc `mmap`s a stack and then calls `clone`/`clone3` with `CLONE_VM | CLONE_THREAD | ...`, so the new task shares the
address space but has its own TID (visible in `/proc/self/task`) [OS]. The 2 MiB is **Rust's** default stack size,
passed explicitly as the pthread attribute [LIB]. It isn't glibc's default, which follows `ulimit -s` (typically 8 MiB).
`Builder::stack_size` or `RUST_MIN_STACK` changes it. The 4 KiB `---p` mapping directly below each stack is the **guard
page**: no permissions, so a stack overflow faults on it instead of silently overwriting the next mapping, and std turns
that fault into `thread '...' has overflowed its stack` (the Part IX interlude).

**2. `F: 'static`.** The spawned thread can outlive the function that spawned it: dropping the `JoinHandle` detaches it.
If the closure could borrow the spawner's locals, the thread could read them after that stack frame was gone, which is a
use-after-free. `'static` says the closure *owns* everything it uses (or borrows only statics). It's an ownership rule,
not a timing rule. `thread::scope` relaxes it to `'scope` because the scope provably joins first (Chapter 11.6).

**3. `join`.** `Result<T, Box<dyn Any + Send + 'static>>`: `Ok(T)` if the closure returned, and `Err(payload)` if it
panicked. The panic unwound only that thread, and its payload is handed to whoever joins. It's a `Result` because a
panicking thread is a normal possibility the joiner has to decide about (log it, restart the worker, propagate). The
payload is whatever was passed to `panic!`: a `&'static str` for a literal message, a `String` for a formatted one, or any
`Any + Send` type via `std::panic::panic_any`. So logging code should try both string types (listing `ch01-02`).

**4. When `main` returns.** The process exits, and every other thread stops wherever it is: no unwinding, no destructors,
no flushing of their buffers (listing `ch01-05`: the uploader never printed). The JVM does the opposite: it waits for all
non-daemon threads. The design rule is to **join what you spawn** (keep every `JoinHandle`, or use `thread::scope`), and
to make shutdown an explicit code path, such as a pool whose `Drop` closes its queue and joins its workers.

**5. 2 MiB isn't 2 MiB.** The stack is a *virtual* reservation. Physical pages are committed only when touched, so a
typical thread's resident stack is a few to tens of KiB [OS]. What actually limits threads on Linux: resident memory for
touched pages; the kernel's per-thread cost (task struct and kernel stack, order of 10–20 KiB); `ulimit -u`;
`/proc/sys/kernel/threads-max`; the cgroup's `pids.max` in containers; and `vm.max_map_count`, since each thread adds a
stack mapping and a guard mapping. Virtual address space is plentiful on 64-bit.

**6. What the ~30 µs and ~5 µs consist of.** `spawn + join` (33.8 µs; 27.9 µs with `scope`, one run): allocating the
result packet and boxing the closure, `mmap` for the stack plus the guard page, `clone`, the scheduler placing and
starting the new task, TLS setup, the thread's exit, `munmap`, and the join handshake (a futex wait/wake). The hand-off
(5.7 µs per round trip) is a channel send that wakes a parked worker (a futex wake system call), a context switch onto
it, the reply, and a second wake plus context switch back. No memory mapping and no task creation, which is why it's
about 6× cheaper.

**7. 64 threads vs a pool of 4.** Total CPU work was fixed, so total time was similar (115 vs 108 ms). But the scheduler
time-slices all 64 runnable threads fairly, so every job progresses a little at a time and most finish near the end:
mean completion 73.1 ms. The pool runs 4 jobs to completion at a time, so early jobs finish early: mean 57.1 ms. It's
Little's law read backwards: for the same throughput, more work in progress means longer time in the system per job.

**8. Pool sizing.** CPU-bound: about the number of cores (logical CPUs), because more threads only add switching
(Chapter 11.6's rayon uses exactly that). Blocking I/O: **Little's law**, `L = λ·W`: threads ≈ arrival rate × the time
each task holds a thread (blocked plus CPU time), sized on a high percentile rather than the mean, plus headroom, and
with a bounded queue in front so overload is refused rather than absorbed.

**9. No `interrupt()`.** Stopping a thread at an arbitrary instruction can't be done safely: it might hold a lock, be
halfway through updating an invariant, or own resources whose destructors must run. Java deprecated `Thread.stop` for the
same reason, and `interrupt()` is itself cooperative (a flag that blocking methods notice). Rust makes cooperation
explicit: an `AtomicBool` the worker checks, closing a queue or dropping the last `Sender` (Chapter 11.5), `select!` on a
shutdown channel, and timeouts on every blocking call so the thread gets a chance to look.

**10. Thread-per-core.** When the workload partitions naturally (by key, connection, or shard), latency matters more than
utilization, and cross-core traffic (locks, shared cache lines, migrations) is the dominant cost. Each pinned core owns its
partition, shares nothing, and talks to other cores by messages. Seastar-based systems such as ScyllaDB are the
well-known example of the model. The price is load imbalance (a hot partition can't borrow an idle core), messaging for
cross-partition operations, and more complex code. A shared pool is the better default when work is irregular or doesn't
partition.

### Debugging exercise (the file counter)

1. `main` spawns one thread per path, **drops every `JoinHandle`** (each thread is detached), and returns as soon as the
   loop ends. When `main` returns, the process exits and kills whatever threads haven't printed yet. How many lines
   appear depends on how far each thread got before the main thread's exit, so the output is nondeterministic. The main
   thread's exit decides.
2. With handles: collect `thread::spawn(..)` results into a `Vec<JoinHandle<()>>` and `join()` each one before returning.
   A worker's panic then surfaces as `Err`. With a scope: `thread::scope(|s| for path in paths { s.spawn(move || ...); })`,
   and the scope joins all of them. **Only the scope** lets the threads borrow a shared `&Config`, because its threads
   provably end before the scope returns. With `thread::spawn`, you'd need `Arc<Config>` or a clone per thread.
3. 50,000 threads: `spawn` starts failing (thread limits, cgroup `pids.max`; `thread::spawn` panics with "failed to spawn
   thread"), memory for touched stacks and kernel state balloons, the process may run out of file descriptors because
   every thread opens its file at once (`EMFILE`), and massive oversubscription makes everything slower. Restructure it as
   a bounded amount of parallelism over a work list: `paths.par_iter().map(count_lines).collect()` with rayon (results in
   input order, printed by the main thread), or a fixed pool pulling paths from a queue. Either way, the number of threads
   and open files is bounded by design.

### Selected exercises

- **Advanced (`spawn_with_retry`).** Retry only on errors that can be transient (`io::ErrorKind::WouldBlock`, the
  `EAGAIN` that `clone` returns at a thread limit), with capped exponential backoff and a final error carrying the last
  cause. Don't use it in a request path: if the process can't create threads, it's overloaded, and retrying adds latency
  and pile-up where shedding the request would help. Don't use it at startup either, where a spawn failure is a
  configuration problem that should fail fast.
- **Architecture (dispatcher tiers).** Little's law with the p99 as holding time: gold 200 × 0.3 s = 60 threads, silver
  50 × 1 s = 50, bronze 5 × 10 s = 50. Add headroom (say 1.5×: 90 / 75 / 75), and make sure timeouts cap the holding time
  (a 30 s timeout on bronze means a stalled partner could hold 150 threads' worth of demand, so the queue bound, not the
  pool, must absorb it). When a tier saturates: its bounded queue fills, new events go to the durable retry table (§9's
  design), no tier borrows another's threads, a per-partner circuit breaker stops calling a failing endpoint, and the
  alert is on queue depth and retry backlog per tier.

---

## Chapter 11.2 — Send and Sync

### Interview & architecture questions

**1. Definitions.** `T: Send`: a value of type `T` may be **moved** to another thread. `T: Sync`: a `&T` may be
**shared** with other threads. Sharing `&T` with another thread means sending a copy of the reference there, so by
definition `T: Sync` ⇔ `&T: Send`.

**2. Why these bounds.** Sending `&mut T` hands the other thread *exclusive* access for the loan's duration, while the
lending thread can't touch `T` at all. That's equivalent to moving `T` there and back, so it needs `T: Send`. Sharing
`&T` lets both threads access `T` at the same time. That's safe only if concurrent shared access can't race, which means
`T` has no unsynchronized interior mutability: `T: Sync`.

**3. `Cell<u64>`.** Moving a `Cell` is fine, because the old thread keeps nothing: `Send`. Sharing it isn't, because
two threads could `set` through `&Cell` with no synchronization: `!Sync`. `&mut Cell<u64>` is `Send` (exclusive access
is as good as ownership), and `&Cell<u64>` is not (`&T: Send` needs `T: Sync`).

**4. `Arc<T>`.** Every clone of an `Arc` gives its thread a `&T`, so concurrent sharing needs `T: Sync`. The last clone
dropped, on *any* thread, drops `T` there, so `T` must also be `Send`. Hence `Arc<T>: Send + Sync` iff
`T: Send + Sync`. `Arc<RefCell<T>>` is therefore neither: `Arc` makes *ownership* shareable, not the *contents*
thread-safe. The compiler's note suggests `RwLock` (or `Mutex`) instead of `RefCell`.

**5. `Mutex<T>` vs `RwLock<T>`.** A `Mutex` gives out exclusive access (`&mut T`) to one thread at a time. That's
moving access between threads, so `T: Send` is enough for `Mutex<T>: Sync`. An `RwLock` lets several readers hold `&T`
*simultaneously*, which is sharing, so it needs `T: Sync` too. Hence the verified rows `Mutex<Cell<u64>>`: `Sync` and
`RwLock<Cell<u64>>`: not `Sync`.

**6. `MutexGuard` isn't `Send`.** The guard's `Drop` unlocks, and some platforms require a mutex to be unlocked by the
thread that locked it (for pthread mutexes, unlocking from another thread is undefined behavior) [OS]. So the guard can't
move. It is `Sync` when `T: Sync`, because a shared `&MutexGuard<T>` only gives out `&T`.

**7. Auto traits.** Traits the compiler implements automatically and structurally: a type is `Send`/`Sync` if all its
fields are, unless std opted it out with a negative impl (`Rc`, raw pointers, `Cell`'s `Sync`, `MutexGuard`'s `Send`) or
someone opted in with `unsafe impl`. A closure is an anonymous struct whose fields are its captures. So proving
`F: Send` for `spawn` means checking each captured place's type: owned values (for `move` closures) must be `Send`, and
captured references need their referents to be `Sync`. The E0277 notes ("required because it's used within this
closure", "appears within the type `Job`") are the steps of that proof.

**8. Disjoint capture (edition 2021).** Closures capture the *places* they use, not whole variables. More permissive:
`move || job.id * 2` captures only the `u64` field, so it's `Send` even though `job.trace` is an `Rc` (2018: E0277).
The other direction: in listing `ch02-04`, a non-`move` closure using `stats.hits` captures `&stats.hits`, a
`&Cell<u64>`, so the solver checks `Cell<u64>: Sync` and never consults the (wrong) `unsafe impl Sync for Stats`. The
program is rejected where the author expected it to compile. Capture granularity changes *which types* get checked.

**9. `unsafe impl Sync`.** It promises that every operation reachable through `&T` is safe when performed concurrently
from several threads: all mutation through shared references is synchronized (atomics, locks), and any contained data is
itself safe to share. When the promise is false, Miri reports it as undefined behavior:
`Data race detected between (1) non-atomic write on thread ... and (2) non-atomic read on thread ...`, pointing at the
unsynchronized access (listing `ch02-04`).

**10. Run-time cost.** None: they're marker traits with no representation, checked and then erased like lifetimes.
What they buy is permission to use cheaper types safely in single-threaded code. The release assembly shows it: an `Rc`
clone is `inc qword ptr [rax]`, and an `Arc` clone is `lock inc qword ptr [rax]`. Because `Rc` can't escape its thread,
it never needs the `lock` prefix (measured 1.1 vs 3.5 ns uncontended, and 57 ns for one `Arc` shared by 4 threads).

**11. A private field as a breaking change.** Auto traits are computed from *all* fields, private ones included. Adding
a private `Rc`, `Cell`, `RefCell`, or raw pointer silently removes `Send` or `Sync` from a public type, and every
downstream `spawn`, `tokio::spawn`, or `Arc<T>` shared across threads stops compiling. No signature changed, but it's
semver-major. `cargo-semver-checks` detects lost auto traits. Guard public types with compile-time assertions.

### Debugging exercise (`audit_later`)

1. The 2018 error is E0277, "`Rc<str>` cannot be sent between threads safely", with the note "required because it
   appears within the type `Request`". The field is `user: Rc<str>`: in 2018 a `move` closure captures all of `req`.
2. In 2021 the closure uses only `req.id`, so it captures just that place, a `u64`. Only the `u64` crosses the thread
   boundary, and `req.user` stays on the original thread for the next `println!`. (In 2018, the whole `req` would have
   moved into the closure, so the later `req.user` would also be a use of a moved value.)
3. `move || log(&req)` uses `req` as a whole, so the closure captures all of it, `Rc` included: E0277 on edition 2021
   as well. And because `req` moved into the closure, the later `println!("handled {}", req.user)` becomes a use after
   move.
4. Make the thread boundary an explicit type. Build an owned, `Send` record on the request thread
   (`AuditRecord { id: u64, user: String }`, or `Arc<str>` for the user if it's shared deliberately) and send it over a
   channel to one long-lived audit thread, rather than spawning a thread per request. Then the question "what crosses?" is
   answered by a type, not by capture rules.

### Selected exercises

- **Beginner (predictions).** `(String, Rc<u8>)`: neither. `Vec<Arc<Mutex<u64>>>`: `Send + Sync`.
  `Option<&RefCell<u8>>`: neither (`&RefCell` needs `RefCell: Sync` for both). `Box<dyn Error + Send>`: `Send`, not
  `Sync` (a trait object has only the auto traits it names). `fn(u64) -> u64`: `Send + Sync`.
  `mpsc::Sender<Rc<u8>>`: neither (`Sender<T>` is `Send` and `Sync` only when `T: Send`).
- **Intermediate (`ThreadAffine<T>`).** `struct ThreadAffine<T> { value: T, _not_send: PhantomData<*const ()> }`: the
  raw-pointer marker is `!Send + !Sync`, so the wrapper is too, whatever `T` is. `PhantomData<T>` would copy `T`'s auto
  traits, making `ThreadAffine<u8>` `Send`, which is the opposite of the goal. Prove it with a listing that expects
  `error:E0277` from `fn assert_send<T: Send>() {}` applied to `ThreadAffine<u8>`.
- **Advanced (`Buf`).** (a) Read-only after construction: `unsafe impl Send` if the C allocator allows freeing from any
  thread (SAFETY: the buffer is uniquely owned, and freeing it is thread-agnostic per the library docs), and
  `unsafe impl Sync` (SAFETY: no `&self` method writes, and concurrent reads of immutable bytes can't race). (b) With a
  `&self` method that writes: not `Sync`, unless every write goes through synchronization (a lock or atomics). `Send` can
  remain, with the same allocator argument.
- **Architecture (review comment).** "A `RefCell` in `Inner` makes `Inner: !Sync`, so `Arc<Inner>` becomes
  `!Send + !Sync`, and so does `Client`. Every user who moves a `Client` to another thread or shares it stops compiling.
  That's a semver-major change hidden in a private field. Alternatives: allocate the scratch buffer per call (or reuse
  one passed in as `&mut Vec<u8>`), use a `thread_local!` buffer, or, if it really must be shared, a `Mutex<Vec<u8>>`,
  after measuring the contention."

---

## Chapter 11.3 — Arc, Mutex, RwLock, and Condvar

### Interview & architecture questions

**1. The lock owns the data.** `Mutex<T>` *contains* the `T`, and the only path to it is `lock()`, which returns a guard
that unlocks on `Drop`. That removes whole classes of bugs: touching the data without the lock (most Java data races),
locking the wrong lock for the data, and forgetting to unlock (the missing `finally`). What remains is guard *lifetime*,
lock *order*, and *what you do* while holding it.

**2. The `bump` assembly.** `lock cmpxchg dword ptr [rdi], ecx` with `eax = 0` and `ecx = 1` acquires: an atomic
compare-and-swap of the state word from 0 (unlocked) to 1 (locked). `jne` goes to `lock_contended` if the word wasn't 0.
Then the poison check (the `GLOBAL_PANIC_COUNT` load, and the poison byte at `[rdi + 4]`), the increment at `[rdi + 8]`,
and the release: `xchg dword ptr [rdi], eax` with `eax = 0` stores 0 and returns the old state, and `cmp eax, 2; je`
calls `futex::Mutex::wake` if waiters were recorded. System calls happen only under contention: `FUTEX_WAIT` in
`lock_contended` after a short spin, and `FUTEX_WAKE` on unlock when the state was 2.

**3. Futex.** A "fast userspace mutex": a 32-bit word in ordinary memory plus two system calls. `FUTEX_WAIT(addr,
expected)` sleeps only if `*addr` still equals `expected`, and `FUTEX_WAKE(addr, n)` wakes up to `n` sleepers on that
address [OS]. It's fast because the uncontended path never enters the kernel. Acquire and release are user-space atomics,
and the kernel is involved only when a thread actually has to sleep or be woken.

**4. Poisoning.** If a thread panics while holding a guard, the lock is marked poisoned, and later `lock()` calls return
`Err(PoisonError)` carrying the guard. It protects against **silently observing a half-done update**: a broken
invariant. It doesn't protect memory safety (the data is valid memory either way). It doesn't detect logic errors that
don't panic, invariants that span two locks, or anything in a `panic = "abort"` binary. And `into_inner` lets any caller
ignore it. Recovering is correct when a panic can't leave the data inconsistent: independent entries each updated by one
std call (a cache map, a set of counters, Ferrite's shards). Then recover, count, alert, and `clear_poison`. Otherwise,
propagate, or repair the invariant first and then clear.

**5. Waiting in a loop.** When `wait` returns, the condition may not hold. **Spurious wakeups** are allowed by the API: a
waiter can return without a matching notify [LIB]. And a real wakeup can be "stolen": another thread may have consumed
the item between the notify and this thread reacquiring the lock. So the predicate must be rechecked after every wakeup,
which is what `wait_while` does.

**6. `RwLock<T>` needs `T: Sync`.** Several readers hold `&T` at the same time, which is sharing. A `Mutex` only ever
gives one thread access at a time (see 11.2 Q5).

**7. `RwLock` slower than `Mutex`.** Taking a read lock *writes* the lock's shared state (the reader count), and
releasing it writes again: two atomic read-modify-writes on a cache line that every reader's core fights over, the same
traffic as a mutex. Parallel readers only pay off when critical sections are long enough to amortize that, and the
reader/writer bookkeeping costs a bit more than a mutex's. Measured: 259 vs 219 ns per read (each including an `Arc`
clone) in Chapter 11.4. On Chapter 11.7's map workload `RwLock` beat a single `Mutex`, but both lost to sharding.

**8. The recursive-read deadlock.** Thread A holds a read lock. Writer W calls `write()` and waits. Linux std's `RwLock`
prefers writers, so once a writer waits, new readers block. A then calls `read()` again (a nested helper) and blocks
behind W, while W waits for A's first read lock to be released: a cycle. Java's `ReentrantReadWriteLock` tracks read
holds per thread and lets a thread that already holds a read lock reacquire it even while a writer waits, so the nested
read goes through. (Java's lock has its own trap: upgrading from read to write deadlocks.)

**9. Edition 2024 and lock-lookup-insert.** In 2024, the temporaries of an `if let` scrutinee (the guard from
`cache.lock().unwrap()`) are dropped **before the `else` block** runs, so `else { cache.lock()...insert(..) }` works. In
2021 they lived through the `else`, and it deadlocked. A `match` still keeps the scrutinee's temporaries alive through
every arm, in every edition. The edition-proof habit: bind what you need with `let`, and lock again only in a later
statement.

**10. Four ways to stall, four structural fixes.** (1) **Lock-order deadlock**: impose a global order (by account id)
and always acquire in it. (2) **Reentrancy**: a method holding the lock calls another that locks. Lock only at API
boundaries, and let internal helpers take `&T`/`&mut T`. (3) **A guard that lives too long** (a `match` scrutinee, a 2021
`if let`, a guard stored in a struct, a nested read). Bind values with `let`, use explicit scopes or `drop`, and pass
data down, not locks. (4) **Slow work under the lock** (I/O, logging, callbacks, blocking sends). Compute under the lock,
act after it, and measure hold time.

### Debugging exercise (`get_or_load`)

1. On a miss, the `match` scrutinee `cache.lock().unwrap().get(&id)` has created a `MutexGuard` temporary, which lives
   until the end of the whole `match` in every edition. In the `None` arm, `cache.lock()` runs again on the same thread.
   std's `Mutex` isn't reentrant, so the thread waits for itself forever. (std documents relocking as "might panic or
   deadlock"; on Linux it deadlocks.) Even without the second lock, the 5 ms database load would run while holding the
   map lock.
2. As `if let Some(p) = cache.lock().unwrap().get(&id) { p.clone() } else { .. }`: fixed on **edition 2024** (the guard
   is dropped before `else`), still a deadlock on 2021. Better on every edition:
   `let hit = cache.lock().unwrap().get(&id).cloned(); if let Some(p) = hit { return p; }`, then load outside the lock.
3. It's a trade-off that becomes a bug at scale. Duplicate loads waste database work, and a hot key that expires
   triggers a stampede. Load each key once without holding the map lock during the load:

   ```rust,ignore
   // Unverified sketch.
   let cell: Arc<OnceLock<Profile>> = Arc::clone(cache.lock().unwrap().entry(id).or_default()); // map lock: ns
   cell.get_or_init(|| load_profile_from_db(id)).clone() // one loader per id; others wait on THIS key only
   ```

   Loads of different ids run in parallel. Concurrent requests for one id wait for its single load. If the load can fail,
   store a `Result` (or accept that a panicking initializer leaves the cell empty, so the next caller retries).

### Selected exercises

- **Intermediate (`pop_timeout`).** Return a three-way answer, not an `Option`: `enum Pop<T> { Item(T), TimedOut,
  Closed }` (or `Result<T, RecvTimeoutError>`, like `mpsc`). `wait_timeout_while` returns the guard plus a
  `WaitTimeoutResult`. If an item is present, return it. If the queue is closed and drained, `Closed`. If the timeout
  elapsed and it's still empty, `TimedOut`. Folding "timed out" and "closed" into `None` makes callers either spin
  forever or quit early (Chapter 11.5's `Empty` / `Timeout` / `Disconnected` lesson).
- **Advanced (`transfer_all`).** For each move, lock `min(from, to)` then `max(from, to)`, and refuse `from == to` (it
  would lock one mutex twice). No deadlock: a wait-for cycle would require some thread to hold a higher-id lock while
  waiting for a lower-id one, and the ordering rule forbids that. With at most two locks held per thread and a total
  order on ids, no cycle can form.
- **Architecture (three locks).** Order: `sessions` before `rate_limits`, and `audit_log` never acquired while holding
  another lock. Audit records are produced under the other locks and written after releasing them (or sent to an audit
  writer thread). Poisoning: `sessions` and `rate_limits` hold independent entries (recover, count, clear). `audit_log`,
  if it's an in-memory buffer whose records could be half-written, propagates or repairs. Review rule: **no I/O, logging
  to a slow sink, callbacks, or blocking channel sends while a guard is alive**, backed by a lock-hold-time metric in the
  wrapper that returns guards.

---

## Chapter 11.4 — Interior Mutability: Cell, RefCell, OnceCell, UnsafeCell

### Interview & architecture questions

**1. Why `UnsafeCell`.** Writing to memory that is only reachable through a `&T`, outside an `UnsafeCell`, is undefined
behavior [LANG]. rustc tells LLVM that `&T` points to `noalias` and `readonly` memory, and the optimizer relies on it
(it keeps values in registers instead of reloading them). `UnsafeCell<T>` changes three things: (1) writes through the
raw pointer from `.get()` behind a shared reference become legal; (2) the `noalias`/`readonly` promises are dropped for
those bytes, so the optimizer reloads them; (3) it's `!Sync`, so every type containing it is `!Sync` until an
`unsafe impl` proves synchronization. It synchronizes nothing itself.

**2. `Cell` vs `RefCell`.** `Cell` never hands out a reference to its interior. `get` copies the value out, and
`set`/`replace`/`take` swap whole values, so no reference can ever observe a write. That's why `get` needs `T: Copy`:
without a reference, the only way to give you the value is to copy it (for other types, use `replace`, `take`, or
`into_inner`). `RefCell` does hand out references, and it counts them at run time: many `borrow()`s or one
`borrow_mut()`, and a conflict panics with "already borrowed".

**3. `OnceLock::get_or_init` under a race.** A `OnceLock` is a `Once` state word (incomplete, running, complete,
poisoned) next to an `UnsafeCell<MaybeUninit<T>>`. The first caller moves the state to *running* with a compare-and-swap
and runs the closure. The others see *running* and sleep on the futex. The winner writes the value, publishes
*complete* with Release ordering, and wakes everyone. Afterwards, a read is one Acquire load (a plain `mov` on x86-64)
and a branch. The listing showed eight racing threads and one initializer run.

**4. Panicking initializers.** `OnceLock`: the state goes back to incomplete and the cell stays empty, so a later
caller may try again (listing: "OnceLock retry: 7"). `LazyLock`: its initializer is a single `FnOnce` it owns, consumed
by the first attempt. After a panic there's nothing left to retry with, so it's **poisoned for good**, and every later
access panics with "LazyLock instance has previously been poisoned". The difference follows from who owns the
initializer: `OnceLock` receives one per call, while `LazyLock` has exactly one.

**5. `OnceCell<String>` = 24 bytes, `OnceCell<u64>` = 16.** A `OnceCell<T>` is essentially `Option<T>` in a cell, so
"uninitialized" needs a representation. `String` has a niche (bit patterns that can never be a valid `String`, such as a
null data pointer or a capacity above `isize::MAX`), so `None` hides in it for free (Chapter 5.2). Every bit pattern of a
`u64` is valid, so a separate tag is needed, and alignment rounds that up to 16 bytes.

**6. `parking_lot::Mutex<()>` = 1 byte.** Its lock is one `AtomicU8` holding a "locked" bit and a "parked" bit. Waiting
threads aren't stored in the lock: they park in a global hash table keyed by the lock's address (`parking_lot_core`)
[LIB]. std's `Mutex<()>` is 8 bytes: a `u32` futex word, a poison `bool`, and padding.

**7. The read-path measurement.** Both lock-based versions **write two shared cache lines on every read**: the lock
word (or reader count), and the `Arc` refcount when cloning. With four cores doing that, both lines bounce
continuously. `RwLock` was slower (259 vs 219 ns) because its read lock's reader-count updates are the same kind of
traffic, and the critical section is far too short for parallel readers to help. `ArcSwap::load` writes only to a
thread-local debt slot, so no shared line is written: 7.8 ns, about 28× faster than the `Mutex` version and within 4× of
an unsynchronized read.

**8. Debts.** A naive lock-free load reads the pointer and then increments the refcount. In between, a writer could swap
the pointer out and drop the last reference, and the reader would increment a freed count: a use-after-free. With debts,
a reader records "I'm using pointer P" in a per-thread slot, and a writer that swaps P out first pays those debts by
incrementing P's refcount on the readers' behalf, then releases its own reference. The common-case load touches only its
own slot.

**9. Who frees the old value.** Whoever drops the **last** `Arc` to it. That's the writer if no reader holds it, and
otherwise the last reader to finish, which may be a request thread in the middle of serving a request. To control it,
the writer sends the old `Arc` to a dropper thread that waits until it's the sole owner (`Arc::try_unwrap` in a loop)
and frees it there. The listing freed all 50 old tables on the dropper and none elsewhere. Also keep reader guards short,
so old versions don't pile up.

**10. `LazyLock` for configuration.** Only for pure, infallible values that don't depend on the environment: a compiled
regex, a lookup table. Not for configuration parsed from environment variables or files. A failure poisons it for the
life of the process (§10's 100%-error outage), health checks don't touch it, tests share it, and initialization order is
implicit. Load configuration eagerly in `main`, fail fast with context, and pass `Arc<Config>` down.

### Debugging exercise (`settings_for`)

1. `let map = s.borrow();` creates a `Ref` guard that lives until the end of the closure. On a miss, the code falls
   through to `s.borrow_mut()` while `map` is still alive, and `RefCell` panics with `BorrowMutError` ("already
   borrowed"). Non-lexical lifetimes don't help: they shorten *compile-time* borrows, but a `Ref` is a value with a
   `Drop`, released at the end of its scope. So every miss panics, not only child tenants. Child tenants were simply the
   first misses production saw.
2. Reentrancy: `load_settings` calls `settings_for(parent)` while the outer call still holds its borrow (and, once
   problem 1 is fixed naively by holding `borrow_mut` across the load, the nested `borrow()` panics instead). A cyclic
   parent relation (A's parent is B, and B's parent is A) would also recurse until the stack overflows.
3. Hold no borrow while loading:

   ```rust,ignore
   // Unverified sketch.
   if let Some(v) = SETTINGS.with(|s| s.borrow().get(&t).cloned()) {
       return v;
   }
   let loaded = load_settings(t); // may recurse freely: no borrow is held
   SETTINGS.with(|s| s.borrow_mut().insert(t, loaded.clone()));
   loaded
   ```

   Add a depth limit or a visited set for parent chains. A per-tenant `OnceCell` expresses "load once" better, but a
   cycle would then be reentrant initialization, which std's `OnceCell` turns into a panic. The design question behind
   it: a `thread_local!` cache duplicates every tenant per worker thread and is never invalidated. A process-wide cache
   (sharded map or `ArcSwap` snapshot) with explicit refresh is usually what was intended.

### Selected exercises

- **Intermediate (`MyOnceCell`).** `get_or_init(&self, f)` checks for `None`, runs `f`, stores the value, and returns
  `&T`. The SAFETY argument: the cell is `!Sync` (single thread), and once `Some`, the value is never written again, so
  any `&T` handed out stays valid. The hole: if `f` calls `get_or_init` on the same cell, the inner call initializes it
  and returns a `&T`, and then the outer call overwrites the value while that `&T` may still be alive. That's an aliasing
  violation, so it's undefined behavior. std's `OnceCell` detects reentrant initialization and panics, because it must
  preserve "a value, once handed out by reference, never changes".
- **Advanced (per-worker `thread_local!` counters).** A reporter can't read another thread's `Cell`: `thread_local!`
  values are reachable only from their own thread, and `Cell` is `!Sync`. The reporter needs something shareable: each
  worker registers an `Arc<AtomicU64>` (one writer per counter, `Relaxed`) in a global registry the reporter iterates.
  Or workers periodically flush their `Cell`s into their own atomics, or send snapshots over a channel.
- **Architecture (feature flags to 400 workers).** One refresher thread builds a new flag table and publishes it with
  `ArcSwap`. Workers `load()` once per request (a consistent view for the whole request). The table is small, so where
  it's freed matters less than for Chapter 3.1's route table, but a dropper thread costs nothing. With `ArcSwap` there's
  no per-worker delivery to fail: every reader sees the latest pointer. So "a worker stopped receiving updates" becomes
  "the refresher stopped": export the refresher's last-success time and the flag version, and have workers tag logs with
  the version they served. With per-worker copies over channels, you'd need a per-worker "applied version" gauge
  instead.

---

## Chapter 11.5 — Channels and Message Passing

### Interview & architecture questions

**1. Ownership through `send`.** `send` takes the value by move. Afterwards the producer can't touch it (E0382 on any
use), and the consumer is its only owner, so there's no shared mutable object to race on. A Java `BlockingQueue` passes
references: the producer can keep mutating an object after `put`, while the consumer reads it, and the compiler has no
idea.

**2. Ending `for msg in rx`.** The loop calls `recv()` until it returns `Err(RecvError)`, which happens when **every
`Sender` has been dropped and the queue is empty**. Dropping the last `Sender` replaces the poison pill. It's more robust
because it can't be forgotten on an early return or a panic path (drops run anyway), can't be consumed by the wrong
consumer (with several consumers, one pill stops only one of them), and needs no sentinel value in the message type.

**3. `Receiver`: `Send`, not `Sync`.** std's channel is single-consumer, and the implementation assumes one consumer at
a time. The receiver can move to one thread but not be used from several. For several workers on one queue: crossbeam
`bounded(n)` (its `Receiver` is `Clone` and `Sync`), Chapter 11.3's `Condvar` queue, or `Arc<Mutex<Receiver>>` (it works,
but every worker then serializes on the mutex just to wait). std's `mpmc` is unstable on 1.98.

**4. Unbounded, bounded, rendezvous.** Unbounded: the producer never waits, and the backlog grows in the heap until
something breaks (an OOM kill). Bounded(n): the producer blocks once n messages are queued (backpressure), or `try_send`
refuses. Rendezvous (0): every `send` waits for a matching `recv`. Default to **bounded** in a service. Overload then
shows up where it happens (a blocked or refused producer, a queue-depth metric flat at its limit), and memory has a
known ceiling.

**5. 7 µs vs 19 ns.** With `sync_channel(1)`, the buffer is almost always full or empty, so nearly every operation parks
one side and wakes the other: futex system calls and two context switches per item, the same hand-off cost as Chapter
11.1. With 1,024 slots, neither side waits in steady state. A send is a compare-and-swap, a write, and a Release store,
and the remaining cost is the message's cache line moving between cores.

**6. The unbounded backlog.** In the heap, in the list flavor's blocks (31 slots each, allocated as the list grows)
[LIB]. Each message costs `size_of::<T>()` plus an 8-byte state word: 72 bytes for a 64-byte record, 7,058 KiB for
100,000 of them (measured, matching the arithmetic to the KiB). Heap data the message owns (a `String`'s buffer) comes
on top. Blocks are freed as the receiver consumes past them, or when the channel is dropped. Even then, freed memory
isn't necessarily returned to the OS (Chapter 3.1).

**7. The owner thread.** One thread owns the state and mutates it with plain `&mut`. Other threads send commands, and
requests that need an answer carry a one-shot reply channel. Prefer it to `Mutex<HashMap>` when operations must be
serialized or ordered, when invariants span the whole structure (an expiry sweep that must be atomic with respect to
every touch), when the resource is thread-affine, or when most commands are fire-and-forget. A read costs a round trip:
two channel operations and at least one wakeup, microseconds (Chapter 11.7 measured 6.5–19.4 µs against ~75 ns for a
sharded map).

**8. Values handed back.** `try_send` returns `Err(Full(v))` and `send` to a dropped receiver returns
`Err(SendError(v))`, so the caller still owns the value and decides what refusal means: retry, spill to disk, fail the
request with a `503` built from it, or count and drop. Nothing is lost silently. For load shedding it's what lets you
answer the client, like L3's pool returning the connection so the accept loop can write `503` on it.

**9. Stopping a worker blocked in `recv`.** Close its channel (drop every `Sender`), and `recv` returns `Err`. Or have
the worker `select!` over its job channel and a shutdown channel, and drop the shutdown channel's `Sender` to wake every
worker at once. Or loop on `recv_timeout` and check a flag between waits.

**10. Go, Java, Rust.** *Ownership*: Rust moves values, and the sender loses access. Java passes references to shared
objects. Go copies values, but slices, maps, and pointers inside them still share memory, and only the run-time race
detector catches misuse. *Closing*: Rust closes by dropping handles (counted automatically), and a send to a dropped
receiver returns the value in an `Err`. Go closes explicitly, and a send on a closed channel panics. Java queues don't
close at all: poison pills or interrupts. *Select*: built into Go; crossbeam's `select!` (or `tokio::select!`) in Rust,
since std has none; nothing equivalent over Java `BlockingQueue`s.

### Debugging exercise (the `Dispatcher` that never stops)

1. A worker's `for job in rx` ends only when **every** `Sender` of that channel has been dropped. `stop` drops
   `d.jobs`, but `d.retry` is a second `Sender` to the same channel, and it's still alive in `d` until `stop` returns,
   which is *after* the join loop. The channel never disconnects, so the workers wait in `recv` forever, and `join` waits
   for them.
2. Every worker is parked inside crossbeam's receive path (the `Receiver::recv` behind the `for` loop, ending in a futex
   wait), and the main thread is parked in `JoinHandle::join`. Without reading the code: a stack dump of every thread
   (`gdb -p <pid> -batch -ex "thread apply all bt"`, or `eu-stack`) shows it directly, and `/proc/<pid>/task/*/wchan`
   shows everyone sleeping in the futex code. A metric for the number of live senders (or "workers idle while shutdown
   is in progress") would show it too.
3. Keeping the retry path: destructure and drop both, `let Dispatcher { jobs, retry } = d; drop(jobs); drop(retry);`,
   then join. Redesigning so it can't recur: stop relying on counting senders held by several parties. Make `stop` signal
   shutdown explicitly (a shutdown channel that every worker `select!`s on), with workers draining what's queued and then
   exiting. Or give the dispatcher exactly one `Sender` that `stop(self)` consumes, and route retries through the
   workers' own processing loop rather than a second long-lived handle.

### Selected exercises

- **Intermediate (`Shutdown { reply }` vs dropping every handle).** The explicit command can be forgotten (an early
  return, a panic between creating and shutting down a client), and then the owner thread runs forever. Dropping the
  handles happens automatically on every path. What the command adds is an acknowledgment ("the owner has flushed").
  The drop-based design gets the same thing by joining the owner thread.
- **Architecture (fan-out to three consumers).** Ledger mirror (must not lose): a bounded channel with **blocking**
  send, so a slow mirror backpressures the source. If its thread dies, `send` returns `SendError`: stop consuming the
  source, alert, restart, and replay from the source's durable offset. Fraud-feature updater (stale after 2 s): a small
  bounded channel with `try_send` drop-and-count, plus a timestamp in each event so the consumer discards anything older
  than 2 s. Analytics exporter (best effort, bursty): a larger bounded buffer with drop-and-count, or spill to local disk.
  For the last two, a dead consumer is counted and restarted by a supervisor, without stopping the other consumers.

---

## Chapter 11.6 — Scoped Threads and Data Parallelism with Rayon

### Interview & architecture questions

**1. Why scoped threads can borrow.** `thread::scope` joins every thread spawned inside it **before it returns**, as part
of its own control flow, even if the closure panics. So spawned closures only need `'scope`: anything that outlives the
scope is valid for the threads' entire lives, and ordinary `&`/`&mut` rules apply. A `thread::spawn` thread may outlive
its caller, so it needs `'static`.

**2. `thread::scoped` vs `thread::scope`.** The pre-1.0 API returned a `JoinGuard` whose `Drop` joined the thread, so
soundness depended on a destructor running. But `mem::forget` is safe (and so are `Rc` cycles), so the guard could be
leaked, and the thread would keep reading a stack frame that no longer existed (Miri: "dangling reference
(use-after-free)" on the spawned thread, listing `ch06-02`). `thread::scope` joins in its control flow, which safe code
can't skip. The rule that came out of it: **destructors may be used for cleanup, never for soundness** (RFC 1066 made
`mem::forget` safe).

**3. Panics in a scope.** If a spawned thread panics and nobody joined its handle explicitly, the scope still waits for
all threads, and then panics itself (listing `ch01-02`: "scope panicked"). If you join a `ScopedJoinHandle` yourself,
you receive the `Err(payload)`, and that thread's panic doesn't propagate from the scope. If the scope's closure `f`
panics, the scope still waits for every spawned thread, and then resumes the panic.

**4. `join` and deques.** Each worker has a double-ended queue. `join(a, b)` pushes `b` onto the bottom of the worker's
own deque, runs `a` right away, then pops the bottom. If `b` is still there, the worker runs it too, and the cost of
potential parallelism was one push and one pop. If `b` was stolen, the worker steals and runs other work while it waits.
The owner works at the bottom (LIFO: the newest, smallest, cache-warm piece, depth-first). Thieves steal from the top
(FIFO: the oldest entries, which in a divide-and-conquer tree are the biggest pieces), so one steal moves a lot of work,
and owner and thieves rarely touch the same end.

**5. Rayon vs scoped logstat (27.9 vs 36.2 ms).** The rayon version split the input into 32 chunks for 4 threads, and work
stealing evened out chunks that happened to be slower. The scoped version had exactly 4 chunks, so the slowest chunk
decided the total: load imbalance. (Rayon's threads also already existed, but ~28 µs per scoped spawn is small next to
30 ms.)

**6. Exact merges.** Every field of `Summary` is a sum: counters, histogram buckets, per-path counts. Addition is
associative and commutative, so the way the input is chunked can't change the result, and percentiles read from the
merged histogram equal the sequential ones. What breaks it: per-chunk percentiles (a p99 per chunk can't be combined),
floating-point sums (not associative), anything order-dependent ("the first 10 errors") unless the order is kept, and
truncated per-chunk top-k lists.

**7. The break-even.** For a `sqrt` per element, between 10,000 and 100,000 elements (0.76× at 10,000, 1.58× at 100,000,
one run): tens of microseconds of total work. There's a threshold because parallelism has fixed costs per call: waking
workers, splitting, stealing, joining, and moving data between caches, microseconds in total. The work must be larger
than that.

**8. No blocking I/O in `par_iter`.** Rayon's global pool has about as many threads as cores, shared by the whole
process. A blocking call occupies a worker without using its CPU, so every other parallel job in the process queues
behind network waits, nested parallelism piles up, and tail latency explodes (§10: p99 from 2.5 s to over 9 s). I/O
concurrency belongs to async, or to a separate bounded blocking pool.

**9. The parallel BFS.** Correct: each node is claimed by exactly one successful `compare_exchange` from `UNSEEN` to
`level + 1`. The CAS publishes nothing except that value, and the `collect` at the end of each level (a join) orders one
level before the next, so `Relaxed` is enough (Part XIV). Deterministic: a node can only be claimed while its level's
frontier is expanded, and every thread writes the same value, `level + 1`. Whichever thread wins, the distance is the
same. The frontier's *order* varies from run to run, and the distances don't.

**10. Parallel streams vs rayon.** *Pool*: `ForkJoinPool.commonPool()` with CPUs − 1 threads plus the calling thread,
versus rayon's global pool with one thread per CPU; both support custom pools. *Scheduling*: both use work stealing
(Doug Lea's fork/join; rayon adds adaptive splitting). *Compile time*: Java checks nothing about thread safety, since
"non-interfering, stateless" lambdas are a documented requirement, and the lost-update bug compiles. Rayon requires
`Fn + Send + Sync` closures, so mutating captured state is E0596 and a data race can't be written.

### Debugging exercise (`score_all`)

1. Worker W1 holds `CACHE` and calls `train_model`, whose inner `par_iter` makes W1 wait for its inner tasks. A waiting
   rayon worker doesn't block: it steals and runs other tasks. One of them can be an *outer* `score_all` task (another
   user's closure), which calls `CACHE.lock()` on W1, which already holds `CACHE` further up the same stack. std's
   `Mutex` isn't reentrant, so W1 never returns. Every other worker's outer task also blocks on `CACHE`, so no thread is
   left to run `train_model`'s inner tasks. Nothing progresses.
2. Even when it doesn't hang, the global lock is held across `train_model` (seconds of CPU work), so every other user's
   scoring, even for cached segments, waits behind it. Parallelism collapses to one worker training while the rest are
   blocked cores.
3. Restructure into phases: (a) collect the needed segments and, under a short lock, find which are missing; (b) train
   the missing ones **outside any lock**, in parallel: `missing.par_iter().map(|s| (*s, Arc::new(train_model(*s))))`;
   (c) insert them under a short lock; (d) take a snapshot (clone the needed `Arc`s into a local map) and score with
   `users.par_iter()`, touching no lock. Chapter 11.3's rules would have caught it: "no slow work under a lock" and
   "don't call code you don't own while holding a lock". So would this chapter's "never block inside rayon", since
   waiting for a mutex is blocking.

### Selected exercises

- **Intermediate (mergeable statistics).** `distinct_paths` as a per-chunk `HashSet<String>` merged by union is exact
  (memory grows with the number of distinct paths). Averaging per-chunk p99s is wrong on skewed input: chunks with p99s
  of 10, 10, 10, and 1,000 ms average to 257.5 ms, while the true global p99 depends on how many requests each chunk
  holds and could be 10 ms or 1,000 ms. Compute percentiles from the merged histogram.
- **Architecture (per-merchant p99 delay).** Give each merchant a fixed-size, log-bucketed histogram (HdrHistogram-style)
  that merges by addition: bounded memory per merchant, exact at bucket resolution. Or use a mergeable sketch
  (DDSketch, t-digest) with a stated error bound. Since the job partitions by merchant, each merchant's data lives in one
  partition anyway, so the merge only concatenates per-merchant results.

---

## Chapter 11.7 — The Concurrency Decision Matrix

### Interview & architecture questions

**1. The ranking.** No sharing (0.3–0.6 ns per increment: a register add, and no cache line leaves the core) < one
shared atomic (9–19 ns: a `lock xadd` on a line that must move to the writing core for every increment, with all writers
serialized on it) < one shared mutex (26–35 ns for `parking_lot`, 38–181 ns for std: two atomic operations on the lock
line plus the data, and for std, `futex` system calls once the lock is marked contended). Five runs each; the ranking
held while absolute numbers moved up to 5×.

**2. False sharing.** Four adjacent `AtomicU64`s sit in one 64-byte cache line. Coherence works per line, so each
core's write takes the line away from the others, exactly as if they shared one variable: 5–23 ns. Fix: put each counter
on its own line (`CachePadded`), or keep counters in per-thread memory. `CachePadded` is 128 bytes on x86-64 (and
aarch64) because Intel's spatial prefetcher fetches lines in adjacent pairs, so a neighbor in the next line can still
interfere [CPU] [LIB].

**3. Variance.** Absolute contention costs depend on where the threads land (SMT siblings or distant cores), what else
the machine is doing, and the CPU model. One run is an anecdote. Decide on **rankings and ratios that are stable across
runs**, report ranges, and confirm in production-like conditions (pinned threads, the real core count, the real
workload). Also remember that microbenchmarks hammer shared state with no work in between, which exaggerates contention
compared with real services.

**4. 16 shards beat one `RwLock`.** With 16 shards and 4 threads, two threads rarely want the same shard at the same
moment, so most lock operations are uncontended. A single `RwLock` has one reader-count word that *every* read writes,
so all four threads fight over one line whether they read or write. Sharding divides the contention by N, and inside a
shard `Mutex` and `RwLock` tie.

**5. When the owner thread is right.** When operations must be serialized or ordered (a state machine, a device, a
protocol), when the resource is thread-affine, when an invariant spans the whole structure (a sweep that must be atomic
with respect to updates), when commands are mostly fire-and-forget (no round trip), or when the rate is low enough that
clarity beats throughput.

**6. The audit write and the ~1,000 requests/s ceiling.** With the write inside, each request holds the lock for about
1 ms, and the lock admits one holder at a time: at most 1 / 1 ms = **1,000 requests/s**, whatever the thread count
(measured 941–943). With the write after the guard is dropped, the lock is held for nanoseconds and the four 1 ms writes
overlap: a ceiling of about 4 × 1,000 = 4,000/s (measured 3,687–3,754), about 3.9×.

**7. The procedure applied.** *Feature-flag table* (read on every request, changed a few times a day): shared,
read-mostly, replaced in bulk, so `ArcSwap<Flags>` (or a plain `Arc` if it never changes at run time). *Per-merchant
rate-limit counters*: shared and updated per key, with a check-and-increment invariant per entry, so a sharded
`Mutex<HashMap>` whose critical section checks and increments together. For a very hot merchant, add per-thread token
leases. *A connection's protocol state machine*: needed by one thread at a time (the one handling the connection), so it
stays local, with no sharing at all. If several parties must drive it, use an owner thread.

**8. `LongAdder`.** A striped counter: a base value plus an array of padded cells that grows under contention. Each
thread adds to a cell chosen by a per-thread probe, and `sum()` adds everything up (not an atomic snapshot). In Rust:
`Vec<CachePadded<AtomicU64>>` indexed by a per-thread slot, `fetch_add(Relaxed)` on your own slot, and a sum on read. Or,
when each counter has exactly one writer, per-worker counters updated with `store(load + 1)` and summed on scrape (§9).

**9. What `ConcurrentHashMap` can't give you.** Atomic multi-key updates (move a value between two keys, keep an invariant
across keys), consistent iteration or snapshots (its iterators are weakly consistent), and an exact size while updates
run. Compound check-then-act across keys needs an external lock. A single `Mutex<HashMap>` gives all of these, at the
price of contention.

**10. Low CPU, flat profiles, low throughput.** The threads are waiting, not computing. Measure, in order: off-CPU time
and thread states (off-CPU flame graphs, `perf sched`, `/proc/<pid>/task/*/wchan`) to learn *what* they wait on;
lock-wait and lock-hold time by call site; context switches and `futex` system calls (`perf stat -e context-switches`,
`strace -c -e futex`); queue depths and pool utilization; I/O latency, especially I/O performed under a lock. Part XX
covers the tools.

### Debugging exercise (`PipelineStats`)

1. `size_of::<PipelineStats>()` is 24 bytes, so the three counters share one 64-byte line (or straddle two). Each stage
   has one writer, but every increment invalidates the line in the other stages' caches: false sharing, turning each
   increment into a coherence transfer. Profile evidence: cycles concentrated on the `lock xadd` instructions, and
   cache-to-cache transfers (HITM events in `perf c2c`) on one address.
2. Separate `static`s are placed by the compiler and linker, with no guarantee of distance. Neighboring statics often end
   up adjacent in `.data`/`.bss`. So the old code may have had the same false sharing, or been lucky. "It used to work"
   isn't evidence of a correct layout, because layout can change with any build.
3. With padding: `pub decoded: CachePadded<AtomicU64>` and so on, 3 × 128 bytes, so each counter owns its line pair and
   the scraper reads three lines, exact at read time. With ownership: each stage keeps a plain local count and publishes
   it into its own padded atomic every N items (or sends snapshots), so the stages never write shared lines on the hot
   path, at the price of slightly stale values. For a scraper running every 10 seconds, periodic publication is enough.
   `CachePadded` is the simplest change.

### Selected exercises

- **Beginner (`thread_local!` `Cell` counters).** Predicted from mechanism: with the "no sharing" row, around a
  nanosecond. A thread-local access adds a TLS address computation (and possibly a lazy-initialization check) but
  touches no shared line. Measure it.
- **Intermediate (`ArcSwap` with copy-on-write writes).** Every write clones the whole map, so with 5% writes the average
  operation pays about 0.05 × (the cost of cloning n entries), plus the swap. At 10,000 keys that swamps the ~8 ns reads
  and loses badly to sharding. At 100 keys the clone is cheap and the fast reads can win. The crossover is where
  0.05 × clone(n) ≈ the sharded operation's cost: predicted from mechanism, so measure it.

---

## Project Level 3 — Architecture review

**1. Capacity.** With one worker per connection and one 50 ms request per connection, 4 workers serve at most
4 / 0.05 s = **80 requests/s**. At 1.5× (120/s), the queue of 2 fills immediately, about 40 requests/s get `503`, and
admitted requests see 50 ms plus a short queue wait. At 10× (800/s), 80 are served and about 720 refused: refusals
dominate, but admitted latency stays bounded. Metrics: queue depth, busy workers, refusals per second, and per-request
latency. Queue near zero means below capacity; a non-empty queue with rising latency means at capacity; a full queue
with refusals means overload.

**2. Queue size.** A queue of 2 bounds queueing delay (at most 2 × 50 ms / 4 workers ≈ 25 ms) and makes overload
visible immediately. A queue of 10,000, by Little's law, means that at saturation a new request waits
10,000 / (80 per second) = **125 s** before being served. Clients time out long before, and the server does work for
clients who have already given up. A larger queue is right when bursts are short and clients tolerate latency: size it
as throughput × the acceptable queueing delay.

**3. Slowloris.** Four clients each send one byte every 400 ms. The 500 ms read timeout applies per `read`, so it never
fires, and each client holds a worker indefinitely. With 4 workers, nobody else is served: the queue fills and everyone
gets `503`. The fix is a per-request deadline: record when the request started, and before each read set the socket
timeout to `min(read_timeout, deadline − now)`, answering `408` when the deadline passes (optionally with a minimum data
rate). Defend at the edge too: a reverse proxy that buffers whole requests before forwarding, and per-IP connection
limits.

**4. Two panic boundaries.** The per-request `catch_unwind` turns a handler bug into a `500` for *that request*, and the
connection (with its keep-alive requests) carries on. The pool's `catch_unwind` catches anything that escapes the
connection code itself (parser or write-path bugs), so the *worker* survives. Without the pool's: each escaping panic
kills a worker thread, and the pool shrinks silently until no workers are left. The server still accepts and queues,
then refuses everything. Without the per-request one: a handler panic unwinds to the pool, the connection is dropped
with no response (the client sees a reset), and the rest of its keep-alive requests are lost. The worker survives.

**5. Shutdown walkthrough.** The handle sets the flag and self-connects, and `accept` wakes, sees the flag, and stops
(the wake-up connection is dropped unserved). `pool.shutdown()` marks the queue closed and wakes every worker. The
in-flight request completes and gets its response (the demo's `slept 300 ms`). The two queued connections are still
served, and with keep-alive each could make up to 100 more requests. The idle keep-alive client holds a worker until
its read timeout fires, and is then closed. Add a drain deadline: once shutdown begins, answer with `Connection: close`
(a shared flag the connection loop checks), wait up to a fixed time (say 10 s), then shut down the remaining sockets
(from a registry of active streams) and join. Report how many were cut off.

**6. The socket is never behind a lock.** Each `TcpStream` is owned by exactly one thread at a time: accept thread, then
queue, then one worker. `try_clone` gives that same worker a reader and a writer. No two threads ever use a socket
concurrently, so there's nothing to lock. If responses came from a different thread (a backend pool), the writer half
would have to move to, or be shared with, that thread, and HTTP/1.1 requires responses in request order. You'd need a
per-connection response sequencer: a writer task that receives responses (with sequence numbers) over a channel and
writes them in order.

**7. Reuse.** `WorkerPool<T>` is generic over any `T: Send + 'static` with a handler `Fn(T) + Send + Sync`. It knows
nothing about HTTP, hands refused items back, and contains panics and shutdown itself. Before publishing it as a crate:
return spawn errors from `new` instead of `expect`; add a blocking `submit` with a timeout next to `try_submit`; make
metrics a hook instead of fixed atomics; add a shutdown deadline and a panic callback for logging; document the
`AssertUnwindSafe` assumption; test shutdown ordering; and design the public API for semver stability (Chapter 22.1).

**8. When thread-per-connection stops working.** Every idle connection costs a thread: 2 MiB of virtual stack, tens of
KiB of touched pages, and order of 10–20 KiB of kernel state. Around 10,000 mostly idle connections means 10,000
threads: hundreds of MB of resident memory, and scheduler overhead. What runs out first depends on configuration: often
the container's `pids.max` or `ulimit -u`, and at the latest `vm.max_map_count` (default 65,530, with two mappings per
thread, so about 32,000 threads) [OS]. An async server holds an idle connection for kilobytes (Part XIII).

---

## Project Level 4 — Architecture review (Ferrite v1)

**1. `&self`.** A store is shared by every connection, so implementations synchronize internally and choose their own
strategy (16 shards now, a WAL writer later). With `&mut self`, every caller would have to wrap the whole store in one
lock: the single-`Mutex<HashMap>` row of Chapter 11.7 (160–272 ns contended against 74–81 ns sharded), with the locking
strategy baked into every caller.

**2. Returning values.** A `&[u8]` would borrow from the map inside the shard lock, so the guard would have to outlive
the reference. The API would have to hand out guards, or it can't be expressed at all. *Returning a guard*: zero-copy,
but callers hold locks for arbitrary durations, possibly across network writes, and a caller that `put`s into the same
shard while holding a read guard deadlocks. *`Arc<[u8]>` values*: a `get` becomes one refcount increment (an atomic,
contended for hot keys), values are immutable, and the trait signature changes. *A callback* (`with_value`): zero-copy,
with the lock scope bounded by the callback, but the callback runs under the lock (slow callbacks are 11.7 §10's
mistake), and it can't hold the value across an `.await`. Several answers are defensible. For v2 on Tokio, shared
immutable values (`Arc<[u8]>` or `bytes::Bytes`) are the natural choice: no lock is ever held across an `.await`, and
large values are never copied.

**3. Poisoning.** Every critical section is one `HashMap` operation on `Vec<u8>` keys, whose `Hash` and `Eq` can't
panic, and entries have no cross-key invariant. So a poisoned shard's map is still a valid map, and refusing it would
turn one bug into an outage for a sixteenth of the keyspace. Counting surfaces the bug anyway. It becomes wrong as soon
as a critical section has two steps that must agree: a secondary index or a byte-size counter updated separately from
the map, a multi-key operation applied in several steps, or a user callback that can panic halfway through an in-place
update.

**4. Shard count.** 16 is a small power of two above the typical thread count, so collisions are rare, and the memory
cost is negligible. One shard is a global lock. Four shards on 32 cores averages 8 threads per lock: heavy contention.
4,096 shards make contention negligible but cost memory and locality (4,096 locks and maps, each with its own growth
slack), and whole-store operations like `len()` must visit every shard. Choose by measuring throughput and p99 against
shard count under the real read/write mix and thread count (Chapter 11.7's Advanced exercise), plus per-shard lock-wait
time to spot hot shards.

**5. The router.** A keyed hash stops clients from crafting keys that all land in one shard (Chapter 9.3's HashDoS, one
level up). In v5, partitioning across machines must agree across processes and restarts, and `RandomState` is random per
process: two nodes would route the same key differently. Use a stable, well-distributed hash (a fixed-seed hash, with
the seed as cluster configuration if keys are attacker-controlled) or range partitioning, behind a versioned partition
map. Each node can keep `RandomState` for its *internal* shards.

**6. Capacity.** One worker per connection, so 4 clients are served at once and 16 more wait in the queue: **20
connections admitted**. The 21st gets `-ERR server busy` and is closed immediately. The 5th is accepted and queued. It
can send commands, but nothing reads them until a worker frees up, which could take up to 30 seconds. So it sees a long
silence, and probably its own client-side timeout.

**7. The flush rule.** If the `BufReader` still holds bytes, more pipelined requests are already waiting, so their
replies are appended to the `BufWriter` and go out together in fewer `write` calls. When it's empty, the client is
waiting for replies, so flush now. Flushing after every reply costs one system call (and often one packet) per reply:
lower throughput for pipelined clients, and no latency gain. Never flushing until the buffer fills would leave a client
waiting for its single reply indefinitely, a deadlock with any client that waits for a reply before sending more.

**8. `INCR`.** With `GET` + `SET` in the client, two clients read `5`, both write `6`, and one increment is lost: Chapter
1.3's TOCTOU race, across the network. In the store: a read-modify-write under the shard's **write** lock (parse, add,
store, return the new value, or an error if the value isn't an integer). Put it in a separate trait (`KvUpdate`, with
`update(&self, key, f)` or a specific `incr(&self, key, delta)`), so `KvStore` stays minimal and stores opt in. For v3,
the WAL must record the **result** (or a deterministic `Incr(delta)` record), so recovery replays the same state. For v5,
the leader executes it and replicates the result through the log. Followers never re-run code. Closures can't be logged
or replicated: they aren't serializable, and nothing guarantees they're deterministic.

---

## Part XI Review — Capstone: the partner-quota PR

### 1. The defects

Ranked roughly by severity. "Money" means partners get service they didn't pay for, or aren't billed for what they got.

| # | Defect | Rule broken (chapter) | What happens in production |
|---|---|---|---|
| 1 | **Check-then-act race**: `try_acquire` reads `used` under one lock acquisition and increments under another, with a 1 ms audit write in between | Check and act in one critical section (1.3 TOCTOU, 11.3) | **Money**: partners exceed their paid quota (26–27 admitted against 20 in five runs), and upstreams sized for the quota get more load. Finance notices in a billing dispute; upstream on-call notices first |
| 2 | **Billing events lost**: the reporter uploads only full batches of 50, and it's a detached thread (its `JoinHandle` is dropped), so nothing flushes the partial batch or drains the queue on shutdown | Join what you spawn (11.1 §10); shutdown is a code path (11.5) | **Money**: every deploy and every quiet period loses up to 49 events per process, plus everything queued (0 of 28 uploaded in the demo). Partners are under-billed, silently. Finance finds it in a reconciliation weeks later |
| 3 | **Lock-order inversion**: `acquire_slot` holds `inflight` and then locks `usage` (via `usage_of`), while `reset_window` holds `usage` and then locks `inflight` | A global lock order (11.3 bug #1) | The gateway hangs at a window boundary under load: the timer thread and a worker deadlock, then every worker piles up behind them. On-call sees every gateway pod stop serving at a minute boundary |
| 4 | **Alert hook called with the `usage` lock held** | Decide under the lock, act after it (11.7 §10); non-reentrant locks (11.3 §8) | A slow hook (an HTTP call to the paging service) serializes every request of every partner behind it. A hook that calls `usage_of` to enrich the alert deadlocks the calling thread against itself |
| 5 | **`unwrap()` on every lock, and a `Drop` that can panic**: one panic while `usage` is held poisons it, then every request panics; `Slot`'s `Drop` unwraps a lock during unwinding | Poisoning policy per lock (11.3 §7); `Drop` must not panic (8.3) | Cascading outage, then process aborts (see question 4). Every pod crash-loops if the triggering bug is deterministic |
| 6 | **Unbounded billing channel with `send().unwrap()`** | Capacity is an admission policy (11.5 §7, §10) | If the reporter falls behind (it can upload at most ~10,000 events/s: 50 per 5 ms), the backlog grows in the heap until the pod is OOM-killed, losing the whole backlog. If the reporter thread ever dies, `send` returns `Err` and the `unwrap` panics *every request* |
| 7 | **Blocking audit I/O on the request path** (`write_audit`, ~1 ms, for every admission) | Keep slow work out of the hot path; owner thread for a sink (11.5 §9) | +1 ms latency on every request, audit-volume stalls become request stalls, and it widens defect 1's race window from nanoseconds to a millisecond |
| 8 | **A thread per partner per window** in `reset_window`, detached | Threads are a budget (11.1 §7) | With thousands of partners, thousands of threads are spawned every minute. Spawn failures panic *while holding `usage`* (poisoning it, defect 5), and audit lines still being written at shutdown are lost |
| 9 | **Slow work under the `usage` lock** in `reset_window` (thread spawns, ~30 µs each) | 11.7 §10 | Every request stalls for the duration of the sweep at each window boundary: a p99 spike every minute |
| 10 | **Alert storm**: `on_exceeded` fires on every rejected request | Edge-trigger alerts (operations) | 53 alerts for one partner in one short burst. On-call gets paged per request and mutes the alert, and the next real one is missed |
| 11 | **Fail-open default**: unknown partners get a quota of 100 | Malformed never defaults (8.1) | A typo in the partner config, or a revoked partner, still gets traffic. The demo admitted `zeta` |
| 12 | **Lost updates on `ADMITTED_TOTAL`**: `load` then `store` on an atomic | Atomic RMW or one writer (11.7 matrix: "lost updates if you do read-modify-write by hand") | The admitted-requests metric under-counts under real concurrency. (The demo didn't show it: the 1 ms sleeps kept increments apart. It's a latent bug.) Also a process-global `static` rather than per-tracker state |
| 13 | **Two global locks for all partners** (`usage`, `inflight`), each taken several times per request | Shard hot shared state (11.7) | Contention on every request from every worker: the "one `Mutex<HashMap>`" row (160–272 ns contended in 11.7), several times per request |
| 14 | **`RwLock` read of `limits` on every request** | Read-mostly data replaced in bulk → `ArcSwap` (11.4) | Every read lock writes the reader count, a shared line bounced by every worker, and a reload's write lock blocks all traffic. `limits` is never reloaded anyway, so there's no API for it |
| 15 | **One billing event (and one `String`) per request** | Aggregate per window, not per request (11.7 §9) | Channel traffic and an allocation on every request for data that billing only needs per window |
| 16 | **A single-threaded test for concurrent code** | Test the concurrency property | `admits_up_to_the_limit` passes, so it gave false confidence. The property that matters, exactness under N threads, was never asserted |

Smaller issues a thorough review also flags: `partner.to_string()` allocates on every admission, `new` returns an
`Arc` and hides a thread with no way to stop it, and `usage_of` is `pub` but reads under the same global lock, so a
dashboard polling it adds contention.

### 2. Reading the output

- **27 admitted against 20.** Each thread checks `used < 20` and releases the lock, spends 1 ms in `write_audit`, then
  increments. Up to 8 threads can pass the check against the same stale count while others are still auditing. A thread
  passes only if fewer than 20 admissions are *counted* when it checks, and at most 7 other threads can be between their
  check and their increment at that moment. So the worst case is 19 + 8 = **27** admissions. Five runs gave 26 or 27:
  the race is the common case, not a rarity.
- **53 alerts.** 80 attempts − 27 admissions = 53 rejections, and every rejection fires the hook. It isn't
  edge-triggered.
- **`zeta` admitted.** `limit_for` defaults unknown partners to 100.
- **0 of 28 billing events uploaded.** 27 for `acme` plus 1 for `zeta`. The reporter uploads only when a batch reaches
  50, so a batch of 28 is never uploaded. When `main` returns, the detached reporter dies with it, taking the batch. In
  production, "when `main` returns" is every deploy.

### 3. The lock orders

- `acquire_slot`: `inflight` → `usage` (inside `usage_of`), plus a read of `limits`.
- `reset_window`: `usage` → `inflight`.
- `try_acquire`'s rejection path: `usage` held → `on_exceeded(..)` → whatever the hook locks. If the hook calls
  `tracker.usage_of`, that's `usage` → `usage` on one thread: a self-deadlock, since std's `Mutex` isn't reentrant.

The first pair deadlocks when a worker is inside `acquire_slot` (holding `inflight`) at the moment the timer thread
enters `reset_window` (holding `usage`). The demo never triggers either one: `reset_window` runs once, with no
concurrent `acquire_slot`, and the demo's hook only increments an atomic. That makes them *more* dangerous. Tests pass,
staging passes, and the deadlock appears only under production timing, at a window boundary under load, where it hangs
every worker of a pod at once and is hard to reproduce afterwards.

### 4. The failure chain with Chapter 8.3's double panic

1. The alert hook panics (a bug in the paging client) while `try_acquire` holds the `_usage` guard. The guard drops
   during unwinding and **poisons `usage`**.
2. Every later `self.usage.lock().unwrap()` panics. Every request now fails, and the lock stays poisoned forever,
   because no code clears it.
3. `acquire_slot` holds `inflight` and calls `usage_of`, which panics on the poisoned `usage`. The unwinding drops the
   `inflight` guard mid-panic, which **poisons `inflight`**.
4. Any request that is unwinding (every request now panics) while holding a live `Slot` runs `Slot::drop`, which calls
   `inflight.lock().unwrap()` on the poisoned lock and panics **during unwinding**: a double panic, and the process
   **aborts**, taking every in-flight request on the pod with it.
5. If the hook's bug is deterministic (every alert fails), each restarted pod repeats the sequence: a crash loop across
   the fleet, triggered by the first partner to exceed a quota.

One bug in an alerting side path took the admission path, then the process, then the fleet. Each link is a Part XI or
Part VIII rule: a callback under a lock, `unwrap` as the poisoning policy, and a panicking `Drop`.

### 5. A redesign

One defensible design (listing `review-02-quota-fixed.rs`), with each piece of state placed by Chapter 11.7's
procedure:

| State | Strategy | How |
|---|---|---|
| Limits | Read-mostly, replaced in bulk | `ArcSwap<HashMap<String, u64>>`; `reload_limits` publishes a whole new map |
| Per-partner window (`used`, `inflight`, `alerted`) | Shared, per key, invariant per entry | 16 shards × `Mutex<HashMap<String, Window>>` with a keyed router. **One** critical section checks and increments. No code ever holds two locks, so there's no lock order to get wrong |
| Alerts | Decided under the lock, fired after | `alerted` flag per window: one alert per partner per window, and the hook runs with no lock held |
| Audit trail | Owner thread + bounded channel | One writer thread owns the audit volume and batches appends. `try_send` on a bounded channel: if it's full, **refuse the request** ("no audit, no admission") and roll the increment back in a second short critical section (`saturating_sub`, because the window may have closed in between) |
| Billing | Aggregated per window | `close_window` collects the counts one shard lock at a time, sorts them (a deterministic report), and sends one summary through the same writer with a **blocking** send: it's rare, and billing data must not be dropped |
| Metrics | Per-tracker atomics | `fetch_add(Relaxed)`, owned by the tracker, not `static` |
| Shutdown | Joined, drained | `shutdown(self)` (and `Drop`) drops the channel's `Sender` and joins the writer, which drains everything queued first |
| Poisoning | Recover, count, clear | Every critical section changes independent counters with statements that can't panic halfway. `Admission`'s `Drop` uses the recovering lock, so it can't panic |
| Unknown partners | Fail closed | `Denied::UnknownPartner` |

Its tests assert what the original test couldn't: exactly 50 admitted by 8 concurrent threads against a limit of 50 (and
exactly one alert), rollback of refused admissions under audit backpressure (`usage_of == admitted`, and every admission
audited), recovery from a deliberately poisoned shard, and window close and reset. Other defensible choices exist. An
owner thread for the whole tracker is simpler to reason about, but costs a round trip per request (Chapter 11.7:
microseconds). Per-worker token leases scale further for a few very hot partners, at the price of short-term
over-admission bounded by the lease size.

---

## Part XI Review — Interview mode

**1. `Send`, `Sync`, and the derived rules.** `T: Send`: may move to another thread. `T: Sync`: `&T` may be shared, so
`T: Sync` ⇔ `&T: Send`. Then: `&mut T: Send` iff `T: Send` (exclusive access is like ownership). `Arc<T>: Send + Sync`
iff `T: Send + Sync` (clones share `&T`, and any thread may drop the last one). `Mutex<T>: Send + Sync` iff `T: Send`
(one thread at a time). `RwLock<T>: Sync` iff `T: Send + Sync` (readers share `&T`). They're marker traits: checked at
the bounds of `spawn`, `Scope::spawn`, `rayon::join`, and `tokio::spawn`, then erased. No run-time cost.

**2. `'static` vs `'scope`.** A `spawn`ed thread may outlive its caller, so it can't borrow the caller's locals. A scope
joins its threads in its own control flow before returning, so its threads may borrow anything that outlives the scope.
The pre-1.0 `thread::scoped` joined in a `Drop`, and `mem::forget` (safe) could skip that `Drop`, leaving a thread
reading a dead stack frame. The rule since RFC 1066 (2015): leaking is safe, so **soundness may never depend on a
destructor running**.

**3. A private `Rc<str>` field.** The struct becomes `!Send + !Sync`. Every downstream `thread::spawn`, `tokio::spawn`,
or `Arc<T>` shared across threads stops compiling, with E0277 notes pointing into the private field. No signature
changed, and it's still semver-major. Catch it with compile-time `Send`/`Sync` assertions and `cargo-semver-checks`, and
use `Arc<str>` if the string really must be shared.

**4. `Mutex::lock` compiled.** Uncontended: `lock cmpxchg` (state 0 → 1), a poison check, the critical section, then
`xchg` (state → 0) and a check for waiters. No system call. Contended: `lock_contended` spins briefly, sets the state to
2, and sleeps in `FUTEX_WAIT`. Unlock sees 2 and calls `FUTEX_WAKE`. The poison flag is a byte right after the futex
word (offset 4 in `Mutex<u64>`, with the data at 8). It's set on unlock when the thread began panicking while holding
the guard.

**5. `OnceLock` and `LazyLock`.** `get_or_init` runs exactly one initializer, even under a race: the others wait and
then see the same value. After initialization, a read is one Acquire load and a branch. A panicking initializer leaves a
`OnceLock` empty (a later caller may retry), and poisons a `LazyLock` forever, because its only initializer was consumed.

**6. Channel endings.** `for msg in rx` stops when `recv` returns `Err`, which means every `Sender` has been dropped and
the queue is drained. A `send` after the receiver is gone returns `Err(SendError(v))`, handing the value back.

**7. The cost ranking.** No sharing (0.3–0.6 ns: nothing leaves the core) < a padded per-thread atomic (2.9–6.7 ns: the
price of a locked instruction on a line only this core writes) < adjacent per-thread atomics (5–23 ns: false sharing)
≈ one shared atomic (9–19 ns: the line moves on every write). Those two middle rows traded places between runs. Then
`parking_lot::Mutex` (26–35 ns) and std's `Mutex` (38–181 ns: futex system calls once it's marked contended). The
absolute numbers varied up to 5× with thread placement and machine load, while the ranking held. So trust rankings and
ranges, not single numbers.

**8. `RwLock` slower than `Mutex`.** A read lock writes the reader count, so readers bounce the lock's line just as
mutex users do, and short critical sections give parallel readers nothing to win. For data replaced in bulk, use
`ArcSwap` (7.8 ns per read against 219–259 ns through a lock). For data updated per key, shard it (74–81 ns per
operation).

**9. `par_iter` over 100 items.** Parallelism has a fixed cost per call (waking workers, splitting, stealing, joining:
microseconds) that 100 cheap items can't repay: 0.2 µs sequential against 8.1 µs parallel in Chapter 11.6. And in a
server, it also competes with every other request's parallel work. The rule: parallelize work measured in milliseconds,
not in elements (break-even was tens of microseconds of total work).

**10. The procedure applied.** *Route table*: read constantly, replaced every 10 minutes, so `ArcSwap` plus a dropper
thread that frees old tables off the request path. *Per-route counters*: hot and write-heavy, so per-worker counters
with one writer each, summed on scrape (after shared atomics proved to be hot lines). *Session store*: per-key updates on
most lookups, so sharded locks, with the expiry sweep done shard by shard. Use an owner thread only if strict ordering is
required. *A connection's protocol state*: owned by the thread handling the connection, no sharing.

**11. Admission control for a thread-per-connection server.** Bound the workers (a fixed pool) and the queue in front of
them (small, sized as throughput × acceptable queueing delay), plus per-request deadlines. A refused client gets an
immediate `503` (with `Retry-After`) and a closed connection, from a refusal path that never blocks the accept loop.
Metrics: queue depth, busy workers, refusals per second, per-request latency histograms, and lock waits. Regimes: an
empty queue means below capacity, a non-empty queue with rising latency means at capacity, and a full queue with rising
refusals means overload.

**12. Porting the Java service.** (1) Nested `synchronized` methods become a self-deadlock, because std's `Mutex` isn't
reentrant. Lock at API boundaries, and let helpers take `&mut` data. (2) Nested reads under `ReentrantReadWriteLock`
become the writer-waiting deadlock with std's `RwLock`. Pass data down, or publish snapshots with `ArcSwap`. (3)
`newFixedThreadPool`'s unbounded queue, ported as an unbounded `mpsc::channel`, becomes an OOM under overload. Use a
bounded queue that refuses (Project L3's pool). Close runners-up: `synchronized` blocks that guarded code rather than
data (put the data *inside* the `Mutex`), and `volatile` flags (atomics with an explicit ordering, Part XIV).

**13. Low CPU, flat profiles, low throughput.** It's waiting. In order: (1) thread states and off-CPU time (off-CPU flame
graphs, `perf sched`, `/proc/<pid>/task/*/wchan`) to learn what they wait on; everything parked in `futex` points to
locks, condition variables, or channels, and threads parked on locks confirm 11.3 §10's recursive-read deadlock.
(2) Lock hold and wait times by call site, which confirm 11.7 §10's audit write under a shard lock. (3) Queue depths and
pool utilization: 11.5 §10's stalled shipper, or 11.6 §10's rayon pool full of blocking fetches. (4) Context switches and
`futex` system-call counts: a contended std mutex (11.7 §4). (5) Connection-pool waits and I/O latency.

**14. The shared-state review checklist.**

1. Does it need to be shared at all? Can it be local or partitioned?
2. Which strategy from Chapter 11.7's procedure, and why not the one to its left?
3. Every lock: what data, what invariant, and the poisoning policy, written next to it.
4. Nothing slow while a guard is alive: no I/O, logging to slow sinks, callbacks, or blocking sends.
5. Check-and-act happens in one critical section.
6. Never two locks without a documented order, and no re-locking in helpers.
7. Every queue is bounded, with a named full-policy (block, refuse, drop-and-count, spill).
8. Every spawned thread is joined, and the shutdown path is tested.
9. Every `unsafe impl Send`/`Sync` has a `SAFETY` comment naming the synchronization, and Miri runs in CI.
10. A concurrency test asserts exactness under N threads, and every performance claim comes with a measurement.
