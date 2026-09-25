# Part 10 report

Note: the original Part X writer died on an API/network error after writing every file except this report. A
finishing pass (a separate fork) checked the files, re-ran verification, fixed two unlabeled excerpts and one missing
forward mention (async closures), and wrote this report from what's on disk.

## SUMMARY.md lines

Replace the Part X draft block with:

```markdown
- [Part X Overview](part-10-iterators/README.md)
  - [10.1 Closures: Fn, FnMut, FnOnce, and Capture](part-10-iterators/ch01-closures.md)
  - [10.2 The Iterator Trait and Laziness](part-10-iterators/ch02-iterator-trait.md)
  - [10.3 How an Iterator Chain Compiles](part-10-iterators/ch03-chain-compiles.md)
  - [10.4 Rust Iterators vs Java Streams](part-10-iterators/ch04-iterators-vs-streams.md)
  - [Part X Review: The Settlement Report PR & Interview Mode](part-10-iterators/review.md)
```

Appendix entry, after "Part IX Answers":

```markdown
  - [Part X Answers](appendix/answers-part-10.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| Closure = anonymous struct of captures; sizes measured (0 / 8 / 16 / 24 B; fn item 0 B, fn pointer 8 B, `Box<dyn Fn>` 16 B) | 10.1 | — |
| Capture per place (edition 2021 disjoint capture; 2018 E0382 verified); `move` decides how captures are taken, not the trait | 10.1 | — |
| Fn/FnMut/FnOnce inferred from the body; E0525 (kind mismatch) vs E0594 (assignment in an `Fn` body) depending on where the bound is written; E0382 calling an FnOnce twice | 10.1 | — |
| Closure MIR: aggregate of captures + separate body fn `{closure#0}(_1: &mut {closure@..}, ..)`, captures as projections, tupled args via `FnMut::call_mut` | 10.1 | XVIII.4 |
| Generic vs `&dyn Fn` vs fn-pointer call asm (vtable slot decoded); non-capturing closures coerce to fn pointers, capturing ones E0308 | 10.1 | — |
| `impl Fn` return is one type (two closures → E0308); `Box<dyn Fn + Send + Sync>` for a choice | 10.1 | — |
| Closure type unique per enclosing generic instance (`describe::<u8>::{{closure}}` observed) | 10.1 | — (7.3 promise kept) |
| `move` over a `Copy` counter copies it (retry counter logged 0; verified) | 10.1 | — |
| Closure compilation of an interpreter: AST walk 11.5 vs closures 6.3 vs hand-written 0.9 ns/record (release, one run) | 10.1 | XVII.7, XXVI |
| RFC 114 unboxed closures (2014); Java lambdas via invokedynamic + LambdaMetafactory, effectively-final capture | 10.1 | — |
| Async closures + `AsyncFn*` traits stable since 1.85 (forward mention only) | 10.1 | XII |
| Iterator = one required method; pull-based, vertical, lazy (observed trace); `unused_must_use`-style laziness lint (verified) | 10.2 | — |
| `iter` / `iter_mut` / `into_iter` and the three `for` forms; `for x in v` moves the Vec (E0382); array `into_iter` edition 2018 vs 2021 item types | 10.2 | — (2.5 promise kept) |
| `size_hint` table for common adapters; ExactSizeIterator, DoubleEndedIterator; FusedIterator (unfused vs fused output) | 10.2 | — |
| Zero-copy `Frames<'a>` iterator with errors as items (24 B, 0 allocations) | 10.2 | XIII (async frames) |
| Lending iterators: std `Iterator` impossible (E0207), GAT version with 0 allocations (miri-ok), holding two items E0499 | 10.2 | — (4.5/6.2 promise kept) |
| `gen` keyword reserved in edition 2024 (verified), `gen` blocks nightly-only (E0554 on stable) [VERSION] | 10.2 | XII.3 |
| `zip` over-pulls from its first iterator (lost 1 item per batch with a non-exact source; `take` fix; Vec + collect hides it via specialization) | 10.2 | — |
| `next()` inside a `for` loop (E0499) and the `while let` fix | 10.2 | — |
| The adapter type IS the pipeline: nested `Enumerate<Take<Map<Filter<Iter>>>>` sizes 16 → 40 B | 10.3 | — (7.1 promise kept) |
| `next()` vs `fold()`: `Chain` driven by `for` is a scalar loop re-testing state; `sum` (fold) is two vectorized loops (`paddq`); 0.42 vs 0.14 ns/elem | 10.3 | XX.6 |
| `Box<dyn Iterator>`: only `next` crosses the vtable; 1.63 vs 0.08 ns/elem (~20×) | 10.3 | XX |
| Bounds-check strategies in asm (index loop, zip, pre-sliced) | 10.3 | XX.6 |
| Chain == index loop 0.41 = 0.41 ns/elem in release (Chapter 1.3's asm, measured); debug numbers 10–25× slower | 10.3 | — (1.3 promise kept) |
| In-place `collect`: `SourceIter` + `InPlaceIterable` [LIB]; 0 allocations and inherited capacity (filter keeps 1% → cap 1,000,000); `shrink_to_fit` = 1 alloc | 10.3 | — (9.1/9.5 promise kept) |
| Mid-pipeline `collect`: 17 allocations / 2.67 MB vs 0 fused | 10.3 | — |
| Collect into `Result`/`Option` (short-circuit), `try_fold` with `ControlFlow`, checked arithmetic in folds | 10.4 | — |
| `Collectors` translated (groupingBy → BTreeMap/HashMap fold, partitioningBy, joining, toMap duplicate-key semantics) | 10.4 | — |
| Iterators are values: reuse E0382, `Clone` iterators; Java streams one-shot (IllegalStateException) | 10.4 | — |
| `?` inside a closure (E0277) and the three fixes | 10.4 | — |
| Rayon `par_iter` vs `parallelStream`: 3.6× on 4 threads, trivial work barely helps, f64 sum not reproducible (verified) | 10.4 | XI.6 |
| Gatherers (JDK 24, JEP 485) vs itertools / scan / windows [VERSION] | 10.4 | — |
| Pipeline model benchmark: static flat 0.41, static boxed 1.26, dyn flat 1.91, dyn boxed 1.96 ns/elem | 10.4 | XX |
| HashMap iteration order randomized per process vs Java's stable-in-practice order | 10.4 | — |

## Promises to later Parts

- **Part XI:** `ArcSwap<Pred>` for hot-swapping compiled rules (10.1 §9); why Rayon closures must be `Fn + Send + Sync`
  and how work stealing splits an iterator (11.6, review "Looking ahead"); `FnMut` state across threads.
- **Part XII:** async closures and `AsyncFn*` (10.1 §7); `gen` blocks and coroutine lowering vs `async fn` (10.2,
  Chapter 12.3).
- **Part XIII:** why Rayon doesn't belong on async executor threads (10.4 §7).
- **Part XX:** confirm the `next()` vs `fold()` and `dyn Iterator` costs with `perf`; vectorization details (10.3).

## Promises kept

- **2.5 answer key → X:** `for x in vec` vs `&vec` vs `&mut vec`, with item types printed (10.2, `ch02-03`).
- **7.1 / 7.3 → X:** the iterator chain taken apart adapter by adapter (10.3 §4, `ch03-01`, `ch03-02` hand-rolled
  adapters); closure types unique per enclosing generic instance (10.1, `ch01-12`).
- **2.4 → X.1:** fn item (0 B) vs fn pointer (8 B) vs closure, and the coercion rules (10.1).
- **4.5 → X.1:** closures that return references and the helper-function fix (10.1, `ch01-17`).
- **4.5 / 6.2 → X:** lending iterators: E0207 with std `Iterator`, the GAT version, E0499 when holding two items (10.2).
- **9.1 / 9.5 → X:** how in-place `collect` is wired (`SourceIter`, `InPlaceIterable`) and its memory consequence (10.3).
- **1.3 → X.3:** the zero-cost claim measured (chain = index loop, 0.41 ns) and its limits (`chain`, `dyn`, collects).
- **Directive items:** Java lambdas (invokedynamic/LambdaMetafactory), Java Streams comparison with Gatherers,
  `gen` [VERSION], async closures as a forward mention (added in the finishing pass).

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Gateway log sampler | config rules like `status>=500 \|\| path^=/payments && latency>250`; closure compilation chosen (20-line `compile`), `Pred` in `ArcSwap`; interpreter would cost ~4.6 ms CPU/s at 400K req/s; analytics pipeline compiles fixed rules to Rust | 10.1 §9 |
| Retry counter incident | `move` over `Copy` `attempts` in the gateway's retry helper → access log reported 0 attempts; retry-budget alert stayed flat while upstream traffic tripled during a processor brownout; fix: helper returns `(Result, attempts)` | 10.1 §10 |
| Market-data replay tool | capture files replayed through `Frames<'a>`; 2 GB segments, 0 allocations per frame; truncated trailing frames are items; `FusedIterator` required for chained segments | 10.2 §9 |
| Ingestion batcher incident | `by_ref().zip(0..size)` batching lost 1 event per batch from a channel (batch 500 → 1 in 501); 0.2% gap in nightly reconciliation, dismissed for a week; fix `take(n)` + tests with non-exact sources | 10.2 §10 |
| Settlement batch job guidelines | ~40M rows/night, CSV + ISO 20022 XML; `Box<dyn Iterator>` per file format (~65 ms dispatch/night, accepted); generic per-row work; no mid-pipeline collects; the real cost was a `String` per CSV field | 10.3 §9 |
| Fraud blocklist memory incident | refresh every 10 min; ~8M candidates (32 B) → ~2% active; in-place collect kept ~256 MB per list; OOM kills at the 1 GB limit; fix `shrink_to_fit` + capacity gauge | 10.3 §10 |
| Merchant statement job (Rust port) | moved from Java; `BTreeMap` for stable order, integer `fold` accumulator, `try_fold` over `Result<Txn, LedgerError>` rows | 10.4 §9 |
| Fee schedule duplicate-key incident | partner CSV export; Java `toMap` threw on duplicates; Rust port's `collect` kept the last row → m-100 charged 190 bps instead of 290 for three days; porting checklist per `Collectors` method | 10.4 §10 |
| Settlement report PR | Part X review capstone (settlement batch job): 14 defects — duplicate fee row silently wins, `unwrap` on partner input, `move` copy of a counter (compiler warned), audit side effect in `map` cut short by `any` (4 of 50,000 audited), discarded suspicious list, `zip` batching dropped payouts (10 of 12 merchants paid), `f64` fee truncation, in-place capacity kept, mid-pipeline collects, per-transaction `String` clones, `fees[&m]` panic, side effect in lazy `map`, `HashMap` order in bank batches, CI accepted warnings; fixed listing with 5 tests | Part X review |

## Verification

- `listings/part-10/`: **58 files, 70 checks, all PASS** on rustc 1.98.1 stable, edition 2024 (full run by the
  finishing pass).
- Outcomes: 35 `debug ok`, 8 `release ok` (timings), 3 `release build` + 1 `debug build` (artifact sources),
  2 `debug@2021 ok`, 1 `debug@2018 ok`, 1 `test` (5 tests, review fix), 1 `miri-ok` (`ch02-11-lending-gat.rs`),
  1 `debug+nightly ok` (`gen` blocks); compile errors: E0382 ×4 (one under edition 2018), E0499 ×2, E0308 ×2, E0597,
  E0594, E0554, E0525, E0277, E0207, E0117, `error:reserved` (`gen` keyword), `error:lazy` (unused iterator lint).
- Artifacts (`tools/emit.ps1`): closure MIR (`ch01-05`); release asm of generic / `&dyn Fn` / fn-pointer calls
  (`ch01-11`), `next()` vs `fold()` over `chain` and `dyn_sum` (`ch03-03`), three bounds-check strategies (`ch03-04`).
- Timings are best-of-N on the shared Playground, labeled one run, noisy; the finishing run reproduced the quoted
  numbers within a few percent (e.g., 11.47 / 6.30 / 0.92 ns vs the chapter's 11.45 / 6.28 / 0.89).
- All 17 `rust` / `rust,compile_fail` blocks in the chapters were machine-checked as verbatim substrings of verified
  listings. The 24 `rust,ignore` blocks are named excerpts, std excerpts marked "abridged", desugaring sketches, or
  Java-labeled illustrations; two excerpts lacked a listing name and were labeled in the finishing pass.
- Unverified, labeled in text: Java/JMH side of the stream comparisons, Java code in production scenarios
  ("illustrative, not verified here").

## Word count

README 585 · 10.1 6,178 · 10.2 5,207 · 10.3 4,730 · 10.4 4,544 · review 1,437 · answers 5,476 → **28,157** total
(`wc -w`, code included).

## Tooling notes

- Nightly-only listings use a `debug+nightly ok` header plus a stable `error:E0554` header in the same file, so the
  full-folder run checks both channels.
- Edition-comparison listings pair `debug@2018` / `debug@2021` headers with the default 2024 check in one file.
- A reusable block checker (PowerShell) compares every `rust` / `rust,compile_fail` block against the listings with
  `// verify:` lines stripped; it lives in the finishing pass's scratch folder and is ~30 lines (read listings,
  normalize CRLF, `.Contains(block)`).
- `size_hint` of `(0..)` prints `(18446744073709551615, None)`: `usize::MAX` lower bound.
