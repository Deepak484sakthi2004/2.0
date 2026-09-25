# Part 05 report

## SUMMARY.md lines

Replace the Part V draft block with:

```markdown
- [Part V Overview](part-05-types-architecture/README.md)
  - [5.1 Algebraic Data Types: Structs, Enums, Option, Result](part-05-types-architecture/ch01-algebraic-data-types.md)
  - [5.2 Type Layout: Size, Alignment, Padding, and Niches](part-05-types-architecture/ch02-type-layout.md)
  - [5.3 Newtypes, Zero-Sized Types, PhantomData, and Markers](part-05-types-architecture/ch03-newtypes-zst-phantom.md)
  - [5.4 The Type-State Pattern](part-05-types-architecture/ch04-type-state.md)
  - [Part V Review: Type-Driven Design Capstone & Interview Mode](part-05-types-architecture/review.md)
```

Appendix entry, after "Part IV Answers":

```markdown
  - [Part V Answers](appendix/answers-part-05.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| Cardinality algebra (product/sum, Option = 1+T, Infallible = 0); "values = states" rule | 5.1 | — (done) |
| Parse, don't validate (private field + parse; E0603 private tuple ctor); sentinels → enums (Limit = 4 bytes via NonZeroU32) | 5.1 | XXII.2 (serde at boundaries) |
| Exhaustiveness: rustc_pattern_analysis, Maranget usefulness on THIR; E0004; guards don't count; `#[non_exhaustive]` (cross-crate, unverified here) | 5.1 | XVIII.4, XXVI.3 |
| Match lowering: MIR `discriminant` + `switchInt` + `otherwise: unreachable`; downcast projections; release asm merges arms | 5.1 | XVIII.4 |
| Patterns: @-bindings, slice patterns, let-else, let chains (1.88, ed. 2024), irrefutable Ok on Result<_, Infallible> (1.82) | 5.1 | — |
| `.ok()` collapsing "malformed" into "absent"; `transpose` | 5.1 | VIII.1 |
| Layout: size/align/validity; default reorders (Order 24 vs repr(C) 32, offsets verified); u128 align 16 (1.77–1.78) | 5.2 | XVI.1, XX.5 |
| repr family: C, transparent, u8 (RFC 2195), packed (E0793 + read_unaligned, Miri-clean), align(N) | 5.2 | XVI.1 |
| Niches counted: Option<Option<NonZeroU64>> 16, Option³<bool> 1, Msg (24) vs Msg2 (16) multi-variant niche, Payload 16, Result<(), Box<str>> 16; guaranteed (std Option docs) vs [RUSTC] | 5.2 | XV.1 |
| Niche asm: Option<NonZeroU64> one register vs Option<u64> two (ScalarPair); Option<&T> None = null test; niche slice sum vectorizes with no checks | 5.2 | XX.6 |
| Invalid enum tag via transmute = UB (Miri "expected a valid enum tag"); TryFrom<u8> for wire enums | 5.2 | XV.1 |
| Padding = uninitialized; bytemuck derive(Pod) rejects padding (E0080 const eval); explicit `_pad` | 5.2 | XXIII.5 |
| Cache-line alignment: CachePadded via repr(align(64)); crossbeam uses 128 on x86-64/aarch64 | 5.2 | XIV.4, XX.5 (measure) |
| Newtypes nominal + erased; `discount_typed = discount_raw` (LLVM merged, asm alias) | 5.3 | VII.1 |
| PhantomData steering table (variance / Send+Sync / dropck); invariant brand (lifetime error verified); PhantomData<T> leaks !Send (E0277 verified) | 5.3 | XI.2, XV |
| Edition 2021 disjoint closure capture hides !Send of a phantom-typed struct (verified) | 5.3 | XI.2 |
| Derive adds bounds on all type params (E0382 "derived Clone adds implicit bounds"; E0277 Debug on Payment<S>) | 5.3, 5.4 | VI.1 |
| ZSTs: Vec<ZST> capacity usize::MAX, 0 allocations (counting allocator); HashSet = HashMap<K, ()> | 5.3 | XV.4 |
| Marker traits as policy (Idempotent) + `#[diagnostic::on_unimplemented]` (1.78) | 5.3 | VI.1 |
| serde transparent + try_from at the JSON boundary (verified) | 5.3 | XXII.2 |
| Type-state: impl per instantiation (E0599 w/ "method was found for") + consuming self (E0382); sealed State (E0277 "sealed trait" note); uninhabited markers | 5.4 | VI.3 (coherence), VII.3 (mono cost) |
| Data-carrying states vs PhantomData markers; failure returns (SameState, Error) | 5.4 | — |
| Type-state builder (RefundBuilder<Set, Missing> E0599) | 5.4 | — |
| Hybrid AnyPayment boundary (from_row / apply delegating to typed transitions) | 5.4 | XXIII.6 |
| Type-state asm: settle_typed = `mov rax, rsi; ret`; run-time check = cmp/jne/store | 5.4 | — |
| History: Strom & Yemini 1986; Rust's built-in typestate removed 2012; init/move tracking survives (E0381/E0382) | 5.4 | — |

