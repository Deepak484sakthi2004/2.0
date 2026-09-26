# Part 17 report

Part XVII was finished by a **resuming writer**. The first writer (stalled by a network outage and stopped) had written
the README, Chapter 17.1, and 39 verified listings covering every chapter and the review. The resuming writer verified
all 39 listings again (46 checks, all pass), checked 17.1, wrote 17.2–17.8, `review.md`, and
`src/appendix/answers-part-17.md`, added 4 listings (three debugging-exercise variants for 17.6, 17.7, and 17.8, and one rustc check for 17.6),
updated the README's listing counts, and wrote this report.

## SUMMARY.md lines

Replace the Part XVII draft block with:

```markdown
- [Part XVII Overview](part-17-compilers/README.md)
  - [17.1 The Compiler Pipeline End to End](part-17-compilers/ch01-pipeline.md)
  - [17.2 Lexing](part-17-compilers/ch02-lexing.md)
  - [17.3 Parsing: Recursive Descent and Pratt Parsing](part-17-compilers/ch03-parsing.md)
  - [17.4 ASTs, Symbol Tables, and Name Resolution](part-17-compilers/ch04-ast-resolution.md)
  - [17.5 Type Checking and Type Inference](part-17-compilers/ch05-type-checking-inference.md)
  - [17.6 Intermediate Representations, CFGs, and SSA](part-17-compilers/ch06-ir-cfg-ssa.md)
  - [17.7 Optimization](part-17-compilers/ch07-optimization.md)
  - [17.8 Code Generation and Register Allocation](part-17-compilers/ch08-codegen-regalloc.md)
  - [Part XVII Review: Sieve's First Compiler & Interview Mode](part-17-compilers/review.md)
```

Appendix entry (in Part order among the answer keys):

```markdown
  - [Part XVII Answers](appendix/answers-part-17.md)
```

## The tiny language: Ore (for Part XXVI to reuse)

**Ore** is the Part's running language. Part XXVI is meant to grow it into a Rust-like language with ownership and a
borrow checker (the README says so).

- **Lexical** (listing 17.2-1): ASCII identifiers; keywords `fn let mut if else while return true false`; integers with
  `_` separators, checked against `i64`; string literals with `\n \t \\ \"` escapes (lexed, unused by later stages);
  `//` comments; operators `+ - * / % = == != < <= > >= && || ! -> ( ) { } , ; :`. Spans are `u32` byte ranges;
  errors are tokens.
- **Grammar** (listing 17.3-1): `program := fn_decl*`;
  `fn_decl := "fn" IDENT "(" params ")" ("->" IDENT)? block`; blocks have statements and an optional tail expression;
  `let` / `let mut` with optional `: type`; expressions with precedence `=` (right) < `||` < `&&` < comparisons
  (non-associative) < `+ -` < `* / %` < unary `- !` < calls `f(..)(..)` < primary (literals, names, parentheses,
  blocks, `if`/`else if`/`else` expressions, `while`, `return`). `if`/`while`/blocks need no `;` as statements.
- **Names** (listing 17.4-3): two passes (items, then bodies); scoped locals with shadowing; the initializer is resolved
  before the new binding; separate type namespace (`int`, `bool` are ordinary names resolved as types); parameters are
  immutable; assignment requires `let mut`; "did you mean" by edit distance.
- **Types**: `int` (i64, wrapping arithmetic), `bool`, `()`, `!` for `return`. Bidirectional checker with an error type
  (listing 17.5-1); an HM inference variant for unannotated functions with let-polymorphism at top level (listing 17.5-2).
