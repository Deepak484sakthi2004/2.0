# Part XVIII — How rustc Works

> **Part question:** *When you run `cargo build`, what does rustc actually compute, in what order, and on which
> representation? And which of those facts can you rely on, and which are implementation details that will change
> under you?*

Part XVII taught compiler construction in general: lexing, parsing, name resolution, type inference, IRs, SSA,
optimization, code generation. Part XVIII opens the one compiler this book has been quoting since Chapter 1.3. Every
chapter so far has shown you rustc's *outputs*: MIR in Chapters 2.3 and 3.1, LLVM IR in 3.3 and 4.1, vtables in 6.4,
monomorphized instances in 7.1. This Part explains the machine that produced them.

The Part keeps one distinction front and center, because the brief demands it and because it is the difference between
knowledge that lasts and knowledge that rots:

| What you're looking at | Stability | Tag |
|---|---|---|
| What the language guarantees (drop order, what borrowck accepts on stable, coherence rules) | Stable, versioned by edition | [LANG] |
| What rustc does today (queries, HIR, THIR, MIR, pass names, attribute dumps, crate names) | Changes release to release | [RUSTC] |
| What depends on a version or channel (nightly flags, the new trait solver, Polonius) | Check the current release notes | [VERSION] |

MIR output from the Playground literally begins with the compiler's own disclaimer:

```text
// WARNING: This output format is intended for human consumers only
// and is subject to change without notice. Knock yourself out.
// HINT: See also -Z dump-mir for MIR at specific points during compilation.
```

Read everything in this Part in that spirit. The *shape* of the pipeline has been stable for years. The names and
details inside it are not.

## How this Part gets its evidence

Every claim about rustc's internals here comes from one of three places, and the text says which:

1. **A real artifact from the Playground** (rustc 1.98.1 stable; nightly 1.100.0 of 2026-09-24 for nightly-only
   output): HIR, MIR, LLVM IR, assembly, macro expansion, and a set of nightly-only **`rustc_attrs` dumps** such as
   `#[rustc_abi(debug)]`, `#[rustc_dump_vtable]`, and `#[rustc_evaluate_where_clauses]`. These attributes are internal
   testing hooks: they make the compiler print a data structure as an error message. They are unstable, undocumented
   for users, and renamed without notice (two names from older write-ups, `#[rustc_variance]` and
   `#[rustc_layout(debug)]`, no longer exist on this nightly). Every one quoted here was verified on the Playground.
2. **Attribution** to the rustc-dev-guide, the Reference, an RFC, or a release announcement, hedged "as of 2026".
3. **A local command** you can run to see it yourself (`-Z self-profile`, `-Z time-passes`, `-Z dump-mir`,
   `-Zunpretty=thir-tree`, `cargo build --timings`), labeled *not verified here*, because the Playground doesn't accept
   arbitrary `-Z` flags.

## Chapter map

```text
18.1 Queries and Incremental Compilation   rustc is a demand-driven database, not a pipeline; DefIds; red-green;
                                           per-body error gating; why one error hides another (verified)
      │
18.2 Macro Expansion, Resolution, HIR      tokens → AST → expansion/resolution fixpoint → HIR; hygiene (verified
                                           22-vs-6 demo); `for`, `?`, `while let`, `.await` desugared in real HIR
      │
18.3 Type Checking and Trait Solving       inference variables, adjustments, method probe, obligations; the solver
                                           evaluating where-clauses (EvaluatedToOk vs Ambig, verified); E0275;
                                           never-type fallback changing with edition 2024 (verified)
      │
18.4 THIR and MIR                          typed tree → control-flow graph; drops, unwind edges, MIR passes;
                                           const evaluation as a MIR interpreter; a `let _ =` bug proven in MIR
      │
18.5 Borrow Checking on MIR                regions as sets of points; two-phase borrows; Drop as a use; closure
                                           requirements; problem case #3 accepted on nightly 1.100 (verified)
      │
18.6 Monomorphization and Codegen          collector, CGUs, fn ABI (where `noalias` comes from, verified), symbols,
                                           vtables, LLVM function merging (same program: `false` in debug, `true`
                                           in release), Cranelift and GCC backends
      │
18.7 Tracing `let x = foo();`              one line through every stage, with a real artifact at each step
      │
Part XVIII Review                          the compiler detective: read a service's artifacts; three build tickets
```

## What you'll be able to do after Part XVIII

- Place any compiler message, artifact, or build-time cost at the stage and query that produced it.
- Explain why fixing one error reveals another, and predict which errors a change will surface.
- Read HIR, MIR, LLVM IR, and assembly for the question each answers, and know which one to reach for.
- Use nightly introspection attributes to ask the compiler what it decided (types, captures, layouts, ABIs, vtables).
- Reason about the build-time and semver consequences of macros, trait-heavy designs, `Drop` impls, and generics.
- Tell stable guarantees from implementation details, and keep your architecture on the right side of that line.

## Listings

`listings/part-18/`: 68 files, 80 checks (13 of them on nightly), all verified on the Playground (rustc 1.98.1
stable, edition 2024; nightly 1.100.0 of 2026-09-24 for checks marked `+nightly`; one check also run on beta
1.99.0-beta.7). Files named `answers-*.rs` back claims made in the answer key. Artifacts were fetched with
`tools/emit.ps1`.