## Promises to later Parts

- **Part VI:** why the sealed-trait trick works (coherence, 6.3); derive and trait bounds (6.1); `AnyPayment`-style enums
  vs `Box<dyn Trait>` (6.4–6.5); `?Sized` (mentioned in 5.3).
- **Part VII:** monomorphization cost of type-state generic impls (7.3); ZST policy parameters in generic containers.
- **Part VIII:** `?`, `transpose`, and error-enum design beyond the 5.1 config loader (8.1–8.2).
- **Part XI:** Send/Sync manual impls and why they're unsafe (11.2); the disjoint-capture subtlety for thread closures.
- **Part XV:** validity invariants and niches under `unsafe` (15.1); `#[may_dangle]` and PhantomData's drop-check row.
- **Part XVI:** `Option<&T>`/`repr(transparent)` guarantees at FFI; `repr(C)` enums (RFC 2195) (16.1).
- **Part XX:** measure false sharing with `CachePadded` (20.5); measure `tagged_in_memory` vs `niche_in_memory`
  (5.2 systems exercise); AoS vs SoA / hot-cold splitting (20.5).
- **Part XXII:** serde `transparent` / `try_from` at boundaries in depth (22.2); `#[non_exhaustive]` semver
  (cargo-semver-checks).
- **Part XXIII:** persistent encodings instead of in-memory layout (23.5); DB compare-and-set for state transitions
  (23.6).

## Promises kept

- **4.4 → V.3:** PhantomData variance steering: full table plus verified invariance (lifetime error) and auto-trait
  (E0277) behavior, and branded indices.
- **2.5 (+ 4.x) → V.4:** payment state machine (Pending/Authorized/Captured/Refunded/Failed) as type-state; invalid
  transition E0599 ("no method named `refund` found for struct `Payment<Pending>`"); double capture E0382; 2.5's design
  exercise (order lifecycle) revisited as V.4's design exercise ("the fourth option").
- **2.2 → V.3:** `Percent` vs `BasisPoints` newtypes; the incident replayed as E0308.
- **Part II review → V.3:** `TenantId` (typed IDs; swapped-ID failure scenario).
- **Part III review → V.2/V.3:** `OrderId`, `Price`/`Cents`, `Side` (`repr(u8)` + `TryFrom<u8>`, `Option<Side>` = 1);
  order-book memory budget (24 vs 32 bytes).
- **2.6 → V.1:** "parse, don't validate" (Email, onboarding scenario).
- **1.2 / 2.6 → V.2:** niches in depth (Option<u64> = 16 explained by counting; Option<Shape>; MessageBoxed/large
  variants), verified sizes and asm showing niche checks.
- **1.2 / 2.3 → V.4:** Rust's early built-in typestate, removed before 1.0.
- **SPEC Part V:** Connection Disconnected/Connected/Authenticated as types (E0599 for query on Connected).

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Merchant onboarding (Java) | Validators class, 14 call sites; CSV importer skipped e-mail validation (~3,000 bad addresses, 2025 audit); Rust core takes Email/Iban/CountryCode, errors collected per row | 5.1 |
| Gateway rate-limit sentinel | Java `int requestsPerSecond`, 0 = unlimited; on-call set 0 to block an abusive partner → unlimited for 40 min; fix `enum Limit { Unlimited, PerSecond(NonZeroU32) }` (4 bytes) | 5.1 |
| Order-book memory budget | 10M resting orders/instance; repr(C) draft 32 B (320 MB) → default 24 B (240 MB); Side repr(u8); parent_order Option<OrderId(NonZeroU64)> 8 B; journal uses explicit encoding; rule "repr(C) marks a boundary" | 5.2 |
| Trade-archive checksum incident (C++) | fwrite of padded struct; nondeterministic checksums across two sites; padding leaked previous request data to a partner export; Rust port blocked by bytemuck derive(Pod) | 5.2 |
| Account-service negative cache | 50M entries; Option<Option<NonZeroU64>> 24-byte entries → Option<NonZeroU64> 16-byte (debugging exercise) | 5.2 |
| meridian-types crate | Cents/Percent/BasisPoints, Id<T> (PhantomData<fn() -> T>, pub(crate) ctor), serde transparent/try_from at JSON boundary; gateway retry middleware requires `R: Idempotent` | 5.3 |
| Swapped-ID cancellation (Java) | cancel(tenantId, orderId) reordered; 11 of 12 call sites updated; tenant 7 cleanup cancelled order #7 of tenant 9; fixes: typed IDs, no forging, colliding-ID fixtures | 5.3 |
| Fraud library multi-currency | 2024 bug summing EUR and JPY (design exercise) | 5.3 |
| Payment core (Rust) | typed core Payment<S> + AnyPayment boundary + DB compare-and-set `UPDATE .. WHERE state = ..` | 5.4 |
| Double-capture incident (Java) | retry on SocketTimeoutException re-called capture(p) with stale AUTHORIZED status; fix layers: consuming self, idempotency key, CAS + reconciliation | 5.4 |
| Refund service | Part V review capstone: Java Refund class (nullable fields, string status), four-eyes rule, 200M rows, 2M active; model answer listing | Part V review |

