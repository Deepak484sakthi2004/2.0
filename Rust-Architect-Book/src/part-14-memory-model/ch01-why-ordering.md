# Chapter 14.1 — Why Memory Ordering Exists: Store Buffers and Reordering

> **Where this sits:** Part XIV · Memory Model and Atomics · chapter 1 of 5
> **Prerequisites:** Chapter 1.3 (the three race fixes, and why `Relaxed` was enough there), Chapter 3.3 (the borrow
> rules as the single-writer/multiple-reader invariant of cache coherence), Chapter 4.1 (atomics in the interior
> mutability table).
> **After this chapter you can:** name the three layers that reorder memory operations (compiler, core, memory
> system); predict the classic litmus outcomes on x86-64 and on Arm; explain why cache coherence doesn't give you
> ordering; and read the assembly that shows a compiler turning a flag loop into an infinite loop.

---

## Pass 1 · User level — *The code-order illusion*

### 1. Problem

Here are two threads and two shared variables, both initially zero:

```text
 Thread A          Thread B
 x = 1             y = 1
 r1 = y            r2 = x
```

Enumerate the interleavings the way you learned to reason about threads: A's two steps and B's two steps, in every
order that respects each thread's program order. Whatever the interleaving, the first write happens before both reads,
so **at least one of `r1`, `r2` must be 1**. The outcome `r1 == 0 && r2 == 0` is impossible.

It isn't. On the Playground's AMD EPYC (x86-64), with `Relaxed` atomics, that outcome happened **5,321 times in
200,000 rounds** (listing `ch01-01-sb-litmus.rs`, verified below). The interleaving model you've used since your first
Java thread is called **sequential consistency** (SC), after Leslie Lamport's 1979 definition: the result is as if all
operations ran in *some* single order that respects each thread's program order. Real machines don't provide SC by
default, and neither do compilers. Each gives you a weaker contract and keeps the difference for performance.

This chapter is about where that difference comes from, down at the hardware and compiler level. Chapter 14.2 then
gives the language-level rule that tames it.

### 2. Mental model

**Three layers may reorder your memory operations.** Each one preserves the illusion of program order *for a single
thread*, and only for a single thread:

```text
 your source code   x = 1; r1 = y;
        │
 1. COMPILER        may reorder, merge, hoist, or delete memory accesses, as long as a SINGLE thread can't tell
        │           the difference ("as-if" rule). It assumes non-atomic memory isn't touched by other threads.
        ▼
 2. CORE            executes out of order; a store waits in a private STORE BUFFER before it reaches the cache,
        │           and a later load to a different address can complete first
        ▼
 3. MEMORY SYSTEM   caches, interconnect, invalidation queues: on some architectures a store reaches other
                    cores at different times (not "multi-copy atomic")
```

**Coherence is not consistency.** Chapter 3.3 compared the borrow rules to the MESI cache-coherence protocol: at any
moment a cache line is either shared by many readers or owned by one writer. That invariant is real, and it's the
reason this chapter's problems exist anyway. Coherence is **per location**. It guarantees that all cores agree on the
order of writes to *one* address. It says nothing about the order in which writes to *two different* addresses become
visible. That second guarantee is called **memory consistency**, and it's what the SB outcome above violates. (The
distinction is standard; Sorin, Hill and Wood's *A Primer on Memory Consistency and Cache Coherence* is the usual
reference.)

**Which reorderings each architecture allows** [CPU], for ordinary loads and stores to ordinary (write-back) memory,
following the published models (x86-TSO: Sewell et al., *CACM* 2010; Armv8: Pulte et al., *POPL* 2018; POWER: Sarkar
et al., *PLDI* 2011):

| A later … may complete before an earlier … | x86-64 (TSO) | Armv8 / AArch64 | POWER |
|---|---|---|---|
| load before load | No | Yes | Yes |
| store before load | No | Yes | Yes |
| store before store | No | Yes | Yes |
| **load before store** (to a different address) | **Yes** (store buffer) | Yes | Yes |
| Stores become visible to all other cores at once | Yes | Yes ("other-multi-copy-atomic") | **No** |

x86 is almost SC. The only reordering it exposes is a store followed by a load of a *different* address: the store
sits in the core's store buffer while the load reads the cache. That's precisely the SB pattern. Arm and POWER allow
all four, and POWER additionally lets different cores see stores in different orders.

