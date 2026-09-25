# Part XIV Review — The SPSC Ring PR & Interview Mode

> Consolidate Part XIV, then use it: review a lock-free ring buffer whose x86 CI is green and whose Miri run is red,
> and answer senior-level questions without notes. Answers are in **Appendix A, Part XIV**.

---

## Part XIV on one page

```text
 WHO REORDERS         the COMPILER (as-if single thread: hoist, merge, memcpy), the CORE (store buffer: a load may
                      pass an earlier store; on Arm, anything may pass anything), the MEMORY SYSTEM (POWER: not
                      multi-copy atomic). Coherence = one order PER LOCATION; consistency = across locations.
        │
 THE ONE RULE         happens-before = sequenced-before + synchronizes-with (Release → Acquire that reads it; unlock →
                      lock; spawn; join; send → recv; OnceLock; Barrier). Data race (non-atomic, unordered, one
                      write) = UB [LANG]. Safe Rust can't express one; unsafe code must not create one.
        │
 THE ORDERINGS        Relaxed: atomicity + coherence only · Release/Acquire: a pair that publishes · AcqRel: both on
                      one RMW · SeqCst: + one total order (needed for SB/Dekker and IRIW, not for publication)
                      Q1 publication? → Rel/Acq · Q2 consume + hand over in one RMW? → AcqRel · Q3 cross-location
                      agreement? → SeqCst
        │
 x86-64 (verified)    loads: all `mov` · stores: `mov`, SeqCst `xchg` · RMWs: all `lock` · fences: compiler-only
                      except SeqCst (`lock or [rsp-64], 0`). So on x86 a wrong ordering usually still "works."
        │
 BUILDING BLOCKS      CAS loops (lock-free, not wait-free) · spinlock = CAS + Acquire/Release · seqlock = atomics +
                      fences · Arc = fetch_sub(Release) + fence(Acquire) if last · ABA → tags / epochs · reclamation
                      → epochs, hazard pointers, refcounts · false sharing → CachePadded / per-thread state
        │
 JAVA                 volatile ≈ SeqCst · VarHandle opaque ≈ Relaxed, acquire/release = Acquire/Release · plain racy
                      field: defined in Java, UB in Rust · read_volatile is NOT volatile · DCL → OnceLock/LazyLock
        │
 TOOLS                Miri (races + weak outcomes, on the execution it runs) · loom (exhaustive, small tests) ·
                      TSan (fast, nightly) · stress tests on each target architecture
```

## Ten ideas to carry forward

1. **Program order is an illusion kept for one thread at a time.** Three layers reorder, and each is entitled to.
2. **Coherence is per location.** It never orders two different variables, so MESI doesn't give you publication.
3. **Happens-before is the only rule you program against.** Find the synchronizes-with edge, or there isn't one.
4. **A data race is UB, not a stale value.** "The assembly is fine today" is not an argument (Chapter 14.2's racy and
   atomic hash caches compile to identical code, and only one is a correct program).
5. **Relaxed is for values that publish nothing.** Counters, IDs, stop flags, gauges.
6. **Release/Acquire is a pair on one atomic, and the Acquire must read the released value.** Half a pair is none.
7. **SeqCst is for cross-location agreement** (SB, IRIW), not for "being safe."
8. **x86 hides ordering bugs.** Test the model with Miri, and the hardware with Arm runners.
9. **Lock-free code has two extra problems: ABA and reclamation.** Java's GC solves both for Java; in Rust, a library
   (`crossbeam-epoch`, `arc-swap`) or a design without freeing solves them for you.
10. **Contention, not ordering, is what makes shared atomics slow.** Shard, pad, or count locally before you reach for
    lock-free structures.

---

## Capstone: review the SPSC ring PR

