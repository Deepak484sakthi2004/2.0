# Chapter 14.5 — Rust Atomics vs Java volatile and VarHandle

> **Where this sits:** Part XIV · Memory Model and Atomics · chapter 5 of 5
> **Prerequisites:** Chapters 14.1-14.4. Java's memory model at the level of *Java Concurrency in Practice*.
> **After this chapter you can:** translate Java's `volatile`, `synchronized`, `AtomicInteger`, `VarHandle` access modes,
> `LongAdder`, `final` fields, and double-checked locking into Rust; say exactly where each translation stops being
> faithful; avoid the two classic porting mistakes (`read_volatile` as `volatile`, and "benign" races); and choose
> testing tools on each side (jcstress; Miri and loom).

---

## Pass 1 · User level — *A mapping, with warnings attached*

### 1. Problem

You already know one memory model well. The Java Memory Model (JSR-133, Java 5, 2004; JLS §17.4) has happens-before,
volatile, monitors, and final-field semantics, and JDK 9 added `VarHandle` access modes weaker than `volatile`. Most
of it maps onto Rust cleanly, which is the danger. The parts that *don't* map are exactly the ones a Java engineer
won't think to question: the word `volatile` means something unrelated in Rust, and racy code that is merely
surprising in Java is undefined behavior in Rust.

The SPEC for this Part asks to compare Rust atomics with `volatile`, `AtomicInteger`, `synchronized`, and `VarHandle`.
This chapter does it as a translation table, then tests the risky translations under Miri.

### 2. Mental model

