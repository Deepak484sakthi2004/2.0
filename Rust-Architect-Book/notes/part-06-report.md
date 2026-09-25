# Part 06 report

## SUMMARY.md lines

Replace the Part VI draft block with:

```markdown
- [Part VI Overview](part-06-traits/README.md)
  - [6.1 Traits, Bounds, and Default Methods](part-06-traits/ch01-traits-bounds.md)
  - [6.2 Associated Types, Generic Traits, and Blanket Impls](part-06-traits/ch02-associated-generic-blanket.md)
  - [6.3 Coherence and the Orphan Rule](part-06-traits/ch03-coherence-orphan.md)
  - [6.4 Trait Objects, vtables, and dyn Compatibility](part-06-traits/ch04-trait-objects.md)
  - [6.5 Static vs Dynamic Dispatch: The Architect's Decision](part-06-traits/ch05-static-vs-dynamic.md)
  - [Part VI Review: Trait-Design Review & Interview Mode](part-06-traits/review.md)
```

Appendix entry, after "Part V Answers":

```markdown
  - [Part VI Answers](appendix/answers-part-06.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| Trait = contract, impl = evidence, bound = requirement; three bound spellings; `impl Trait` arg = anonymous generic | 6.1 | — (done) |
| Generic bodies checked at definition (E0369) vs C++ templates; Java also checks against bounds | 6.1 | VII.1 |
| Default methods instantiated per impl; default-method trap (Retry-After 0 → retry storm); rules for safe defaults | 6.1 | — |
| Method resolution algorithm (deref chain × U/&U/&mut U, inherent before trait, in-scope traits); `Tracked<T>` Deref counter (2 reads) | 6.1 | — (done; promise from 2.6/3.4 kept) |
| AsRef/Into parameters measured (Into<String>: 0 allocs for String, 1 for &str); std inner-fn pattern (sketch) | 6.1 | VII.3 |
| Fully qualified syntax; E0034 ambiguity; defaulted method = "possibly breaking" (Cargo SemVer guide) | 6.1 | XXII (semver tooling) |
| `#[derive]` bounds every type parameter (E0599 on `Id<Merchant>.clone()`); manual impls for PhantomData tags | 6.1 | — |
| RPIT captures all in-scope generics/lifetimes in ed. 2024; `use<..>` (1.82) | 6.1 | XII (async return types) |
| Associated types = outputs, params = inputs; projection/normalization; E0282 with generic traits | 6.2 | — |
| ATB syntax `Decoder<Error: Display>` (1.79); GATs (1.65) | 6.2 | X (lending iterators) |
| Blanket impls: ToString/Display, Into/From, reflexive From; std's internal specialization for to_string | 6.2 | VII.2 |
| Supertraits = where Self: Trait; implied bounds only for supertraits | 6.2 | — |
| New blanket impl = breaking change (E0119 in downstream `Money` impl); opt-in marker pattern | 6.2 | XXII |
| Coherence; precise orphan rule (Reference); fundamental types; dyn LocalTrait local; E0117/E0210; RFC 2451 (1.41) covered params | 6.3 | — (done; promise from 2.1/2.7 kept) |
| Overlap check "intercrate mode"; negative reasoning only for local items ("upstream crates may add a new impl") | 6.3 | XVIII (trait solver) |
| Reflexive `impl<T> From<T> for T` collides with `impl<T> From<T> for Local` (E0119 verified) | 6.3 | — |
| Newtype vs extension trait vs upstream feature vs behavior-as-value; Deref newtype: view vs leak | 6.3 | — |
| Sealed traits explained (privacy + supertrait obligation; coherence makes it pay off) | 6.3 | — (Part V promise kept) |
| Coherence vs C++ ODR vs Java runtime registries; feature unification; two semver-incompatible crate versions = distinct types (unverified, labeled) | 6.3 | XXII |
| Specialization unstable (1.98); std uses min_specialization | 6.3 | XXVI (language design) |
| Fat pointers all 16 bytes (measured, incl. Option<Box<dyn>> niche); size_of_val via vtable | 6.4 | — |
| vtable in LLVM IR: [drop glue, size, align, methods]; null drop entry for no-drop-glue types [RUSTC]; unsizing = pairing with constant | 6.4 | XVIII |
| Dyn call asm `call qword ptr [rax + 24]`; f64 accumulator spilled per call (SysV XMM caller-saved) vs static unrolled ×8 | 6.4 | XX |
| Dyn-compatibility rules derived from vtable; E0038 (generic method; Clone supertrait); `where Self: Sized`; extension trait over ?Sized; `fn _assert(&dyn T)` guard | 6.4 | XII (async fn in traits) |
| Trait upcasting (1.86); Any/TypeId downcast; Box<dyn Any> type_id trap (clippy type_id_on_box) | 6.4 | VIII (Error::downcast_ref) |
| dyn Trait + Send + Sync (E0277 chain through Box/Vec/spawn); auto traits in object types | 6.4 | XI.2 |
| Trait object variance: generic args invariant, lifetime bound covariant (verified) | 6.4 | — (promise from 4.4 kept) |
| vtable duplication across CGUs; `ptr::addr_eq` (1.76); ZST data pointers | 6.4 | — |
| Java: thin pointer/fat object vs fat pointer/thin object; itables, inline caches, CHA, deopt; PECS vs Rust variance | 6.4 | VII.2 |
| Dispatch benchmark (1M elements): dyn mixed 3.26–3.28, dyn grouped 1.99–2.00, enum 1.01–1.02, static per-type 0.80–0.82 ns (4 runs); allocs 1,000,001/1/3 | 6.5 | XX (perf stat confirmation) |
| Enum dispatch asm: inlined match, branch-free Tiered arm (setge + indexed load), /10_000 as multiply-shift | 6.5 | X, XX |
| Ratio test (dispatch overhead ÷ work per call) with Meridian numbers | 6.5 | XX |
| Seven-dimension decision matrix (compile time, size, dispatch, locality, extensibility, ABI, plugins) | 6.5 | VII.3 |
| impl Trait return is one type (E0308) → Box<dyn> or enum; static Stack<A,B> vs Plugins vs hybrid (type_name growth) | 6.5 | XXII.3 (Tower BoxService) |
| No stable Rust ABI; hand-built C-ABI vtable (`#[repr(C)]`, extern "C", plugin-provided drop), Miri-clean | 6.5 | XVI |
| Plugin boundaries: C ABI vs WASM vs out-of-process | 6.5 | XVI, XXII |

