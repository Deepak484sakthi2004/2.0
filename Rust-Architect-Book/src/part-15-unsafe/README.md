# Part XV — Unsafe Rust

> **Part question:** *What exactly does safe Rust guarantee, what must `unsafe` code maintain so that the guarantee
> stays true for every safe caller, and how do you prove, review, and test that it does?*

For fourteen Parts, the compiler has been the one proving things: no use-after-free, no data races, no invalid values.
Part XV is about the code where *you* carry the proof. That's not an exotic corner. `Vec`, `String`, `HashMap`, `Arc`,
`Mutex`, every channel, and every syscall wrapper are built from `unsafe` Rust, and a senior Rust engineer is expected
to read it, review it, and occasionally write it.

The brief for this Part says not to teach `unsafe` as "dangerous Rust," and the Part doesn't. It teaches `unsafe` as
**a proof obligation with a contract**: the language defines what counts as undefined behavior, `unsafe` marks where a
human must show it can't happen, and the module around the `unsafe` block is where that proof lives. Every `unsafe`
example states its **safety invariant** explicitly. Every example of a *violation* is minimal, is paired with the tool
that catches it (usually **Miri**, run under both of Rust's candidate aliasing models), and ends with the fix.

> **Status:** Chapters 15.1–15.3 are written. Chapters 15.4–15.6 and the Part review are **not written yet** (marked
> below). The verified listings for 15.4 are already in the repository: `listings/part-15/ch04-*.rs` builds a
> `MyVec<T>` step by step (RawVec split, growth, `Drain` with leak amplification, ZSTs, `try_reserve`, the push fast path
> in release assembly), each Miri-clean or deliberately flagged by Miri. Until those chapters exist, read the listings
> alongside the Rustonomicon's "Implementing Vec" and the `std::alloc` documentation.

## Chapter map

```text
15.1 What unsafe Means              the five superpowers; validity vs safety invariants; soundness; UB as a contract
                                    with the optimizer (a bool returning 2); the module as the unit of trust
      │
15.2 Raw Pointers, Provenance,      a pointer = address + provenance; in-bounds arithmetic (`getelementptr inbounds`);
     and Aliasing Models            strict provenance; Stacked vs Tree Borrows; split_at_mut / get_disjoint_mut
                                    shapes; `noalias` in action (release reports 70, memory holds 100)
      │
15.3 MaybeUninit, ManuallyDrop,     three wrappers, each switching off one assumption; panic-safe initialization;
     and UnsafeCell                 drop order; the only door to mutation behind `&`; drop check and #[may_dangle]
      │
15.4 Implementing Vec<T>            [not yet written; listings ch04-* verified] a safe abstraction end to end:
                                    RawVec, growth, Drain and leak amplification, ZSTs, fallible allocation, under Miri
      │
15.5 Custom Allocators              [not yet written] GlobalAlloc (the counting allocator explained), bump/arena
                                    allocation, bumpalo, the unstable Allocator API, choosing an allocator
      │
15.6 Verifying unsafe               [not yet written] Miri in CI, sanitizers, loom, fuzzing; what each catches and misses
      │
Part XV Review                      [not yet written] an unsafe-code audit of a container PR; interview mode
```

## What you'll be able to do after Part XV

- Name exactly what `unsafe` unlocks, and tell a validity invariant from a safety invariant.
- Decide whether an API is **sound**, and write the safe program that proves it isn't when it isn't.
- Write `# Safety` sections and `// SAFETY:` comments that a reviewer can check line by line.
- Write pointer code that passes Miri under both Stacked Borrows and Tree Borrows.
- Build values piece by piece without reading or dropping uninitialized memory, and keep that panic-safe.
- Build and review a safe abstraction over raw memory: a `Vec`, an arena, an allocator (15.4–15.5, not yet written).
- Set up the tooling (Miri, sanitizers, loom) that turns "I think it's sound" into evidence (15.6, not yet written;
  Miri is used throughout 15.1–15.3).

## Listings

`listings/part-15/`: every listing verified on rustc 1.98.1 (edition 2024) with `tools/verify.ps1`. Chapters
15.1–15.3 use 58 files and 98 checks, 46 of them runs under **Miri** (the `debug+tree` checks use Tree Borrows instead
of the default Stacked Borrows, and three `debug+nightly` checks need unstable features). The 15 `ch04-*` listings for
15.4 add 22 checks, all passing. Where the native output of
UB code is quoted, it's there to show what the optimizer did with a false promise, never as behavior to rely on.