**The language sits above all three.** Rust doesn't let you program against "x86" or "Arm." It defines an abstract
model (Chapter 14.2: happens-before; Chapter 14.3: orderings), and the compiler maps it onto each CPU, inserting
exactly the instructions that CPU needs. Your job is to say *what ordering you need*, and the layers below promise not
to reorder across it.

### 3. Rust code

**The store-buffering (SB) litmus test on real hardware** (listing `ch01-01-sb-litmus.rs`, release, verified). Each
round uses a fresh pair of atomics, and a two-thread spin barrier starts both threads at almost the same moment:

```rust,ignore
/// One side of the store-buffering (SB) litmus test: write my flag, then read yours.
fn side(v: Variant, mine: &[AtomicU32], theirs: &[AtomicU32], arrived: &AtomicUsize) -> Vec<u32> {
    let mut seen = vec![0u32; N];
    for i in 0..N {
        // Two-thread spin barrier: both threads start round i at (almost) the same moment.
        arrived.fetch_add(1, SeqCst);
        while arrived.load(SeqCst) < 2 * (i + 1) {
            spin_loop();
        }
        seen[i] = match v {
            Variant::Relaxed => { mine[i].store(1, Relaxed); theirs[i].load(Relaxed) }
            Variant::ReleaseAcquire => { mine[i].store(1, Release); theirs[i].load(Acquire) }
            Variant::SeqCst => { mine[i].store(1, SeqCst); theirs[i].load(SeqCst) }
            Variant::SeqCstFence => { mine[i].store(1, Relaxed); fence(SeqCst); theirs[i].load(Relaxed) }
        };
    }
    seen
}
```

```text
Relaxed                  r1 == r2 == 0 in   5321 of 200000 rounds
Release/Acquire          r1 == r2 == 0 in  14901 of 200000 rounds
SeqCst                   r1 == r2 == 0 in      0 of 200000 rounds
Relaxed + fence(SeqCst)  r1 == r2 == 0 in      0 of 200000 rounds
```

One run; a second run gave 6,953 / 15,018 / 0 / 0. The counts depend on timing, and the zeros don't. `Relaxed` and
`Release`/`Acquire` compile to the *same* plain `mov` instructions on x86 (Chapter 14.3 shows the assembly), so the
difference between the first two rows is noise, not semantics. **Release/Acquire does not forbid the SB outcome.** Only
`SeqCst`, on every access or as a fence between the store and the load, does.

> **What actually happens?** Core A executes `x = 1`: the store enters A's store buffer. A then executes `r1 = y`: the
> load doesn't have to wait for the buffer to drain, so it reads `y` from the cache, where B's store hasn't arrived yet,
> because it's still in *B's* store buffer. B does the mirror image. Both loads read 0, and then both buffers drain.
> Each core sees its *own* store immediately (store-to-load forwarding), which is why x86 is "total store order" and
> not SC. A `SeqCst` store compiles to `xchg`, an implicitly locked instruction that drains the store buffer before
> anything after it executes.

**Message passing (MP) on the same machine** (listing `ch01-02-mp-x86.rs`, release, verified). A writer does
`data = 1; flag = 1`, and a reader does `r1 = flag; r2 = data`. The weak outcome is "saw the flag, missed the data":

```text
flag seen in 190369 of 200000 rounds; flag == 1 but data == 0 in 0
```

Zero, and that's expected on x86: TSO never lets a store overtake an earlier store, or a load overtake an earlier load.
The same test with `Relaxed` on Arm or POWER can produce the weak outcome [CPU] (the published litmus results for those
architectures report it; not verified here, since the Playground is x86-64 only). And the Rust *language* allows it
with `Relaxed` on every architecture: Chapter 14.3 shows Miri producing it 21 times in 40 trials. **A test that passes
on x86 tells you nothing about Arm.**

**The compiler reorders too, even on x86** (listing `ch01-03-hoisted-flag.rs`). Three ways to wait for a flag:

