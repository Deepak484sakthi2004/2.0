# Part 18 report

Status: **complete.** Chapters 18.1–18.3 and the README were written by the original Part XVIII writer, which stopped
on an API error while starting 18.4. A second writer resumed and was stopped by a coordination request before writing
prose. This report's writer (the third, resuming) verified everything on disk, confirmed 18.1–18.3 (all 14 template
sections, every `rust`/`rust,compile_fail` block a verbatim substring of a verified listing), and wrote 18.4–18.7, the
review, the full answer key, the new listings, and this report. The earlier writers had already drafted most listings
for 18.4–18.7 and the review; all were re-verified, and 18 listings were added (8 for the chapters, 10 `answers-*.rs` for the answer key).

## SUMMARY.md lines

Replace the Part XVIII draft block with:

```markdown
- [Part XVIII Overview](part-18-rustc/README.md)
  - [18.1 rustc's Architecture: Queries and Incremental Compilation](part-18-rustc/ch01-queries-incremental.md)
  - [18.2 Macro Expansion, Name Resolution, and HIR Lowering](part-18-rustc/ch02-expansion-resolution-hir.md)
  - [18.3 Type Checking and Trait Solving](part-18-rustc/ch03-typeck-trait-solving.md)
  - [18.4 THIR and MIR](part-18-rustc/ch04-thir-mir.md)
  - [18.5 Borrow Checking on MIR](part-18-rustc/ch05-borrowck-mir.md)
  - [18.6 Monomorphization and Codegen Backends (LLVM, Cranelift)](part-18-rustc/ch06-mono-codegen.md)
  - [18.7 Tracing `let x = foo();` Through the Compiler](part-18-rustc/ch07-tracing-let-x.md)
  - [Part XVIII Review: The Compiler Detective & Interview Mode](part-18-rustc/review.md)
```

Appendix entry (in Part order among the answer keys):

