# Part 07 report

## SUMMARY.md lines

```markdown
- [Part VII Overview](part-07-generics/README.md)
  - [7.1 Generics from Call Site to Binary](part-07-generics/ch01-generics-call-site-to-binary.md)
  - [7.2 Monomorphization vs Java Type Erasure](part-07-generics/ch02-monomorphization-vs-erasure.md)
  - [7.3 Code Bloat, Compile Time, and How to Control Them](part-07-generics/ch03-code-bloat-compile-time.md)
  - [Part VII Review: Instance Accounting & Interview Mode](part-07-generics/review.md)
```

Answer key, to add under the appendix list after Part IV (and V/VI) Answers:

```markdown
  - [Part VII Answers](appendix/answers-part-07.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| Four levels of a generic fn (language / typeck / mono / machine); `process::<i32/String/MyType/u8>` observed via `type_name`/`size_of` | 7.1 | — |
| Bounds checked at definition: E0369 (author) vs E0277 (caller); C++ templates check at instantiation; C++20 concepts don't check bodies | 7.1 | VI.1 |
| Post-monomorphization errors: inline `const { assert!(N > 0) }` → E0080 "while instantiating `fn first_byte::<0>`"; `cargo check` can miss them | 7.1 | XVIII.6 |
| Monomorphization collector (roots, mono items, CGUs); unused generics → no code | 7.1 | XVIII.6 |
| v0 symbol mangling observed on 1.98.1 (`_RINv…7largesthEB2_` = `largest::<u8>`; `RSh` = `&[u8]`); legacy scheme; `-C symbol-mangling-version=v0` (1.59) | 7.1 | XVIII.6 |
| Per-type asm: `largest::<u8>` cmova, `<i64>` cmovg, `<f64>` maxsd/cmpltsd blend; `Option<u8>` returned as `{ i1, i8 }` | 7.1 | — |
| Per-instance drop glue: `byte_len::<Vec<u8>>` calls `__rust_dealloc`, `byte_len::<&[u8]>` is `mov rax, rsi` | 7.1 | — |
| Iterator chain: 22 debug IR functions → 2 release functions (explains 1.3's asm) | 7.1 | X.3 |
| Lifetimes not monomorphized (reason: erased before mono) | 7.1 | XVIII.5 |
| Cross-crate instances compiled downstream from MIR; shared generics in unoptimized builds | 7.1, 7.3 | XVIII.6 |
| E0284 "type annotations needed" for `parse()` (return-type-only generic); turbofish; `where` with associated-type bounds (`K::Err: Display`) | 7.1 | XVIII.3 |
| Const generics (stable subset since 1.51; `generic_const_exprs` unstable); `Ring<T, const N>` 32 B / 528 B, no heap | 7.1 | — |
| APIT = anonymous type parameter; logstat's instances `ingest::<BufReader<File>>`, `ingest::<StdinLock>`, `report::write::<BufWriter<StdoutLock>>` observed | 7.1 | — |
| Homogeneous vs heterogeneous translation; Java erasure rationale (migration compatibility) | 7.2 | — |
| Java: erasure to bound, `checkcast`, bridge methods (ACC_BRIDGE/ACC_SYNTHETIC), Signature attribute + super type tokens, raw types, heap pollution, Integer cache (JLS 5.1.7) | 7.2 | — |
| HotSpot type profiles, mono/bi/megamorphic sites, uncommon traps/deopt, profile pollution | 7.2 | XX (PGO) |
| Boxing measured: `Vec<i64>` 1 alloc / 8,000,000 B vs `Vec<Box<i64>>` 1,000,001 allocs / 16,000,000 B | 7.2 | IX.1, XX.4 |
| Rust can do with T what Java can't: `T::default()`, `vec![T; n]`, associated consts, `TypeId` distinguishes `Vec<String>`/`Vec<u8>`; `type_name` format unspecified | 7.2 | — |
| Per-type semantics: `sum::<u32>` vectorized (paddd, 2 accumulators), `sum::<f64>` sequential addsd (FP non-associative) | 7.2 | XX.6 |
| Static vs dyn in asm: static inlines `fee_cents` (÷10,000 → magic multiply + shr 11); dyn loads vtable slot `[rsi+24]` once, `call r13` per element | 7.2 | VI.4, VI.5 |
| C# reified (shared ref-type body, per-value-type bodies), Go GC-shape stenciling + dictionaries (PlanetScale 2022), Valhalla (JEP 401) hedged | 7.2 | — |
| `dyn Any` as erasure you chose (downcast `None` → default = failure mode) | 7.2 | — |
| Instantiation growth measured (1/4/16 types): debug +104 fns/+15,275 IR lines per type; release +4 fns/+2,564 IR lines/≈+1,600 asm instrs per type; exactly linear | 7.3 | XVIII.6 |
| LLVM function merging observed in release (identical `(u32, String)` rows: small sort helpers merged into the `R0` instance; large ones not) | 7.3 | XVIII.6 |
| Non-generic inner function pattern: fat 88 IR fns / 4,830 lines vs thin 26 / 1,345 (debug); release fat bodies inlined into caller (2,269 lines) vs `inner` once (530) | 7.3 | — |
| Closures in generic fns are generic over the enclosing params → multiply instances | 7.3 | X.1 |
| `#[inline]` on generic fns adds nothing; on non-generic fns it makes them per-crate/per-CGU | 7.3 | XVIII.6 |
| CGU defaults (16 non-incremental / 256 incremental); crate splits help front end more than mono | 7.3 | XVIII.1 |
| Measuring: count IR `define`s (verified), `cargo build --timings`, `cargo llvm-lines` (unverified, third-party), `-Z self-profile` (nightly); Cranelift debug backend (nightly, hedged) | 7.3 | XVIII, XX |
| RPIT = opaque type; edition 2024 captures all in-scope lifetimes (E0502 under 2024, OK under 2021, verified); `use<..>` precise capturing (1.82) as public contract | 7.3 | — |
| APIT can't be turbofished (E0107 + note); explicit param → APIT is a semver-breaking change | 7.3 | XXV (API design) |
| Code-size budget in CI (IR lines / top generic fns / `.text`) | 7.3 | XX |

