# Chapter 14.2 — Happens-Before

> **Where this sits:** Part XIV · Memory Model and Atomics · chapter 2 of 5
> **Prerequisites:** Chapter 14.1 (the three reorderers), Chapter 1.3 (Send/Sync and the `Relaxed` counter),
> Chapter 4.1 (`UnsafeCell`).
> **After this chapter you can:** define sequenced-before, synchronizes-with, happens-before, and data race the way the
> language does; find the happens-before edge (or its absence) in any design; explain why a data race is undefined
> behavior in Rust but defined in Java; use Miri's race detector; and justify Chapter 1.3's `Relaxed` counter
> rigorously.

---

## Pass 1 · User level — *One rule instead of three machines*

### 1. Problem

Chapter 14.1 explained reordering by looking at store buffers, Arm's weak model, and LLVM's optimizations. That's the
right way to understand *why* the problem exists, and the wrong way to write programs. You can't reason about every
CPU your code will run on, or every optimization a future compiler will do. You need one rule, stated at the level of
your source code, that all of those layers promise to respect.

That rule is **happens-before**. The `std::sync::atomic` documentation states that Rust's atomics follow the C++20
memory model (without `memory_order_consume`), and the Rustonomicon is frank about why: Rust "pretty blatantly just
inherits the memory model for atomics from C++20," not because it's easy, but because it's what compilers and
hardware vendors already implement [LANG]. This chapter gives you that model in its working form, then uses Miri to
check it against real programs.

### 2. Mental model

Four definitions, simplified from C++20 [intro.races] to what you need day to day [LANG]:

```text
 sequenced-before (sb)     within ONE thread: A comes before B in program order (statement order, evaluation order)

 synchronizes-with (sw)    ACROSS threads: a Release operation on atomic M, and an Acquire operation on M that
                           reads the value it wrote (or a later value in its "release sequence", Chapter 14.3).
                           Library operations are specified in the same terms: unlock → next lock, spawn → start of
                           the new thread, end of a thread → join() returning, send → matching recv, ...

 happens-before (hb)       the transitive closure of sb and sw: follow program order within threads and
                           synchronizes-with arrows between them

 data race                 two accesses to the same location, from different threads, at least one a WRITE,
                           at least one NON-ATOMIC, and neither happens-before the other
```

And the two consequences that everything else in this Part builds on:

1. **A data race is undefined behavior** [LANG] (the Reference, "Behavior considered undefined"). Not "a garbage
   value": the whole program has no meaning, and the compiler optimizes on the assumption that it never happens
   (Chapter 14.1's hoisted loop was one such optimization).
2. **Without a data race, a non-atomic read sees the last write that happens-before it.** Visibility is a consequence
   of happens-before, not a separate mechanism. There's no "flush to main memory" step in the model. Java engineers who
   learned `volatile` as "flushes the cache" can drop that picture here (Chapter 14.5).

A happens-before graph for the simplest correct publication:

```text
   Thread A                                   Thread B
   data = 42          (non-atomic)
      │ sb
   flag.store(true, Release)  ───── sw ─────► flag.load(Acquire) == true
                                                 │ sb
                                              read data   → must see 42: data=42 hb read
```

Remove either the `Release` or the `Acquire`, and the `sw` arrow disappears. The write and the read of `data` are
then unordered: a data race, and undefined behavior. There's no partial credit for getting one side right.

**Atomics have one more rule of their own: coherence.** Every atomic location has a single **modification order**, a
total order of all writes to it, that all threads agree on, *whatever the orderings used* [LANG]. Reads by a thread
never go backwards in it, and a read-modify-write (`fetch_add`, `swap`, `compare_exchange`) always reads the latest
value in the modification order. That's why no increment of a `Relaxed` counter is ever lost.

### 3. Rust code

**Safe Rust can't express the race** (listing `ch02-06-race-rejected.rs`, verified):

```rust,compile_fail
use std::thread;

fn main() {
    let mut hits: u64 = 0;
    thread::scope(|s| {
        s.spawn(|| hits += 1);
        s.spawn(|| hits += 1);
    });
    println!("hits = {hits}");
}
```

```text
error[E0499]: cannot borrow `hits` as mutable more than once at a time
   --> src/main.rs:9:17
    |
  7 |     thread::scope(|s| {
    |                    - has type `&'1 Scope<'1, '_>`
  8 |         s.spawn(|| hits += 1);
    |         ---------------------
    |         |       |  |
    |         |       |  first borrow occurs due to use of `hits` in closure
    |         |       first mutable borrow occurs here
    |         argument requires that `hits` is borrowed for `'1`
  9 |         s.spawn(|| hits += 1);
    |                 ^^ ---- second borrow occurs due to use of `hits` in closure
    |                 |
    |                 second mutable borrow occurs here
```

That's Chapter 4.1's aliasing XOR mutation doing the work. Two threads that both need `&mut hits` at once is the
definition of a write-write conflict, and the borrow checker rejects it before any memory model question arises.

**With `unsafe`, the race compiles, and it's UB** (listing `ch02-01-data-race.rs`, verified three ways):

```rust
use std::thread;

/// A raw pointer that we (falsely) promise is safe to send to another thread.
/// This `unsafe impl` is the only reason the program below compiles.
#[derive(Clone, Copy)]
struct SendPtr(*mut u64);
unsafe impl Send for SendPtr {}

const THREADS: u64 = 4;
const PER_THREAD: u64 = if cfg!(miri) { 10 } else { 1_000_000 };

fn main() {
    let mut hits: u64 = 0;
    let p = SendPtr(&mut hits);
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(move || {
                let p = p; // capture the whole SendPtr (edition 2021+ would otherwise capture only p.0)
                for _ in 0..PER_THREAD {
                    // SAFETY: NONE. Four threads do this at once: a data race, which is undefined behavior.
                    unsafe { *p.0 += 1 };
                }
            });
        }
    });
    println!("hits = {hits} (expected {})", THREADS * PER_THREAD);
}
```

```text
debug build:    hits = 1778134 (expected 4000000)
release build:  hits = 4000000 (expected 4000000)

