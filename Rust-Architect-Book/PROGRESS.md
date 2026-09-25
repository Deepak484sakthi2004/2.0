# Progress & Continuity Ledger

Last updated: 2026-09-25 (Parts V–IX complete; X, XI, XII, XIV, XV in progress) · Baseline: Rust 1.98.1 stable, edition
2024 (Playground-verified)
Published to: https://github.com/Deepak484sakthi2004/2.0 (folder `Rust-Architect-Book/`)

Parts V onward are written by parallel writers following `notes/AUTHORING-BRIEF.md`; each writer's full report
(verification details, word counts, tooling notes) is `notes/part-NN-report.md`. This ledger merges what later Parts
need: concepts, promises, and Meridian facts.

## Status

| Part | Title | Status | Listings verified |
|---|---|---|---|
| — | Preface | **Written** | — |
| I | Why Rust Exists | **Written** (4 chapters + review + answer key) | 26 files / 27 checks, all pass |
| II | Rust From First Principles | **Written** (7 chapters + Project L1 `logstat` + review + answer key) | 39 files / 44 checks, all pass |
| III | Ownership: The Core of Rust | **Written** (6 chapters + Project L2 `redact` + review + answer key) | 38 files / 44 checks, all pass |
| IV | The Borrow Checker | **Written** (6 chapters + review capstone + answer key; no project by design) | 34 files / 36 checks (incl. 2 Miri), all pass |
| V | Types as Architecture | **Written** (4 chapters + review capstone + answer key) | 46 files / 48 checks (incl. 3 Miri), all pass |
| VI | Traits | **Written** (5 chapters + review capstone + answer key) | 41 files / 43 checks (incl. 1 Miri), all pass |
| VII | Generics and Monomorphization | **Written** (3 chapters + review capstone + answer key) | 27 files / 28 checks, all pass |
| VIII | Error Handling | **Written** (4 chapters + review capstone + answer key) | 38 files / 42 checks (incl. 2 Miri), all pass |
| IX | Collections and Memory | **Written** (5 chapters + BFS/DFS interlude + review + answer key) | 32 files / 40 checks, all pass |
| X, XI, XII, XIV, XV | Iterators · Concurrency (L3, L4) · Async · Memory Model · Unsafe | In progress (wave 2) | — |
| XIII, XVI–XXVI | … | Planned (see `src/SUMMARY.md`) | — |

All listing counts were re-verified by the integrator after each writer finished (independent `tools/verify.ps1` run).

## Part IX concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| Vec layout (cap, ptr, len) seen in asm; field order [RUSTC], triple [LANG] | 9.1 | — |
| Vec growth max(2·cap, cap+1), min cap 8/4/1 by element size [LIB], verified + in `grow_amortized` asm | 9.1 | — |
| push fast path (compare/store/inc) vs cold `grow_one`; OOM in push = abort; `try_reserve` | 9.1 | XV (fallible alloc) |
| reserve vs reserve_exact; truncate/clear keep capacity; shrink_to_fit; capacity policies | 9.1 | XIII (buffer pools) |
| collect sizing: exact size_hint = 1 alloc; filter grows; in-place collect = 0 allocs | 9.1 | X (how the specialization works) |
| glibc realloc: in-place for small, mremap for ≥ mmap threshold (inferred, strace to confirm) | 9.1 | XX |
| arrays / slices / Box<[T]> / Vec / SmallVec sizes (verified) | 9.1 | — |
| String building: `s + &t` reuses buffer; `format!("{s}..")` quadratic; write! into buffer; format! capacity heuristic | 9.2 | XX |
| format_args! compile-time parsing; runtime Arguments + dyn Write | 9.2 | XVII/XVIII (macros, builtins) |
| UTF-16: encode_utf16, from_utf16 lone surrogates, from_utf16_lossy; OsString/WTF-8 on Windows | 9.2 | XVI (FFI strings) |
| JNI Modified UTF-8 vs FFM standard UTF-8 | 9.2 | XVI |
| Case mapping allocates, can change length (ß, ﬁ, İ, final sigma); ASCII variants in place; Kelvin sign | 9.2 | — |
| String vs Box<str> vs Arc<str> vs Cow<str> (sizes, clone costs verified); small-string crates | 9.2 | — |
| memchr/memchr3/memmem SIMD search (~10× measured); skip-search only pays for rare targets; 256-entry byte-class table | 9.2 | XX |
| SwissTable/hashbrown: control bytes, h1/h2, SIMD groups, 7/8 load, tombstones | 9.3 | — |
| HashMap capacity sequence, rehash-on-resize counts (verified with counting hasher) | 9.3 | — |
| entry vs get_mut+insert vs hashbrown entry_ref: hashes AND allocations (verified) | 9.3 | — |
| Hasher choice: SipHash (keyed), foldhash/ahash, FxHash; HashDoS (measured quadratic with Fx + crafted keys) | 9.3 | XXI (network input) |
| f64 keys → E0599 (bounds on impl block); OrderedFloat; integer minor units | 9.3 | — |
| Hash/Eq and Ord/Eq contracts = logic errors, not UB | 9.3, 9.4 | XV |
| Concurrent map options (Mutex/RwLock, sharding, ArcSwap, owner thread) | 9.3 | XI |
| VecDeque ring (head+len since 1.67), as_slices, make_contiguous | 9.4 | — |
| BTreeMap B=6, 11 keys/node, linear in-node search; BTree vs HashMap measured | 9.4 | — |
| BinaryHeap max-heap, Reverse, top-k, EDF, lazy deletion; derived Ord = field order | 9.4 | — |
| LinkedList: 1M allocs, 4.7×/47× slower traversal; cursors unstable on 1.98.1 (E0658 verified) | 9.4 | XV (intrusive lists) |
| Memory hierarchy rules: bytes touched, dependent loads, predictability; MLP; throughput ≠ 1/latency | 9.5 | XX |
| AoS vs SoA measured; Vec<Box<T>> in-order vs shuffled measured; lookup structures by n measured | 9.5 | XX |
| rustc field reordering; tuple padding | 9.5 | — |
| False sharing, CachePadded (mentioned) | 9.5 | XIV |
| Recursive frame size measured (224 B debug / 96 B release); overflow verified at 110% of prediction | Interlude | — |
| Guard page + sigaltstack SIGSEGV handler; stack probes (asm verified) | Interlude | XIX (binary/OS) |
| Explicit-stack DFS with (node, edge) frames; mark-on-push is not DFS (verified) | Interlude | — |
| BFS frontier width vs DFS depth (verified); shortest path property | Interlude | — |
| Stack sizes by platform/runtime (2 MiB spawned, 8 MiB Linux main, 1 MiB Windows main, Tokio 2 MiB, JVM 1 MiB, Go growable); RUST_MIN_STACK | Interlude | XII (async recursion Box::pin), XIII |
| serde_json recursion limit 128 as depth-limit example | Interlude | XXII |
| Twelve-criterion decision matrix format | Interlude | every later "X vs Y" |

