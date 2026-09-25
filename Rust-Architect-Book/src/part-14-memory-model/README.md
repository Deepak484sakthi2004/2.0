# Part XIV — Memory Model and Atomics

> **Part question:** *When two threads share memory, what is each one guaranteed to see, who guarantees it (the
> language, the compiler, or the CPU), and how do you choose the weakest ordering that is still correct, and prove
> that it is?*

Part I promised that "memory ordering deserves more than a paragraph." Chapter 1.3 used `Ordering::Relaxed` for a
shared counter and justified it in two sentences. Chapter 3.3 compared the borrow rules to the cache-coherence
protocol. Chapter 4.1 listed atomics as the one kind of interior mutability the *CPU* enforces. This Part delivers the
full story, from the store buffer in a core up to the formal model the language defines.

It is a senior topic, and it rewards precision. You'll see real hardware produce an outcome that "can't happen"
(two threads each reading the other's flag as zero, 5,321 times in 200,000 rounds on the Playground's AMD EPYC). You'll
see the compiler turn a flag loop into an infinite loop, and a racy counter into a single unlocked `add`. And you'll
see **Miri**, the Rust interpreter, reject programs that print the right answer on x86: publication through a
`Relaxed` counter, a seqlock over plain data, a spinlock without Acquire/Release, a `Relaxed` reference count, and
Java-style `volatile`. Each chapter pairs a hardware-level explanation with the language-level rule that makes your
program portable.

## Chapter map

```text
14.1 Why Memory Ordering Exists     three reorderers: compiler, core (store buffer), memory system
                                     SB litmus observed on real x86 hardware; MP never seen on x86; a flag loop
                                     compiled to `jmp` to itself; coherence ≠ consistency
      │
14.2 Happens-Before                 sequenced-before + synchronizes-with; data race = UB [LANG]; six std
                                     mechanisms checked by Miri's race detector; Relaxed + join (Chapter 1.3's promise);
                                     the "benign" Java race that compiles to identical code as the atomic fix
      │
14.3 Relaxed, Acquire/Release,      what each ordering promises; "publish a buffer via a counter" (the promised
     and SeqCst                      counterexample); weak outcomes exhibited by Miri (MP, SB, IRIW); LLVM IR and
                                     x86 asm for every ordering; measured costs
      │
14.4 CAS, Fences, and Lock-Free     CAS loops (and the try_update rename); spinlock; seqlock done right and wrong;
     Building Blocks                 ABA replayed deterministically; epoch reclamation; fences and Arc's drop;
                                     false sharing measured
      │
14.5 Rust Atomics vs Java volatile  VarHandle modes ↔ Rust orderings; DCL; `read_volatile` is not `volatile`;
     and VarHandle                   LongAdder ↔ striped counters; where the two memory models differ
      │
Part XIV Review                     review a lock-free SPSC ring PR (x86 CI green, Miri red); interview mode
```

## What you'll be able to do after Part XIV

- Explain, for any two-thread interaction, which layer may reorder it (compiler, core, memory system) and which rule
  forbids it.
- State happens-before precisely, and identify the synchronizes-with edge (or its absence) in any design.
- Choose Relaxed, Acquire/Release, or SeqCst from a stated requirement, and name the counterexample that rules out the
  next weaker one.
- Read the x86-64 instructions each ordering produces, and say which costs are hardware and which are lost compiler
  freedom.
- Build and review CAS loops, spinlocks, seqlocks, reference counts, and lock-free stacks, including ABA and memory
  reclamation.
- Translate Java's `volatile`, `AtomicInteger`, `VarHandle` modes, `LongAdder`, and double-checked locking into
  Rust, knowing where each analogy stops being true.
- Use Miri as a routine check for concurrent `unsafe` code, and know what it can't see.

## Listings

`listings/part-14/`: 38 files, 62 checks, all verified on rustc 1.98.1 (edition 2024), including **25 runs under
Miri** (9 that must report a data race, 16 that must be clean, one of those under Tree Borrows) and one nightly check.
Timing listings are single runs on a shared 4-vCPU Playground machine (AMD EPYC 9R14) and are labeled as noisy
wherever they're quoted.