```markdown
  - [Part XVIII Answers](appendix/answers-part-18.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| rustc as memoized, dependency-tracked queries (`TyCtxt`); demand-driven; E0391 cycle notes are the query stack | 18.1 | — |
| Error gating verified: per-body independence; type error suppresses same-body borrowck; privacy/lints behind crate-wide gate; post-mono only in full builds | 18.1 | — |
| DefId (session) vs DefPathHash (stable); `#[rustc_dump_def_parents]`; red-green, fingerprints, early cutoff; CGU work-product reuse | 18.1 | — |
| Parallel front end `-Z threads` (2023 announcement, hedged); 1.52.1 incremental disable episode | 18.1 | XX |
| Expansion ⟷ resolution fixpoint; mixed-site hygiene table (verified 22-vs-6 demo, E0425 with hygiene note); `$crate`; expanded output ≠ source | 18.2 | XXVI |
| Real HIR of `for`, `?`, `while let`, `async fn`, `.await`, ranges, let chains, `format_args!` (pre-encoded template, `super let`); lang items | 18.2 | — |
| Proc macros = host dylibs loaded by rustc; build-time code execution; critical path; E0659 glob ambiguity | 18.2 | XIX, XXII |
| Items declared / bodies inferred; `typeck` results; obligations, fulfillment, candidates → winnow → confirm; canonicalization | 18.3 | — |
| `#[rustc_evaluate_where_clauses]`: EvaluatedToOk vs EvaluatedToAmbig (verified); E0277 read bottom-up; E0275 overflow and long-type file | 18.3 | — |
| Never-type fallback: `!` in 2024 (E0277 `!: Default`), deny lint `dependency_on_unit_never_type_fallback` in 2021 (verified) | 18.3 | — |
| Method probe order; `use std::borrow::Borrow` breaks `RefCell::borrow` (E0282, verified); next-gen trait solver status (1.84 coherence, hedged) | 18.3 | — |
| Typeck dumps: `rustc_capture_analysis`, `rustc_dump_item_bounds` (implicit Sized), `rustc_dump_hidden_type_of_opaques` | 18.3 | — |
| THIR: explicit adjustments/overloaded ops; exhaustiveness + unsafety (E0133) run on THIR; `-Zunpretty=thir-tree` (unverified) | 18.4 | — |
| MIR vocabulary table; debug vs release MIR of a `match` (MIR inliner, `assume(len <= isize::MAX)`, storage markers only in release) | 18.4 | — |
| `let _ =` drop proven in MIR; `match` scrutinee guard lives to end of match (MIR + `try_lock` demo; 2024 `if let` doesn't help, verified) | 18.4 | — |
| `#[must_use]` silenced by `let _ =` (help text suggests it, verified); allow-by-default `let_underscore_drop` (verified) | 18.4 | — |
| MIR pipeline (built → promoted → borrowck → drop elaboration → optimized / mir_for_ctfe); RFC 3027 infallible promotion | 18.4 | — |
| Const eval = MIR interpreter shared with Miri: invalid `bool` E0080 (verified); `const _: () = assert!` build-time validation; deny-by-default `long_running_const_eval` (verified) | 18.4 | XV.6 |
| Borrowck pipeline: renumber → MIR type check → liveness (drop-live) → region inference (sets of points, SCCs) → dataflow; universal vs existential regions | 18.5 | — |
| Verified: dead code still borrow-checked (E0499 in `if false`); empty `Drop` extends loans (E0502 "when `span` is dropped"); two-phase only for autoref receivers etc. (`Vec::push(&mut v, v.len())` and `(&mut v).push(..)` E0502) | 18.5 | — |
| `#[rustc_regions]` closure external requirements (`where '?1: '?3`); closure args list | 18.5 | — |
| Problem case #3: E0502 on stable 1.98.1 and beta 1.99.0-beta.7, **accepted on nightly 1.100.0 (2026-09-24) with no flags** [VERSION] | 18.5 | XXVI |
| Adding `Drop` / lifetime params / `&mut self` / wider RPIT capture as breaking changes via borrowck | 18.5 | XXII |
| Collector roots/edges, instance kinds (shims), shared generics, `#[inline]` local copies, CGU partitioning, `rustc_codegen_ssa` | 18.6 | — |
| `#[rustc_abi(debug)]`: release `NoAlias`/`ReadOnly`/`NonNull`..., debug only `NonNull | NoUndef`; debug IR has only `align` (verified); noalias chain borrowck → ABI → IR → one load | 18.6 | — |
| `#[rustc_dump_symbol_name]` (v0, crate disambiguator hash, `p` placeholder for uninstantiated generics); `#[rustc_dump_vtable]` (default methods included; supertrait methods first); `#[rustc_dump_layout(debug)]` | 18.6 | — |
| LLVM function merging: `fee::<Wallet> = fee::<Card>` alias; `fn_addr_eq` false (debug) / true (release); `unnamed_addr`; lint `unpredictable_function_pointer_comparisons` (verified) | 18.6 | XX |
| Pass modes Ignore/Direct/Pair/Cast/Indirect (sret); Cranelift (nightly component) and GCC backends (unverified, [VERSION]) | 18.6 | XX |
| GraalVM Native Image reachability vs rustc collector | 18.6 | — |
| Full trace of `let x = foo();`: expand (prelude injection), HIR (`#[attr = Inline(Never)]`), typeck probe, debug/release MIR, 18 vs 3 IR functions (incl. debug UB precondition checks), IR (`sret`, `invoke`/`landingpad`, `range` return attr), asm (cap/ptr/len at rsp, "meridian" as a 64-bit immediate, len kept in rbx) | 18.7 | XIX |
| Artifact-choice table: debug artifacts for semantics, release for performance | 18.4, 18.7 | XX |

## Promises to later Parts

- **Part XIX:** GOT-relative calls (`call qword ptr [rip + foo@GOTPCREL]`) and relaxation; v0 symbols in the symbol
  table and demangling; `lang_start` wrapping `main`; the `personality` routine and unwind tables behind landing pads;
  allocator shims (`__rust_alloc`, `__rust_no_alloc_shim_is_unstable_v2`); proc macros as host dylibs loaded with
  `dlopen` (18.2 §6); `split-debuginfo`/`strip`.
- **Part XX:** `-Z self-profile` / `--timings` on trait-heavy and generic-heavy crates (18.1, 18.3 exercises); function
  merging and profile attribution (`-Z merge-functions=disabled`, 18.6 §9); Cranelift vs LLVM build-time measurement
  (18.6 systems exercise); release-profile experiments (18.6 architecture exercise); the artifact-regression CI check
  (18.7 design exercise); parallel front end `-Z threads`.
- **Part XXII:** proc-macro allowlist and `cargo expand` (18.2); compile-time SQL checking trade-off (18.2 design
  exercise → 22.4); Tower `BoxService` to cap type depth (18.3 §10 → 22.3); `cargo test --release` in CI and
  reverse-dependency builds for shared crates (18.5, 18.6 → 22.7); semver checks for `Drop`/lifetime additions.
- **Part XV:** `#[may_dangle]` and drop check (18.5 points to 15.3); const eval and Miri share an engine (18.4 → 15.6).
- **Part XVI:** `repr(C, u32)` enums and explicit encodings at boundaries (18.6 debugging exercise); `conv: Rust` vs
  `extern "C"` in the ABI dump.
- **Part XXVI:** mirror rustc's architecture in the toy compiler (queries/HIR/MIR stages as the reference design,
  borrow checking on a MIR-like IR in 26.5–26.6); local type inference, hygiene, and Polonius-style analyses as
  language-design choices.