## Promises to later Parts

- **Part VII:** monomorphization mechanics (7.1), erasure vs monomorphization as the root of dyn-compatibility rules
  (7.2), code-size/compile-time bill of generics and `dyn` at boundaries to cap it (7.3). The inner non-generic function
  pattern is shown only as an unverified sketch (`std::fs::read`).
- **Part VIII:** `Error::downcast_ref` as a legitimate `Any` use; decoder associated `Error` types → proper error enums
  (6.2 §9: bound `D::Error: std::error::Error`).
- **Part X:** GATs and lending iterators (6.2 §4).
- **Part XI:** Send/Sync auto traits in depth (6.4 used `dyn Trait + Send + Sync` and E0277 only).
- **Part XII:** `async fn` in traits and dyn compatibility (6.4 table + design exercise leave it open).
- **Part XVI:** loading a `cdylib` plugin (libloading) for the C-ABI vtable in 6.5; `repr(transparent)` ABI guarantee
  at FFI.
- **Part XVIII:** trait solver / intercrate mode / vtable layout internals (mentioned as [RUSTC]).
- **Part XX:** confirm the branch-misprediction explanation with `perf stat -e branch-misses`; PGO indirect-call
  promotion.
- **Part XXII:** `cargo-semver-checks`, `cargo tree -d`/`cargo deny` for duplicate crate versions; Tower
  `BoxService` (Chapter 22.3) as the hybrid static/dynamic pattern.

