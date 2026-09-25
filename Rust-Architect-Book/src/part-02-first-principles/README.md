# Part II — Rust From First Principles

> **Part question:** *What is a Rust program physically, from the files on disk to the bytes in the binary? And how
> do the basic language constructs (bindings, types, expressions, patterns, structs, enums, modules) map onto what the
> compiler actually generates?*

You already know how to program, so this Part doesn't teach programming. It teaches **Rust's specific answers** to
questions every language has to answer: what a unit of compilation is, what a variable is, what a function call costs,
how data is laid out, and where privacy is enforced. Each answer is followed down into the compiler. You'll read real
MIR, LLVM IR, and x86-64 assembly produced by rustc 1.98.1, and see what `let x = 10;` actually turns into in a debug
build versus a release build.

Syntax you already know from other languages is compressed. The parts that are different in Rust get the space:
expression orientation, exhaustive patterns, enums as tagged unions, explicit `self` receivers, and module privacy as
the boundary that keeps invariants intact.

## Chapter map

```text
2.1 The Toolchain                rustup → cargo → rustc; crates, packages, editions, targets;
                                 what an .rlib is; setting up on a disk-constrained Windows machine
      │
2.2 Cargo.toml & Profiles        dependencies, semver, features, build profiles: how configuration
                                 decides what code exists in the binary and how it fails
      │
2.3 Bindings & Scalars           does `x` exist at run time? MIR → LLVM IR → assembly, debug vs release;
                                 inference; integer/float semantics; `as` vs TryFrom
      │
2.4 Compound Types & Functions   expressions vs statements, tuples, arrays, `()`, `!`;
                                 how values are passed and returned (registers, hidden pointers)
      │
2.5 Control Flow & Patterns      exhaustiveness as a proof; match → jump tables and lookup tables;
                                 slice patterns, let-else, if-let chains
      │
2.6 Structs, Enums & Methods     product and sum types, `self` receivers as ownership modes,
                                 layout (reordering, niches), what #[derive] generates
      │
2.7 Modules & Crate Architecture privacy as the invariant boundary; facades; crate graphs that
                                 enforce layering
      │
Project Level 1                  `logstat`: a production-grade CLI log analyzer with bounded memory,
                                 streaming input, exit codes, broken-pipe handling, and tests
      │
Part II Review                   architecture review of `logstat`, interview mode
```

## What you'll be able to do after Part II

- Explain what `cargo build` does step by step, and what ends up in `target/`.
- Choose build profiles deliberately (`opt-level`, `lto`, `codegen-units`, `panic`, `overflow-checks`, debug info) for
  a production service, and justify each choice.
- Read a function's MIR and assembly and say which variables exist in memory, which live only in registers, and which
  vanish.
- Predict the size and layout of structs and enums, including niche optimizations, and spot a bloated enum.
- Use exhaustive pattern matching to make "forgot a case" a compile error, and know when a wildcard is a bug waiting
  to happen.
- Structure a crate so that privacy protects invariants, and a workspace so that the crate graph enforces the
  architecture.

## Listings and tools

All Rust code in this Part is in `listings/part-02/`, verified against rustc 1.98.1 (edition 2024) by
`tools/verify.ps1`. Compiler output (MIR, LLVM IR, assembly, macro expansion) was produced with `tools/emit.ps1`, which
asks the Playground for the same artifacts `rustc --emit` would produce. Rerun it yourself: compiler output changes
between versions, and seeing it change is part of the lesson.