```rust,ignore
/// A plain `&bool`: nothing says another thread may change it, so the compiler may read it ONCE.
#[inline(never)]
pub fn wait_plain(flag: &bool) {
    while !*flag {}
}

/// A `static mut` read by value (allowed in edition 2024; references to it are not): the same problem.
#[inline(never)]
pub fn wait_static_mut() {
    // SAFETY: none, if another thread writes STOP concurrently: that's a data race (UB).
    while !unsafe { STOP } {}
}

/// An atomic, even with Relaxed ordering: every iteration performs a real load.
#[inline(never)]
pub fn wait_atomic(flag: &AtomicBool) {
    while !flag.load(Ordering::Relaxed) {}
}
```

Release assembly (rustc 1.98.1, `tools/emit.ps1 -Target asm -Mode release`; comments added):

```text
playground::wait_plain:
	cmp	byte ptr [rdi], 0         ; read the flag ONCE
	je	.LBB0_1
	ret
.LBB0_1:
	jmp	.LBB0_1                   ; ...then spin forever without reading it again

playground::wait_static_mut:
	cmp	byte ptr [rip + playground::STOP.0], 0
	je	.LBB2_1
	ret
.LBB2_1:
	jmp	.LBB2_1                   ; same: an infinite loop

playground::wait_atomic:
.LBB1_1:
	movzx	eax, byte ptr [rdi]       ; a real load on every iteration
	test	al, al
	je	.LBB1_1
	ret
```

The plain versions aren't miscompiled. The compiler assumed, as it's entitled to, that no other thread writes
non-atomic memory without synchronization, so the value can't change during the loop. The `Relaxed` load is the
cheapest possible fix: the same `movzx` a plain read would use, but the compiler must perform it every time.

---

## Pass 2 · Systems level — *Store buffers, caches, and the optimizer*

### 4. Under the hood

**What the compiler may do to non-atomic accesses** [RUSTC]. LLVM (like GCC, and like the Java JIT for plain fields)
optimizes non-atomic memory under the single-thread as-if rule, because the language says a concurrent unsynchronized
access would be a data race (Chapter 14.2). Transformations you'll meet in concurrent code:

| Transformation | Effect on another thread's view | Seen in this chapter |
|---|---|---|
| Hoisting a load out of a loop | a flag is read once; the loop never sees a change | `wait_plain` → `jmp .LBB0_1` |
| Merging/sinking stores | intermediate values are never written | progress counter below |
| Replacing a loop by a library call | stores happen in a different order and granularity | `memcpy` below |
| Reordering independent accesses | another thread sees them in a different order | (enabled by all of the above) |

The progress-reporting pair in the same listing makes the store side concrete. Copying a slice while publishing
progress through a plain `&mut usize` (release asm, trimmed, comments added):

```text
playground::copy_with_progress_plain:
	...
	call	qword ptr [rip + memcpy@GOTPCREL]    ; the whole copy became one memcpy...
	mov	qword ptr [rbx], r14                  ; ...and progress is stored ONCE, at the end
	...
playground::copy_with_progress_atomic:
.LBB4_4:
	lea	r9, [rsi + 1]
	mov	r10, qword ptr [rdi + 8*rsi]
	mov	qword ptr [rdx + 8*rsi], r10         ; copy element i
	mov	qword ptr [r8], r9                    ; progress.store(i + 1, Relaxed): every one kept
	...
```

A monitoring thread watching the plain counter would see 0, then the final value, never anything in between, and it
would be undefined behavior to look at all. With a `Relaxed` atomic, every store is performed, in order, and the copy
is done element by element as the source says.

**What the compiler does to atomics** [RUSTC]. LLVM treats atomic operations conservatively: it keeps each one, and it
doesn't move non-atomic accesses across an Acquire or Release in the forbidden direction. The language would allow
more (for example, merging two adjacent `Relaxed` increments into one); compilers mostly don't do it (JF Bastien's WG21
paper N4455, "No Sane Compiler Would Optimize Atomics," 2015, discusses what's allowed). Don't write code that depends
on the compiler *not* doing something the model permits.

**`compiler_fence`** constrains only the compiler, not the CPU. Its LLVM form is `fence syncscope("singlethread")
seq_cst`, and on x86 it emits no instruction at all, just a `#MEMBARRIER` marker in the assembly listing (Chapter 14.4).
It's for code that races with *the same thread*, such as a signal handler. It's never a tool for ordering between
threads.

### 5. Memory

**The store buffer, drawn.** Two cores, the SB test, the moment both loads execute:

```text
        Core A                                      Core B
 ┌───────────────────────┐                  ┌───────────────────────┐
 │ executes: x = 1       │                  │ executes: y = 1       │
 │ store buffer: [x = 1] │                  │ store buffer: [y = 1] │
 │ executes: r1 = y  ────┼──► L1: y = 0     │ executes: r2 = x  ────┼──► L1: x = 0
 └───────────────────────┘                  └───────────────────────┘
               │                                        │
               └────────── coherent cache hierarchy ────┘
                     x = 0, y = 0 (neither store has drained yet)
 later: both buffers drain → x = 1, y = 1. Both loads already returned 0.
```

Nothing here violates coherence. Each location has one agreed history (`x`: 0 then 1; `y`: 0 then 1). What's broken is
the *cross-location* intuition that "A's write came before A's read, so anyone who reads after A's read sees A's
write." The store buffer lets a core's later load pass its own earlier store.

**Why store buffers exist.** A store that misses in the cache must obtain the line in the Modified state (a
read-for-ownership, possibly from another core's cache or DRAM), which can take anywhere from tens of cycles to
hundreds of nanoseconds (order of magnitude). Without a buffer, the core would stall on every such store. With one, it
keeps executing and retires the store later. Every high-performance CPU has one. x86 promises that stores leave the
buffer in program order (hence "total store order"). Arm doesn't.

**MESI and the borrow rules, revisited.** Chapter 3.3's analogy holds exactly as far as coherence goes: one line, one
writer at a time, many readers otherwise. The SB result shows where it ends. The hardware enforces single-writer per
*line* and says nothing about ordering *across* lines. Rust's `&mut` enforces single-writer per *value* and, because a
data race can't compile in safe Rust (Chapter 14.2), every cross-thread interaction goes through a type (a lock, a
channel, an atomic with an ordering) that states the ordering it needs.

### 6. CPU / OS

- **x86-64** [CPU]: loads are not reordered with other loads, stores are not reordered with other stores, and loads may
  be reordered with *earlier stores to different locations* (Intel SDM Vol. 3A, "Memory Ordering"). Locked instructions
  (`lock`-prefixed RMWs, `xchg` with memory) have a total order and act as full barriers. `mfence` exists but compilers
  often prefer a locked instruction instead (Chapter 14.4 shows LLVM emitting `lock or` for a `SeqCst` fence).
- **Armv8 / AArch64** [CPU]: any of the four reorderings. Ordering comes from `ldar`/`stlr` (acquire/release loads
  and stores), `dmb` barriers, and a set of dependency rules (an address or data dependency on a loaded value orders
  that load before the dependent access). Meridian runs part of its gateway fleet on Graviton (aarch64) (Chapter 2.1),
  so this isn't academic.
- **The OS hides weak behavior on one core** [OS]. A context switch involves the kernel, which executes serializing
  instructions, so two threads time-sliced on one core behave as if SC. Weak outcomes need true parallelism, which is
  why the SB test needs two cores running at once, and why "it never failed on my laptop" is weak evidence. The
  Playground reports 4 vCPUs (`available_parallelism() = 4`, AMD EPYC 9R14), enough to show it.
- **Speculation.** Out-of-order cores also *speculatively* execute loads early and replay them if the line is
  invalidated before the load retires. On x86 this machinery is what preserves load→load order while still running
  loads out of order. It's invisible to correct programs, which is exactly its job.

---

## Pass 3 · Architect level — *Designing for a machine that reorders*

### 7. Trade-offs

**Why the hardware doesn't just give you SC.** Store buffers, out-of-order loads, and non-blocking caches exist to hide
memory latency. A core that had to make every store globally visible before issuing the next load would stall on
every store miss. The weaker the model, the more a CPU can overlap. x86 kept a strong model (for compatibility with
decades of software) and pays for it in load-ordering machinery. Arm chose a weak model and gives you acquire/release
instructions to buy ordering back where you need it.

**Why the compiler doesn't either.** Register allocation, loop-invariant code motion, vectorization, and store merging
all move or delete memory accesses. You saw two in this chapter: hoisting (`wait_plain`) and loop-to-`memcpy`
(`copy_with_progress_plain`). Forbidding them for all memory would slow down all single-threaded code to protect the
small fraction of accesses that are actually shared.

**The deal the language offers.** Mark the shared accesses (atomics, locks, channels), say what ordering each one
needs, and everything else is optimized freely:

| You write | Compiler | CPU (x86 / Arm) | Cost |
|---|---|---|---|
| non-atomic access to unshared data | anything as-if single-thread | anything | none |
| `Relaxed` atomic | keeps the access, no tearing | plain `mov` / plain `ldr`/`str` | lost compiler freedom only |
| `Acquire` / `Release` atomic | no hoisting/sinking across it (one direction) | `mov` / `ldar`, `stlr` | small on Arm, ~none on x86 |
| `SeqCst` store | as above, both directions | `xchg` / `stlr` (+ `ldar` on loads) | a store-buffer drain on x86 (measured in 14.3) |

The cost column is order-of-magnitude, from the mechanism. Chapter 14.3 measures the x86 rows.

### 8. Java comparison

Java's memory model (JLS §17.4) permits the same reorderings for **plain** fields. The SB outcome is allowed in Java if
`x` and `y` are ordinary `int` fields, and it's forbidden if both are `volatile`, because volatile accesses are
sequentially consistent with each other (JLS §17.4.4: they belong to the single *synchronization order*). The flag loop
is also a classic Java bug. It's the `NoVisibility` example in *Java Concurrency in Practice* (Goetz et al., 2006,
§3.1), where HotSpot's JIT hoists a non-volatile `ready` read out of a loop.

```java
// Not verified here (requires a JDK). A C2-compiled loop may never observe the update.
class Worker {
    boolean stop;            // plain field: the JIT may read it once
    void run() { while (!stop) { /* work */ } }
}
```

| Java | Rust |
|---|---|
| plain field: reorderable, may be hoisted | non-atomic: same, and a concurrent unsynchronized write is UB |
| `volatile` field: SC among volatiles | `SeqCst` atomics (Chapter 14.5 refines this) |
| `jcstress` for litmus tests on the JVM | Miri (the model), plus real-hardware tests like `ch01-01` |