## Promises kept

- **PROGRESS "Part XVIII" list:** trait solver and next-gen solver status (18.3); vtable layout internals (18.6,
  `rustc_dump_vtable`, supertrait order); monomorphization collector, CGU partitioning, shared generics, LLVM function
  merging, Cranelift (18.6); exhaustiveness and match lowering on THIR/MIR (18.4); `?` desugaring (HIR in 18.2, MIR
  referenced in 18.4 and the review); borrow checking on MIR with places/loans/regions, liveness, and Polonius status
  (18.5); post-monomorphization errors (18.1 table, 18.6 collector); privacy checked in phases, E0616-hides-E0451
  generalized into verified gating rules (18.1); drop elaboration and drop flags (18.4); `noalias` emission (18.6);
  queries and red-green incremental (18.1); macro expansion and hygiene (18.2); HIR desugarings shown in real HIR (18.2);
  `let x = foo();` traced with a real artifact at each step, cross-referencing 2.3 (18.7).
- **Part VII (18.3, 18.6):** trait solving for generic bodies; collector/CGUs/shared generics/merging/Cranelift. The
  `-Z self-profile`/`-Z time-passes` runs are given as local commands (not verifiable on the Playground).
- **Part IV rows (4.1, 4.2, 4.3 → XVIII.5):** places/loans in MIR, NLL algorithm, universal regions, lifetime erasure.
- **Part III (3.1 → XVIII):** drop flags explained as drop elaboration.
- **Part I (1.3, 1.4):** lifetimes erased before codegen; `noalias` chain; compile-time cost anatomy (18.1 §6, 18.2 §6).
- **Part V (5.1 → XVIII.4):** exhaustiveness on THIR, `discriminant`/`switchInt`/`unreachable` lowering.
- **Part IX (9.2 → XVII/XVIII):** `format_args!` compile-time parsing (18.2's HIR).
- **Partially kept:** the coherence checker's "intercrate mode" (Part VI) is mentioned only via the next-gen solver's
  use for coherence (18.3); auto cross-crate inlining of small leaf functions (1.3) is referenced, not re-explained.

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Rust build policy | All Rust codebases pinned to 1.98.1; dev incremental on; PR CI `CARGO_INCREMENTAL=0`, caches registry + compiled deps keyed on `Cargo.lock` + toolchain; nightly full `--release` build gates the release train; `--timings` then `-Z self-profile` for slow crates | 18.1 §9 |
| CI cache incident (early 2026) | Whole `target/` cached keyed on branch name with incremental on; tens of GB in two months; mostly misses; nightly fuzzing job hit an unstable-fingerprint ICE; policy replaced | 18.1 §10 |
| Macro policy | Proc-macro/`build.rs` allowlist (serde, thiserror, tracing, clap pre-approved); PRs paste expansions of hot/security-sensitive derives; `--timings` tracks proc-macro crates; gateway workspace consolidated three `syn` major versions to one | 18.2 §9 |
| Audit-macro PAN leak | `macro_rules! audit` called bare `mask` (call-site resolution); refunds module's own `mask` returned the full PAN; six days of refund audit lines with full card numbers; DLP scan found it; logs purged; fix `$crate::audit::mask`; rule: every macro path is a parameter or `$crate::`, collision tests | 18.2 §10 |
| payments-core edition-2024 migration | `cargo fix` applied `::<()>` at 11 call sites; 2 were startup validations `settings::load()?` with a `FromEnv` impl for `()`, validating nothing for ~a year; fixed to `load::<PaymentsConfig>()`; rules on generic effect-only calls | 18.3 §9 |
| Partner-API edge middleware | Recursive `Stack<Metered<L>>` where-clause; E0275 at layer 5; `recursion_limit = "512"` made `cargo check` take minutes; layer 6 timed out CI; fix: wrap once at construction + `BoxService` split; never raise `recursion_limit` without design review | 18.3 §10 |
| payments-core fee tiers | Standard tier table as `const FEE_TIERS` with `const` assertions (ordering, 0–1,000 bps, first tier at 0); per-merchant negotiated rates stay in the DB with load-time validation; reviewers ask for debug MIR on "when" questions | 18.4 §9 |
| Payouts service (Rust, 2026) | Pays marketplace sellers; scheduler on 3 replicas; DB lease per seller (compare-and-set) released in `Drop`, `#[must_use]`; `let _ = leases.acquire(seller_id)?;` refactor → double payouts when schedules overlapped; bank idempotency keys caught most, a few retries with new keys went through; found by reconciliation 2 days later; fixes: `let _lease`, helper takes `&Lease`, CI enables `let_underscore_drop`, server-side lease expiry + check before transfer | 18.4 §10 |
| Settlement batcher retry path | Debugging exercise: `match m.lock().unwrap().status` held the guard through `bump`, which locked again (deadlock in production, intermittent on status 0) | 18.4 §13 |
| Nightly canary job | Weekly workspace build/test on latest nightly, allowed to fail; saw problem case #3 accepted; `// NLL-LIMITATION: problem case #3` markers; no code changes for nightly-only acceptance; borrowck compile-fail tests moved to the canary | 18.5 §9 |
| `meridian-telemetry` 2.4.0 `Drop` incident | Minor release added `impl Drop for Span<'_>`; 9 of the 30 dependent services failed to build (E0502 "when `span` is dropped"); yanked; 3.0.0 stores `Arc<str>` names (no lifetime), keeps the `Drop` backstop; checklist + reverse-dependency CI builds | 18.5 §10 |
| payments-core profiling note | Wallet-only load test flame graph showed `fee::<Card>` (merged with `fee::<Wallet>`, both 290 bps); runbook: merged symbols stand for all merged functions | 18.6 §9 |
| Webhook intake chargeback incident | Dispute-notification registry deduplicated handlers by `fn_addr_eq` (the lint's suppression hint); placeholder `ack_refund`/`ack_chargeback` merged in release; chargebacks fell to an "unknown event" path returning 200; several late dispute responses; fixes: key on event kind, error on duplicates, `cargo test --release` in the nightly job, unknown events alert | 18.6 §10 |
| Market-data shared-memory ring | Debugging exercise: `Reply` enum bytes copied to a C++ consumer declared with a `u32` tag; tag 0x2A00 symptoms; fix `repr(C, u32)` or explicit encoding | 18.6 §13 |
| Artifact-backed performance reviews | Gateway and payments-core: PRs claiming hot-path effects attach the right artifact (counting-allocator test or release asm for allocations, release asm for dispatch/inlining, debug MIR for drop/lock timing) | 18.7 §9 |
| Market-data frame encoder length cache | Cached `len` parameter added after reading debug MIR/IR; later `push_str` made the length prefix short; ~1 in 400 frames truncated downstream; fixes: remove cached length, release-artifact rule, property test on prefix | 18.7 §10 |
| Ledger client (capstone) | `post` with `ensure!`, `?`, `&mut dyn Sink` journal with `Drop`; three build tickets (alias cycle, removed braces E0499, merged `to_major` in release) | Part XVIII review |

## Verification

- `listings/part-18/`: **68 files, 80 checks, all PASS** on the Playground (final full-folder run saved to scratch
  `part-18/final/verify-final.txt`): rustc 1.98.1 stable, edition 2024; 13 checks on nightly 1.100.0 (2026-09-24) via
  `+nightly`; `ch05-07-problem-case-3.rs` additionally run with `-Channel beta` (1.99.0-beta.7: E0502). 10 `answers-*.rs`
  listings back claims made in the answer key.
- Artifacts via `tools/emit.ps1` (all quoted trimmed and labeled): debug/release MIR of `ch04-01` (`cost`), debug MIR of
  `ch04-02` (lease) and `ch04-08` (match guard), debug MIR of `ch05-02` (two-phase), release LLVM IR + asm and debug IR of
  `ch06-02` (noalias), release/debug asm of `ch06-05` (merging), the full trace of `ch07-01` (expand, HIR, MIR debug and
  release, LLVM IR debug and release, asm debug and release), HIR and release asm of `review-ledger-client.rs`, release
  asm of `answers-ch07-variants.rs`. Symbols in quoted release IR were shortened by hand and say so.
- Every `rust` / `rust,compile_fail` block in the Part's chapters and review (26 blocks) was machine-checked to be a
  verbatim substring of a verified listing (script: scratch `part-18/final/check-blocks.ps1`). `rust,ignore` blocks name
  their listing or are labeled.
- No `unsafe` in any listing except `ch04-05`/`ch04-07` (a deliberate E0133 and a deliberately invalid `transmute` in a
  `const`, both compile errors by design); no Miri runs needed.
- Unverifiable here and labeled with the local command: THIR dumps (`-Zunpretty=thir-tree`), borrowck-stage MIR
  (`-Z dump-mir`), `-Z self-profile`, `-Z time-passes`, `-Zprint-mono-items`, `-Z merge-functions=disabled`, the
  Cranelift and GCC backends, `cargo build --timings`, javac/javap output, clippy's `significant_drop_in_scrutinee`.
- Hedged [VERSION] claims: next-gen trait solver scope (1.84 announcement), Polonius work (2025 project goal), nightly
  acceptance of problem case #3, Cranelift component availability.

## Word count

| File | Words |
|---|---|
| README.md | ~830 |
| ch01-queries-incremental.md | 4,615 |
| ch02-expansion-resolution-hir.md | 4,755 |
| ch03-typeck-trait-solving.md | 5,436 |
| ch04-thir-mir.md | 6,106 |
| ch05-borrowck-mir.md | 4,692 |
| ch06-mono-codegen.md | 4,541 |
| ch07-tracing-let-x.md | 4,805 |
| review.md | 2,632 |
| answers-part-18.md | 7,021 |
| **Total** | **≈ 45,400** (`wc -w`, code included) |

## Tooling notes

- **Working nightly `rustc_attrs` dumps** (nightly 1.100.0, 2026-09-24; need `#![feature(rustc_attrs)]` and
  `#![allow(internal_features)]`; output is printed as `error:` so check with `debug+nightly error:<word>`):
  `#[rustc_dump_def_parents]` (DefIds), `#[rustc_evaluate_where_clauses]` (Ok/Ambig verdicts),
  `#![rustc_dump_hidden_type_of_opaques]` (crate-level), `#[rustc_dump_item_bounds]` (on an associated type),
  `#[rustc_capture_analysis]` (on a closure expression; needs `stmt_expr_attributes`), `#[rustc_regions]` (prints
  *notes*, not errors: check with `debug+nightly ok`), `#[rustc_abi(debug)]` (fn ABI; differs debug vs release),
  `#[rustc_dump_symbol_name]`, `#[rustc_dump_vtable]` (on the impl), `#[rustc_dump_layout(debug)]`,
  `#[rustc_dump_variances]` (integrator-verified). **Gone:** `#[rustc_variance]`, `#[rustc_layout(debug)]`.
- The Playground's MIR is `optimized_mir` (post-borrowck, post-elaboration). Debug MIR has no `StorageLive/Dead`;
  release MIR has them. Enum constructors also print a `// MIR FOR CTFE` body.
- `-Target hir` prints parsed built-in attributes as `#[attr = Inline(Never)]` and drops `pub`.
- `verify.ps1 -Channel beta` works for spot checks (the header prints the beta version).
- A 50-million-iteration `const` loop trips `long_running_const_eval` (deny) quickly: a fast, reliable failing check.
- Lint names that are easy to get wrong: `.clone()` on `&&T` is `suspicious_double_ref_op` (not `noop_method_call`);
  `==` on fn pointers is `unpredictable_function_pointer_comparisons`; `let _ =` on a destructor type is
  `let_underscore_drop` (allow by default); `#[must_use]` doesn't fire on `let _ =`.
- `#[macro_export]` macros used from another module of the same crate need `use crate::{name};`.
- Release asm shows merged functions as `a = b` alias lines, and a `fn_addr_eq` on merged functions may be
  constant-folded to a stored `1`.
- LLVM propagates constants across `#[inline(never)]` functions in the same module (the `Box<str>` length 8 in
  `answers-ch07-variants.rs`): keep that in mind when "hiding" a value behind a non-inlined function.
