# Part VII — Generics and Monomorphization

> **Part question:** *When you write `fn process<T>(x: T)`, what does the compiler actually build, how does that
> compare with the erased generics you know from Java, and what does it cost in binary size and build time?*

Part VI, *Traits*, covered the contracts. Part VII covers what the compiler does with them. A Rust generic function
is type-checked **once**, against its bounds, and then **stamped out once per concrete type** that the program uses.
Each copy is an ordinary function with its own symbol and its own optimized machine code. That one decision explains
three things this book has already shown without explaining: Chapter 1.3's iterator chain compiling to the same
assembly as a hand-written loop, Chapter 2.1's "your crate compiles the library's generics", and Chapter 1.4's warning
that generics slow builds down.

For a Java engineer, this Part is a change of mental model. Java compiles one erased body, boxes primitives, inserts
casts, and relies on the JIT to specialize hot paths at run time. Rust specializes everything at build time. The run-time
consequences are no boxing, no casts, and direct inlinable calls from the first request. The build-time consequences are
more code and more compile time. Part VII shows both sides with verified compiler output: symbols, LLVM IR function
counts, per-type assembly, and allocation counts. It ends with the techniques that keep the cost under control.

## Chapter map

```text
7.1 Generics from Call Site to Binary      process::<i32>, ::<String>, ::<MyType> — the conceptual transformation,
                                           observed; bounds checked at the definition (E0369 vs E0277) vs C++
                                           templates; v0 symbols that spell out type arguments; per-type asm; the
                                           iterator chain as 22 debug functions that inline to 2; const generics;
                                           the post-monomorphization exception (E0080)
      │
7.2 Monomorphization vs Java Type Erasure  homogeneous vs heterogeneous translation; checkcast, bridge methods,
                                           boxing (1 allocation vs 1,000,001, measured); what Rust can do with T
                                           that Java can't; the JIT's type profiles, deoptimization, and profile
                                           pollution; C#, Go, C++ on one map; Valhalla, hedged; "erasure-style Rust"
      │
7.3 Code Bloat, Compile Time, and How to   instantiation growth measured (1 → 4 → 16 types, exactly linear);
    Control Them                           the non-generic inner function (88 → 26 debug IR functions); closures
                                           multiply; dyn at cold boundaries; cargo check, crate splits, timings;
                                           return-position impl Trait and edition 2024 capture rules (use<..>)
      │
Part VII Review                            review a generic publisher API: count its instances, decide what stays
                                           generic; interview mode
```

There is no project in this Part. The techniques feed into later projects directly: every project from Level 4 on
decides where its generic boundaries go.

## What you'll be able to do after Part VII

- Predict what instances a program contains and what each instance's code looks like, and check the prediction with
  emitted IR or assembly.
- Explain Rust generics to a JVM team in their terms: erasure, boxing, bridge methods, type profiles, and deoptimization.
  Say exactly where each analogy stops holding.
- Choose between generic parameters, `impl Trait`, `dyn Trait`, enums, and concrete types for each parameter of an API,
  based on whether the type changes the machine code on a hot path.
- Diagnose and reduce compile time and binary size caused by generics, and put a code-size budget into CI.
- Design with return-position `impl Trait` and state its capture contract with `use<..>` in edition 2024.

## Listings

`listings/part-07/`: 27 files, 28 checks, all verified on rustc 1.98.1 (edition 2024, one listing also checked under
edition 2021). Compiler artifacts (symbols, LLVM IR function counts, and per-type assembly) were fetched with
`tools/emit.ps1` and are quoted trimmed.