Meridian's market-data fan-out (the Rust rewrite of the C++ service from Chapter 1.1) decodes quotes on a
feed-handler thread (Chapter 6.2's `MRDN,12550` format) and hands them to the fan-out thread. A PR replaces the
`crossbeam` channel between the two with a hand-written single-producer/single-consumer ring. The PR description says:
"lock-free, zero allocations per handoff, no fences because x86 is TSO; CI green: 1,000,000 quotes received in order."

Here is the PR (listing `review-spsc-buggy.rs`, verified both ways below):

```rust,ignore
// The PR under review: a single-producer/single-consumer ring for Meridian's market-data fan-out
// (feed-handler thread → fan-out thread). Its x86 CI is green. Find the defects before reading on.
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::thread;

pub struct Ring<T> {
    buf: Box<[UnsafeCell<MaybeUninit<T>>]>,
    head: AtomicUsize, // next slot to read
    tail: AtomicUsize, // next slot to write
}

// x86 is TSO, so Relaxed is fine here and saves the fences.
unsafe impl<T> Sync for Ring<T> {}

impl<T> Ring<T> {
    pub fn with_capacity(cap: usize) -> Self {
        let buf = (0..cap).map(|_| UnsafeCell::new(MaybeUninit::uninit())).collect();
        Ring { buf, head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }

    pub fn push(&self, v: T) -> Result<(), T> {
        let tail = self.tail.load(Relaxed);
        let head = self.head.load(Relaxed);
        if tail - head == self.buf.len() {
            return Err(v);
        }
        unsafe { (*self.buf[tail % self.buf.len()].get()).write(v) };
        self.tail.store(tail + 1, Relaxed);
        Ok(())
    }

    pub fn pop(&self) -> Option<T> {
        let head = self.head.load(Relaxed);
        let tail = self.tail.load(Relaxed);
        if head == tail {
            return None;
        }
        let v = unsafe { (*self.buf[head % self.buf.len()].get()).assume_init_read() };
        self.head.store(head + 1, Relaxed);
        Some(v)
    }
}

const N: u64 = if cfg!(miri) { 50 } else { 1_000_000 };

fn main() {
    let ring = Ring::with_capacity(64);
    let received = thread::scope(|s| {
        s.spawn(|| {
            for seq in 0..N {
                let mut quote = (seq, format!("MRDN,{}", 12_500 + seq % 100));
                while let Err(q) = ring.push(quote) {
                    quote = q;
                    std::hint::spin_loop();
                }
            }
        });
        let mut next = 0;
        while next < N {
            if let Some((seq, _text)) = ring.pop() {
                assert_eq!(seq, next, "out of order");
                next += 1;
            }
        }
        next
    });
    println!("received {received} quotes in order");
}
```

What the CI saw (x86-64, release build):

```text
received 1000000 quotes in order
```

What Miri sees (Meridian's Miri CI job, with `cfg!(miri)` sizes):

```text
error: Undefined Behavior: Data race detected between (1) retag write on thread `unnamed-1` and (2) retag read of type `std::mem::MaybeUninit<(u64, std::string::String)>` on thread `main` at alloc328
  --> src/main.rs:42:26
   |
42 |         let v = unsafe { (*self.buf[head % self.buf.len()].get()).assume_init_read() };
   |                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ (2) just happened here
   |
help: and (1) occurred earlier here
  --> src/main.rs:31:18
   |
31 |         unsafe { (*self.buf[tail % self.buf.len()].get()).write(v) };
   |                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
```

**Your task.** There are at least **eight** defects. For each one:

1. Classify it: **memory model** (a missing happens-before edge), **API soundness** (safe code can cause UB),
   **resources** (leaks, overflow), or **performance**.
2. Say what goes wrong, and *where* it would show up: under Miri, on Arm hardware, after a compiler upgrade, under
   misuse by a future caller, or in a profile.
3. Write the fix.

Then answer four design questions:

- Miri reports only one race. Name the *other* missing handover, the one that goes from the consumer back to the
  producer. What would it corrupt, and when?
- `push` and `pop` take `&self`. What stops two threads from calling `push` at the same time? What should the *types*
  say about "single producer, single consumer"?
- Why does the x86 CI pass, reliably, for a million quotes? Answer at all three layers (compiler, core, memory system).
- The PR removed a `crossbeam` channel. Under Chapter 14.4's review rule, what evidence should the PR have included
  before a hand-written ring was acceptable, and what would you merge instead if the evidence isn't there?

A fixed version (`review-spsc-fixed.rs`, verified clean under Miri, with two unit tests for the full/empty/wrap-around
cases and for dropping unconsumed items) prints the same line natively:

```text
received 1000000 quotes in order
```

Write your review first, then compare with the fixed listing and the model answers.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. What is a data race in Rust's memory model, and why is it undefined behavior rather than "some value"?
2. Define happens-before. Where do synchronizes-with edges come from in std?
3. What does each of the five orderings promise? Which one would you use for a request counter, a published config,
   a lock, a reference count, and a Dekker-style handshake?
4. What is a release sequence, and which std type depends on it?

### Compiler

5. Show two optimizations the compiler performed on non-atomic memory in this Part, and explain why both are legal.
6. What do the orderings become in LLVM IR, and which LLVM ordering has no Rust counterpart? Why does it exist?
7. What do `fence` and `compiler_fence` each constrain? What does each compile to on x86-64?

### Hardware

8. What is a store buffer, and which litmus outcome does it produce on x86? Which outcomes does Arm add?
9. Coherence vs consistency: what does MESI guarantee, and what doesn't it?
10. Why is `xchg` the SeqCst store on x86, and what did it cost on the Playground compared with a plain `mov`?

### Performance

11. What made the shared counter in Chapter 14.4 slow: the ordering or the cache line? What were the three fixes and
    their measured costs?
12. When is a seqlock better than an `RwLock`, and what does it cost the writer and the readers?

### Architecture

13. When is a hand-written lock-free structure justified? What must its PR contain?
14. How do you make sure concurrency code that passes on x86 is correct on Arm?
15. You're porting a Java service that uses `volatile`, `AtomicLong`, `LongAdder`, `synchronized`, and double-checked
    locking. Give the Rust translation of each, and the one Java habit that becomes undefined behavior.

---

## Looking ahead: Part XV

Every listing in this Part that failed under Miri had an `unsafe impl Sync` or an `unsafe` block making a promise it
didn't keep. Part XV makes those promises precise: what `unsafe` means (soundness, safety and validity invariants),
raw pointers and provenance, the Stacked and Tree Borrows aliasing models (the reason `crossbeam-epoch` passed Miri
only under Tree Borrows in Chapter 14.4), `MaybeUninit` (the ring's slots), building a sound `Vec<T>`, and the tools
for verifying `unsafe` code, from Miri to sanitizers and loom.