> **Analogy limit.** In Java, the racy flag loop is a *correct program with surprising behavior*: the JLS defines which
> values a racy read may return, and the JIT hoisting is one allowed outcome. In Rust the equivalent program doesn't
> compile without `unsafe` (a `&bool` can't be written while shared; Chapter 14.2 shows the E0499), and with `unsafe`
> it's undefined behavior. There is no "surprising but defined" middle ground to reason about.

### 9. Production scenario

**Meridian's gateway drain handshake.** During a deploy, each gateway pod drains: stop accepting work, let in-flight
requests finish, exit. The Rust gateway (Chapter 2.7's workspace) implements it with two kinds of shared state: a
per-worker `in_flight` flag and a global `draining` flag.

```text
 worker w, per request                      drainer, once
 in_flight[w] = 1                           draining = true
 if draining { reject with 503; ... }       wait until every in_flight[w] == 0, then exit
 ... handle request ...
 in_flight[w] = 0
```

The review question was: which orderings? The first draft used `Release` for the stores and `Acquire` for the loads,
"because that's what publication needs." But this isn't publication. It's the **SB pattern**: each side writes its own
flag and then reads the other's. The dangerous outcome is exactly `r1 == r2 == 0`. The worker reads `draining ==
false` and proceeds, *and* the drainer reads `in_flight[w] == 0` and exits, killing the request mid-flight. The listing
in §3 answers the review question with data: Release/Acquire allowed that outcome 14,901 times in 200,000 rounds on
x86, and SeqCst allowed it 0 times.

The team made all four accesses `SeqCst` and documented why, next to the code: *"SB/Dekker pattern: each side
writes then reads the other's flag; Release/Acquire permits both to read stale values."* The cost is one `xchg` per
request on the worker's `in_flight` store, about 2 ns uncontended (measured in Chapter 14.3). At the gateway's 400K
requests/s across 55 pods, that's noise. The alternative, a per-worker mutex around the check, was rejected as more
code for no benefit.

### 10. Failure scenario

**The reconciliation worker that ignored SIGTERM.** Meridian's nightly settlement-reconciliation worker (a Rust batch
service) checked a stop flag between batches. A signal-handling thread set it on SIGTERM. The flag was a
`static mut STOP: bool`, read by value in the loop, so the edition-2024 `static_mut_refs` lint, which targets
*references* to a `static mut`, never fired. The `unsafe` block was waved through in review as "just a bool."

It worked in every local test. In production, pods never stopped on SIGTERM. Kubernetes waited out the 30-second grace
period and sent SIGKILL, in the middle of writing a batch. Restarts reprocessed batches (the job was idempotent, so
money was safe), but every deploy of the worker took the full grace period, and one killed batch left a partial
report file that a downstream consumer picked up.

The root cause is the `wait_static_mut` assembly in §3: in the release build, the flag is read once before the loop.
Local tests ran debug builds, where the loop re-reads memory every iteration (verified in the debug assembly: `test
byte ptr [rip + playground::STOP], 1` inside the loop). The fix was one type: `static STOP: AtomicBool`, stored and
loaded with `Relaxed`, because the flag says "stop" and publishes nothing else. Had the signal thread also written a
"final checkpoint" for the worker to read, the store would need `Release` and the load `Acquire` (Chapter 14.3).

The broader lesson is Meridian's rule since then: **a `static mut` in a code review is a question, not a detail.** In
edition 2024 it's almost always the wrong tool: use an atomic, a `Mutex`, `OnceLock`, or a thread-local.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XIV).*

1. What is sequential consistency? Give a two-thread program whose SC outcomes you can enumerate, and a real-hardware
   outcome outside that set.
2. What's the difference between cache coherence and memory consistency? Which one does MESI provide?
3. What is a store buffer, why does every fast CPU have one, and which litmus outcome does it produce on x86?
4. Why can the MP weak outcome happen on Arm but not on x86? What does that imply for a test suite that only runs on
   x86?
5. Name three things a compiler may do to non-atomic memory accesses that another thread could observe. What
   assumption licenses them?
6. Why did `wait_plain` compile to an infinite loop, and why is that not a compiler bug?
7. What does `compiler_fence` do, what does it compile to on x86, and when is it the right tool?
8. Why do weak behaviors need two cores running at once, and what does that mean for "we never saw this in testing"?

### 12. Exercises

- **Beginner.** Predict the possible `(r1, r2)` outcomes of the SB test under SC, under x86-TSO, and under the Rust
  model with `Relaxed`. Then change `ch01-01` to use a fence only on *one* side and run it. Explain the result.
- **Intermediate.** Write the load-buffering (LB) test: A does `r1 = y; x = 1`, B does `r2 = x; y = 1`. Which outcome
  is weak? Run it on the Playground with `Relaxed`. Why do you expect zero on x86?
- **Advanced.** In `ch01-01`, remove the spin barrier and let the threads run freely through their arrays. Predict what
  happens to the weak-outcome count, then measure. What does that tell you about how to write litmus tests?
- **Systems.** Emit the release assembly for a function that writes two different `AtomicU64`s with `Release` and then
  reads a third with `Acquire`. Which x86 instructions appear, and which reorderings are still possible in hardware?
- **Architecture.** Your service's CI runs on x86 only, and production is moving 30% of its fleet to aarch64. List
  the kinds of code that could change behavior, how you'd find them (tools, grep patterns, review rules), and what
  you'd add to CI.

### 13. Debugging exercise

A health-check endpoint reports "ready" once initialization finishes. It works in debug builds and hangs forever in
release:

```rust,ignore
static mut READY: bool = false;

fn wait_until_ready() {
    while !unsafe { READY } {
        std::thread::yield_now();
    }
}

fn init() {
    load_config();
    unsafe { READY = true };
}
```

1. Would `yield_now()` in the loop body prevent the hoisting seen in `wait_static_mut`? Reason about what the compiler
   knows about `yield_now`, then check it by emitting the assembly.
2. Even if the loop does re-read `READY`, name the second bug: what can a reader that sees `READY == true` still fail
   to see?
3. Fix both with the smallest change, and say which orderings you need and why.

### 14. Design exercise

**A multi-architecture concurrency test policy for Meridian.** The gateway, the fraud feature library (Chapter 1.4),
and the market-data fan-out all contain hand-written concurrent code. Design a policy: which code needs litmus-style
stress tests on real hardware, which needs Miri, which needs a model checker (Chapter 14.5 mentions loom), which CI
runners (x86, aarch64) run what, and what review rule catches new `static mut`, `unsafe impl Sync`, and non-`SeqCst`
orderings. Estimate the CI cost, and say which single check you'd keep if you could keep only one.