## Verification

- `listings/part-05/`: **46 files, 48 checks, all PASS** on the Playground (rustc 1.98.1, edition 2024). Full log was
  produced with `tools/verify.ps1 listings/part-05 -ShowStderr`; no warnings in any `ok` listing.
- Outcomes used: `ok` (25), `build` (4, artifact sources), `miri-ok` (2), `miri` (1), compile errors: E0004, E0080-via-`error:padding`, E0277 (×4),
  E0308 (×2), E0382 (×2), E0599 (×3), E0603, E0793, `error:lifetime` (invariance; the error has no code).
- **Miri**: 3 runs: `ch02-05-packed-fixed.rs` (miri-ok, read_unaligned), `ch02-09-repr-u8.rs` (miri-ok, transmute of
  `None::<Side>` to u8), `ch02-10-invalid-tag.rs` (miri: "constructing invalid value of type Side ... expected a valid
  enum tag").
- **Artifacts** (`tools/emit.ps1`): MIR (debug) of `ch01-09-match-mir.rs`; release asm of `ch01-09`, `ch02-03-niche-asm.rs`,
  `ch03-06-zero-cost-asm.rs` (shows `playground::discount_typed = playground::discount_raw`), `ch04-09-typestate-asm.rs`.
- Every `rust` / `rust,compile_fail` block in the chapters was machine-checked to be a verbatim substring of a verified
  listing (17 blocks, all matched). `rust,ignore` blocks are excerpts naming their listing; one answer-key sketch (manual
  `Debug` for `Payment<S>`) is labeled unverified.
- Unverifiable here and labeled: `#[non_exhaustive]` cross-crate behavior (needs a two-crate workspace);
  `-Zprint-type-sizes` (local nightly command given); false-sharing timing (deferred to 20.5 as an exercise); niche vs
  tagged loop timing (exercise, "predicted from mechanism").
- Crates used: `bytemuck` (Pod derive), `serde` + `serde_json` (boundary listing). Both available on the Playground.

## Word count

- README 586; 5.1 5,025; 5.2 5,220; 5.3 4,729; 5.4 5,267; review 1,408; answers 4,334. **Total ≈ 26,600** (`wc -w`,
  code included).

## Tooling notes

- `error:<word>` needles are a single `\w+` word; for errors without a code (e.g., "lifetime may not live long enough")
  use `error:lifetime`.
- bytemuck's padding rejection surfaces as `error[E0080]: evaluation panicked: derive(Pod) was applied to a type with
  padding` (use `error:padding` as the needle, since E0080 is generic).
- Edition 2021+ disjoint closure capture: `move || id.raw` captures only the `u64` field, so auto-trait errors (Send)
  don't fire. Force a whole-value capture (pass the value to a function inside the closure) when demonstrating `!Send`.
- `#[derive(Debug)]` on a type-state `Payment<S>` with uninhabited marker enums fails as soon as `Debug` is *used*
  (E0277 on the marker); it compiles silently if unused. Listing authors: implement `Debug` manually or omit it.
- LLVM merges identical functions into aliases in release asm (`a = b` lines in emit output). Functions that differ only
  in panic `Location` (source line) are not merged.
- `Option<Side>` for a `repr(u8)` enum with discriminants 1 and 2 stored `None` as 0 on rustc 1.98.1 ([RUSTC] choice).