under Miri:
error: Undefined Behavior: Data race detected between (1) non-atomic write on thread `unnamed-1` and (2) non-atomic read on thread `unnamed-2` at alloc215
  --> src/main.rs:24:30
   |
24 |                     unsafe { *p.0 += 1 };
   |                              ^^^^^^^^^ (2) just happened here
   |
help: and (1) occurred earlier here
```

Three results, and only Miri's is the truth. The debug build lost 56% of the increments, which is the textbook
load-add-store race. The release build printed the *correct* total, and that's the dangerous one. Its assembly shows
why:

```text
std::sys::backtrace::__rust_begin_short_backtrace::<playground::main::{closure#0}::{closure#0}, ()>:
	add	qword ptr [rdi], 1000000      ; the thread's whole loop: ONE non-atomic add
	ret
```

Assuming no data race, LLVM collapsed each thread's million increments into a single `add` of 1,000,000, with no `lock`
prefix. The race window shrank from a million instructions to one, and in this run the four adds didn't overlap. The
program is exactly as broken as the debug build. It just stopped showing it.

**The fix, and Chapter 1.3's promise** (listing `ch02-02-atomic-counter.rs`, verified under Miri and natively):

```rust
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::thread;

const THREADS: u64 = 4;
const PER_THREAD: u64 = if cfg!(miri) { 10 } else { 1_000_000 };

fn main() {
    let hits = AtomicU64::new(0);
    let before_spawn = String::from("config loaded"); // written BEFORE the threads exist
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                // spawn edge: everything the parent did before spawn() happens-before this line.
                assert_eq!(before_spawn, "config loaded");
                for _ in 0..PER_THREAD {
                    // Relaxed: atomic (no lost updates), but orders nothing else.
                    hits.fetch_add(1, Relaxed);
                }
            });
        }
    }); // join edge: every thread's last action happens-before scope() returns.
    // A Relaxed load is enough here: the joins already ordered all increments before it.
    println!("hits = {} (expected {})", hits.load(Relaxed), THREADS * PER_THREAD);
}
```

```text
under Miri:     hits = 40 (expected 40)
release build:  hits = 4000000 (expected 4000000)
```

Here is the rigorous version of Chapter 1.3's two-sentence argument. **No increment is lost**, because each
`fetch_add` is a read-modify-write, and an RMW always reads the latest value in the counter's modification order,
whatever the ordering argument. **The final read sees all of them**, because each thread's increments are
sequenced-before its end, the end of each thread synchronizes-with the scope's join, and the join is sequenced-before
the final load. So every increment happens-before the load, and coherence forbids the load from returning an older
value than one that happens-before it. `Relaxed` contributes atomicity. The *threading library* contributes the
ordering.

The compiler does keep every increment this time: release assembly shows the loop unrolled ten times, each iteration a
separate `lock inc qword ptr [rsi]`.

**Six ways std creates happens-before, checked by Miri** (listing `ch02-03-hb-edges.rs`). Each function writes a plain,
non-atomic `u64` on one thread and reads it on another, synchronized by exactly one std mechanism. The shared cell is
an `UnsafeCell` with an `unsafe impl Sync`: a promise that every access is ordered, which Miri's race detector checks:

```rust,ignore
fn via_release_acquire() -> u64 {
    let (p, flag) = (Plain::new(), AtomicBool::new(false));
    thread::scope(|s| {
        s.spawn(|| { unsafe { p.write(50) }; flag.store(true, Release); });
        while !flag.load(Acquire) { std::hint::spin_loop(); }
        unsafe { p.read() }
    })
}
```

```text
spawn + join:      2
Mutex:             10
channel:           20
OnceLock:          30
Barrier:           40
Release/Acquire:   50
```

Miri reported no race for any of them (`miri-ok`). Replace the Release/Acquire pair with `Relaxed` and Miri reports
the race (that's listing `ch03-01`, the centerpiece of Chapter 14.3).

---

## Pass 2 · Systems level — *How the edges are made, and how races are caught*

### 4. Under the hood

**How std builds each edge** [LIB]. None of these is magic. Each is an atomic Release/Acquire pair (or a syscall that
acts as one) inside the library:

| Mechanism | The Release side | The Acquire side | Documented as |
|---|---|---|---|
| `thread::spawn` / `scope.spawn` | everything before the spawn call | the start of the new thread's closure | spawn edge |
| `JoinHandle::join` / scope end | the thread's completion | `join` returning | "the completion of the associated thread synchronizes with this function returning" (std docs) |
| `Mutex` | `unlock` (guard drop): a Release store/swap on the lock word | a successful `lock`: an Acquire CAS/swap | lock/unlock pairs |
| channels (`mpsc`) | `send` | the `recv` that returns that message | message passing |
| `OnceLock` | completing initialization | a `get` that returns `Some` | one-time init |
| `Barrier` | everything before `wait` in every thread | everything after `wait` returns in every thread | all-to-all |

Miri's clean runs confirm that std provides these edges *in the executions it explored*. That's a strong sanity check,
not a proof. The proof is the library's documentation and its own reasoning.

**How Miri detects races.** Miri runs your program in an interpreter and tracks, for every thread, a **vector clock**
(one logical timestamp per thread), and for every memory location, the clocks of its last accesses. A Release
operation publishes the releasing thread's clock on the atomic; an Acquire that reads it merges that clock into the
acquirer's. A non-atomic access whose location was last accessed by another thread *at a time the current thread's
clock doesn't cover* is a race, detected on the spot, with both source locations. (Vector-clock race detection is the
same family of algorithms as FastTrack and ThreadSanitizer; Miri's implementation cites Lidbury and Donaldson's
"Dynamic Race Detection for C++11," POPL 2017, as its basis.) Two properties matter in practice:

- **Miri checks the execution it runs**, not all executions. Its scheduler preempts threads at pseudo-random points
  (reproducibly: every Miri output in this Part was identical across re-runs), which finds many races, but a race on a path it didn't explore stays
  hidden. The races in this Part were all found on the first run.
- **Miri treats creating a reference as an access.** Chapter 14.4's spinlock shows Miri reporting a race on a
  "retag write" at the moment a `&mut` is created, before any data is touched, because the compiler may insert
  speculative reads or writes through a reference (the diagnostic says exactly that).

**ThreadSanitizer** (TSan) is the compiled counterpart: fast enough for integration tests, available for Rust on
nightly only. *Not verified here* (needs a nightly toolchain and a C runtime):
`RUSTFLAGS=-Zsanitizer=thread cargo +nightly test -Zbuild-std --target x86_64-unknown-linux-gnu`.

### 5. Memory

**What "the last write that happens-before" means, per location.** For atomics, the modification order is the
per-location history. For the counter in `ch02-02`:

```text
 modification order of `hits`:   0 → 1 → 2 → 3 → … → 3,999,999 → 4,000,000
                                     ↑   ↑   ↑
                  each fetch_add reads the latest value and appends the next one (atomicity)

 thread T1:  fetch_add … fetch_add ─┐ sb         (T1's end)  ─── sw ──►  join returns
 thread T2:  fetch_add … fetch_add ─┘ (etc.)                              │ sb
                                                                           ▼
                                                              hits.load(Relaxed): every increment
                                                              happens-before it → reads 4,000,000
```

**What Relaxed still guarantees, measured** (listing `ch02-07-coherence.rs`). One thread increments a `Relaxed` counter
5 million times while a "scraper" thread reads it in a loop and counts how often a read is *smaller* than the previous
one:

```text
under Miri:     289 scraper reads, 0 went backwards
release build:  60982933 scraper reads, 0 went backwards
```

Read-read coherence [LANG]: a thread's successive reads of one atomic never go backwards in its modification order,
with any ordering. A metrics scraper can rely on that. What it can't rely on is anything about *other* locations,
which is the subject of §9.

### 6. CPU / OS

- **On x86, Release and Acquire are free in hardware** [CPU]: TSO already provides them for ordinary loads and stores.
  What they cost is compiler freedom (Chapter 14.1's table). On Arm they're real instructions (`stlr`/`ldar`).
- **Thread creation and join involve the kernel** [OS]: `clone` and a futex wait. Syscalls and context switches
  involve serializing instructions, so the spawn and join edges are cheap to implement: the hardware work is already
  being done.
- **A lost update is a cache-line race.** In the debug build of `ch02-01`, each `+= 1` was a load, an add, and a store
  to the same line, from four cores. Coherence guarantees each store lands atomically, but between one core's load and
  its store, another core's store can land, and one of them is overwritten. `lock inc` makes the load-add-store a
  single indivisible operation on the line.

---

## Pass 3 · Architect level — *Designing with edges*

### 7. Trade-offs

**How languages treat data races** (the contract, not the hardware):

| Language | A data race is… | Reachable from ordinary code? | Consequence |
|---|---|---|---|
| **Rust** | undefined behavior [LANG] | only through `unsafe` (a wrong `unsafe impl Send/Sync`, raw pointers, `static mut`) | safe code is data-race-free by construction; `unsafe` code must uphold it |
| C / C++ | undefined behavior | yes, anywhere | every shared access is a potential UB bug |
| Java | defined: a racy read returns *some* value written to that variable (JLS §17.4.5, §17.4.8), with no out-of-thin-air results for ordinary code | yes | memory-safe, but racy code can see stale, reordered, or (for `long`/`double`) torn values |
| Go | the 2022 memory model says races on word-sized values behave like Java's; races on multiword values (interfaces, slices, strings) can corrupt memory | yes | mostly safe, with a memory-unsafe corner |

(Go: "The Go Memory Model," revised in 2022 for Go 1.19; Russ Cox's accompanying "Memory Models" essays explain the
design.) Rust chose C++'s rule and made it unreachable from safe code. That's the trade: a data race can't happen by
accident, and when `unsafe` makes one possible, there's no defined fallback behavior to lean on.

**Tools for finding the edges you forgot:**

| Tool | What it checks | Speed | Limit |
|---|---|---|---|
| Miri | races + many weak-memory outcomes + other UB, on the execution it runs | very slow (an interpreter); use small inputs (`cfg!(miri)` in this Part's listings) | one schedule per run; no real syscalls beyond a supported subset |
| TSan | races, in compiled code | ~5-15× slowdown (order of magnitude) | nightly only for Rust; doesn't model weak memory |
| loom | exhaustively explores interleavings *and* weak-memory outcomes of a small test | exponential; tiny tests only | requires writing tests against loom's types (Chapter 14.5) |

### 8. Java comparison

The Java memory model (JLS §17.4) has the same shape, and Java engineers already know most of the edges:

| Java happens-before edge (JLS §17.4.5) | Rust equivalent |
|---|---|
| program order within a thread | sequenced-before |
| monitor unlock → subsequent lock of that monitor | `Mutex` guard drop → next `lock()` |
| volatile write → subsequent volatile read of that field | `SeqCst` (or Release → Acquire) store → load that reads it |
| `Thread.start()` → first action of the started thread | `thread::spawn` edge |
| last action of a thread → `join()` returning | `JoinHandle::join` edge |
| end of a constructor → `final` field freeze | no equivalent needed: a value is fully built before any reference to it exists |

What Java adds is a **definition for racy programs**. JLS §17.7 even spells out the one non-atomic case: writes to
non-volatile `long` and `double` may be split into two 32-bit halves, so a racy reader can see half of each (Chapter
1.1 mentioned this; it's the "torn read"). Rust has no racy-but-defined category: the same program is UB, and the fix,
an atomic, never tears.

> **Analogy limit.** "Java's `String.hashCode()` caches in a plain field; that's a benign race" is true in Java, and
> it's worth knowing *why* it's benign there. The JMM guarantees a racy `int` read returns some value actually written
> (0 or the hash). But the JMM doesn't even guarantee that two racy reads of the same field go forwards in time, which
> is why the JDK's implementation reads the field into a local exactly once. A second read could return 0 after the
> first returned the hash. Even Java's benign race depends on careful code. In Rust, the same idiom is UB outright (§10).

### 9. Production scenario

**Meridian's gateway metrics.** Chapter 4.1 left the gateway with per-request `Cell` counters and process-wide atomics
(the compiler's E0277 pointed the way). The process-wide ones (`requests_total`, `bytes_out_total`, and Chapter 8.3's
`panics_total`) are `AtomicU64`s incremented with `Relaxed` on every request by every worker, and read by a scrape
handler every 15 seconds.

Why `Relaxed` is right for them, stated as edges:

- Each counter is **self-contained**. It doesn't tell a reader that some *other* memory is ready, so there's no
  publication, and no Release/Acquire pair is needed.
- **Coherence** makes each counter's series monotonic for the scraper (verified in `ch02-07`), and RMW atomicity makes
  every increment count.
- At shutdown, the final flush runs after the worker threads are **joined**, so it sees every increment (the `ch02-02`
  argument).

And what `Relaxed` does *not* give, which the team wrote into the metrics guide: **no consistency across counters**.
A scrape reads `errors_total` and `requests_total` at two different instants, and with `Relaxed` there's no promise
that a worker's `requests_total` increment is visible to the scraper just because its later `errors_total` increment is.
That's the MP pattern from Chapter 14.1, harmless on x86 and allowed on Arm and by the language. An error ratio
computed from a single scrape can therefore be slightly off. Dashboards compute rates over windows (`rate()` over a
minute), so it doesn't matter. The one place it would, a per-tenant billing counter pair, uses a `Mutex<(u64, u64)>`
per shard instead: exact consistency, bought only where it's needed.

### 10. Failure scenario

**The "benign race" that Miri refused.** When the fraud team ported its feature library from Java (Chapter 7.2), one
engineer ported a `String.hashCode()`-style cache for merchant keys: a hash stored in a plain field, 0 meaning "not yet
computed" (listing `ch02-04-racy-hash-cache.rs`):

```rust,ignore
pub struct Symbol {
    text: String,
    hash: UnsafeCell<u64>, // 0 = not computed yet (like Java's `hash` field)
}

// SAFETY: WRONG. This claims shared access is fine, but hash() writes `hash` without synchronization.
unsafe impl Sync for Symbol {}

impl Symbol {
    #[inline(never)]
    pub fn hash(&self) -> u64 {
        let h = unsafe { *self.hash.get() }; // racy read
        if h != 0 {
            return h;
        }
        let h = fnv1a(&self.text);
        unsafe { *self.hash.get() = h }; // racy write: "benign" in Java, UB in Rust
        h
    }
}
```

The PR's tests passed. The Miri job that Meridian added to CI after Chapter 4.6's incident did not:

```text
error: Undefined Behavior: Data race detected between (1) non-atomic write on thread `unnamed-1` and (2) non-atomic read on thread `unnamed-2` at alloc373+0x18
  --> src/main.rs:26:26
   |
26 |         let h = unsafe { *self.hash.get() }; // racy read
   |                          ^^^^^^^^^^^^^^^^ (2) just happened here
   |
help: and (1) occurred earlier here
  --> src/main.rs:31:18
   |
31 |         unsafe { *self.hash.get() = h }; // racy write: "benign" in Java, UB in Rust
   |                  ^^^^^^^^^^^^^^^^^^^^
```

The review thread argued "it's benign: every racing thread writes the same value." That argument is about the
*hardware*, and it's roughly true of x86 today. It isn't an argument about the *language*. Because the access is
non-atomic, the compiler may assume no other thread writes `hash` while this function runs. It could, for example,
re-read the field for the `return` instead of reusing `h` (the Java re-read problem from §8), or fold the store into
code that assumes the old value. None of that happens in today's assembly. "Today's assembly is fine" isn't a
correctness argument, and it isn't what `unsafe` code is allowed to rely on.

The fix costs nothing (listing `ch02-05-hash-cache-atomic.rs`, verified clean under Miri): make the field an
`AtomicU64` and use `Relaxed`, because the cached hash is self-contained and every racer writes the same value. The
release assembly of the racy version and of the atomic version of `hash()` are **identical**, instruction for
instruction (verified by diffing the two emitted functions, labels aside):

```text
<playground::Symbol>::hash:                   (labels simplified; comments added)
	mov	rax, qword ptr [rdi + 24]     ; self.hash.load(Relaxed): a plain mov
	test	rax, rax
	je	.LBB_1                        ; not computed yet → hash the text
	ret
	...                                   ; (the FNV-1a loop, identical in both)
.LBB_8:
	mov	qword ptr [rdi + 24], rax     ; self.hash.store(h, Relaxed): a plain mov
	ret
```

The change is entirely in what the compiler is *allowed* to do, not in what it does today. That's the Part XIV version
of Chapter 4.6's lesson: `unsafe` code that works isn't the same as `unsafe` code that's correct, and the gap is
exactly the set of optimizations a future compiler is entitled to make.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XIV).*

1. Define happens-before in terms of sequenced-before and synchronizes-with. Give three ways std creates a
   synchronizes-with edge.
2. What exactly is a data race in Rust's model? How is it different from a race condition (Chapter 1.3's TOCTOU)?
3. Why is a data race undefined behavior rather than "you get some value"? What does the compiler gain?
4. Prove that the `Relaxed` counter in `ch02-02` prints exactly 4,000,000. Which part of the proof comes from the
   ordering, and which from the threading library?
5. What does coherence guarantee for a single atomic, whatever the ordering? What does it *not* guarantee?
6. The racy release build printed the right answer. Explain why, from its assembly, and why that's no comfort.
7. How does Miri detect a data race? What can it miss?
8. Java defines racy programs; Rust doesn't. Give one advantage of each design.

### 12. Exercises

- **Beginner.** For each function in `ch02-03`, draw the happens-before graph from the write of the plain value to its
  read. Mark the synchronizes-with edge.
- **Intermediate.** Change `via_mutex` in `ch02-03` so the writer writes the plain value *after* releasing the lock.
  Predict Miri's verdict, then run it.
- **Advanced.** Write a program where two threads race only on a path taken when an input is odd. Run it under Miri
  with an even input and an odd one. What does this tell you about Miri in CI, and how would you choose Miri's test
  inputs?
- **Systems.** Emit the release assembly of `ch02-01` with `PER_THREAD` read from `std::env::args()` instead of a
  constant. Does LLVM still collapse the loop? What does the race look like now?
- **Architecture.** Your team has 40 `unsafe impl Sync` in a codebase. Design a review checklist that forces each one
  to name its happens-before edges, and a CI setup that tests them.

### 13. Debugging exercise

A request-ID generator is shared by all worker threads:

```rust,ignore
pub struct IdGen { next: UnsafeCell<u64> }
unsafe impl Sync for IdGen {}

impl IdGen {
    pub fn next(&self) -> u64 {
        let id = unsafe { *self.next.get() };
        unsafe { *self.next.get() = id + 1 };
        id
    }
}
```

1. Name the data race precisely: which accesses, which threads, and which happens-before edge is missing.
2. Describe the observable symptom on x86 in a debug build and in a release build. Can two requests get the same ID?
3. Fix it with an atomic. Which method, which ordering, and why is that ordering enough?

### 14. Design exercise

**A config snapshot shared by all gateway workers.** A reloader thread builds a new routing config every 10 minutes
(Chapter 3.1's route table) and must make it visible to 64 worker threads, each of which reads it on every request.
Compare: `Mutex<Arc<Config>>`, `RwLock<Arc<Config>>`, an `AtomicPtr<Config>` you manage yourself, and `arc_swap::ArcSwap`.
For each, name the happens-before edge that publishes the new config, the cost on the worker's hot path, and when the
old config is freed. Pick one, and write the one-paragraph justification you'd put in the code.
