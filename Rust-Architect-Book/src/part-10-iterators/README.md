# Part X — Closures, Iterators, and Zero-Cost Abstractions

> **Part question:** *When you write `data.iter().filter(..).map(..).collect()`, what exactly have you built, what does
> it compile to, when is it as fast as the loop you'd have written by hand, and when isn't it? And what changes when a
> Java engineer's stream habits are ported onto it?*

Chapter 1.3 made a promise with one piece of assembly: an iterator chain and a hand-written loop compiled to the same
machine code. Chapter 7.1 explained half of it (monomorphization turns 22 debug functions into 2). Part X finishes the
job, from the bottom up.

It starts with **closures**, because every adapter in an iterator chain is a struct holding a closure. You'll see a
closure as the compiler does: an anonymous struct of captures, with a call trait chosen from what its body does, lowered
to MIR as a field projection per captured variable. Then comes the **`Iterator` trait**: one required method, laziness,
the three ownership forms, and the one thing it can't express (items that borrow from the iterator), which generic
associated types fix. Then the chain is **taken apart and compiled**, adapter by adapter, with the assembly of the two
paths every chain has (`next()` and `fold()`), measurements of where "zero-cost" holds and where it fails by 20×, and
the in-place `collect` that can keep memory you thought you'd freed. Finally, **Java streams** get a line-by-line
translation and a list of the semantic differences that turn faithful ports into incidents.

## Chapter map

```text
10.1 Closures: Fn, FnMut, FnOnce, and Capture     closure = struct of captures (sizes measured); capture per PLACE;
                                                  trait from the body; MIR of a closure; generic vs dyn vs fn-pointer
                                                  asm (vtable decoded); log-sampler rules compiled to closures
      │
10.2 The Iterator Trait and Laziness              pull-based and vertical (observed); iter/iter_mut/into_iter;
                                                  size_hint, FusedIterator; a zero-copy frame iterator; lending
                                                  iterators: E0207 with std, zero allocations with a GAT; `gen`
      │
10.3 How an Iterator Chain Compiles               the type IS the pipeline; next() vs fold() in asm (scalar vs two
                                                  vectorized loops); dyn Iterator ~20×; bounds checks; in-place
                                                  collect measured; debug vs release
      │
10.4 Rust Iterators vs Java Streams               push vs pull; collect into Result; try_fold; Collectors translated;
                                                  reuse; Gatherers; Rayon vs parallelStream; the toMap duplicate-key
                                                  port that stopped failing
      │
Part X Review                                     a settlement-report PR with 14 defects; interview mode
```

## What you'll be able to do after Part X

- Read a closure's size, capture modes, and call traits off its source, and design callback APIs that ask for the
  weakest trait they need.
- Explain an iterator chain's machine code from its types, and predict when a consumer's `fold` path beats a `for` loop.
- Find the four places where iterator code isn't zero-cost in practice (`dyn` in hot loops, mid-pipeline `collect`,
  `next`-driven `chain`/`flatten`, debug builds), and fix each without giving up iterators.
- Write custom iterators with honest `size_hint`s, errors as items, and fused behavior, and know when you need a lending
  iterator instead.
- Port Java stream code by its semantics: duplicate keys, error policy, reuse, ordering, and floating-point
  reproducibility.

## Listings

`listings/part-10/`: 58 files, 70 checks, all verified on rustc 1.98.1 (edition 2024), including edition 2018/2021
comparisons, one nightly check (`gen` blocks), and one run under **Miri**. Compiler artifacts (closure MIR; release
assembly of generic/`dyn`/function-pointer calls, of `next()` versus `fold()` over `chain`, of `dyn Iterator`, and of
three bounds-check strategies) come from `tools/emit.ps1`. Timings are best-of-N runs on the shared Playground machine
and are labeled as one run, noisy.
