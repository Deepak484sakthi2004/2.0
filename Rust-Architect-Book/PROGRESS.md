# Progress & Continuity Ledger

Last updated: 2026-09-25 (Part III complete) · Baseline: Rust 1.98.1 stable, edition 2024 (Playground-verified)
Published to: https://github.com/Deepak484sakthi2004/2.0 (folder `Rust-Architect-Book/`)

## Status

| Part | Title | Status | Listings verified |
|---|---|---|---|
| — | Preface | **Written** | — |
| I | Why Rust Exists | **Written** (4 chapters + review + answer key) | 26 files / 27 checks, all pass |
| II | Rust From First Principles | **Written** (7 chapters + Project L1 `logstat` + review + answer key) | 39 files / 44 checks, all pass |
| III | Ownership: The Core of Rust | **Written** (6 chapters + Project L2 `redact` + review + answer key) | 38 files / 44 checks, all pass |
| IV | The Borrow Checker | Next | — |
| V–XXVI | … | Planned (see `src/SUMMARY.md`) | — |

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

## Promises made to later Parts (forward references to honor)

- ~~**Part II:** `let x = 10`, debug vs release, profiles, Windows toolchain setup~~ **KEPT** (2.1–2.3).
- ~~**Part III:** move/copy/clone String diagram, graphs, stable address, Project L2~~ **KEPT** (3.1–3.6, `redact`).
  (`for x in vec` vs `&vec` covered in 2.5 answer key; revisit in X.)
- **Part IV:** reading every borrow-checker error as an ownership proof (promised in 1.1); how the compiler knows a
  returned sub-slice can't outlive its input (2.4 header parser); explain `longest<'a>` (3.3), `Record<'a>` (L1),
  `Tx<'db>` (3.5), `redact<'a>` returning `Cow<'a, str>` (L2); reborrow nesting (3.3); drop check / `#[may_dangle]`
  (3.5); partial moves + E0509 (3.2 answers).
- **Part V:** niches in depth (promised in 1.2's `Option<u64>` = 16 bytes, deepened in 2.6); type-state (Rust once had
  built-in typestate — mention in 5.4; payment state machine promised to move to type-state in 2.5, order lifecycle
  design exercise 2.5); newtypes `Percent`/`BasisPoints` (2.2 failure), `TenantId` (Part II review), "parse, don't
  validate" (2.6).
- **Part VI:** auto-deref method resolution through Box/Rc (2.6); orphan rule (2.1, 2.7); `impl Trait` params (L1).
- **Part VIII:** replace `String` errors in header parser and logstat with error enums; `?` properly (L1 used it).
- **Part IX:** SipHash default hasher (L1); Interlude BFS vs DFS promised for deep recursion (2.4).
- **Part XI:** parallel `logstat` with per-thread Summary merge (L1 review Q7).
- **Part XIII:** frame split across two network reads (2.4 design exercise); async tail service reusing logstat lib.
- **Part XVI:** `extern "C"` stable ABI, `catch_unwind` at FFM entry points (2.2), `cdylib` exported symbols (2.7).
- **Part XIX:** mmap input for logstat; linking in depth (2.1).
- **Part XX:** PGO/BOLT (2.2); runtime CPU feature detection/multiversioning (2.1); measure opt-level 2 vs 3.
- **Part XXII:** clap, anyhow/thiserror, serde tagged enums (2.6), cargo-hack/cargo-deny/semver-checks, fuzzing the
  header parser.
- **Part VII / X:** monomorphization and inlining explaining the verified asm of 1.3.
- **Part XI:** poisoning (`lock().unwrap()`), interior mutability, the concurrency decision matrix.
- **Part XII:** async recursion needs `Box::pin`; why async/await fits "no runtime"; stackless sizes.
- **Part XIII:** cancellation = dropping a future mid-flight (promised in 1.3 and 1.4).
- **Part XIV:** Relaxed vs Acquire/Release with the "publish a buffer via counter" counterexample.
- **Part XV:** Miri, safety invariants, `set_len` example, leak amplification (`Vec::drain`).
- **Part XVI:** Rust library called from Java via FFM (Meridian fraud features).
- **Part XX:** measure the atomic/mutex/no-sharing ranking; PGO; the gateway benchmark (Meridian).
- **Part XXI:** harden the Meridian gateway (the one reviewed in Part I).

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

**Ferrite** (the reader's own system) starts at Project Level 4 (Part XI).

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