- **Semantics**: `&&`/`||` short-circuit (lowered to branches); division or remainder by zero is a run-time error.
- **Middle and back end**: three-address CFG with `Copy/Bin/Un/Call` and `Jump/Branch/Return` (17.6-1), dominators and
  frontiers (Cooper–Harvey–Kennedy), SSA by Cytron et al. (17.6-2), interpreters for both IRs, an SSA optimizer with
  simplify/GVN/DCE/merge and a `may_trap` legality predicate (17.7-1), and a linear-scan back end emitting x86-flavored
  text plus an emulator (17.8-1; that listing's input is standalone three-address code, not lowered Ore).
- **Reuse notes**: the lexer + parser block (~430 lines) is duplicated verbatim in 7 listings so each file is
  self-contained; the first writer assembled them with `compose.sh` in the scratch folder `part-17/` (with
  `ore_front.rs` and `stage_*.rs` pieces). Part XXVI should either keep that pattern or define its own Ore v1 with
  explicit differences (ownership, references, structs/enums, patterns).

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| Pipeline stages: knows/decides/forgets; "earliest stage with the information"; front/middle/back end | 17.1 | — |
| One Rust fn at four levels (HIR, MIR debug/release, LLVM IR, asm): `scale` = `lea` + `inc` | 17.1 | XVIII.7 |
| Interpreter / closure compilation / bytecode VM / JIT / AOT trade-off (10.1's 11.45 / 6.28 / 0.89 ns) | 17.1, 17.8 | XXVI |
| Maximal munch; tokens as (kind, span); errors as tokens; lazy line/column via a line index | 17.2 | — |
| Lexer = DFA: 256-byte class table + 12×11 transitions (388 B); longest-match loop; differential test (2,000 inputs) | 17.2 | — |
| Token cost measured: spans 0 allocs / owned 233,756 / `chars()` 516,192; 1,264.7 vs 92.9 vs 60.3 MB/s (one run) | 17.2 | XX |
| rustc lexing layers (`rustc_lexer` pure classifier, `rustc_parse` spans/interning/token trees); 8-byte `Span` | 17.2 | XVIII.2 |
| Unicode identifiers (RFC 2457, 1.53, UAX #31, NFC); confusable lints (verified errors); bidi lint (1.56.1, Trojan Source) | 17.2 | — |
| Java `\uXXXX` pre-lexing translation (JLS §3.3) vs Rust literal-only escapes | 17.2 | — |
| Recursive descent (one fn per level, loops for left assoc) vs Pratt (binding powers; (l,r) asymmetry = associativity) | 17.3 | XXVI.1 |
| Non-associative comparisons; turbofish; Rust grammar restrictions (braces, struct literals in conditions) | 17.3 | — |
| Error recovery: synchronization (4 errors) vs skip-one-token (7, cascades, phantom statement) | 17.3 | — |
| AST storage measured: Box 1,500,001 allocs / arena 21 / bump 18; 68.1 / 42.8 / 30.5 ms (one run) | 17.3 | — |
| Parser stack per nesting level: RD 1,968 B debug / 224 B release, Pratt 753 / 144; overflow aborts (verified); depth limit; `stacker` | 17.3 | XIX (guard pages) |
| rustc-style Visitor with `walk_*` defaults; the forgotten-walk bug | 17.4 | XVIII |
| Two-pass resolution, scopes, shadowing (init before declare), namespaces, "did you mean" (edit distance ≤ len/3) | 17.4 | XVIII.2 |
| Hygiene: mixed-site (`macro_rules!`) vs call-site (proc macros); names = (symbol, syntax context) | 17.4 | XVIII.2 |
| AST node layout 72 → 32 → 24 → 12 bytes; rustc static size assertions | 17.4 | — |
| Interning measured: 200,017 vs 2,039 allocs; lookups 4.04 vs 2.36 ms; equality 0.16 vs 0.04 ms (one run) | 17.4 | — |
| Side tables keyed by node id; HIR bakes resolutions in | 17.4 | XVIII.2 |
| Bidirectional checking (infer/check), `{error}` type, `!` coerces; rustc `ErrorGuaranteed` | 17.5 | XVIII.3 |
| HM inference: type variables, unification over union-find, occurs check, zonk, generalization (let-polymorphism) | 17.5 | XXVI.2, XXVI.5 |
| Rust inference boundaries: E0121 (signatures), closures not generalized (E0308 with provenance notes), literal fallback i32/f64 | 17.5 | — |
| Interned types (`Ty<'tcx>` pointer equality), `ena` union-find with snapshots; HM worst case DEXPTIME (Mairson 1990) | 17.5 | XVIII.3 |
| Java `var`, lambdas as poly expressions (target typing), JLS 18 inference | 17.5 | — |
| Basic blocks, CFG lowering (short-circuit as branches), dominators (Cooper–Harvey–Kennedy), frontiers, back edges | 17.6 | XXVI.6 |
| SSA via Cytron et al. (iterated DF + renaming); minimal vs pruned SSA (dead φs shown); Braun et al. 2013; Cranelift | 17.6 | XXVI.6 |
| Dataflow: definite assignment (forward must) and liveness (backward may); top vs bottom start (bug verified, 17.6-6) | 17.6 | XVIII.5 |
| E0381 on paths not values; `if true { x = 1 }` rejected (17.6-7) vs Java accepting (JLS 16, not verified) | 17.6 | — |
| MIR = CFG over places (not SSA); LLVM debug allocas vs release SSA (`sroa`, `lcssa`, loop rotation, `nuw nsw`) | 17.6 | XVIII.4 |
| Kam–Ullman bound for iterative dataflow; RPO iteration | 17.6 | — |
| Optimizer passes on SSA: simplify (fold/copy/identities/branches), GVN over dominator tree, DCE (may_trap roots), block merge; 19→4 instructions, 14,687→9,667 dynamic | 17.7 | XXVI |
| LICM legality: trapping ops only if executed anyway; naive vs safe LICM (verified table) | 17.7 | — |
| LLVM verified: wrapping fold `x == i64::MAX`, CSE, LICM + SSE2 vectorization, SCEV closed form with 128-bit mul, DSE, div checks + 32-bit fast path, versioned LICM in `guarded_sum` | 17.7 | XX.6 |
| UB and data-race freedom as optimization licenses (14.1's hoisting and memcpy from the compiler's side) | 17.7 | — |
| C2 speculation, deopt, implicit null checks via SIGSEGV; precise Java exceptions | 17.7 | — |
| Instruction selection (two-address, `lea`), liveness → live intervals (back-edge liveness), linear scan with spilling | 17.8 | XXVI.7 |
| Emulator as oracle for generated code; K table: 138 vs 15 memory operands (K=4 vs 12) | 17.8 | — |
| System V in real asm: 7th arg at `[rsp+8]`, callee-saved push/pop across calls, red zone, GOT calls | 17.8 | XVI.1, XIX |
| Allocators: linear scan (C1, Graal), graph coloring (C2), LLVM greedy, Cranelift regalloc2; spill weights | 17.8 | — |

## Promises to later Parts

- **Part XIX:** object files, symbols, relocations, and GOT-relative calls (`@GOTPCREL`) seen in 17.8's asm; call
  relaxation; stack frames, the red zone, and 16-byte alignment at calls (17.8 §5).
- **Part XX:** benchmarking loops that LLVM turns into formulas (`triangle`, `guarded_sum`; `black_box`), 20.2; spill
  costs and register pressure as a performance factor (17.8 §6); lexer throughput with class tables versus comparison
  chains (17.2 systems exercise); LLVM pass-pipeline cost (`-C llvm-args=-print-after-all`, `-Z time-passes`).
- **Part XXII (22.7):** fuzzing lexers and parsers ("never panics, every byte covered by one token"); differential
  testing and shadow mode as general techniques; catalog/library CI that recompiles dependents (17.4 §10).
- **Part XXVI:** grow Ore (see "The tiny language") into the book's language with structs, enums, patterns, ownership,
  and a borrow checker on a MIR-like IR; reuse the Part XVII pipeline listings; 26.7 targets LLVM (Ore's back end in
  17.8 emits x86-flavored text for an emulator); the Sieve rule language can serve as a second, DSL-sized example.

## Promises kept

- **PROGRESS "Part XVII (Compilers)":** SSA/φ/register allocation deepened from 2.3 (17.6, 17.8); type inference by
  unification from 2.3 (17.5); Java definite assignment as dataflow from 4.2 (17.6 §8, with a verified Rust edge case,
  listing 17.6-7); LICM hoisting, loop → `memcpy`, loop collapse and their data-race-freedom license from 14.x (17.7 §4,
  with SCEV's closed form verified in `triangle`); closure compilation of an interpreter from 10.1 (17.1 §7, 17.8 §7 and
  §9).
- **Chapter 1.1:** `x + 1 < x` and UB exploitation, revisited from the optimizer's side with Rust's exact wrapping fold
  (17.7 §4).
- **Chapter 1.4:** compile-time anatomy (17.1 §6, 17.7 §6).
- **Chapter 2.4 / Part IX interlude:** frame sizes and stack depth tied to register allocation (17.3 §6, 17.8 §5).

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Sieve (rule language) | typed rule language started 2026 by the risk platform team; shared by onboarding, fraud, payments risk; compiled at upload time in a rule service; evaluation services load compiled rules (Arc swap), evaluated by closure compilation; never parse at evaluation | 17.1 §9 |
| Pre-Sieve JSON string-comparison rule | `{"gt": ["amount_minor", "100000"]}` compared as strings; review queue +~3,100 cases over 9 days | 17.1 §10 |
| Sieve workload (design exercise) | ~60 analysts, a few hundred rule changes/week, ~2M evaluations/s (fraud 50K scores/s × ~40 rules) | 17.1 §14 |
| Sieve lexer | spans, error tokens, `Money` token (currency + exact minor units, never via f64), ASCII identifiers and literals (escapes for non-ASCII), ~300 lines, fuzzed per commit, differential test | 17.2 §9 |
| Cyrillic country-code incident (April 2026, Sieve beta) | `country == "DЕ"` (U+0415) from a regulator PDF; never matched for 16 days; found by the weekly "rules that haven't fired in 7 days" report | 17.2 §10 |
| Sieve parser | Pratt with a reviewed binding-power file; v0.4 added `in`; ~4,100 rules in the store at migration; both parsers compared on every rule; depth limit 64; parenthesized hover in the editor; shadow mode mandatory for compiler changes | 17.3 §9 |
| `&&`/`||` precedence incident (v0.3) | merged table row; caught in shadow mode before cutover (listing simulation: 906 vs 100 flags, 806 differ); fixes: tree diff over the rule store, every-pair precedence test, hover | 17.3 §10 |
| Sieve feature catalog | the symbol table: name, type, owning service, cost class (local/remote), stable id; features and functions in separate namespaces; `let` may not reuse a catalog name; rules store ids + catalog version; reverse index blocks deletions | 17.4 §9 |
| `velocity_1h` redefinition (June 2026) | card-level → merchant-level (card feature renamed `card_velocity_1h`); 37 stored rules re-resolved by name; review queue ~1,200/day → ~19,000 in 5 hours; pinned to previous catalog; fixes: ids, immutable meanings, catalog CI recompiles rules | 17.4 §10 |
| Catalog size (design exercise) | ~900 features across 14 owning services | 17.4 §14 |
| Sieve types | Int, Bool, Str, Money, Duration, Percent, lists; no implicit conversions; literals checked against context (`2.5%`); money literals must name a currency; local inference only | 17.5 §9 |
| JPY threshold incident (May 2026, Japan launch) | old-engine rule `amount_minor > 100000` meant ¥100,000 for JPY; ~1 in 9 JPY payments to review vs intended ~1 in 200; noticed in 3 days via merchant complaints | 17.5 §10 |
| Sieve rule IR and prefetch planner | rules lowered to a CFG; evaluator follows paths (lazy remote reads); planner prefetches only *anticipated* remote features (backward must); planner output shown in rule review | 17.6 §9 |
| Prefetch may-analysis incident (v0.6, July 2026) | prefetched every possibly-read remote feature; graph-service calls ~2×, its p99 ~9 → ~31 ms; fraud scoring fell back to degraded mode; reverted after 25 minutes | 17.6 §10 |
| Sieve optimizer | checked constant folding (overflow = compile error), CSE of feature reads, dead-condition warnings not deletions, nothing moves across a guard; differential test over ~2M recorded evaluations | 17.7 §9 |
| Hoisted-division incident (v0.7, August 2026) | "compute derived values once" pass hoisted `volume_30d / merchant_age_days` above its guard; fail-closed rule; 47 minutes; ~2,300 payments declined at ~180 newly onboarded merchants; fixes: legality rule, boundary values, second reviewer for fail-closed rules | 17.7 §10 |
| Sieve back end | tree-walking reference interpreter (oracle) + closure compilation with per-evaluation slots (lifetimes from liveness); Cranelift JIT prototype rejected (executable memory, security review; evaluation dominated by feature fetches); differential tests in CI and on a 1% live shadow sample | 17.8 §9 |
| Quantifier slot-reuse bug (v0.8, September 2026) | `any`/`all` loops; slot lifetimes from textual uses; caught in shadow mode: 3 of 11 quantifier rules, ~0.4% of evaluations (lists with >1 element) | 17.8 §10 |
| Support-console query language (design exercises) | ~400 support agents; queries like `status == "failed" && ...`; ~2 billion payment rows | 17.3 §14, 17.7 §14 |
| Pricing-rules engine (design exercise) | ~3,000 pricing rules, 5–40 features each, ~400,000 evaluations/s at checkout, p99 1 ms for the pricing call | 17.8 §14 |
| sieve-compiler v0.1 PR | Part XVII review artifact: nine defects (Unicode literals, literal-overflow panic, unterminated-string/unknown-operator panics, chained comparisons + bool→int coercion, spanless errors + ignored trailing input, unknown features → 0, no type checking, unchecked folding, eager `&&`/`||`), plus compile-per-evaluation; v0.2 redesign | Part XVII review |

## Verification

- `listings/part-17/`: **43 files, 50 checks** on rustc 1.98.1 (edition 2024): 25 `debug ok`, 4 `release ok`, 4 `debug test`
  (13 unit tests in total), 2 `debug build` + 4 `release build` (artifact sources), 8 intended compile errors
  (`confusable`, `text_direction_codepoint_in_literal`, `chained`, E0425, E0308, E0121, E0381 ×2), 1 `crash`
  (`overflowed its stack`), 2 `panic` (the 17.7 and 17.8 debugging variants). Final full-folder run: see the last line of
  this section.
- Four listings added by the resuming writer: `ch06-06-dataflow-bottom-bug.rs` (17.6-3 with a bottom start: spurious
  "`n` is possibly unassigned"), `ch06-07-rust-if-true.rs` (E0381 on `if true`), `ch07-04-dce-trap-bug.rs` (17.7-1
  without trapping roots: `guarded[-6, 0]` Err vs Ok(0)), `ch08-03-intervals-bug.rs` (17.8-1 with textual intervals:
  K=2 gives -15318 vs -190). Each is its base listing with one line changed (and an unused name prefixed with `_`).
- Compiler artifacts (from the first writer, via `tools/emit.ps1`, saved in scratch `part-17/`): HIR, MIR debug/release,
  LLVM IR release, asm debug/release for `scale` (17.1); MIR debug and LLVM IR debug/release for `sum_to` (17.6); release
  asm for listing 17.7-3 and listing 17.8-2. Quoted trimmed; debug-IR excerpt has `#dbg_declare`/`!dbg` removed (stated).
- Every `rust` / `rust,compile_fail` block in Part XVII's chapters (6 blocks) was machine-checked as a verbatim substring
  of a verified listing. `rust,ignore` blocks were scanned line by line against the listings; the only non-matching
  lines are labeled sketches of hypothetical changes (debugging exercises in 17.1 and 17.3) and 17.1's excerpt whose
  stage comments were added (now labeled).
- Measurements are one Playground run each and labeled noisy; allocation counts are exact. Predictions in the answer
  key are labeled "predicted".
- Not verifiable here, labeled: Java behavior (`if (true)` definite assignment, javac internals), LLVM pass-pipeline
  commands (`-print-after-all`), `logos`, Cranelift.
- Final full-folder run (resuming writer, 2026-09-26): **43 files, 50 checks, all PASS** (`part-17/verify-final.txt` in scratch).

## Word count

| File | Words |
|---|---|
| README.md | 777 |
| ch01-pipeline.md (first writer) | 4,414 |
| ch02-lexing.md | 4,812 |
| ch03-parsing.md | 4,989 |
| ch04-ast-resolution.md | 4,399 |
| ch05-type-checking-inference.md | 4,627 |
| ch06-ir-cfg-ssa.md | 4,869 |
| ch07-optimization.md | 5,237 |
| ch08-codegen-regalloc.md | 4,668 |
| review.md | 2,272 |
| answers-part-17.md | 9,329 |
| **Total** | **≈ 50,400** |

## Tooling notes

- **Non-ASCII in captured verify output.** Redirecting `tools/verify.ps1` output to a file from bash replaces non-ASCII
  characters with `?` (PowerShell 5.1 writes to the console code page); the programs' real output is correct UTF-8.
  Reconstruct from the code points the programs print, or have `verify.ps1` set
  `[Console]::OutputEncoding = [Text.Encoding]::UTF8` (integrator's call; tools are not writer-owned).
- **Verifiable debugging exercises.** Derive the buggy variant from a verified listing with a one-line `sed` change,
  and assert the failure (`panic <needle>` on a differential assertion, or `ok` with the wrong output quoted). Rename
  now-unused bindings with `_` so the variant compiles without warnings.
- **Invisible characters.** Listing `ch02-05` contains a real U+202E. Never paste such characters into Markdown; show
  `<U+202E>` and say so.
- **Code-block checker.** `part-17/blockcheck.ps1` in the scratch folder checks `rust`/`rust,compile_fail` blocks for
  verbatim inclusion and scans `rust,ignore` lines; worth moving into `tools/` for every Part.
- **Duplicated front end.** Seven listings embed the same ~430-line Ore lexer and parser; if one changes, regenerate
  the others (the first writer's `compose.sh` and `stage_*.rs` pieces are in scratch `part-17/`).