## Promises kept

- **Deref / auto-deref / deref coercion** (Ch 3.4, 2.6 method resolution through Box/Rc): 6.1 §3–§4, with a counting
  `Tracked<T>` smart pointer (verified: 2 reads).
- **AsRef/Into for flexible parameters** (Ch 3.3): 6.1 §3, measured allocations.
- **`impl Trait` params** (Project L1): 6.1 §2/§4.
- **Orphan rule** (Ch 2.1, 2.7): 6.3 in full.
- **dyn Trait variance** (Ch 4.4 answers): 6.4 §4, verified.
- **Expression problem** (Ch 2.5): 6.5 §2 mental model and interview Q1.
- **Send/Sync as auto traits** (Ch 1.3): 6.4 §9 (briefly; Part XI deep).
- **Function items vs pointers → static vs dynamic dispatch** (Ch 2.4): 6.5 §2.
- **Part V promises:** sealed-trait mechanism (6.3 §7), derive and bounds (6.1 §4), AnyPayment-style enum vs `Box<dyn>`
  (6.5 §3 `AnyFee`), `?Sized` (6.2).
- **Part VII left the Java PECS/variance comparison to Part VI:** done in 6.4 §8.

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Gateway rate limiter trait | `RateLimiter { try_acquire(&mut self, now_ms) }` + defaults; TokenBucket (burst keys) and FixedWindow (partner quotas); time as a parameter; per-key `HashMap<ApiKey, TokenBucket>` | 6.1 |
| Retry-After default incident | `retry_after_ms()` defaulted to 0; GlobalConcurrency limiter forgot override → retry storm (metastable); rules for safe defaults, conformance test | 6.1 |
| Market-data feed decoders | `Decoder { type Output; type Error }`: CSV quote feed (`MRDN,12550`) + 4-byte BE sequencer; `Decoder<Output = Quote>` for the quote book | 6.2 |
| `meridian-log` blanket impl incident | v1.4 (minor) added `impl<T: Display> LogLine for T`; payments' redacting `impl LogLine for Money` → E0119; deleting it logged amounts in clear; now opt-in marker + major bump | 6.2 |
| Pool metrics export | third-party `pgpool::PoolStatus` + third-party metrics `Collector`: orphan; newtype `PoolCollector` + `PoolMetricsExt`; upstream feature issue opened | 6.3 |
| Two `uuid` versions | audit lib on uuid 0.8, service on 1.x → `Uuid: AuditKey` not satisfied; workspace deps + `cargo tree -d` + `AuditId` newtype (unverified: multi-crate) | 6.3 |
| Gateway middleware pipeline | config strings `"auth, ratelimit=500, tenant"`, factory registry `HashMap<&str, fn(&str) -> Box<dyn Middleware>>`, `clone_box`, `Any` supertrait for inspection; shared via `Arc<Vec<Box<dyn Middleware + Send + Sync>>>` | 6.4 |
| Middleware crate v2.3 incident | generic `record<M: Debug>` broke every `dyn` user (E0038); `: Clone` hotfix also E0038; fix `where Self: Sized` + ext trait + dyn guard; semver policy lists dyn-incompatibility | 6.4 |
| Fraud-rule engine design | core rules = enum `CoreRule`; features = static generics over contiguous data; experimental rules = `Vec<Box<dyn Rule + Send + Sync>>` from config (ship with library); analytics logic = out-of-process sidecar with timeout; C-ABI cdylib rejected | 6.5 |
| Market-data `Box<dyn Field>` incident | 30 fields/msg × 2M msg/s = 60M boxes + indirect calls/s; redesign: enum field kinds + one `dyn FeedHandler` per feed; zero-allocs-per-message CI benchmark | 6.5 |
| Fraud rules SDK v0.1 → v0.2 | Part VI review capstone: E0117 + E0038 (Clone, then generic explain) + silent `block_at() = 0`; v0.2 `Rule: Send + Sync`, `&mut dyn fmt::Write`, engine-level `block_at: 80` | Part VI review |