## Promises to later Parts

- **Part X (10.1, 10.3):** take the iterator chain apart adapter by adapter; closure types and capture (the
  closures-multiply-instances fact from 7.3 relies on closure types being unique per enclosing instance).
- **Part XVIII (18.3, 18.6):** trait solving for generic bodies; the monomorphization collector, CGU partitioning,
  shared generics, LLVM function merging, Cranelift. Use `-Z self-profile`/`-Z time-passes` on a generic-heavy crate.
- **Part XX:** measure pointer chasing (boxed vs flat), i-cache effects of duplicated hot code, the chunked/unordered
  `f64` sum, and a CI code-size budget in practice; PGO as the AOT answer to JIT profiles.
- **Part XVI (FFM):** concrete `extern "C"` exports over generic Rust internals (the fraud library's `score_batch` shape
  in 7.2 §9, and the cache design exercise).
- **Part VIII:** `Result<T, E>` as an ordinary monomorphized generic enum; `?` and `From` (set up in the review's
  "Looking ahead").
- **Chapter 5.3 / 5.4 back-references:** 7.2 §10 cites `Key<T>` with `PhantomData` (5.3), and the answer key cites
  type-state (5.4) as an alternative to type parameters for "can't retry" guarantees.

## Promises kept

- **Chapter 1.3 / PROGRESS "Part VII / X: monomorphization and inlining explaining the verified asm of 1.3":** 7.1 §4.
  The debug IR has 22 functions (names listed) and the release IR has 2; monomorphization supplies the static calls and
  inlining collapses them.
