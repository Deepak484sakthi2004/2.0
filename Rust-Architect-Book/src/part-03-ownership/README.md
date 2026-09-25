# Part III — Ownership: The Core of Rust

> **Part question:** *For every value in a program: who owns it, who else may use it and how, and exactly when does it
> die? And how do the answers to those three questions eliminate double frees, use-after-free, dangling pointers, and
> data races without a garbage collector?*

Part I argued that ownership is Rust's bet. Part II kept circling it without naming it: `self` receivers, a `Record<'a>`
borrowing from a line buffer, a `Summary` that copied out only what had to outlive that buffer. Part III makes it the
subject, and goes as deep as the rest of the book depends on it going. Every later Part (borrow checking, traits,
concurrency, async, unsafe, FFI, storage engines, compilers) is built on the model you build here.

This Part measures instead of asserting. Several listings install a small **counting allocator** that records every
heap allocation and free. So "a move allocates nothing," "a clone allocates 1,001 times," and "a clean line passes
through with zero allocations" are all **measured** results, reproducible on the Playground.

## Chapter map

```text
3.1 Ownership: One Owner, One Drop     the three rules; the ownership TREE; drop obligations,
                                       drop flags, and drop glue in MIR; the cost of dropping big trees
      │
3.2 Move, Copy, and Clone              what `let b = a;` does at every level; move = memcpy + a compile-time
                                       death certificate; Copy vs Clone; moving out of places
      │
3.3 Borrowing                          & and &mut; aliasing XOR mutation; the optimizer payoff (noalias,
                                       verified in assembly); the cache-coherence parallel; split borrows
      │
3.4 Slices, String, and &str           owned/borrowed pairs; fat pointers; UTF-8 and char boundaries;
                                       Cow: borrow when you can, own when you must
      │
3.5 Drop, RAII, and Determinism        every drop-order rule, verified; guards and temporaries (and what
                                       edition 2024 changed); unwinding; why Drop can't fail
      │
3.6 Ownership for Graphs               Rc/Weak, RefCell, arenas, generational indices, an index-based LRU;
                                       choosing among them
      │
Project Level 2                        `redact`: a streaming secret-masking filter where clean lines cost
                                       ZERO allocations (verified in tests)
      │
Part III Review                        architecture review, interview mode
```

## What you'll be able to do after Part III

- Draw the ownership tree of any data structure and say when each piece is freed and by whom.
- Predict exactly what a move, a copy, and a clone cost, in bytes copied and allocations made, and check it.
- Explain the borrowing rules as *aliasing XOR mutation*, and name what they buy you: memory safety, data-race freedom,
  and optimizations C compilers can't make.
- Work with text correctly: UTF-8, char boundaries, and when to borrow, own, or `Cow`.
- Design RAII guards that are correct on every exit path, and know the three ways destructors *don't* run.
- Choose, for a graph-shaped problem, among `Rc`/`Weak`, arenas, generational indices, and index-linked structures,
  with reasons.

## Listings

`listings/part-03/`, verified against rustc 1.98.1 (edition 2024; several listings are also checked against edition
2021 to show exactly what edition 2024 changed). Compiler artifacts come from `tools/emit.ps1`.