## Verification

- `listings/part-06/`: **41 files, 43 checks, all PASS** on rustc 1.98.1 stable, edition 2024 (`tools/verify.ps1`).
  Outcomes: 22 `ok` (one `release ok` benchmark), 3 `release build` (codegen listings), 17 `error:*` (E0369, E0034,
  E0599, E0282, E0119 ×3, E0117 ×2, E0210, E0038 ×4, E0277, E0308, `lifetime`), 1 `miri-ok`.
- **Miri:** `ch05-06-c-abi-vtable.rs` (hand-built C-ABI vtable with raw pointers, Box::into_raw/from_raw): clean.
- **Artifacts** (`tools/emit.ps1`, release): LLVM IR of two vtables (`ch04-03-vtable.rs`: Label with drop glue, size 40;
  Circle with a null drop entry); asm of `total_area_dyn` vs `total_area_static::<Circle/Square>` (`ch04-02`); asm of
  `sum_dyn` vs `sum_enum` (`ch05-05`). Symbol names in the IR are v0-mangled (`_R…`) and were demangled by hand (stated in
  the text).
- **Measurements:** `ch05-01-dispatch-bench.rs` run 4 times on the Playground (release); ranges quoted; labeled
  "one run each, shared machine, noisy". Allocation counts via the Part III counting allocator.
- A script check confirmed all 22 `rust`/`rust,compile_fail` blocks in the Part VI chapters match a verified listing
  verbatim (minus `// verify:` headers). `rust,ignore` blocks are named excerpts or labeled sketches.
- **Unverifiable, labeled:** two semver-incompatible `uuid` versions (needs a Cargo workspace; exact local commands
  given); loading a `cdylib` plugin (Part XVI); `perf stat` confirmation of branch misses (exercise); the
  `std::fs::read` inner-fn pattern and the `CloneBox` helper in the answer key are marked as unverified sketches.
- Two first-draft mistakes caught by verification and fixed: the `Convert<T>` ambiguity is E0282 (not E0283) on 1.98.1;
  the pipeline listing's single `println!` borrowing `req` twice (E0502), which is mentioned in 6.4 §9 as a teaching
  aside.

## Word count

Chapters 23,461 (6.1: 4,798 · 6.2: 3,686 · 6.3: 3,929 · 6.4: 5,679 · 6.5: 5,369) + README 668 + review 1,703 +
answers 5,906 = **31,738 words**.

## Tooling notes

- `error:E0283` vs `E0282`: rustc 1.98.1 reports **E0282** for an unannotated `let x = y.convert()` when two impls of a
  generic trait apply and the value's method is used next. Don't assume E0283.
- The Playground's LLVM IR uses **v0 symbol mangling** (`_RNv…`). Demangle by hand and say so, or read the `; comment`
  lines rustc emits above each `define`.
- Drop-glue functions appear as `core::ptr::drop_glue::<T>` in 1.98.1 IR, and vtables of types without drop glue store
  a **null** first word (`[24 x i8]` of data at the start of the constant).
- The E0038 diagnostic lists **one dyn-compatibility reason at a time** when a `Sized` supertrait is present: fix it
  and the next (e.g., a generic method) appears. Useful for exercises; verified in `review-rules-sdk-step2.rs`.
- Timing listings: use `std::hint::black_box` on inputs and a best-of-N loop. The Playground gave very stable numbers
  (±0.02 ns across 4 runs) for 1M-element loops.
- `verify.ps1` accepts two headers in one file (for example `error:E0038` + `error:E0117`) to assert several error codes
  in one compile. Each header is a separate Playground request.