## Part VIII concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| Option vs Result vs Result<Option<T>,E>; absence may default, malformed never does | 8.1 | — |
| `?` desugaring: Try::branch / FromResidual (unstable, [VERSION]); verified MIR of two `?`s | 8.1 | XVIII.4 (MIR) |
| E0277 for `?` in `()` fn, missing From, and Option-`?` in Result fn (all verified) | 8.1 | — |
| `main -> Result`: Termination prints `Error: {Debug}`, exit 1 (verified) | 8.1, 8.3 | — |
| `#[must_use]` Result warning text (verified); `-D warnings` / deny(unused_must_use) policy | 8.1 | XXII |
| Result sizes: Result<u64,()>=16, <&u64,()>=8, <NonZeroU32,()>=4, ParseIntError=1, io::Error=8, Result<(),io::Error>=8, Box<dyn Error+Send+Sync>=16, anyhow::Error=8 (verified) | 8.1 | V (niches) |
| Release asm of `?` happy path: Result<u32,ParseIntError> packed in rax, one test+branch per `?` | 8.1 | XX |
| Error type as API: programs (variants) vs people (Display) vs developers (Debug); chain rule (cause once, via source) | 8.2 | — |
| Library vs application errors; thiserror (expansion shown) vs anyhow ({}, {:#}, {:?}, downcast_ref, chain, root_cause) | 8.2 | XXII.1 |
| Box<dyn Error> / anyhow: 1 alloc per error, 0 on success (counting allocator, verified) | 8.2 | — |
| Large error cost: Result<u64,516-byte E>=520 B; sret + 504-byte memcpy per `?` layer vs cmove for boxed (verified asm); clippy result_large_err (unverified, labeled) | 8.2 | XX |
| std::backtrace::Backtrace: capture() Disabled by default; force_capture ~30 µs, first render ~4.6 ms (one run) | 8.2 | XIX |
| #[non_exhaustive] (no effect inside defining crate); opaque io::Error-style errors; don't leak dependency error types | 8.2 | XXII |
| Panic flow: hook → panic runtime → two-phase unwind → landing pads; payload types (&str vs String) | 8.3 | XVIII, XIX |
| Landing pad in release asm (`_Unwind_Resume` after `ret`) | 8.3 | XIX |
| Aborts: double panic ("panic in a destructor during cleanup"), extern "C" since 1.81 ("panic in a function that cannot unwind"), panic=abort (process::abort analog) — all verified | 8.3 | XVI |
| Exit statuses verified by re-exec: Err→1, panic→101, exit(3)→3, abort/extern-C→signal 6 | 8.3 | XIX.4 |
| Panic cost measured: Err 3.0/15.8 ns vs panic+catch_unwind 1.8/3.2 µs at depth 1/10 (release, one run) | 8.3 | XX |
| Mutex poisoning (into_inner, clear_poison 1.77), join() Err payload, UnwindSafe/AssertUnwindSafe | 8.3 | XI.3 |
| Panic boundaries: hook + per-request catch_unwind; Tokio JoinError::is_panic (verified); FFI entry catch_unwind → codes; extern "C-unwind" | 8.3 | XIII.2, XVI.3 |
| Fallible cleanup: Tx::commit(self) -> Result<Committed, CommitError{RolledBack, OutcomeUnknown}> | 8.3 | XXIII.6 |
| Error taxonomy: Rejected / Transient / Ambiguous / Internal; four boundary questions | 8.4 | XXI.4, XXIV.1 |
| Exhaustive error→status mapping (E0004 on new variant, verified); stable codes; safe client bodies with request_id | 8.4 | XXI.6, XXII.3 |
| Retries: exponential backoff + full jitter, deadline budget, Retry-After, retry only transient / ambiguous-if-idempotent (verified sim) | 8.4 | XXI.4 |
| Idempotency keys: store with effect, fingerprint, in-progress + conflict; key-per-attempt bug (verified) | 8.4 | XXIV |
| Timeouts ambiguous at the socket; io::ErrorKind by phase (refused vs reset vs read timeout) | 8.4 | XXI.1 |
| Retry amplification (27×), retry budgets, deadline propagation, fail open/closed | 8.4 | XXI.4 |
| Observability: log once at boundary, level by class, chain in logs only, metric labels low-cardinality (tracing output verified) | 8.4 | XXII.5 |

## Part VII concepts introduced

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

## Part VI concepts introduced

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

## Part V concepts introduced

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

## Part IV concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| Places/projections, loans (place, kind, region), conflict table incl. prefixes/extensions; E0503 | 4.1 | XVIII.5 |
| `&mut` = unique, `&` = shared; UnsafeCell as sole primitive; Cell/RefCell/OnceCell/Mutex/atomics table | 4.1 | XI, XIV, XV |
| Verified IR/asm: `&i32` noalias readonly (add eax,eax) vs `&Cell<i32>` no noalias (reload); black_box = empty inline asm | 4.1 | — |
| Iterator invalidation (E0502) + fixes (collect/extend, retain, index loop); remove-in-index-loop panic (verified) | 4.1 | IX |
| `balances[_]` E0499 with split_at_mut help; get_disjoint_mut (stable ≥1.86; OverlappingIndices/IndexOutOfBounds) | 4.1 | — |
| Cell !Sync E0277 with compiler note suggesting RwLock/AtomicU32 | 4.1 | XI.2 |
| NLL history (2018/1.31, all editions 1.36, migrate removed 1.63); liveness per path; 5-step algorithm | 4.2 | XVIII.5 |
| Reborrow stack; generic T moves &mut (E0382 + "consider creating a fresh reborrow") | 4.2 | — |
| Problem case #3 still E0502 on 1.98.1 (verified); Polonius explanation; entry API (1 lookup) | 4.2 | IX.3 |
| Two-phase borrows; Java definite assignment as same dataflow family | 4.2 | XVII.6 |
| Lifetime params = caller-chosen regions; annotations relate; elision 3 rules; `'_`; T:'static vs &'static | 4.3 | — |
| Tokenizer<'a> tokens outlive &mut self vs elided E0499 (verified) | 4.3 | — |
| E0373 thread::spawn borrow + move fix; lifetimes erased/not monomorphized; universal regions; implied bounds | 4.3 | VII, XVIII |
| Box::leak for 'static per request: measured 1000 blocks leaked / 1000 calls | 4.3 | — |
| Decoded earlier signatures table (parse_header, Record<'a>, find, Tx<'db>, redact<'a>, longest) | 4.3 | — |
| Variance table (Reference); overwrite invariance E0597 "type annotation requires ... 'static"; fn contravariance; E0308 "one type is more general" | 4.4 | V.3 (PhantomData) |
| Variance inference from fields; PhantomData steering; #25860 = implied bounds + fn variance | 4.4 | XV |
| Java covariant arrays/ArrayStoreException; PECS use-site vs Rust declaration-site | 4.4 | VI, VII |
| HRTB for<'a>; caller-chosen lifetime E0597; E0521 escape (notes cite invariance); closure return-ref "lifetime may not live long enough"; fn item / HRTB-bound inference fixes | 4.5 | VI, X.1 |
| DeserializeOwned = for<'de> Deserialize<'de>; lending iterators impossible with std Iterator; GATs 1.65 | 4.5 | XXII.2, X |
| Netty ByteBuf use-after-release analogy | 4.3, 4.5 | XXI |
| A–B–C method; error-code translation table; five fix strategies; diagnostics "best blame" | 4.6 | — |
| E0716 vs temporary lifetime extension (`let x = &temp`); E0384 expression-style fix | 4.6 | — |
| unsafe silencing E0502: runs & prints 1 natively, Miri reports dangling reference UB (verified); safe version miri-ok | 4.6 | XV.6 |

## Part III concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| Three ownership rules; owner kinds; ownership TREE; five lifetime strategies (owner/borrow/Rc/arena/leak) | 3.1 | — |
| Drop flags in real MIR (`_5 = const true/false`, `switchInt(copy _5)`), elided in release asm (direct `__rust_dealloc`) | 3.1 | XVIII |
| Drop glue `drop_in_place::<T>`; needs_drop table | 3.1, 3.5 | — |
| Counting allocator (GlobalAlloc wrapper, SAFETY comment) as instrumentation; measured frees 1 vs 1001 | 3.1+ | XV.5 |
| Deferred drop: 1M-entry map inline drop ~60 ms vs handoff ~74 µs (one Playground run) | 3.1 | XX |
| Freed memory ≠ returned to OS (RSS) | 3.1 | XIX.5 |
| Move = size_of bytes (String 24 B: movups+mov verified); MIR shows `copy _2` post-optimization | 3.2 | — |
| Measured: move 0 allocs, clone Vec<String> 1001, HashMap<String,String> clone 2001 vs Arc 0 | 3.2 | — |
| E0507 move out of index; mem::take/replace/swap, Option::take, swap_remove | 3.2 | — |
| Copy vs Clone; E0184 Copy+Drop; &mut not Copy | 3.2, 3.5 | — |
| Borrow rules; E0506, E0505; two-phase borrows; reborrowing; split borrows (E0502 via method) | 3.3 | IV |
| noalias verified: `add_twice` loads *b once (`add eax,eax`) vs raw pointer reload; IR attributes | 3.3 | IV.1 |
| Borrow rules ≈ MESI single-writer/multiple-reader; data-race freedom | 3.3 | XI, XIV |
| Parameter-type guidance (&str/&[T] not &String/&Vec) | 3.3 | VI (AsRef/Into) |
| Owned/borrowed pairs; DSTs; fat pointers (String 24, &str 16, Box<str> 16) | 3.4 | V.2, VI.4 |
| UTF-8 encoding table; is_char_boundary; E0277 string index; byte-slice panic "end byte index 10 is not a char boundary" | 3.4 | — |
| bytes vs chars vs graphemes vs display width (CJK alignment observed) | 3.4 | — |
| Cow measured (2 owned of 6 headers); from_utf8_lossy is a Cow | 3.4, L2 | — |
| Java substring (pre-7u6 leak) vs &str borrow | 3.4 | — |
| Drop rules table; temporaries; match scrutinee guard lives through match; 2024 if-let else + tail-expression temporaries (2021 E0597 verified) | 3.5 | XIII |
| Unwind cleanup blocks in MIR (`(cleanup)`, `resume`, `unwind terminate`) | 3.5 | VIII.3 |
| Drop can't fail/await; explicit fallible close + Drop backstop; process::exit loses BufWriter data (verified empty output) | 3.5 | XIII, XXIII |
| Transaction guard (Tx<'db>, commit(self), rollback on drop/early return/panic) | 3.5 | XXIII |
| Rc/Weak internals (RcBox counts), RefCell runtime checks ("RefCell already borrowed" verified), Rc !Send | 3.6 | XI |
| Generational arena (ABA, stale key → None) | 3.6 | XXIV |
| Index-linked LRU (no unsafe/Rc/RefCell), mem::replace eviction | 3.6 | IX, XXIV |
| Rc<RefCell> reentrancy panic + closure Rc cycle leak | 3.6 | — |
| redact: byte scan + ASCII-boundary invariant, lazy Option<String> + Cow, chained Cows, 1 alloc for 10,000 clean lines (test-verified), Luhn | Project L2 | IX (memchr), XX |

## Part II concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| rustup/cargo/rustc roles; package/crate/module/workspace; editions table (2015–2024) | 2.1 | — |
| `.rlib` = object code + metadata incl. MIR of generic/inline fns; pipelined compilation; `cargo check` = metadata only | 2.1 | VII, XVIII |
| Two major versions coexist; "perhaps two different versions of crate" error; vs Maven mediation | 2.1 | XXII |
| Target triples, tiers, `"target-cpu"="x86-64"` baseline seen in real IR; SIGILL from `target-cpu=native` | 2.1 | XX (multiversioning) |
| musl static vs glibc; musl malloc caveat | 2.1 | XIX |
| Disk-constrained Windows setup (gnu + minimal profile, RUSTUP_HOME/CARGO_HOME, cloud env) | 2.1 | — |
| Profiles table (dev/release defaults), strip default since 1.77, `lto=false` = thin-local | 2.2 | XX |
| `add_one` asm debug (spills, overflow `jb` → `panic_const_add_overflow`, static Location) vs release (`lea`) | 2.2 | — |
| cfg-stripped code not type-checked; `unexpected_cfgs` / check-cfg since 1.80 | 2.2 | — |
| Semver caret rules (0.x), feature unification, resolver 2/3, build.rs as supply-chain surface | 2.2 | XXII |
| Panic strategy per system (Tokio unwind; FFM lib unwind + catch_unwind, extern "C" aborts since 1.81; CLI abort) | 2.2 | VIII, XVI |
| Per-package profile overrides (not panic/lto) | 2.2 | — |
| `let x = 10` through MIR (debug x => const), debug LLVM IR (`x.dbg.spill`), release IR (SSA `%x = mul`), asm | 2.3 | XVII, XVIII |
| SSA, φ nodes, mem2reg, register allocation (conceptual) | 2.3 | XVII.6, XVII.8 |
| Type inference by unification, function-local; E0282; `{float}` inference variable | 2.3 | XVII.5 |
| `as` semantics (truncate/sign-extend/saturate; float→int saturating since 1.45, was UB), From/TryFrom | 2.3 | — |
| Validity invariants & niches for bool/char; producing invalid value = UB | 2.3 | V.2, XV |
| f64 not Ord, total_cmp, `invalid_nan_comparisons` lint | 2.3 | IX |
| Division checks always on (div by zero, MIN/-1) | 2.3 | — |
| Money at JSON boundary (2^53) | 2.3 | XXII |
| Expressions vs statements, `()` real type, `!` never type, labeled block break | 2.4 | — |
| ABI: `(u64,u64)` in rax:rdx; `[u64;8]` via sret hidden pointer; strength reduction seen | 2.4 | XVI.1 |
| Fn item ZST (0 B) vs fn pointer (8 B) vs closure (captures) | 2.4 | VI, X.1 |
| Large stack arrays: 2 MiB spawned-thread default, "overflowed its stack" abort (verified) | 2.4 | IX interlude |
| No guaranteed TCO (`become` reserved) | 2.4 | IX interlude |
| Exhaustiveness (usefulness, Maranget 2007, witnesses like `u8::MAX`), guards count as nothing | 2.5 | XVIII |
| Match lowering: lookup table / jump table (no bounds check thanks to validity) / compare chain (verified asm) | 2.5 | XX |
| `for` desugaring; match ergonomics (RFC 2005); 2024 binding-mode reservations | 2.5 | III, X |
| Wildcards fail closed; `#[non_exhaustive]`; Java 21 MatchException vs Rust recompilation | 2.5 | V |
| let-else, if-let chains (2024, 1.88), slice patterns | 2.5 | — |
| Receivers as ownership modes; no constructors; method resolution (T, &T, &mut T, deref) | 2.6 | III, VI |
| Real `#[derive(Debug, Clone, PartialEq)]` expansion (nightly -Zunpretty=expanded) | 2.6 | VI |
| Layout: default repr reorders (16 vs repr(C) 24); enum tag+union; niches (Option<Shape>=24, MessageBoxed=8, Option<MessageBoxed>=16) | 2.6 | V.2 |
| Large enum variant bloat (4097 B) & `clippy::large_enum_variant` | 2.6 | IX |
| Derived Debug ignored by dead-code analysis | 2.6 | — |
| Visibility rules (module + descendants), pub(super)/pub(in)/pub(crate), E0616/E0451/E0603 | 2.7 | — |
| Privacy checked in different phases (E0616 in typeck hides E0451) — observed | 2.7 | XVIII |
| Privacy as memory-safety boundary (Vec len/cap) | 2.7 | XV.1 |
| Crate graph enforces hexagonal layering; public dependencies & semver; cargo-semver-checks | 2.7 | XXII |
| `#[inline]` and cross-crate inlining (auto since 1.75) | 2.7 | VII.3 |
| logstat: bounded histogram, zero-copy Record<'a>, reusable read_until buffer, BrokenPipe/SIGPIPE (std ignores SIGPIPE), BufWriter+lock, ExitCode 0/1/2, golden test | Project L1 | XI (parallel), XIX (mmap), XX (profile) |

## Concepts already introduced (don't re-teach from zero; deepen and cross-reference)

| Concept | Introduced in | Depth so far | Full treatment planned |
|---|---|---|---|
| Memory lifecycle + 5-family bug taxonomy (spatial/temporal/init/concurrency/type) | 1.1 | Full | — |
| The central question ("who frees this memory…") + 5 answers | 1.1 | Full | — |
| Undefined behavior, optimizer exploitation (`x+1<x`, CVE-2009-1897) | 1.1 | Solid | XV (unsafe), XVII (optimization) |
| Borrow checking is local & signature-based; false positives | 1.1 | Conceptual | IV, XVIII |
| NLL (2018), Polonius (in development) | 1.1 box | Mention | IV.2, XVIII.5 |
| Vec reallocation & growth (layout ptr/cap/len) | 1.1, 1.2 | Diagram | IX.1 |
| Allocator free lists, tcache, why UAF doesn't fault | 1.1 | Solid | XIX.5, XV.5 |
| ASan / Valgrind / MTE / CHERI as mitigations | 1.1 | Table | XV.6 |
| GC headroom (Hertz & Berger 2005), GC costs | 1.1, 1.4 | Solid | XX |
| Java memory safety without data-race freedom (JLS 17.7) | 1.1 | Solid | XIV.5 |
| GC prevents use-after-free, not use-after-invalidate | 1.1 | Key idea | III, IV |
| Five languages' runtimes (JVM JIT/GC/safepoints; Go G-M-P, pacer, netpoller) | 1.2 | Solid | XII.6 |
| Rust's pre-1.0 GC/green threads/segmented stacks and their removal (RFC 230) | 1.2 box | Story | XII.1 |
| Escape analysis (Go), everything-on-heap (Java), E0515 | 1.2 | Example | III |
| Lifetime elision (one input ref → output) | 1.2 | Mention | IV.3 |
| `size_of` table; null-pointer optimization guarantee `Option<&T>` | 1.2 | Output shown | V.2 |
| Java object layout (12 B header, compressed oops), Lilliput, Valhalla | 1.2 | Solid | IX.5 |
| Stackful vs stackless concurrency; std thread 2 MiB default | 1.2 | Table | XII.6, XI.1 |
| JIT speculation vs AOT; PGO/BOLT/LTO | 1.2 | Mention | VII, XX |
| Shared-map failure across languages (Java 7 HashMap loop, Go fatal error) | 1.2 | Table | XI.3 |
| Destructive moves vs C++ moves; drop flags; drop glue; drop order | 1.3 | Solid | III.2, III.5 |
| Lifetimes erased before codegen | 1.3 | Stated | XVIII.5 |
| `&mut` → LLVM `noalias` (re-enabled 2021) | 1.3 | Stated | IV.1, XVIII.6 |
| Send/Sync as auto traits + library signatures (`spawn`, `Scope::spawn`) | 1.3 | Solid | XI.2 |
| Race fixes: atomic vs mutex vs no-sharing; cache-line bouncing | 1.3 | Mechanism (not measured) | XI.7, XX.5 |
| `Ordering::Relaxed` justified by join happens-before | 1.3 | Brief | XIV.3 |
| Overflow semantics (never UB; debug panic, release wrap; checked/wrapping/saturating) | 1.3 | Output shown | II.3 |
| Zero-cost: verified asm, loop == iterator chain, bounds check eliminated | 1.3 | Verified | X.3 |
| Auto cross-crate inlining of small leaf fns (1.75+) and `#[inline(never)]` for asm inspection | 1.3 box | Mention | XVIII.6 |
| Guarantees vs non-guarantees table; trusted computing base; I-unsound, #25860 | 1.3 | Full | XV.1 |
| Leakpocalypse / RFC 1066; destructors never for soundness | 1.3 box | Story | XV.1, XI.6 |
| Mutex<T> owns data vs `synchronized`; `final` vs inherited mutability | 1.3 | Example | XI.3, XI.4 |
| RAII pool guard; Drop on `?` and on panic unwinding; caveats (exit/abort/forget/async cancel) | 1.3 | Example | III.5, XIII.4 |
| TOCTOU race condition (−700 demo), deadlock (lock order), Rc cycle leak | 1.3 | Examples | XI, III.6 |
| `let _ =` vs `let _x =`; `let_underscore_lock` deny lint | 1.3 debug ex. | Example | II.3 |
| Compile-time cost anatomy (monomorph, codegen units, proc macros, lld default 1.90) | 1.4 | Overview | VII.3, XVIII |
| Ownership tree vs object graph; Rc/Weak tree; arena + indices; slotmap/generations | 1.4 | Examples | III.6 |
| Lifetime propagation; zero-copy vs owned (`RequestRef<'a>` vs `Request`) | 1.4 | Example | IV.3, XXIII.5 |
| Scale lens: per-op budget table; latency numbers table; fleet arithmetic | 1.4 | Full | XX.1 |
| Reported rewrites: Cloudflare Pingora (2022), Discord (2020); Android data (2024) | 1.1, 1.4 | Attributed | — |
| Java FFM (JEP 454, JDK 22) as bridge | 1.4 | Mention | XVI.3 |

## Promises made to later Parts (by target Part; honor these)

Kept promises are struck through with where they were kept. Open promises are grouped by the Part that owes them,
with the chapter that made the promise in parentheses.

- ~~**Part II:** `let x = 10`, debug vs release, profiles, Windows toolchain setup~~ **KEPT** (2.1–2.3).
- ~~**Part III:** move/copy/clone String diagram, graphs, stable address, Project L2~~ **KEPT** (3.1–3.6, `redact`).
- ~~**Part IV:** errors as proofs, sub-slice lifetimes, longest/Record/Tx/redact signatures, reborrow nesting~~
  **KEPT** (4.1–4.6). Partial moves + E0509 are covered only in the 3.2 answer key.
- ~~**Part V:** PhantomData variance steering, type-state payment machine, newtypes (Percent/BasisPoints, TenantId,
  OrderId/Price), niches in depth, parse-don't-validate, Rust's removed built-in typestate~~ **KEPT** (5.1–5.4).
- ~~**Part VI:** auto-deref/Deref, AsRef/Into, orphan rule, `impl Trait` params, dyn variance, sealed traits, derive
  bounds, enum vs `Box<dyn>`, Java PECS vs Rust variance~~ **KEPT** (6.1–6.5).
- ~~**Part VII:** monomorphization and inlining explaining 1.3's asm (22 debug fns → 2 release), compile-time
  mitigations, `.rlib` MIR of generics, `#[inline]`~~ **KEPT** (7.1–7.3). Only back-referenced, not measured: the
  monomorphization cost of type-state impls (5.4).
- ~~**Part VIII:** error enums for the header parser and logstat, `?` properly, panic strategy per system, fallible
  commit~~ **KEPT** (8.1–8.4).
- ~~**Part IX:** SipHash default hasher, HashMap internals behind the entry API, retain/drain complexity, BFS vs DFS
  interlude, memchr for redact, f64 keys~~ **KEPT** (9.1–9.5, Interlude).

### Open promises

- **Part X (Iterators):** `for x in vec` vs `&vec` vs `&mut vec` properly (2.5 answer key); iterator chain adapter by
  adapter (7.1, 7.3); closure types unique per enclosing generic instance (7.3); fn item vs fn pointer vs closure (2.4);
  HRTB closures returning references (4.5); GATs and lending iterators (4.5, 6.2); how in-place `collect`
  specialization is wired (9.1, 9.5); zero-cost claims of 1.3 (bounds checks, vectorization).
- **Part XI (Concurrency):** Cell→atomics transition (4.1); Arc swap for catalogs/route tables (3.1, 3.3); parallel
  `logstat` with per-thread Summary merge (L1 review Q7); poisoning policy per lock, worker pools that survive job
  panics, Ferrite v1 panic policy (8.3); interior mutability in full (4.1); the concurrency decision matrix (Part I);
  Send/Sync manual impls and why they're unsafe, disjoint-capture subtlety (5.3, 6.4); concurrent maps measured
  (sharding, ArcSwap, owner thread), parallel `chunks_mut`/rayon, level-synchronous parallel BFS, channels (9.1, 9.3,
  Interlude); measure atomic vs mutex vs no-sharing (1.3; XX deepens).
- **Part XII (Async):** async recursion needs `Box::pin` (2.4, Interlude); why async/await fits "no runtime"; stackless
  future sizes; RFC 230 green threads removal (1.2); Go G-M-P/netpoller vs stackless (1.2); `async fn` in traits and
  dyn compatibility (6.4); RPIT capture rules for async return types (6.1, 7.3).
- **Part XIII (Tokio):** cancellation = dropping a future mid-flight (1.3, 1.4); frame split across two network reads
  (2.4 design exercise); async tail service reusing the logstat library; no locks across `.await`; `Box<dyn Error>`
  without `Send + Sync` across `.await` (8.2); Ferrite v2 `-ERR <CODE>` errors with busy/shutting-down codes (8.4);
  buffer pools with capacity caps (9.1); `spawn_blocking` for CPU-heavy work (9 answers); Drop can't await (3.5);
  `JoinError::is_panic` (8.3).
- **Part XIV (Memory model):** Relaxed vs Acquire/Release with the "publish a buffer via a counter" counterexample (1.3);
  Relaxed justified by join (1.3); JLS 17.7 (1.1); MESI analogy (3.3); false sharing / `CachePadded` measured (5.2, 9.5).
- **Part XV (Unsafe):** Stacked/Tree Borrows (4.2, 4.6); Miri in CI (4.6); drop check + `#[may_dangle]` and PhantomData's
  drop-check row (3.5, 5.3); `split_at_mut` / `get_disjoint_mut` internals (4.1); safety vs validity invariants and
  niches (2.3, 5.2); `set_len`, leak amplification (`Vec::drain`); privacy as memory-safety boundary (2.7); ZST handling
  (5.3); `try_reserve`/fallible allocation, intrusive linked lists, `GlobalAlloc` explained (9.1, 9.4, Part III).
- **Part XVI (FFI):** `extern "C"` stable ABI, `catch_unwind` at FFM entry points, `extern "C-unwind"` (2.2, 8.3); `cdylib`
  exported symbols (2.7); the fraud library via FFM: `meridian_score` codes 0/-1/-99 (8.3), concrete `score_batch`
  exports over generic internals (7.2), Java string transfer FFM vs JNI (9.2); `Option<&T>`/`repr(transparent)`
  guarantees, `repr(C)` enums (RFC 2195) (5.2, 6.5); loading a `cdylib` plugin with the C-ABI vtable of 6.5.
- **Part XVII (Compilers):** SSA/φ/register allocation deepened (2.3); type inference by unification (2.3); Java
  definite assignment as dataflow (4.2).
- **Part XVIII (rustc):** trait solver, intercrate mode, vtable layout internals (6.3, 6.4); monomorphization collector,
  CGU partitioning, shared generics, LLVM function merging, Cranelift, `-Z self-profile`/`-Z time-passes` (7.1, 7.3);
  exhaustiveness/match lowering on THIR/MIR (2.5, 5.1); `?` in MIR (8.1); borrow checking on MIR, Polonius (4.2);
  post-monomorphization errors (7.1); privacy phases (2.7).
- **Part XIX (Binary/OS):** mmap input for logstat; linking in depth (2.1); unwind tables (`.eh_frame`, LSDA) and
  backtrace symbolization (8.2, 8.3); exit statuses (8.3); freed memory vs RSS (3.1); guard pages and stack probes
  (Interlude); musl static linking (2.1).
- **Part XX (Performance):** PGO/BOLT (2.2, 7.2); runtime CPU feature detection (2.1); opt-level 2 vs 3; atomic/mutex/
  no-sharing ranking (1.3); the gateway benchmark; false sharing (5.2); tagged vs niche in memory (5.2); AoS/SoA and
  hot-cold splitting (5.2, 9.5); `perf stat -e branch-misses` for dispatch (6.5); pointer chasing, i-cache, chunked f64
  sum, code-size budget (7.x); panic cost vs depth, Result-vs-exception benchmark (8.x); gateway access-log p99, fraud
  feature-vector p99, THP/huge pages, order-book benchmark, allocator contention (9.x); deferred drop (3.1).
- **Part XXI (Networking):** harden the Meridian gateway (Part I review); timeouts, retries, circuit breakers, retry
  budgets, deadline propagation (8.4); HashDoS and network input (9.3); Netty ByteBuf analogy (4.3, 4.5); io::ErrorKind
  by connection phase (8.4).
- **Part XXII (Ecosystem):** serde zero-copy (`Cow<'a, str>`, `DeserializeOwned`) (4.3, 4.5); serde `transparent` /
  `try_from` at boundaries (5.3); tagged enums (2.6); money as strings in JSON (2.3); clap; thiserror/anyhow in crate
  choice (8.2); axum `IntoResponse for PaymentError` (8.4, unverified sketch); tower Retry policy by error class; Tower
  `BoxService` hybrid (6.5); tracing observability (8.4); clippy `result_large_err` (8.2); cargo-hack / cargo-deny /
  `cargo tree -d` / cargo-semver-checks, `#[non_exhaustive]` semver (2.2, 5.1, 6.2, 6.3); fuzzing the header parser;
  serde_json recursion limit (Interlude).
- **Part XXIII (Storage):** transaction guard → real transactions (3.5); commit with unknown outcome (8.3); persistent
  encodings instead of in-memory layout, padding (5.2); DB compare-and-set for state transitions (5.4); Ferrite v3
  `FerriteError` and a fallible store API (8.2 design exercise: reconcile with the brief's contract by adding a
  fallible trait or adapter, keeping v1's `KvStore` usable); zero-copy `RequestRef<'a>` (1.4).
- **Part XXIV (Distributed):** idempotency + reconciliation for ambiguous outcomes, "did it happen?" (8.4); generational
  arena / LRU reused (3.6).
- **Part XXVI (Language):** specialization and language-design trade-offs (6.3); `#[non_exhaustive]` and exhaustiveness
  (5.1).

## Running case study: Meridian (fictional)

Payments-and-marketplace company; mostly Java, one C++ team, Go tooling. Systems introduced so far:

| System | Facts established | Where |
|---|---|---|
| Market-data fan-out (C++) | `std::vector<Subscriber>` dangling refs incident; 17-day diagnosis; Rust rewrite candidate | 1.1, 1.2, 1.4 |
| API gateway (Java/Netty) | 400K req/s peak, ~0.5 ms CPU/req, ~330 cores, 55 pods, 6 GB ZGC heaps, p99.9 45 ms, 2 GC-related SLO misses/qtr, JWT, ~120 upstreams, per-key rate limits | 1.2, 1.4, Part I review |
| Ledger (Java) | ~3K TPS, 90% DB time, stays Java | 1.2, 1.4 |
| Fraud feature extraction | 50K scores/s, p99 < 5 ms, ~3 ms CPU, 120 cores; Rust lib via FFM | 1.2, 1.4 |
| Risk-limits service | 100K ops/s, p99 < 1 ms, no-breach invariant (design exercise) | 1.2 |
| Session cache | 2M sessions, 100K lookups/s (design exercise) | 1.3 |
| Payments connection-pool leak (Java) | 03:10 exhaustion, early return skipped `close()` | 1.3 |
| K8s operators (Go) | stays Go | 1.2, 1.4 |
| Gateway CI/build policy | toolchain pinned 1.98.1, musl + mimalloc images, aarch64 Graviton pool, x86-64 baseline; SIGILL incident from `target-cpu=native` | 2.1 |
| Release profiles per system | gateway: unwind, thin LTO, cgu=1, line-tables; fraud FFM lib: unwind + catch_unwind; billing crate overflow-checks | 2.2 |
| Checkout discount incident | `debug_assert!` + release wrap → 18446744073709546616 cents; basis points vs percent | 2.2 |
| Money at JSON boundary | i64 minor units inside; strings in JSON (2^53) | 2.3 |
| Market-data frame header | magic 0xCAFE, version, flags, BE u32 body length; zero-alloc parser | 2.4, 2.5 |
| Export service | 4 MiB stack array crash-loop on 2 MiB worker threads | 2.4 |
| Payment state machine | Pending/Authorized/Captured/Refunded/Failed; fail-closed catch-all | 2.5 |
| Ledger chargeback incident | wildcard `_ => 0` swallowed `Chargeback`; only signal was a dead-code warning | 2.5 |
| Payment method model | Java nullable-fields class → Rust enum; 2025 incident with card+IBAN rows | 2.6 |
| Market-data queue | `Message::Data([u8;4096])` → 4.1 GB for 1M pings; boxed fix | 2.6 |
| Gateway workspace | gateway-core / gateway-proto / gateway-io / gateway (bin) | 2.7 |
| Account `pub` field incident | migration made `balance_cents` pub; refund feature bypassed overdraft check | 2.7 |
| quota-service PR | the Part II review artifact (13 issues) | Part II review |
| Route-table refresh | 1M entries every 10 min; drop on request thread → p99.9 spike; dropper thread + Arc swap | 3.1 |
| Ingestion pipeline | ownership handoff decode→validate→enrich→publish replacing Java defensive copies | 3.2 |
| Per-request config clone | HashMap<String,String> 1,000 routes → 2,001 allocs/request | 3.2 |
| Interest run / split borrows | rate getter E0502; field access or snapshot-before-loop | 3.3 |
| Header normalization | Cow; 2 of 6 headers allocate | 3.4 |
| Display-name truncation panic | "Kristina Øberg" byte index 10 | 3.4 |
| Transaction guard | rollback on drop/early return/panic; commit(self) | 3.5 |
| Export CLI | process::exit lost BufWriter rows | 3.5 |
| Session LRU | index-linked list in a Vec + HashMap | 3.6 |
| In-process event bus | RefCell reentrancy panic on user.created + closure Rc cycle | 3.6 |
| Matching-engine order book | Part III review artifact: Rc<RefCell> Java-shaped design, 12 issues; arena + BTreeMap levels redesign | Part III review |
| Leases in job scheduler | design exercise: Drop backstop + server-side expiry | 3.5 |
| Request stats via Cell | per-request Cell counters; global metrics via atomics after E0277 | 4.1 |
| Session-expiry index loop | remove-in-loop panic "len is 3 but the index is 3"; fix retain | 4.1 |
| JWT claims cache | problem case #3 → entry API (1 hash instead of 3) | 4.2 |
| Metrics refactor | generic record<T> moved &mut in 40 call sites; fix &impl Debug | 4.2 |
| Frame parser API | FrameRef<'a> + Frame(Bytes) + to_owned boundary | 4.3 |
| 'static metrics labels | Box::leak per request → OOM every few days | 4.3 |
| Request-scoped interner | RefCell<Vec<&'a str>> invariance pain → indices | 4.4 |
| Java handler array ArrayStoreException | variance failure analog | 4.4 |
| Log visitor library | for_each_line with HRTB + ControlFlow; anomaly detector E0521 → to_owned | 4.5 |
| Netty ring-buffer data exposure (Java) | retained ByteBuf after release | 4.5 |
| Borrow-error triage playbook | A–B–C in PRs, strategies, no unsafe for borrow errors, Miri in CI | 4.6 |
| unsafe cache borrow extension | later eviction made it UB; found by Miri weeks later | 4.6 |
| Session manager triage | Part IV review capstone (4 errors) | Part IV review |
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
| Gateway config loader (Rust) | ~40 settings; 2025 Java incident: `UPSTREAM_TIMEOUT_MS=250ms` swallowed by catch → kept 1,000 ms default for 40 min; rule "absence may default, malformed never" | 8.1 §9 |
| Settlement batch job (Rust) | ignored `remove_file` Result + 300 warnings → stale lock, settlement a day late; CI now `-D warnings`, `deny(unused_must_use)` | 8.1 §10 |
| Market-data ingest (Rust, fan-out rewrite) | String-error dispatch `e.contains("need")`; reworded message → split frames closed connections → reconnect storm, stale prices ~25 min; `HeaderError::Incomplete` fix; review rules (no String errors in libs, no matching on messages) | 8.2 §10 |
| Ledger library | `LedgerError` (thiserror): AccountNotFound, InsufficientFunds, Storage(#[from] io), Corrupt{line, source} | 8.2 §3 |
| Gateway panic boundary | hook → one structured line `code=INTERNAL_PANIC` + `panics_total`; per-request/task containment → 500 INTERNAL; alert on panics_total > 0 | 8.3 §9 |
| Gateway double-panic incident | handler panicked holding pool mutex → poisoned; pooled-conn Drop `lock().unwrap()` panicked during unwinding → abort, ~150 in-flight requests/pod lost (Little's law arithmetic), exit 134, rolling restarts | 8.3 §10 |
| Fraud FFM library | `meridian_score` entry: catch_unwind → rc 0 / -1 (invalid) / -99 (panic); Java maps -99 to IllegalStateException + alert | 8.3 §7 |
| payments-core (new Rust service) | orchestrates charges gateway → payments-core → card processor (Java ledger behind it); `PaymentError` taxonomy with codes PAY_INVALID_AMOUNT/…/PAY_INTERNAL, classes Rejected/Transient/Ambiguous/Internal, statuses 400/402/409/429/503/504/500 | 8.4 §1, §3 |
| Double-charge incident (Java, March 2025) | gateway generic client retried POST /charges after 2 s payments-upstream read timeout; 1,140 customers double-charged over 40 min | 8.4 §9 |
| payments-core idempotency | mandatory Idempotency keys on POST /charges, one UUID per checkout attempt, stored with the charge (unique (tenant,key)), derived key forwarded to processor; one retry layer (gateway); reconciliation job for PAY_PROCESSOR_TIMEOUT | 8.4 §9 |
| 4xx page storm (Java payments) | declines logged at ERROR; Black Friday 40K ERROR lines/min; muted alert buried a ledger fsync EIO; fix: level by class, `payment_errors_total{code,class}`, alert on internal class + SLO burn | 8.4 §10 |
| Refunds PR | Part VIII review artifact: 14 defects (unwrap on input, negative amounts, stringly retry, retry w/o key, fall-through 200 with empty id, leaked host IP, processor-before-ledger ordering, Box<dyn Error>, panic on business rule, log-and-return, Debug to client, swallowed audit, path traversal + truncating audit file) | Part VIII review |
| Gateway header vectors | 11–14 headers typical; Vec grew 0→4→8→16 (3 allocs/request, 1.2M allocs/s at 400K req/s); per-connection reused Vec, replaced when capacity > 64 | 9.1 |
| Ingestion batch buffer | replayed backlog → one batch of 2.1M events (~400 MB); clear() kept capacity; OOM kills; fix capacity policy + batch cap | 9.1 |
| Market-data snapshots | design exercise: 40,000 instruments, top 200 = 90% of trades, last 1,000 trades per instrument, 48-byte Trade | 9.1 |
| Gateway access logs | format! per log line → ~5 allocs/request (~2M allocs/s); per-worker String with write!, 4 KiB cap → 0 | 9.2 |
| Fraud JNI name incident | GetStringUTFChars Modified UTF-8 → 0.3% errors for emoji names; from_utf8_lossy "fix" changed hashed feature → 2 weeks of "model drift"; fix GetStringChars/from_utf16 or FFM | 9.2 |
| Ledger statement export | design exercise: ~2M statements overnight, 50–5,000 lines, Greek/Turkish/Japanese | 9.2 |
| Session cache (Rust port) | 2M sessions, 16-byte SessionId, 200-byte Session; inline HashMap ≈ 910 MB vs IndexMap ≈ 490 MB (chosen); foldhash seeded; presized, never shrinks intraday | 9.3 |
| Gateway rate limiter HashDoS | FxHashMap swap for "4% SipHash"; scraper IDs `counter << 32` → quadratic probing, CPU 100%; fix keyed hasher + CI lint `// TRUSTED-KEYS:` | 9.3 |
| Payments idempotency keys | design exercise: ~300M keys/day, 36-char UUIDs, 12 nodes, 24 h retention | 9.3 |
| Matching-engine order book (Rust prototype) | Slab<Order> arena + BTreeMap<i64, VecDeque<usize>> levels; lazy cancel + compaction | 9.4 |
| Settlement batcher EDF incident | BinaryHeap<Reverse<Job>> with derived Ord; field reorder PR → jobs by ID; missed partner cutoffs 2 nights; explicit Ord + property test | 9.4 |
| Gateway metrics time-series | design exercise: 5,000 routes, 10 s interval, 24 h retention | 9.4 |
| Fraud feature vector | HashMap<String, f64> of ~400 features per score → name→index at model load + reused Vec<f64>; ~3% CPU + 400 allocs/score removed (estimate, p99 to verify in XX) | 9.5 |
| Matcher SoA incident | SoA arena for risk report (6× faster) slowed matcher p99; reverted in 2 days; columnar snapshot for report instead | 9.5 |
| Risk-limits service | design exercise: 3M merchants, per-payment counter updates + per-minute 80% scan | 9.5 |
| Fraud linked-accounts traversal | Rust port uses BFS (distance correctness), frontier up to ~80K at hop 2, cap 100K ("too connected"), heap frontier independent of JVM -Xss, reused per worker | Interlude |
| Merchant-onboarding rule engine crash loop | analyst JSON rules, recursive evaluator on Tokio workers; partner rule ~40,000 deep → stack overflow abort → crash loop across replicas; quarantine + depth limit 64 + explicit stack | Interlude |
| Dependency resolver (build tooling, Go today) | design exercise: ~40,000 packages, depth ~15, one ~9,000-deep legacy chain | Interlude |
| Velocity store (fraud) | Part IX review capstone: Java-shaped port; profile ~2,000 events/s, ~150K merchants, ~5M cards/day | review |

**Ferrite** (the reader's own system) starts at Project Level 4 (Part XI); its v1–v5 contract is in `notes/AUTHORING-BRIEF.md`.

## Style decisions made so far

- Chapter numbering is Part-local ("Chapter 1.3").
- Interview questions numbered per chapter; answers in `src/appendix/answers-part-NN.md` under matching headings.
- External stats always attributed with org + year; hedged as "reported".
- Performance rankings without measurement are labeled "predicted from mechanism" and paired with a measuring exercise.
- Compiler artifacts (MIR / LLVM IR / asm / macro expansion) are fetched with `tools/emit.ps1` and quoted verbatim
  (trimmed; labels may be simplified and are marked as such). Nightly-only artifacts are tagged [VERSION].
- Project chapters use their own structure: requirements → design (with ownership boundaries) → walkthrough →
  production concerns → testing (golden test = verified output) → omissions → architecture review → extensions.
- Part reviews may use a "review this PR" capstone (Part II) instead of an ADR (Part I); vary it per Part.

## Tooling notes (learned the hard way)

- 2026-09-24: `tools/verify.ps1` had a bug: PowerShell variables are case-insensitive, so a per-check `$edition`
  overwrote the `$Edition` parameter and silently compiled later listings as edition 2021. Fixed (uses `$checkEdition`);
  Part I and Part II re-verified afterwards. Never reuse a parameter's name for a local in PowerShell.
- Put each intended compile error in its own listing: an earlier-phase error (e.g., typeck E0616) hides later-phase
  errors (privacy-pass E0451).
- 2026-09-25: `tools/verify.ps1` gained `+tree` for Miri checks (`// verify: debug+tree miri-ok`) to run Tree Borrows
  instead of Stacked Borrows. Verified: `let p = &mut x as *mut _; let r = &mut x; *p = 1;` is UB under Stacked Borrows
  and accepted under Tree Borrows.
- Parts V–IX tooling lessons (details in each `notes/part-NN-report.md`, summarized in `notes/AUTHORING-BRIEF.md`):
  `error:lifetime` / `error:padding` needles; `process::abort` prints nothing (print a marker for `crash` checks);
  re-exec via `current_exe()` for exit codes; E0282 (not E0283) for ambiguous generic-trait impls on 1.98.1; E0038
  lists one dyn-compatibility reason at a time; v0 mangling in IR; LLVM merges identical functions in release; debug IR
  of generic-heavy code can reach ~20 MB (delete after use); `emit.ps1 -CrateType bin` and `-Target expand` with crates
  both work; edition 2024 denies `&static mut`; disjoint closure capture hides `!Send`; the stack-overflow 90%/110%
  test pattern; a counting `BuildHasher` for hash counts.