- **Chapter 4.3 "lifetime parameters aren't monomorphized (Part VII)":** 7.1 §4, with the reason (erased before mono,
  can't affect layout or code).
- **Chapter 1.4 compile-time mitigations (Part VII covers):** 7.3. `cargo check` (and its post-mono blind spot), crate
  splits (with the caveat that instances compile downstream), thin generic functions (measured: 88 → 26 IR functions),
  `cargo build --timings`, and `cargo llvm-lines` (labeled unverified, third-party).
- **Chapter 2.1 "generic-heavy dependencies make your crate slower to compile (Part VII)":** 7.1 §4 and 7.3 §4.
- **Chapter 1.2 Go GC-shape stenciling + dictionaries:** 7.2 §2 table and §8 (with the PlanetScale 2022 attribution).
- **Chapter 1.2 "Part VII covers monomorphization" (JIT vs AOT):** 7.2 §4 and §8 (type profiles, profile pollution,
  deopt).
- **Projects L1/L2 `impl BufRead` / `impl Write` parameters:** 7.1 §3. The real `logstat` binary's instances were
  emitted and quoted, and 7.3 explains why the reader stays generic while cold sinks can be `dyn`.
- **PROGRESS row "Java covariant arrays; PECS use-site vs declaration-site (4.4 → VI, VII)":** partially kept. 7.2
  explains why `new T[n]` is illegal (reified covariant arrays + erasure). The PECS/variance comparison is not repeated;
  it's left for Part VI if it's not done there.
- **PROGRESS row ".rlib … MIR of generic/inline fns (2.1 → VII)":** 7.1 §4 and 7.3 §4.
- **PROGRESS row "`#[inline]` and cross-crate inlining (2.7 → VII.3)":** 7.3 §3 "Why not just mark it `#[inline]`?"
  box.

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Gateway latency windows | Per-upstream `Ring<u32, 64>` (272 B inline, no allocation after startup); one route class uses `Ring<u32, 256>`; Java had `ArrayDeque<Long>` | 7.1 §9 |
| Gateway config parser profile | v0 symbols separated `parse_pairs::<String, u32>` (rate limits, hot) from `<u8, f64>` (weights); sidecar re-pushing config every second; fix in the sidecar | 7.1 §9 |
| Fraud ID de-dup incident | Home-grown `ToF64` trait; u64 user IDs above 2^53 merged (verified: 2 of 3 distinct); partner snowflake-style IDs; fix: dedupe with `T: Hash + Eq`, only counts to f64 | 7.1 §10 |
| Fraud feature library port | Java used `double[]`, primitive `LongOpenHashSet`, and banned `List<Long>` on the hot path; Rust uses `HashSet<u64>`/`Vec<f64>`; `u32` sums vectorize, `f64` stay sequential (documented; explicit chunked sum with a tolerance test where order doesn't matter); FFM exports are concrete (`score_batch(ids: *const u64, n, ...)`) | 7.2 §9 |
| Risk-limits settings incident | Java `Map<String, Object>` → Rust `HashMap<&str, Box<dyn Any>>`; refund limit stored as u32, read as u64 → `None` → default `u64::MAX` = unlimited refunds; caught by daily reconciliation; fix: typed `Limits` struct + typed keys; rule "`dyn Any` is erasure you chose" | 7.2 §10 |
| Gateway workspace diet | `Pipeline<T: Transport, C: Codec, M: Metrics, L: Limiter>`; `--timings` showed bin crate codegen as critical path; metrics → `Arc<dyn Metrics>`, limiter → enum `{ TokenBucket, Disabled }`; transport + codec stay generic; thin shells on `AsRef`/`Into` entry points; `cargo check` loop; CI IR-line budget with PR label for exceptions | 7.3 §9 |
| Session manager edition-2024 migration | Edition flipped by hand (no `cargo fix --edition`); E0502 at 14 call sites from RPIT capture; "fixed" with `&sessions.clone()` per request; p99 regression flagged; right fix `+ use<T>`; runbook now requires `cargo fix --edition` + review of each `use<..>` | 7.3 §10 |
| Event publisher (review capstone) | Before: `Publisher<T, C, R, M>` + `publish<V: Serialize>(topic: impl AsRef<str>)`, 40 event types, ~80 publish instances per service + ~80 in tests; after: `Box<dyn Transport>`, thin `publish<E: Encode + ?Sized>`, non-generic `send_encoded` (verified, 2 tests) | Part VII review |
| `meridian-log` 1.4.1 | Patch release changed `log_field<V: Display>` to `impl Display` → E0107 downstream (semver break) | 7.3 §13 (debugging exercise) |
| `meridian-telemetry` | Shared crate used by 30 services (design exercise: instantiation budget) | 7.3 §14 |

## Verification

- `listings/part-07/`: **27 files, 28 checks, all PASS** on rustc 1.98.1 (edition 2024). `ch03-05-rpit-capture.rs`
  is checked twice: `debug error:E0502` and `debug@2021 ok`. One listing runs in release (`ch02-01-boxing-cost.rs`, for
  a meaningful allocation count). There are no compiler warnings. No Miri runs were needed (the only `unsafe` is the
  counting allocator, with a SAFETY comment).
- Error codes quoted from real output: E0369, E0277 (×2), E0284, E0080, E0599, E0502, E0107. Note: `parse()` without a
  type gives **E0284**, not E0282. Argument-position `impl Trait` turbofish gives **E0107**, not E0632.
- Artifacts via `tools/emit.ps1`: release asm and IR (`ch01-02`, `ch02-03`, `ch02-04`), debug and release IR
  (`ch01-08`, `ch03-01`, and the growth listings `ch03-02/03/04`), release asm (growth), and debug asm of Project L1's
  `project-01-logstat.rs` with `-CrateType bin` (to show its `ingest::<…>` instances). All counts in 7.3 come from
  counting `define` lines and per-function line spans in those outputs.
- **Unverified, and labeled in text:** all Java/JVM behavior (no JDK in the toolchain; `javap` output marked
  "representative, abridged"; object sizes marked "typical HotSpot layout, order of magnitude, measure with JOL");
  `cargo build --timings`, `cargo llvm-lines`, `-Z self-profile`, and Cranelift (not runnable on the Playground);
  predictions in the answer key marked **(predicted)**; the x86-64 bytes-per-instruction estimate in 7.3 §5 (labeled
  an estimate); "`cargo check` can miss post-mono errors" (stated as [RUSTC], not run, since the Playground has no
  check mode).
- Code fences: runnable bin listings are `rust`; library listings (no `main`) and the test-bearing review listing are
  `rust,ignore`, with the listing file named in the text (the convention of Parts I–IV); sketches are `rust,ignore` and
  labeled "sketch".

## Word count

`wc -w`, including code and output blocks:

| File | Words |
|---|---|
| README.md | 594 |
| ch01-generics-call-site-to-binary.md | 5,981 |
| ch02-monomorphization-vs-erasure.md | 5,660 |
| ch03-code-bloat-compile-time.md | 5,233 |
| review.md | 1,852 |
| answers-part-07.md | 4,633 |
| **Total** | **23,953** |

The chapters' prose alone falls within the 3,500–5,000 target. The totals above include the long listings and compiler
output.

## Tooling notes

- **v0 mangling is the default on the Playground's 1.98.1.** LLVM IR `define` lines carry `_R…` symbols, and each is
  preceded by a `; crate::path::<Args>` comment line with the demangled name. Count functions with `grep -c '^define'`,
  and attribute them with an awk pass over the comment line before each `define` (see 7.3). Asm output is already
  demangled (`playground::largest::<u8>:`).
- **Debug IR for generic-heavy listings is large** (a 16-type listing produced ~20 MB of IR text). On this machine
  (little free disk), delete emitted artifacts from the scratchpad after extracting numbers.
- **`emit.ps1 -CrateType bin`** works on project listings with `main` (used for `project-01-logstat.rs`) and shows the
  program's real instances. `#[cfg(test)]` instances are absent (non-test build).
- **Release lib crates:** a trivially small `#[inline(never)]` instance can still vanish from its *caller* through
  LLVM's `returned`-argument propagation (`len_borrowed` became `mov rax, rsi` even though `byte_len::<&[u8]>` stayed
  out of line). Check both the callee and the caller before drawing conclusions.
- **LLVM merges identical instances in release** (observed: the small sort helpers of 16 identically laid-out row types
  collapsed into the `R0` copy). Growth experiments should use debug IR, or types with different layouts, if they want
  to count "one copy per type".
- **`sed -i` on listings is safe**, but re-run `verify.ps1` afterwards. Bash-side paths passed to `verify.ps1` must use
  forward slashes (`"C:/…/$f"`), because `"…\\$f"` escapes the `$`.
