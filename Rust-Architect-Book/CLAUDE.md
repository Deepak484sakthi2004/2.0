# Working rules for continuing this book

This folder is a book in progress: **Source to Silicon: Rust for the Systems Architect**.
The contract is `SPEC.md` (the reader's brief, verbatim). The continuity ledger is `PROGRESS.md`.
Read both before writing anything.

## The reader

An experienced Java backend engineer (distributed systems, databases, concurrency, system design) who wants
to reach principal-engineer and language/compiler-architect depth in Rust. Compress familiar material; go deep on what is
unique to Rust and systems programming. Never teach like a beginner course.

## Workflow for each new Part

1. Read `SPEC.md` (the Part's section plus the global rules), `PROGRESS.md` (open promises, concepts already introduced,
   running case study), and the previous Part's `review.md`.
2. Write the listings first: `listings/part-NN/chMM-KK-slug.rs`, each with one or more `// verify:` headers.
   Run `powershell -ExecutionPolicy Bypass -File tools\verify.ps1 listings\part-NN`. Only quote real compiler output.
3. Write chapters in `src/part-NN-slug/chMM-slug.md` using the chapter template below. Part overview in `README.md`.
4. Write `review.md` (architecture review + interview mode + capstone) and the answer key `src/appendix/answers-part-NN.md`.
5. Update `src/SUMMARY.md` (turn draft entries `[Title]()` into links) and `PROGRESS.md` (status, concepts, promises made/kept).
6. **Publish**: the user wants every finished Part pushed to https://github.com/Deepak484sakthi2004/2.0, inside the folder
   `Rust-Architect-Book/` (the repo also holds other study material: never touch other folders). Shallow-clone into the
   scratchpad, copy this whole folder over `Rust-Architect-Book/`, commit with the attribution trailers, push to `main`.
   Needs GitHub auth on this machine (`gh auth login` + `gh auth setup-git`, or Git Credential Manager); if a
   non-interactive push fails with "could not read Username", ask the user to log in rather than prompting.

## Chapter template (three passes → the brief's 14 sections)

```
# Chapter N.M — Title
> Where this sits / prerequisites / after this chapter you can...
## Pass 1 · User level — How do I use this?
### 1. Problem   ### 2. Mental model   ### 3. Rust code
## Pass 2 · Systems level — What happens in the compiler, memory, OS, and CPU?
### 4. Under the hood   ### 5. Memory   ### 6. CPU / OS
## Pass 3 · Architect level — When do I choose this, and what happens at scale?
### 7. Trade-offs   ### 8. Java comparison   ### 9. Production scenario   ### 10. Failure scenario
## Practice
### 11. Interview & architecture questions (no answers inline)
### 12. Exercises (beginner / intermediate / advanced / systems / architecture)
### 13. Debugging exercise   ### 14. Design exercise
```

Recurring boxes (blockquotes): **What actually happens?**, **Why not X?**, **Analogy limit** (where a Java comparison stops being true).

## Layer tags (the brief's "important distinction")

- **[LANG]** language semantics, guaranteed (Reference, std docs "guarantees" sections)
- **[RUSTC]** current compiler implementation; may change
- **[RUNTIME]** what happens during execution (std runtime, allocator, panics)
- **[OS]** operating system behavior (Linux unless stated)
- **[CPU]** hardware behavior (x86-64 unless stated)
- **[LIB]** library/ecosystem implementation choices (std internals, Tokio, Serde, ...)
- **[VERSION]** edition- or version-sensitive; name the version

## Conventions

- Baseline: Rust **1.98 stable** (September 2026), **edition 2024**. Playground verified: rustc 1.98.1.
- Code fences: `rust` (compiles and runs), `rust,compile_fail` (intentionally rejected; show the real error),
  `rust,no_run` (compiles, not meant to run), `text` for diagrams and output. Other languages: `cpp`, `java`, `go`, `c`.
- Numbers: never invent benchmark results. Latency tables are order-of-magnitude and labelled as such. External statistics
  carry who and when ("Microsoft reported (2019)..."). Performance claims need a measurement or are phrased as a hypothesis
  plus an exercise to measure it.
- Answers never inline: they go to `src/appendix/answers-part-NN.md`.
- Running case study: **Meridian**, a fictional payments-and-marketplace company (mostly Java, some C++ and Go).
  The reader's own evolving system from Project Level 4 on is **Ferrite**, a key-value store that grows into a
  distributed database.
- Chapter numbering is Part-local: "Chapter 3.2" means Part III, chapter 2.

## Environment constraints (this machine)

- Windows 10, single 32 GB SSD with ~1.5 GB free. **No local Rust toolchain; do not install one without asking.**
  Verify code through the Rust Playground via `tools/verify.ps1` (PowerShell 5.1; there is no working Python).
- `verify.ps1` header outcomes: `ok`, `build`, `test` (runs #[cfg(test)] tests), `panic <needle>`, `crash <needle>`
  (aborts, e.g. stack overflow), `error:E0xxx`, `error:<word>`, `miri <needle>` (UB must be reported, nightly Miri),
  `miri-ok`. Mode may carry an edition override: `debug@2021`, and (for Miri) `+tree` to use Tree Borrows instead of
  Stacked Borrows: `debug+tree miri-ok`, and `+nightly` to compile one check on nightly (`#![feature]`, rustc_attrs
  dumps such as `#[rustc_dump_variances]`, which prints `['a: +, T: o]` as an error): `debug+nightly error:<word>`.
- `tools/emit.ps1 <file> -Target asm|llvm-ir|mir|hir|expand -Mode debug|release` fetches compiler artifacts
  (`hir`/`expand` use nightly). Save outputs to the scratchpad and quote them trimmed.
- PowerShell variables are case-insensitive: never name a local the same as a script parameter (this bit us once).
- Parts V+ are written by parallel writers per `notes/AUTHORING-BRIEF.md`; integrate each finished Part with
  `bash tools/merge-report.sh NN` (inserts its concept table in Part order and appends its Meridian rows to
  `PROGRESS.md`), then edit the status row, the open-promises list, and `src/SUMMARY.md` by hand, re-verify its
  listings, and exclude unfinished Parts' folders when publishing (robocopy `/XD` + `/XF`).
- One intended compile error per listing (earlier-phase errors hide later-phase ones).
- The Playground `/compile` endpoint (target `asm` / `llvm-ir` / `mir`) is useful for "what does the compiler emit" claims.
  Mark functions `#[inline(never)]` when inspecting a lib crate: rustc (since 1.75) treats small leaf functions as
  cross-crate-inlinable and does not emit them standalone otherwise.