**The mapping** (Java left, Rust right; the orderings are Chapter 14.3's):

| Java | Rust | Faithful? |
|---|---|---|
| plain field, raced | *no safe equivalent*; `UnsafeCell` + `unsafe impl Sync` = UB if raced | **No** (§8) |
| `VarHandle` **plain** mode (`get`/`set`) | non-atomic access (unshared data only) | as long as nothing races |
| `VarHandle` **opaque** (`getOpaque`/`setOpaque`) | `Relaxed` load/store | close: both atomic and coherent per variable |
| `VarHandle` **acquire/release** (`getAcquire`/`setRelease`), `AtomicX.lazySet` | `Acquire` load / `Release` store | yes |
| `volatile` field, `VarHandle` **volatile** mode, `AtomicX.get`/`set` | `SeqCst` load/store | yes, for the variable itself |
| `AtomicInteger.incrementAndGet`, `getAndAdd` | `fetch_add(1, SeqCst)` (often `Relaxed` is what you meant) | yes |
| `compareAndSet` / `weakCompareAndSet{Plain,Acquire,Release,}` | `compare_exchange` / `compare_exchange_weak` with matching orderings | yes |
| `VarHandle.fullFence` / `acquireFence` / `releaseFence` | `fence(SeqCst)` / `fence(Acquire)` / `fence(Release)` | yes |
| `VarHandle.loadLoadFence` / `storeStoreFence` | no direct equivalent (use the stronger Acquire / Release fence) | Rust is coarser |
| `synchronized` (monitor enter/exit) | `Mutex::lock` (Acquire) / guard drop (Release), Chapter 11.3 | mostly (reentrancy differs) |
| `final` field freeze (JLS §17.5) | not needed: no value can be shared before it's fully built | different mechanism |
| `Thread.start` / `join` | `thread::spawn` / `join` (Chapter 14.2's edges) | yes |
| `LongAdder` | striped, cache-padded counters (listing below) | yes |
| lazy holder idiom, double-checked locking | `OnceLock`, `LazyLock` | yes, and simpler |
| `ThreadLocal<T>` | `thread_local!` | yes |

Doug Lea's guide to the JDK 9 modes ("Using JDK 9 Memory Order Modes") draws the same correspondence from the Java
side: Opaque as the counterpart of C/C++ "relaxed," Release/Acquire as theirs, Volatile as sequential consistency.

**And the word that doesn't map:** Rust's `std::ptr::read_volatile` / `write_volatile` are **not** Java's `volatile`.
They exist for memory-mapped I/O: they tell the compiler not to elide, merge, or reorder the access *with respect to
other volatile accesses*, but they're not atomic and create no happens-before edges. Using them for thread
communication is a data race (§3 runs it under Miri).

### 3. Rust code

**Double-checked locking, done the Java 5 way** (listing `ch05-01-dcl.rs`, verified under Miri and natively):

```rust,ignore
static MODEL: AtomicPtr<Model> = AtomicPtr::new(ptr::null_mut());
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Hand-rolled DCL. Acquire on the fast path pairs with the Release publication below.
pub fn model() -> &'static Model {
    let p = MODEL.load(Acquire); // first check, no lock
    if !p.is_null() {
        return unsafe { &*p }; // SAFETY: published with Release, never freed (lives for 'static)
    }
    let _guard = INIT_LOCK.lock().unwrap();
    let p = MODEL.load(Acquire); // second check, under the lock
    if !p.is_null() {
        return unsafe { &*p };
    }
    let p = Box::into_raw(Box::new(load_model())); // build it fully...
    MODEL.store(p, Release); // ...then publish the pointer: the writes above happen-before any Acquire that sees p
    unsafe { &*p }
}

/// The same thing, in one line of std.
pub fn model_once() -> &'static Model {
    static M: OnceLock<Model> = OnceLock::new();
    M.get_or_init(load_model)
}
```

```text
DCL sum = 1, OnceLock version = 7; all threads agree: true
```

The hand-rolled version is correct. It's also 15 lines of `unsafe` reasoning that `OnceLock` (or `LazyLock` for a
`static`) replaces with one line. The history is the argument: "The 'Double-Checked Locking is Broken' Declaration"
(around 2000, signed by David Bacon, Joshua Bloch, Doug Lea, Bill Pugh and others) showed that the pre-Java-5 idiom
couldn't be made correct at all, and JSR-133 fixed it by giving `volatile` acquire/release semantics.

**The pre-Java-5 version, ported** (listing `ch05-02-dcl-relaxed.rs`): the pointer is atomic but `Relaxed`, the
equivalent of the missing `volatile`. A request thread spins on the fast path until the model "is there," then uses
it. Miri:

```text
error: Undefined Behavior: Data race detected between (1) retag write on thread `unnamed-1` and (2) retag read of type `Model` on thread `unnamed-2` at alloc3211
   --> /playground/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/src/rust/library/core/src/ptr/mut_ptr.rs:266:57
    |
266 |         if self.is_null() { None } else { unsafe { Some(&*self) } }
    |                                                         ^^^^^^ (2) just happened here
    |
help: and (1) occurred earlier here
   --> src/main.rs:25:17
    |
 25 |         let p = Box::into_raw(Box::new(Model { weights: vec![0.25, 0.5, 0.25] }));
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
```

The reader saw the pointer, but the construction of the `Model` it points to doesn't happen-before the reader's use.
It's exactly the broken Java idiom, with a stronger penalty: in Java the reader might see default field values; in
Rust the program has no defined behavior.

**`read_volatile` is not `volatile`** (listing `ch05-03-volatile-not-atomic.rs`):

```rust
use std::cell::UnsafeCell;
use std::ptr;
use std::thread;

struct Mailbox {
    ready: UnsafeCell<bool>,
    payload: UnsafeCell<u64>,
}

// SAFETY: WRONG. Nothing here synchronizes the two threads.
unsafe impl Sync for Mailbox {}

fn main() {
    let m = Mailbox { ready: UnsafeCell::new(false), payload: UnsafeCell::new(0) };
    let m = &m; // move closures below capture this reference (not the individual UnsafeCell fields)
    let got = thread::scope(|s| {
        s.spawn(move || unsafe {
            *m.payload.get() = 42;
            ptr::write_volatile(m.ready.get(), true); // "volatile store", Java-style... but not in Rust
        });
        s.spawn(move || unsafe {
            while !ptr::read_volatile(m.ready.get()) {
                std::hint::spin_loop();
            }
            *m.payload.get()
        })
        .join()
        .unwrap()
    });
    println!("payload = {got}");
}
```

```text
error: Undefined Behavior: Data race detected between (1) non-atomic write on thread `unnamed-1` and (2) non-atomic read on thread `unnamed-2` at alloc214+0x8
  --> src/main.rs:25:20
   |
25 |             while !ptr::read_volatile(m.ready.get()) {
   |                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ (2) just happened here
   |
help: and (1) occurred earlier here
  --> src/main.rs:22:13
   |
22 |             ptr::write_volatile(m.ready.get(), true); // "volatile store", Java-style... but not in Rust
   |             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
```

Miri reports the race on the *flag itself*. Volatile accesses are non-atomic accesses the compiler promises not to
optimize away. They race like any other. The fix is `AtomicBool` with `Release`/`Acquire` (Chapter 14.3's
publication pattern).

**`LongAdder`, in Rust** (listing `ch05-04-striped-counter.rs`):

```rust,ignore
pub struct StripedCounter {
    cells: Box<[CachePadded<AtomicU64>]>,
}

impl StripedCounter {
    pub fn add(&self, n: u64) {
        let i = THREAD_INDEX.with(|i| *i) % self.cells.len();
        self.cells[i].fetch_add(n, Relaxed);
    }
    /// Sums the cells one by one: concurrent adds may or may not be included.
    pub fn sum(&self) -> u64 {
        self.cells.iter().map(|c| c.load(Relaxed)).sum()
    }
}
```

```text
single AtomicU64: 20000000 in 86 ms
striped (8 cells): 20000000 in 10 ms
sums read during the run: [2669873, 8691589, 8692975]
```

(Four threads, 5 million adds each, release build, one run.) About 8× faster, for the reason Chapter 14.4 measured:
each thread's increments stay in a cache line it owns. And `sum()` has `LongAdder.sum()`'s documented weakness, which
the javadoc states directly: it's not an atomic snapshot, and concurrent updates may or may not be included. The three
mid-run sums are each "somewhere in between." `LongAdder` also grows its cell array when it detects CAS contention.
This version assigns each thread a fixed stripe instead, which is simpler and good enough when threads are few and
long-lived.

---

## Pass 2 · Systems level — *Same hardware, different contracts*

### 4. Under the hood

**Both languages compile to the same instructions.** The x86-64 column for Rust is verified (Chapter 14.3). The Java
column describes HotSpot's C2 output as documented by the OpenJDK sources and commonly observed with
`-XX:+PrintAssembly`. *Not verified here*: that requires a JDK with the `hsdis` disassembler.

| Operation | Rust (verified) | Java / HotSpot C2 (not verified here) |
|---|---|---|
| volatile / SeqCst load | `mov` | `mov` |
| volatile / SeqCst store | `xchg` | `mov` + `lock addl` to a stack slot (a StoreLoad barrier) |
| Release store / Acquire load | `mov` / `mov` | `mov` / `mov` |
| `incrementAndGet` / `fetch_add` | `lock xadd` | `lock xadd` |
| `compareAndSet` / `compare_exchange` | `lock cmpxchg` | `lock cmpxchg` |
| full fence | `lock or dword ptr [rsp - 64], 0` | `lock addl` to a stack slot |

The shared idea is a locked no-op on the stack instead of `mfence`, and it isn't a coincidence. Both compilers are
choosing the cheapest full barrier on the same hardware. The contracts differ above the instructions, in what the
compiler may assume about everything else.

**What the JVM guarantees that LLVM doesn't** [RUNTIME]. HotSpot must implement the JMM for *every* field access,
including racy plain ones. It must never produce a torn `int` or reference, never invent a value out of thin air, and
never let a racy plain read crash the VM. LLVM even has an ordering for exactly this, `unordered`, which its Language
Reference says is "intended to provide a guarantee strong enough to model Java's non-volatile shared variables."
Rust exposes no way to request it. Non-atomic accesses in Rust are LLVM's plain accesses, with no guarantees under
races. So the Java engineer's "worst case for a racy read is a stale value" has no Rust counterpart.

**Why Rust doesn't need final-field semantics.** Java's `final` fields get a special guarantee (JLS §17.5): an object
published *through a data race* still shows its `final` fields' constructed values to other threads. That rule exists
because Java lets you publish a reference racily. Rust doesn't. A reference to a value can only reach another thread
through `Send`/`Sync` types and a synchronizing operation (spawn, a channel, a lock, a Release/Acquire pair), and a
value can't be referenced at all before it's fully constructed. The problem final fields solve can't be expressed.

### 5. Memory

**Publishing an object, in both languages:**

```text
 Java (unsafe publication — legal, surprising)        Rust (no unsafe publication in safe code)
 ─────────────────────────────────────────────        ─────────────────────────────────────────
 shared = new Model(weights);   // plain field        let m = Arc::new(Model { weights });
   → another thread may see shared != null            tx.send(m.clone())          // channel: send → recv edge
     and weights == null (non-final field),           or: current.store(m)        // ArcSwap: Release/Acquire
     or a fully built object (final fields)           or: MODEL.set(m)            // OnceLock: init → get edge
                                                      A racy path requires `unsafe` — and is UB.
```

Java makes safe publication a discipline (JCIP §3.5 lists the safe idioms). Rust makes it the only thing the type
system lets you write, unless you write `unsafe`.

### 6. CPU / OS

The hardware is identical: the same store buffers, the same cache lines, the same false sharing. A Java `AtomicLong`
hammered by 64 threads bounces its cache line exactly like a Rust `AtomicU64`, which is why `LongAdder` exists in
the JDK. `@Contended` (JDK 8's JEP 142) pads fields to avoid false sharing, the JVM's version of `CachePadded`. The
performance intuitions you built on the JVM transfer directly. Only the correctness contract changes.

---

## Pass 3 · Architect level — *Two memory models, one set of trade-offs*

### 7. Trade-offs

**Where the two models genuinely differ:**

| Question | Java (JMM, JLS §17) | Rust (C++20 model) |
|---|---|---|
| What is a racy plain read? | defined: returns some value written to that field (except `long`/`double` tearing, JLS §17.7) | UB |
| Are plain accesses coherent? | no (a thread may see a newer value, then an older one) | not applicable: races are UB; `Relaxed` is coherent |
| Default strength of atomics | sequentially consistent (`volatile`, `AtomicX.get/set`) | you choose on every call |
| Out-of-thin-air values | forbidden by the JMM's causality rules (which are notoriously hard to reason about) | not formally ruled out for `Relaxed` by the C++ model, which only discourages them in a note; no real implementation produces them (Boehm & Demsky, "Outlawing Ghosts," MSPC 2014, discusses the open problem) |
| Safe publication | a discipline (final fields, volatile, locks) | enforced by `Send`/`Sync` and the absence of races |
| Mutex reentrancy | monitors are reentrant | `std::sync::Mutex` isn't: re-locking on the same thread deadlocks or panics (std docs) |
| Reclaiming lock-free nodes | the GC | your job (Chapter 14.4) |

The practical upshot: Java optimizes for **racy programs failing gracefully**, and Rust optimizes for **racy programs
not compiling**. Both are defensible. Rust's choice means the compiler never has to preserve anything for racy code,
and the cost is that the "benign" Java idioms (racy hash caching, unsafe publication of immutable objects) need atomics
in Rust. With `Relaxed`, that cost is zero instructions on x86 (Chapter 14.2's identical assembly).

**Testing tools on each side:**

| Need | Java | Rust |
|---|---|---|
| Litmus tests on real hardware | jcstress (OpenJDK) | hand-written stress tests like `ch01-01`, run on each target architecture |
| Explore interleavings and weak outcomes of a small test | jcstress (partially, by brute force) | **loom** (a model checker, after CDSChecker, Norris & Demsky 2013) |
| Detect races in larger runs | (races aren't UB; tools like RacerD are static) | Miri (slow, exact for the explored execution), TSan (fast, nightly) |

**loom**, *not verified here* (not on the Playground): you write tests against `loom::sync::atomic` and
`loom::thread`, and loom runs the test body under every interleaving (and many weak-memory outcomes) the model allows,
up to a bound. The usual setup is a `cfg(loom)` switch between `std` and `loom` types, run with
`RUSTFLAGS="--cfg loom" cargo test --release`. For a small lock-free component, it's the closest thing to a proof you
can get from a test.

### 8. Java comparison

This whole chapter is the comparison. Here, then, are the three **analogy limits** that matter most, collected:

> **Analogy limit 1: `volatile`.** Java's `volatile` is a thread-communication tool. Rust's `read_volatile` and
> `write_volatile` are an I/O tool with no thread semantics (§3, Miri-verified). The Java concept maps to Rust's
> atomics, never to Rust's "volatile."

> **Analogy limit 2: `synchronized`.** Monitor enter/exit map to `Mutex` lock/unlock for ordering. But Java monitors
> are reentrant and attached to every object, while Rust's `Mutex<T>` owns its data, isn't reentrant, and can be
> poisoned by a panic (Chapter 8.3). Porting a Java class that calls one `synchronized` method from another deadlocks
> in Rust unless you restructure it (Chapter 11.3).

> **Analogy limit 3: Opaque ≈ Relaxed.** Both are atomic, coherent, and unordered with respect to other variables. But
> Java's plain mode, one step weaker, has no Rust counterpart at all. Code that "worked with plain fields" in Java has
> to become `Relaxed` atomics in Rust, not `UnsafeCell`.

### 9. Production scenario

**Porting the fraud library's concurrency.** The fraud feature library (Chapter 1.4: 50K scores/s, p99 under 5 ms,
called from Java through FFM) is moving its shared state from the Java side into Rust. The team's translation sheet,
now part of Meridian's porting guide:

| Java original | Rust port | Why |
|---|---|---|
| `volatile Model model` + double-checked locking in `getModel()` | `ArcSwap<Model>` (the model is reloaded hourly) | reloadable publication plus reclamation (Chapter 14.3) |
| `static final Map<..> RULES` built in a static initializer | `LazyLock<HashMap<..>>` | one-time init with an init → get edge |
| `LongAdder scoresTotal`, `LongAdder timeouts` | per-worker `CachePadded` counters, summed on scrape | same design as `LongAdder`, fixed stripes (listing `ch05-04`) |
| `AtomicLong lastModelLoadMillis` read by a health check | `AtomicU64` with `Relaxed` | self-contained value; publishes nothing |
| `ConcurrentHashMap<String, double[]> featureCache` | a sharded `Mutex<HashMap<..>>` (Chapter 9.3's options) | per-shard locks, no lock-free map needed |
| `synchronized` block around a rare cache refresh | `Mutex` | direct translation; checked for reentrancy |

The review found nothing to hand-roll: every row maps onto a std or ecosystem type whose orderings were already
reviewed. That's the goal of the sheet. The ordering decisions in the port are the ones in the "Why" column, stated
as requirements, not as `Ordering::` choices.

### 10. Failure scenario

**The config flag that was "volatile."** In the same port, a model-reload flag was translated literally. The Java
original was `volatile boolean reloadRequested`, set by an admin endpoint and polled by the scoring threads, which
then read the new thresholds the endpoint had stored just before. The Rust port used `std::ptr::write_volatile` and
`read_volatile` on a `bool` inside an `UnsafeCell`, with a comment: "volatile, as in the Java version." That's
`ch05-03`'s pattern exactly.

It passed code review, because "volatile" read as correct to a Java-trained eye, and it passed every test on x86. The
Miri job (Chapter 14.2's `ch02-04` incident had made Miri mandatory for any `unsafe impl Sync`) failed with the data
race shown in §3. Without Miri, the failure mode would have been the one Chapter 14.3's log shipper hit: the flag
visible before the thresholds it announces, on an Arm host, or after a compiler upgrade that merged or hoisted the
volatile reads relative to the non-volatile threshold reads.

The fix was `AtomicBool` with `store(true, Release)` and `load(Acquire)`, which is what Java's `volatile` actually
provides (Java's is even stronger: SeqCst). The porting guide gained a line at the top: **"Java `volatile` → Rust
atomic. Never `read_volatile`/`write_volatile`, which are for memory-mapped hardware."** The engineer who wrote the port
added a lint to the workspace: a `clippy.toml` `disallowed-methods` entry for `std::ptr::read_volatile` and
`std::ptr::write_volatile`, with a pointer to this chapter.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XIV).*

1. Map Java's four `VarHandle` access modes onto Rust orderings. Where is the mapping inexact?
2. Why is Rust's `read_volatile` not Java's `volatile`? What *is* it for?
3. Why did double-checked locking break before Java 5, and what's the Rust equivalent of the fix? What should you use
   instead?
4. Java has final-field semantics; Rust doesn't. Why doesn't Rust need them?
5. Java racy reads are defined, and Rust's are UB. What does each design gain and lose?
6. How does `LongAdder` avoid contention, and what does its `sum()` not guarantee? How would you build it in Rust?
7. What are jcstress, loom, and Miri each for? Which would you use to test a new lock-free queue in each language?
8. A Java class calls `synchronized` method `b()` from `synchronized` method `a()`. What happens if you port it
   naively to one `Mutex` in Rust?

### 12. Exercises

- **Beginner.** Translate each row of the §2 table into a one-line Rust snippet, and check each with the Playground.
- **Intermediate.** Replace the hand-rolled DCL in `ch05-01` with `LazyLock`, and compare the number of `unsafe`
  blocks, lines, and `Ordering` arguments.
- **Advanced.** Make `ch05-04`'s `StripedCounter` grow like `LongAdder`: start with one cell, and when a
  `compare_exchange` on a cell fails, move the thread to another cell and double the cell count (up to the number of
  CPUs). What reclamation problem does growing the array create, and how does `LongAdder` avoid it?
- **Systems.** Write the same message-passing program in Java (plain fields, then `volatile`, then
  `setRelease`/`getAcquire`) and run it under jcstress on an Arm machine, then the Rust version under Miri. Compare
  the outcomes each tool reports. (Not verifiable on the Playground; requires a JDK and an Arm host.)
- **Architecture.** Write the porting guide's section on concurrency for a team moving a Java service to Rust: a
  translation table, a list of banned patterns, and the review questions for each `unsafe impl Sync`.

### 13. Debugging exercise

A port of Java's `String.intern()`-style cache:

```rust,ignore
pub struct Interner {
    map: Mutex<HashMap<String, &'static str>>,
    hits: UnsafeCell<u64>, // "just statistics, like a Java plain long field"
}
unsafe impl Sync for Interner {}

impl Interner {
    pub fn intern(&self, s: &str) -> &'static str {
        let mut map = self.map.lock().unwrap();
        if let Some(v) = map.get(s) {
            unsafe { *self.hits.get() += 1 };
            return v;
        }
        let leaked: &'static str = Box::leak(s.to_owned().into_boxed_str());
        map.insert(s.to_owned(), leaked);
        leaked
    }
    pub fn hits(&self) -> u64 { unsafe { *self.hits.get() } }
}
```

1. The `hits` increment happens under the mutex. Is it a data race? Look carefully at *every* access to `hits`.
2. What would the Java version's behavior be for the same access pattern, and why is the Rust one worse?
3. Fix it two ways (inside the mutex's data; as an atomic), and say which ordering the atomic needs.
4. Chapter 4.3 had an opinion about `Box::leak` per call. Does it apply here? Why or why not?

### 14. Design exercise

**Meridian's porting guide, concurrency section.** Design the one-page guide that every Java team uses when moving
code to Rust: which Java constructs map to which Rust types by default, which need a design review, which are banned,
and which tools (Miri, loom, stress tests on aarch64) are mandatory for which kinds of code. Use this Part's
listings as the evidence for each rule, and state which rule you'd drop first if teams found the guide too heavy.
