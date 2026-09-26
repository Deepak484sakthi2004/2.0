# Progress & Continuity Ledger

Last updated: 2026-09-26 (Parts I–XIV, XVI–XX complete; XV partial (15.1–15.3); XXI on hold; XXIII, XXVI in
progress) · Baseline: Rust 1.98.1 stable, edition
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
| X | Closures, Iterators, and Zero-Cost Abstractions | **Written** (4 chapters + review capstone + answer key) | 58 files / 70 checks (incl. 1 Miri, 1 nightly), all pass |
| XI | Concurrency | **Written** (7 chapters + Project L3 HTTP server + Project L4 Ferrite v1 + review + answer key) | 55 files / 69 checks (incl. 5 Miri), all pass |
| XII | Async Rust | **Written** (6 chapters + review capstone + answer key) | 59 files / 75 checks (incl. 12 Miri), all pass |
| XIII | Tokio and Production Async | **Written** (5 chapters + Project L5 Ferrite v2 + review capstone + answer key) | 44 files / 47 checks, all pass |
| XIV | Memory Model and Atomics | **Written** (5 chapters + review capstone + answer key) | 38 files / 62 checks (incl. 25 Miri), all pass |
| XV | Unsafe Rust | **Partial**: 15.1–15.3 + overview + answers for 15.1–15.3 written. 15.4–15.6 and the review are **not written**: three writer attempts were stopped by a safety classifier (see `notes/part-15b-report.md`); 15.4's listings exist and are verified | 73 files / 120 checks (incl. 15.4's 15 files / 22 checks), all pass |
| XVI | FFI and Systems Programming | **Written** (4 chapters + review capstone + answer key) with the narrower scope the user approved: interface engineering, no UB demonstrations (the first attempt was stopped by a safety classifier) | 39 files / 61 checks (incl. 18 Miri, all on correct code), all pass |
| XVII | Compilers | **Written** (8 chapters + review capstone + answer key) | 43 files / 50 checks, all pass |
| XVIII | How rustc Works | **Written** (7 chapters + review capstone + answer key) | 68 files / 80 checks (13 on nightly), all pass |
| XIX | Binary, Linker, and OS | **Written** (6 chapters + review capstone + answer key) | 29 files / 31 checks, all pass |
| XX | Performance Engineering | **Written** (7 chapters + promise table + review capstone + answer key) | 45 files / 48 checks (incl. 1 Miri, 1 nightly), all pass |
| XXI | Networking | **Not written**: the writer was stopped by a safety classifier while writing a 21.4 slow-client timeout listing; 17 verified listings for 21.1–21.3 exist; waiting for the user's decision on a retry (see `notes/part-21-report.md`) | — |
| XXII | The Rust Backend Ecosystem | Planned | — |
| XXIII | Databases and Storage | In progress (Project L7, Ferrite v3) | — |
| XXIV, XXV | Distributed Systems · Blockchain and Infrastructure | Planned | — |
| XXVI | Building a Language | In progress (Projects L12, L13, final capstone) | — |

All listing counts were re-verified by the integrator after each writer finished (independent `tools/verify.ps1` run).

## Part XX concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| A claim = metric + statistic + workload + comparison; the eight metrics (brief's list) with Rust/Java measurement | 20.1 | — |
| One number hides the distribution: 4M pushes, wall/n 60.3 ns vs mean 29 vs p50 20 (= timer), p99.9 = page faults (1 per 512 pushes), max 778 µs = reallocation | 20.1 | — |
| Amdahl measured (f = 12–14%): 2.57×/2.22× on 4 threads vs 2.95×/2.80× predicted; memory-bound parallel part limited by bandwidth | 20.1 | — |
| Gateway hot-path model: ~0.97 µs/request, 0 allocations, HMAC 0.24 µs, httparse 0.23 µs; why "0.6 cores vs 333" is not a fleet plan | 20.1 | XXI (whole-service load test) |
| Knuth 1974 full quote; premature pessimization (Sutter & Alexandrescu 2004) | 20.1 | — |
| Benchmark lies demonstrated: DCE (0.0 ns), closed forms (Gauss in asm; sum of squares with 0x5555…56), cold start (83×, 32,770 faults), clock (Instant 25 ns, 20 ns step, tsc), order bias (identical fns "11×") | 20.2 | — |
| LAYOUT BIAS: same 7-instruction loop, 1.7× apart; swapping closure definition order flips it (ch02-09 vs ch02-11; Mytkowicz et al. 2009, STABILIZER 2013) | 20.2 | XX (policy), XXII.7 |
| The book's harness (warm-up, rotated interleaving, 31 samples, p10..p90, overlap verdict) | 20.2 | — |
| Coordinated omission simulated: closed-loop p99 1.0 ms vs open-loop 1.5 s; record_correct; wrk2/k6 | 20.2 | XXI |
| Failure-path cost: Result vs panic at depth 1/10/100 × 0/1/10% failures; panic zero-cost happy path; break-even ~0.2–0.6% | 20.2 | — |
| Build-profile experiments as A/B load tests (opt-level, LTO, CGU); load curves (wrk2) for L3 | 20.2 | — |
| perf blocked on the Playground (perf_event_paranoid = 4, EPERM) | 20.3 | — |
| In-process sampler with seqlock: on-CPU vs wall-clock; aliasing with fixed interval (28/50/22 vs true 20/50/30) | 20.3 | — |
| getrusage user/system split (unbuffered: 43 ms user / 87 ms system; BufWriter 1.1/0.1) | 20.3 | — |
| Inlined frames vanish in release backtraces (4 → 1); frame pointers vs DWARF vs LBR; distros enabling frame pointers | 20.3 | XIX |
| Merged functions mislabel profile leaves; `-Z merge-functions=disabled` | 20.3 | — |
| perf stat / off-CPU / PGO / BOLT / `--timings` / `-Z self-profile` / LLVM `-time-passes` commands (unverified) | 20.3 | XXII |
| Allocation-site profiler via GlobalAlloc + thread-local site (108 allocs/order) | 20.4 | — |
| glibc malloc: tcache, arenas, dynamic mmap threshold, 8-byte header + 16-byte rounding (48 → 64 B) | 20.4 | XIX.5 |
| Malloc costs measured: local ~11 ns/pair scaling to 4 threads; bump arena ~3×; cross-thread frees ~118 ns each | 20.4 | — |
| Live heap vs RSS: 53 vs 71 MiB; frees don't shrink RSS; malloc_trim returns all | 20.4 | XIX.5 |
| Zeroing cost vs size: 2× small, 1.5× large; fresh ≈ zeroed (dynamic mmap threshold) | 20.4 | — |
| Latency ladder on this host: L1 1.6 ns, L2 ~5, L3 ~16, DRAM 130–171 ns (cache sizes read from /sys) | 20.5 | — |
| Branch misprediction ~5.9 ns (~20 cycles); sign-bit tricks (test/jns, psrlw 7); branch-free count | 20.5 | — |
| Byte-class table vs comparison chain: 1.07 vs 7.05 ns/byte on random order (6.6×) | 20.5 | XXVI (lexer) |
| Niche vs tag: 4.4× in cache (compute), 2.2× from DRAM (bytes) | 20.5 | — |
| Hot/cold vs SoA vs AoS: scan 26×/65× faster split; random record access AoS fastest | 20.5 | — |
| False sharing slows readers 4× (1.41 vs 0.35 ns/read); `perf c2c` | 20.5 | — |
| THP requested but mostly not granted (10 of 256 MiB); defrag=madvise doubled first touch | 20.5 | XIX.5 |
| Order book: arena 1.4× throughput, p99 120 vs 246 ns, 0.82 → 0.22 allocs/op | 20.5 | XXIV |
| Vectorization in asm: paddd, addsd chain, addpd lanes, saxpy_raw overlap check, early exit stays scalar; f64 Sum starts at −0.0 | 20.6 | — |
| f64 8-lane sum 8× faster, 9e-16 relative difference, tolerance test | 20.6 | — |
| Runtime dispatch: AVX2 21.4 GB/s vs plain 3.0 vs memchr 45 GB/s; Miri runs scalar path; E0133 for target_feature calls (1.86) | 20.6 | — |
| `target_feature(enable = "avx2")` doesn't imply popcnt (count_ones becomes a bit trick) | 20.6 | — |
| std::simd u8x32 on baseline = two SSE2 halves, 16.5 GB/s (nightly) | 20.6 | — |
| noalias: runtime check ~free for disjoint raw pointers; overlapping case 46× slower (true dependence) | 20.6 | — |
| x86-64 micro-architecture levels v1–v4; build baseline + dispatch; register pressure and spills | 20.6 | — |
| M/M/1 simulated: p99 9×/22×/43×/308× service time at 50/80/90/99%; Little's law exact | 20.7 | XXI, XXIV |
| Fan-out 1 − 0.99^N reproduced; hedging after 3 ms cuts p99 60 → 6 ms for ~1% extra calls; fails at N = 100 | 20.7 | XXI.4, XXIV |
| Contention = wait time (hold 180 ns constant; wait p99 240 ns at 4 threads); second run showed median/throughput noise | 20.7 | — |
| Ferrite shards: Mutex 10.2 vs RwLock 11.3 vs RwLock+Arc<[u8]> 19.2 M ops/s (4 threads) | 20.7 | XIII (L5), XXIII |
| Core-to-core round trip 56–70 ns; sched_setaffinity works; 1 NUMA node, no SMT siblings on the Playground | 20.7 | — |
| NUMA first-touch, numactl commands; thread-per-core (Seastar, Glommio, monoio); spin_loop/pause backoff (predicted) | 20.7 | — |
| StampedLock optimistic read = seqlock; Kingman; Universal Scalability Law | 20.7 | — |

## Part XIX concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| Object file = sections + symbols + relocations; one section per function (rustc); `lang_start::<()>` instance defined in the user crate | 19.1 | — |
| Relocation types PC32 / PLT32 / GOTPCREL / GOTPCRELX / RELATIVE / GLOB_DAT; 677 dynamic relocs in a small debug exe (67 GLOB_DAT, 2 JUMP_SLOT, 608 RELATIVE) | 19.1 | — |
| GOT slot read from the running process: `r--p` after RELRO, equals `dlsym(getpid)` | 19.1 | — |
| Relaxation: gcc emits GOTPCRELX (`addr32 call helper`); rustc 1.98.1 emits plain GOTPCREL so calls into std stay indirect; `-Z relax-elf-relocations=yes` → GOTPCRELX → `addr32 call _print`; `-Z plt=yes` → PLT32 → direct call [RUSTC][VERSION] | 19.1 | XX (measure) |
| Target spec: `plt-by-default: false`, `relro-level: full`, `position-independent-executables: true`, `default-uwtable: true`, `linker-flavor: gnu-lld-cc` | 19.1, 19.3 | — |
| Symbol kinds (593 t, 452 r, 283 T, 95 U, ...); v0 raw vs `nm -C`; `rust_eh_personality`, `DW.ref.rust_eh_personality` | 19.1 | — |
| Allocator shims in pseudo-crate `__rustc`: default `__rust_alloc` = `jmp __rdl_alloc`; with `#[global_allocator]` calls `<A as GlobalAlloc>::alloc`; `__rust_no_alloc_shim_is_unstable_v2` | 19.1 | XV.5 (not written) |
| Strip sizes: 4,927,280 full / 613,848 strip-debug / 459,696 strip-all; backtraces per level; `addr2line` offline; build-id survives strip | 19.1 | — |
| Build-id reproducibility: no `-g` same across dirs; `-g` differs (comp dir); `--remap-path-prefix` same | 19.1 | XXII (reproducible builds) |
| Rust code static, glibc dynamic; static-pie via `+crt-static` (368 KB vs 1.46 MB); spawn 877 vs 511 µs (second run 716 vs 406; one run, noisy) | 19.2 | — |
| LD_PRELOAD interposition (4242 in dynamic, real pid in static) | 19.2 | — |
| glibc floor = newest mandatory version need; `__libc_start_main@GLIBC_2.34`; std's weak `pidfd_spawnp`/`pidfd_getpid` create a mandatory `GLIBC_2.39` need (`Flags: none`, with rust-lld AND GNU ld); simulated loader error | 19.2 | — |
| Loader search incl. `glibc-hwcaps/x86-64-v4/v3/v2` subdirs; RUNPATH `$ORIGIN`; exit 127 for missing lib; exit 1 for missing version | 19.2 | — |
| cdylib exports only `#[no_mangle]` (1 dynamic symbol); dlopen/dlsym | 19.2 | XVI (not written) |
| `-C prefer-dynamic`: 5,344-byte exe + `libstd-<hash>.so`; proc macros = host dylibs dlopen'd by `librustc_driver` (`__rustc_proc_macro_decls_<hash>__`) | 19.2 | XXII (proc-macro policy) |
| rust-lld default (`.comment: Linker: LLD 22.1.8`), `-C linker-features=-lld` for GNU ld; both give BIND_NOW + GNU_RELRO; musl std not installed on the Playground (E0463) | 19.2 | — |
| Hand-written ELF64 parser (header, PHDRs, sections, .interp, DT_NEEDED) cross-checked with AT_PHDR/AT_ENTRY; 5.1 MB debug file, 457 KB mapped | 19.3 | — |
| Unwind tables: CIE "zPLR" w/ personality, FDE w/ LSDA, 12-byte `.gcc_except_table`; `extern "C"` callee → no landing pad (nounwind since 1.81); `C-unwind` → pad + `_Unwind_Resume`; `panic=abort` + `C-unwind` → pad runs drop glue then `panic_cannot_unwind` | 19.3 | — |
| Debug-info variants measured (d0 has std's 6 .debug sections; line-tables-only; split-debuginfo packed .dwp / unpacked .dwo; strip=debuginfo 462 KB; strip=symbols 353 KB); objcopy only-keep-debug + debuglink | 19.3 | — |
| Frame pointers: +`push rbp; mov rbp,rsp`, +6 bytes, same .eh_frame | 19.3 | XX (runtime cost) |
| wasm32 module read by hand (338 bytes: type/function/memory/global/export/code + custom sections) | 19.3 | XXV.4 |
| auxv entries and where they point; backtrace from `_start` to user main (two catch_unwinds); generated C `main`; `_start` aligns rsp to 16 and zeroes rbp | 19.4 | — |
| mini-strace (ptrace) of process start: 62 syscalls clean env vs 134 with cargo's env (40 failed openat); std init: poll(fds 0-2), SIGPIPE ignore, /proc/self/maps, sigaltstack + SIGSEGV/SIGBUS handlers | 19.4 | — |
| println 100 lines = 100 writes; BufWriter = 1 write | 19.4 | — |
| ASLR observed across 3 runs; `setarch -R` blocked by the sandbox (personality) | 19.4 | — |
| Exit statuses decoded: 0x300→3, 0x6500→101, 0x86 (SIGABRT+core)→134, stack overflow→134, SIGKILL→137, SIGBUS→135; pipefail demo | 19.4 | — |
| Address space tour: glibc per-thread arena (64 MiB aligned reservation), thread stack + guard page, mmap'd large Vec, vvar/vdso/vsyscall; canonical 47-bit addresses | 19.5 | — |
| Demand paging: 16,384 faults for 64 MiB; MADV_DONTNEED → zeros; THP madvise: 32 faults + 64 MiB AnonHugePages in one run, 2–4 MiB in others | 19.5 | XX.5 (cross-referenced) |
| Thread start: VmSize +67,600 KiB, RSS +16 KiB; 1 MiB frame → +1,024 KiB and 256 faults on entry (stack probes) | 19.5 | — |
| RSS after free (glibc): 128 MiB block returned; 1M small boxes → 78.5 MiB retained; malloc_trim → 2.2; every-64th survivor → 86.1 MiB even after trim | 19.5 | XX.4 (cross-referenced) |
| read_until vs fs::read vs mmap (44 MB): 13 / 29 / 5.6 ms; faults 6 / 10,730 / 672; RssAnon vs RssFile; glibc dynamic mmap threshold caveat (22 MB) | 19.5 | XXIII.1 |
| mmap + truncate → SIGBUS (status 135); why `Mmap::map` is unsafe | 19.5 | XXIII.1 |
| Red zone + kernel signal frames; alternate signal stack | 19.5 | — |
| Syscall ladder and cost: ~540 ns per getpid any layer; vDSO clock_gettime 24.5 ns vs 653 ns forced syscall; Instant::now 55.7 ns | 19.6 | XX |
| FDs: std sets CLOEXEC (File, TcpListener), raw libc::open inherited by child; RLIMIT_NOFILE 1024/524288; EMFILE → `TooManyOpenFiles`; EPIPE → `BrokenPipe` | 19.6 | XXI |
| fork COW: 4,097 faults for 4,096 pages written; std Command = clone(CLONE_VM|CLONE_VFORK|SIGCHLD) with 36 KiB stack; clone3 → ENOSYS in the sandbox | 19.6 | — |
| Thread clone flags 0x3d0f00 decoded; comm truncated to 15 chars; join via futex (CHILD_CLEARTID) | 19.6 | — |
| Mutex futex counts: 1×10,000 uncontended → 0 (1 is join); 4×10,000 contended → 311 (varies: 105, 530 in earlier runs) | 19.6 | — |

## Part XVIII concepts introduced

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

## Part XVII concepts introduced

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

## Part XVI concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| ABI = layout + calling convention + symbols + unwinding; the pinning feature for each; nobody checks (loader matches names) | 16.1 | — |
| `repr(C)` vs default layout measured (24 vs 16 B); `const` + `offset_of!` layout assertions; failing assertion = `E0080 evaluation panicked` | 16.1 | — |
| `Option<&T>`/`Option<Box<T>>`/`Option<NonNull<T>>`/`Option<extern "C" fn>`/`repr(transparent)` newtype = 8 B nullable pointer, FFI-safe with the lint denied; `Option<u32>` rejected | 16.1 | — |
| RFC 2195 layouts verified: `repr(C, u32)` payload at 8 vs `repr(u32)` at 4 (both 16 B), read through the defined struct/union shapes (Miri-clean) | 16.1 | — |
| "Send enums, receive integers": `DecisionCode(u32)` + `TryFrom`; code 3 → `Err` | 16.1 | — |
| `#[rustc_abi(debug)]`: `Point` Pair (Rust) vs Cast to 2 Float regs (C); `Triple` Indirect Pointer (Rust) vs OnStack (C); `can_unwind` true/false/true (Rust/C/C-unwind) | 16.1 | — |
| Release IR `byval([24 x i8])` (C) vs `dead_on_return … readonly` (Rust); asm: C caller copies 24 B, Rust caller tail-`jmp`s with its own pointer | 16.1 | XIX |
| `extern "C"` panic path in IR: `invoke` → `landingpad filter []` → `panic_cannot_unwind` | 16.1 | XIX (unwind tables) |
| System V vs Windows x64 conventions; LP64 vs LLP64 `long`; syscall ABI (`r10`, `-errno`); getpid three ways (std / libc / `syscall(39)`) | 16.1 | XIX |
| `improper_ctypes_definitions` checks by-value types only (by-reference/pointer struct: no warning, verified) | 16.1 | — |
| Declaration / call / safe wrapper; C-contract → Rust-type translation table | 16.2 | — |
| `strlen`/`getenv` wrappers (copy out; `set_var` unsafe in 2024), CStr/CString/OsStr conversions, `c"…"` literals | 16.2 | — |
| `snprintf` wrapper with truncation retry; `E0617` for `f32` to variadics | 16.2 | — |
| `qsort_r` with an `extern "C"` comparator + user data; why a generic safe comparator wrapper is unsound (C11 7.22.5); `total_cmp` ranks NaN first | 16.2 | — |
| `open` + `io::Error::last_os_error()` + `OwnedFd`; errno clobbered by logging (verified wrong error) | 16.2 | — |
| Foreign call asm: `jmp/call [rip + strlen@GOTPCREL]`; `&CStr` is two words; LLVM libcall attributes on `declare @strlen` | 16.2 | XIX (GOT/PLT) |
| Lints: `improper_ctypes`, `dangling_pointers_from_temporaries` (denied) | 16.2 | — |
| `CBuf`: malloc-owned buffer with `Drop` → `free`, `into_raw` hand-off (Miri-clean) | 16.2 | — |
| `sys` + safe binding layers; opaque `repr(C)` type with `PhantomData<(*mut u8, PhantomPinned)>`; `safe fn` extern items in a real binding | 16.2 | XXII (-sys crates) |
| Ten rules of a C API written in Rust; `ffi_guard`; checked `slice_in`/`slice_out`; `Sync` assertion for "thread-safe" headers | 16.3 | — |
| `#[unsafe(no_mangle)]` in asm; cdylib exports vs executable (`dlsym(this program)`: undefined symbol); dlopen/dlsym; `Symbol<'lib, F>` | 16.3 | XIX |
| FFM binding (`Linker`, `downcallHandle`, `Arena`, `allocateFrom`), jextract, cbindgen header; JNI `extern "system"` | 16.3 | XX (downcall cost) |
| One Java name, three encodings: FFM UTF-8 and JNI UTF-16 give the same hash; Modified UTF-8 rejected; lossy "fix" changes the hash | 16.3 | — |
| Safe `extern "C" fn` with raw-pointer params is unsound for Rust callers → `unsafe extern "C" fn` | 16.3 | — |
| Rust code called from Java runs on the Java thread's stack (`-Xss`) | 16.3 | — |
| Four questions per pointer; five ownership rules | 16.4 | — |
| `Option<Box<T>>` create/destroy with no `unsafe`; RAII wrapper on the Rust-caller side | 16.4 | — |
| Caller buffer + size query vs library `MeridianBuf` via `into_raw_parts` (len 53, cap 98) + `meridian_buf_free` | 16.4 | — |
| Callback trampolines with `void *user`; higher-ranked `&CStr`; panics parked and `resume_unwind`ed after C returns | 16.4 | — |
| Two heaps: counting global allocator vs `malloc`; allocators per cdylib (two Rust runtimes in one JVM) | 16.4 | XV.5 |
| Handles as integers: exposed provenance (Miri warning) vs generational registry (misuse = `-1`) | 16.4 | — |
| Thread-affine engine: `!Send` (E0277) + owner thread + bounded queue + one-shot replies; `EngineDown` | 16.4 | — |
| FFM arenas mapped to Rust ownership; `reinterpret(len, arena, cleanup)` adopting a Rust buffer | 16.4 | — |

## Part XV concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| Five superpowers; unsafe defines vs discharges; edition-2024 `unsafe_op_in_unsafe_fn`, `unsafe extern` + `safe` items, `#[unsafe(no_mangle)]` (1.82 syntax) | 15.1 | XVI |
| Soundness defined; validity vs safety invariants table; library UB vs language UB | 15.1 | — |
| Invalid bool returns 2 (release), `test`+`cmovne` variant returns 10; null ref, uninit int, invalid char, misaligned read (release OK / debug check / Miri) | 15.1 | — |
| Non-UTF-8 `str`: release "capacity overflow", debug precondition, Miri "entering unreachable code" in core validations | 15.1 | — |
| Debug precondition checks (1.78+, "optional, cannot be relied on"); `invalid_from_utf8_unchecked`, `useless_ptr_null_checks` lints | 15.1 | XV.6 |
| #25860 status on 1.98.1: classic snippet rejected, higher-ranked variant compiles (Miri: dangling) | 15.1 | XVIII.3 |
| Unsafe code may trust private fields and unsafe traits, never safe traits (`ExactSizeIterator` liar → heap overflow; `TrustedLen`) | 15.1 | — |
| Module = unit of trust (safe `restore` broke `HeaderBlock`); enforcement table (type / run-time / debug / unsafe fn / unsafe trait) | 15.1 | — |
| Pointer = address + provenance; same address, different provenance (native reads `b`, Miri UB; Miri spaces allocations) | 15.2 | XVI |
| `add` → `getelementptr inbounds nuw`, `wrapping_add` → plain GEP (debug IR); both `lea` in release, merged | 15.2 | XVIII.6 |
| OOB arithmetic UB without deref (Miri "in-bounds pointer arithmetic failed") | 15.2 | — |
| Strict provenance (1.84): `addr`/`with_addr`/`map_addr`; tagged pointer Miri-clean in both models; exposed provenance warning | 15.2 | — |
| Stacked Borrows mechanics (tags, retags, per-location stack) + Tree Borrows (Reserved/Active/Frozen/Disabled); POPL 2020 / PLDI 2025 | 15.2 | XV.6 |
| `as_mut_ptr()` twice: UB under SB, OK under TB; std `split_at_mut` shape (one pointer) OK in both | 15.2 | — |
| `container_of` via `&field`: UB under SB (retag covers `[0x8..0x10]` only), OK under TB; `&raw const (*whole).field` passes both | 15.2 | XV.6 (intrusive lists) |
| Two `&mut` to one element: SB error at creation, TB error at use ("foreign write" → Disabled); `get_disjoint_mut` shape (bounds + pairwise distinct) | 15.2 | — |
| `noalias` exploited: `transfer(&mut, &mut)` returns 70 while memory holds 100 (release asm, no reload); debug 100/100 | 15.2 | XX |
| `set_len` after init via `spare_capacity_mut`; `invalid_reference_casting` deny lint | 15.2 | — |
| Frame header: raw cast (release `0xfeca`/`704643072`, debug misaligned abort, Miri alignment UB) vs safe `from_be_bytes` (cmp + load + `bswap`) vs `zerocopy` (`U32<BigEndian>`, align 1) | 15.2 | XXIII.5 |
| Low-bit vs high-bit pointer tags; canonical addresses; TBI/LAM; CHERI and strict provenance | 15.2 | XIX |
| Three wrappers = three switched-off assumptions; niche table: `Option<MaybeUninit<&u8>>` 16, `Option<ManuallyDrop<&u8>>` 8, `Option<UnsafeCell<NonZeroU32>>` 8 vs 4 | 15.3 | — |
| `*p = v` on uninit slot drops garbage (Miri in `raw_vec` via `Vec<u8>` Drop); `write` vs assignment | 15.3 | — |
| `mem::uninitialized` today: 0x01 fill (`u64 = 0x0101…`, `bool = true`), panics for `&u64` | 15.3 | — |
| Panic during init: leak (0 drops) vs guard (2 drops); `write` then `init += 1` order | 15.3 | XV.4 |
| `ManuallyDrop` for drop order and Vec round trip; `Vec::into_raw_parts` stable on 1.98; `ptr::read` double drop (Miri use-after-free) | 15.3 | XVI.4 |
| `MyCell` over `UnsafeCell`; shared-write via helper evades lint, Miri "SharedReadOnly"; `!Sync` E0277 inherited | 15.3 | XI.4 |
| Why `UnsafeCell` hides niches (tag would change behind `&`) | 15.3 | — |
| Drop check: plain Drop E0597; std Vec `#[may_dangle]`; nightly `dropck_eyepatch`; PhantomData<T> keeps element drop checked; without it: compiles + UAF (Miri) | 15.3 | — |
| Zeroing cost: `resize` → `memset` in asm; measured 166–175 vs 287–293 ns per 16 KiB read (3 runs); pooled buffer kept initialized; `BorrowedBuf` unstable (E0658, #117693) | 15.3 | XX.4 |

## Part XIV concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| Three reorderers (compiler, core, memory system); SC (Lamport 1979); coherence (per location) ≠ consistency | 14.1 | — |
| SB litmus on real x86 (AMD EPYC 9R14, 4 vCPUs): Relaxed 5,321 / Rel-Acq 14,901 / SeqCst 0 / SeqCst fence 0 of 200,000 (one run); MP weak outcome 0 of 200,000 on x86 | 14.1 | — |
| Reordering table x86-TSO / Armv8 / POWER (store→load only on x86; POWER not multi-copy atomic), with citations | 14.1 | XX.5 |
| Compiler hoisting a flag loop into `jmp` to itself (`&bool`, `static mut`) vs `Relaxed` load (verified asm); loop → `memcpy` with one progress store | 14.1 | XVII.7 |
| `compiler_fence` = `#MEMBARRIER` only; for signal handlers, never for threads | 14.1, 14.4 | XIX.6 |
| Happens-before = sb + sw; data race definition and UB [LANG]; C++20 model adopted (no consume) | 14.2 | — |
| Safe Rust can't race (E0499 on two scoped threads); `unsafe` race: debug lost 56% of increments, release collapsed loop to one `add`, Miri reports | 14.2 | XV |
| Rigorous `Relaxed` counter proof (RMW atomicity + spawn/join edges + coherence), Chapter 1.3's promise | 14.2 | — |
| Six std hb mechanisms (spawn/join, Mutex, channel, OnceLock, Barrier, Release/Acquire) checked by Miri (`miri-ok`) | 14.2 | XI |
| Miri race detection: vector clocks, retags count as accesses, one schedule per run; TSan (nightly, unverified here) | 14.2 | XV.6, XXII.7 |
| Read-read coherence measured: 61M scraper reads, 0 backwards | 14.2 | — |
| Language table: data races in Rust / C / C++ / Java / Go (Go 2022 model) | 14.2 | — |
| "Benign" racy hash cache: UB under Miri; atomic fix compiles to identical asm | 14.2 | — |
| Three questions (Q1 publication → Rel/Acq, Q2 RMW handover → AcqRel, Q3 cross-location agreement → SeqCst) | 14.3 | — |
| "Publish a buffer via a counter" counterexample: correct natively on x86, Miri data race; Release/Acquire fix | 14.3 | — |
| Miri weak-memory emulation: MP Relaxed 21/40, SB Relaxed 20/40, IRIW Rel/Acq 3/40, all 0 with the stronger orderings | 14.3 | — |
| Release sequences; P0668 (C++20 SeqCst revision) | 14.3 | — |
| Orderings → LLVM IR (`monotonic` … `seq_cst`; `unordered` unexposed) → x86 asm (only SeqCst store differs: `xchg`) | 14.3 | XVIII.6 |
| Ordering costs on x86 (one run): mov store 0.14 ns, xchg 2.08, lock xadd 2.07, SeqCst fence 1.98 | 14.3 | XX |
| AArch64 mappings (ldar/stlr/dmb, LSE vs LL/SC) — unverified, labeled | 14.3 | XX |
| `invalid_atomic_ordering` deny lint; run-time "there is no such thing as a relaxed fence" panic | 14.3 | — |
| ArcSwap for snapshot publication + reclamation (Miri-clean) | 14.3 | XXI, XXIII |
| RMW family and x86 lowering (`lock inc`, CAS loop for `fetch_or` with used result); CAS loop vs `try_update` vs `fetch_max` | 14.4 | — |
| `fetch_update` renamed `try_update` (deprecated on nightly 1.100.0, `update` added) [VERSION] | 14.4 | — |
| Spinlock (TTAS + backoff): Relaxed version keeps exclusion, Miri reports retag race | 14.4 | XI.3 |
| Seqlock with atomics + fences (Boehm 2012), naive UnsafeCell seqlock UB under Miri | 14.4 | — |
| Fence rules; `fence(SeqCst)` = `lock or dword ptr [rsp - 64], 0`; Arc drop = `lock dec` + `#MEMBARRIER`; MiniArc (Miri-clean) vs Relaxed refcount (Miri race) | 14.4 | XV |
| ABA replayed deterministically on an index free list; tagged head fix; no `AtomicU128` on baseline x86-64 (E0432) | 14.4 | — |
| Reclamation schemes table (EBR, hazard pointers, refcount, don't free); crossbeam-epoch Treiber stack Miri-clean under Tree Borrows only | 14.4 | XV.2 |
| False sharing measured (shared 13.64, adjacent 14.96, 64-aligned 4.62, CachePadded 5.76, thread-local 1.20 ns/inc; one run) | 14.4 | XX.5 |
| Spinning vs OS descheduling; `pause` latency varies by microarchitecture | 14.4 | XX.7 |
| Java ↔ Rust mapping table (VarHandle modes, volatile, AtomicX, fences, synchronized, final, LongAdder, DCL) | 14.5 | — |
| DCL hand-rolled (Miri-clean) vs Relaxed pre-Java-5 port (Miri race); `OnceLock`/`LazyLock` | 14.5 | — |
| `read_volatile`/`write_volatile` are not Java `volatile` (Miri race on the flag) | 14.5 | XVI (MMIO/FFI) |
| Striped counter (LongAdder) 8× faster than one AtomicU64 (one run); `sum()` not a snapshot | 14.5 | XX |
| Final-field semantics unnecessary in Rust; JMM vs C++20 differences (racy reads, OOTA, reentrancy, reclamation) | 14.5 | — |
| Testing tools: jcstress vs loom vs Miri vs TSan (loom/TSan unverified here) | 14.5 | XXII.7 |

## Part XIII concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| Runtime seen from `/proc`: 4 `tokio-rt-worker` threads, one epoll instance (plus its `dup`), an eventfd, and the signal driver's process-global socketpair (still open after the runtime is dropped) | 13.1 | XIX.6 |
| `#[tokio::main]` expansion (real `-Zunpretty=expanded`): `Builder::new_multi_thread().enable_all().build().block_on(body)`; the body runs on the main thread | 13.1 | — |
| Tokio 1.53.1 defaults read from its own source at run time: coop budget 128, local queue 256 (steal half), LIFO ≤ 3 polls, event_interval 61 ("copied from golang"), global queue 31 / self-tuned ~10 ms, blocking pool 512 + 10 s keep-alive, nevents 1024, timer wheel 6 × 64 (MAX_DURATION 2^36−1 ms), BOX_FUTURE_THRESHOLD 2,048 debug / 16,384 release | 13.1 | technique reusable everywhere |
| LIFO slot: 1,000 of 1,000 children on the parent's worker; can't be stolen: heartbeat waited 300 ms vs 28 µs after one more spawn | 13.1 | XX.7 |
| Coop budget: recv loop heartbeat 2 ms late; `unconstrained` 38 ms; always-ready non-Tokio futures 399 ms; `yield_now` fixes it | 13.1 | — |
| Timers: 1 ms ticks, rounded up (1 µs sleep ≈ 1.06 ms); 100,000 timers registered at once (worst lateness 24 ms); `MissedTickBehavior` Burst vs Skip | 13.1 | — |
| Task memory: one allocation, 88 B + the future; futures above the threshold boxed first (2 allocations) | 13.1 | XX.4 |
| Costs: spawn+join ~300 ns (current_thread) / ~800 ns (4 workers); round trip ~180 ns / 250–400 ns; OS-thread round trip ~9 µs | 13.1 | XX |
| Stable `RuntimeMetrics` (num_workers, num_alive_tasks, global_queue_depth, worker_total_busy_duration, park counts) vs `tokio_unstable` (E0599 for worker_steal_count) | 13.1 | XXII.5 |
| `spawn` needs `Send + 'static` (E0373; "future cannot be sent ... used across an await"); no safe scoped spawn (leak argument); `LocalSet` for `!Send` | 13.2 | — |
| `JoinHandle`: drop detaches, `abort` at next `.await`, panic → `JoinError::is_panic`, abort after completion is a no-op | 13.2 | — |
| `thread_local!` under task migration: 1,489 of 1,600 reads saw another request's ID; `task_local!` 0 | 13.2 | XXII.5 (tracing spans) |
| Blocking measured: 1 of 4 workers blocked 1.9 ms, 4 of 4 196 ms, spawn_blocking 1.1 ms; CPU work inline 444 ms vs spawn_blocking 2.5 ms vs rayon + oneshot 17 ms | 13.2 | XX.7 |
| Blocking pool: grows to the cap, unbounded queue, keep-alive shrink, running closure can't be aborted; `block_in_place` panics on current_thread | 13.2 | — |
| mpsc as backpressure (sends at the consumer's pace); `send` vs `try_send` vs `reserve`; closure signals | 13.3 | — |
| Actor pattern: limits actor grants exactly 66 of 100; a panicked actor is visible to callers | 13.3 | XXIV |
| broadcast `Lagged(6)`; watch latest-only; Notify permit semantics, the lost wakeup, `enable()` | 13.3 | — |
| Channel memory: mpsc(1,000,000) 800 B empty, ~9 B per queued u64; broadcast(1,000) preallocates 41,096 B | 13.3 | — |
| Semaphore (limit / shed / close); std Mutex 4 ns vs tokio Mutex 22 ns; convoy 500 ms vs 10 ms | 13.3 | — |
| Cancellation = drop: which steps ran under 100/60/20 ms timeouts; JoinSet completion order and abort on drop | 13.4 | — |
| Tokio's per-method cancel-safety statements (read from source) | 13.4 | — |
| `select!` + `read_exact` lost 8 bytes and glued frames; `FramedRead` intact | 13.4 | — |
| Graceful shutdown: CancellationToken + TaskTracker + drain deadline | 13.4 | XXI, XXIV |
| `Drop` can't await: BufWriter dropped lost 3,520 bytes; `Handle::try_current` backstop; runtime drop waits for blocking tasks (280 ms) vs `shutdown_timeout` | 13.4 | — |
| TCP backpressure (2.5 MiB accepted, then `Pending`) vs UDP (92 of 20,000 kept; 9,000-byte datagram truncated to 2,048) | 13.5 | XXI.1 |
| Metastable simulation: unbounded goodput 294 (906 wasted), bounded + shed 1,020, skip-if-expired 1,049 (p50 493 ms) | 13.5 | XXI.4, XXIV |
| Tower layer order: `Timeout` doesn't time `poll_ready`; `Buffer` moves the wait into `call` | 13.5 | XXII.3 |
| Deadline propagation vs per-hop timeouts (110 ms of wasted processor work vs a refusal at 110 ms) | 13.5 | XXI.4 |
| Listen backlog: Tokio's `bind` → mio → `listen(128)` ("same as std"); overflow: `connect()` returns but `accept` waits for retransmits (243–609 of 1,000 accepted in 0.5 s vs all with 2,048) | 13.5 | XIX.6, XXI.1 |
| tokio-util 0.7.19: FramedRead + FramedWrite = 16,384 B per idle connection (1.64 GB at 100,000) | 13.5, L5 | XX.4 |
| Ferrite v2: task per connection; Semaphore shed with BUSY; `-ERR <CODE>`; LineCodec (split frames, linear scan); flush rule; `max_unflushed` + write timeout; lingering close; JoinSet shutdown with drain; per-server stats; configurable backlog; `Arc<[u8]>` values with a defaulted `KvStore::get_shared` | L5 | XXIII (v3) |

## Part XII concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| Thread per connection measured: `EAGAIN` at connection 503 (sandbox task limit); 2,064 KiB virtual / ≈9.5 KiB resident per thread; glibc malloc arenas (64 MiB each, up to 8 × cores) explain the first-100-threads jump | 12.1 | XIX.5 |
| Nonblocking spin (19,617 `read` calls for 4 bytes); select/poll/epoll/kqueue table; epoll level- vs edge-triggered verified natively and under Miri (Miri models epoll + socketpairs) | 12.1 | XXI.1 |
| mio C10K: 10,000 connections, 1 server thread, 8,807 `epoll_wait` wakeups, RSS +264 KB; `RLIMIT_NOFILE` raised via libc; kernel sockstat | 12.1 | XIII.1 |
| Completion I/O (io_uring, IOCP) needs owned buffers because drop = cancel (`tokio-uring`, `glommio`, `monoio`) | 12.1 | XXI, XXIII.1 |
| Kernel limits for threads (`pids.max`, `vm.max_map_count`, `threads-max`/`pid_max`, `RLIMIT_NPROC`, `RLIMIT_NOFILE`); Java "unable to create native thread" | 12.1 | XIX.6 |
| RFC 230 → `Future` in `core`, async as a library; timeline 2014–2025 (async-std discontinued 2025 [VERSION]); C10K (Kegel 1999), C10M (Graham 2013), thread-per-core runtimes | 12.1 | — |
| `Future` contract table (Pending obligation, no blocking, no poll after Ready, spurious polls); lazy futures (16-byte `charge` future); `unused_must_use` on futures as an error | 12.2 | — |
| Lost wakeup three ways (NeverRegisters / RegistersOnce hang, RefreshesEveryPoll ok); `Waker::will_wake`; check-and-publish atomically | 12.2 | XIII.3 |
| `join`/`select` by hand; cancellation = drop (verified with a `Drop` reporter); `CompletableFuture` comparison (`cancel(true)` doesn't interrupt) | 12.2 | XIII.4 |
| In-flight waiter coalescing (`WaitFor`): wake after releasing the lock; never hold a lock across `.await` | 12.2 | XIII.3 |
| Future sizes (1 B `nothing` … 1,026 B array across await, 8 B dead before it, 1,028 B sequential vs 2,072 B joined, 16 B boxed); hand-written enum 24 B vs compiler 40 B | 12.3 | — |
| Real coroutine MIR (`coroutine layout`, variants Unresumed/Returned/Panicked/SuspendN, `storage_conflicts`), resume `switchInt`, "`async fn` resumed after completion" panic; release asm jump table + waker vtable call `[rcx + 16]` | 12.3 | XVIII.4 |
| `async fn` arguments stored twice (prefix upvar + saved local): 64 B vs 40 B (`answers-ch03-sizes.rs`) [RUSTC] | 12.3 answers | XVIII.4 |
| `!Send` futures verified: `Rc`, `MutexGuard`, `Box<dyn Error>` across `.await` (error has no code: `error:future` needle); fixes | 12.3 | XIII.2 |
| E0733 async recursion; `Box::pin` = 1 × 16-byte allocation per level (counting allocator) | 12.3 | — |
| `async fn` in traits (1.75): E0038 for `dyn`; Send bound problem (E0277 + compiler's `-> impl Future + Send` suggestion); RTN (RFC 3654) unstable; async closures (1.85) with `AsyncFn` | 12.3 | XXII.3 |
| Returned future captures all argument lifetimes (E0502 before polling, `ch03-18`); RPITIT always captures, edition 2024 extends to free fns | 12.3 | — |
| 1 MiB future overflows a 512 KiB stack (debug + release); cancellation safety + guard pattern (`payout_v1`/`v2`) | 12.3 | XIII.4 |
| Self-referential struct dangling under Miri; pinned version miri-ok (Stacked + Tree Borrows); E0277 "cannot be unpinned"; every async future `!Unpin` (even `async { 1 + 1 }`) | 12.4 | XV.2 |
| Pin guarantee (no move + drop guarantee); `Pin::new` / `pin!` / `Box::pin` / `new_unchecked` table; `Pin<&mut F>` and `Pin<Box<F>>` 8 B | 12.4 | — |
| Intrusive waiter list (40-byte node inside the future), miri-ok under both models; without the `Drop` unlink → use-after-free under Miri | 12.4 | XIII.3, XV.4 |
| Hand-written pin projection (`WithBudget`) + the five structural-pinning obligations; one safe `impl Unpin` → UAF (`ch04-06`); `pin-project-lite` + E0119 on a manual `Unpin` | 12.4 | XV.1 |
| `&mut T` for `T: !Unpin` loses `noalias` and `dereferenceable` in LLVM IR; `UnsafePinned` (RFC 3467) [VERSION] | 12.4 | XVIII.6 |
| `block_on` with the park/unpark token (Miri-clean); `RawWaker` vtable by hand (16 B; clone/wake/wake_by_ref/drop accounting, Miri) | 12.5 | — |
| Executor: run queue + `queued` dedup flag cleared before the poll; timer thread + `BinaryHeap` + `Condvar`; `JoinHandle` as a future | 12.5 | XIII.1 |
| Reactor on mio: 100 clients, 1 thread, 201 tasks, 302 polls, 2 `epoll_wait` calls (derived from the code) | 12.5 | XIII.1 |
| Spawn cost: ≈158 ns / 2 allocations per task vs ≈36 µs per OS thread spawn + join (one run) | 12.5 | XX |
| Deterministic simulation executor (virtual time, seeded scheduling): check-then-act double charge in 183 of 1,000 seeds, replayable; FoundationDB attribution; turmoil/madsim | 12.5 | XXII.7, XXIV |
| Wake dedup measured (n = 1,000: 252,000 vs 2,500 child polls); `futures::join_all` switches to `FuturesOrdered` above 30 children [LIB] | 12.5 | XIII |
| Memory per unit measured: thread 2,064 KiB virtual / 9.6 KiB resident vs task 247 B heap / 0.28 KiB resident (34× resident, not 8,500×); `LocalPool` spawn-queue high-water mark (3 MiB) retained, not leaked | 12.6 | XIX.5 |
| Round trip: threads 7.7–9.6 µs vs async tasks 115–118 ns (three runs) | 12.6 | XX.7 |
| Blocking on the executor: 194 ms heartbeat stall vs 1 ms with `spawn_blocking`; CPU-bound: 143 ms vs 1 ms with `yield_now` every ≈140 µs (1,025 yields, no measurable cost); coop budget doesn't cover pure computation | 12.6 | XIII.2 |
| Stackful runtimes: Go (copying stacks 1.3, 2 KiB 1.4, adaptive start 1.19, netpoller, P handoff, async preemption 1.14, cgo stack switch); Java VT (JEP 444 mount/unmount, heap stack chunks, no time-slicing, pinning, JEP 491 in JDK 24); Erlang reductions; why M:N failed in C and in pre-1.0 Rust | 12.6 | — |
| Function coloring (E0728); `Handle::block_on` inside a runtime panics ("Cannot start a runtime from within a runtime"); ways-across table (`spawn_blocking`, `block_in_place`) | 12.6 | XIII.2 |
| Cancellation compared: Rust drop, Java interrupt (`Thread.stop` throws since JDK 20), Go `context`, Erlang exit signals | 12.6 | XIII.4 |
| Five-model × ten-criterion concurrency decision matrix; "choose by where the waiting is and who owns the code that waits" | 12.6 | XXI, XXIV |

## Part XI concepts introduced

| Concept | Where | Full treatment planned |
|---|---|---|
| std::thread = 1:1 OS thread; spawn = pthread_create → clone + 2 MiB mmap stack + 4 KiB `---p` guard page, seen in `/proc/self/maps` and `/proc/self/task` (TID = PID for main) | 11.1 | XIX.4, XIX.6 |
| `'static` on spawn as an ownership statement; JoinHandle = result moved back (`Ok(T)` or panic payload `&str`/`String`); main returning kills other threads (verified: detached uploader never printed) | 11.1 | — |
| spawn+join ~28–34 µs vs hand-off ~5.7 µs (one run); oversubscription: same total, mean completion 73 vs 57 ms (64 threads vs pool of 4) | 11.1 | XX |
| Thread memory: virtual vs resident stack, kernel per-thread cost, limits (ulimit -u, threads-max, pids.max, max_map_count); Little's law pool sizing; no `interrupt()`, cooperative cancellation | 11.1 | XIX.5 |
| Send/Sync defined via aliasing XOR mutation; auto-trait table read from the compiler (inherent-const probe trick); E0277 notes as the proof chain; std's on_unimplemented RwLock suggestion | 11.2 | — |
| Disjoint capture changes which types are checked (2018 E0277 vs 2024 OK; non-move `stats.hits` bypassing a wrong `unsafe impl Sync`) | 11.2 | — |
| False `unsafe impl Sync` on Cell counters → Miri "Data race detected"; AtomicU64 version miri-ok; rusqlite `Connection` Send-not-Sync from its RefCells | 11.2 | XV |
| Rc vs Arc clone asm (`inc` vs `lock inc`); Rc 1.1 / Arc 3.5 / shared Arc 57.2 ns (4 threads) / per-thread Arc 3.6 ns | 11.2 | XIV.1 |
| Auto traits as implicit public API (a private field is semver-major); `unsafe impl` justification table; bounds for APIs (`Box<dyn Error + Send + Sync>`, `Arc<dyn Trait + Send + Sync>`) | 11.2 | XXII (semver-checks) |
| Mutex owns data; guard = run-time `&mut`; `Arc::try_unwrap(..).into_inner()` / `get_mut` need no locking | 11.3 | — |
| Futex mutex asm (`lock cmpxchg` / `xchg`, state 0/1/2, poison byte at +4, data at +8; `Mutex<u64>` = 16 B); futex-based since 1.62 | 11.3 | XIX.6 |
| Poisoning: three policies (propagate / ignore / repair + `clear_poison` 1.77) with verified output; per-lock policy table | 11.3 | — |
| Condvar bounded queue (`wait_while` consumes/returns the guard; spurious wakeups; close + drain) | 11.3 | XIII.3 |
| Linux std RwLock prefers writers: recursive read while a writer waits deadlocks (verified via `try_read`); parking_lot same, `read_recursive` | 11.3 | — |
| Guard lifetime = temporary lifetime: `if let` else-branch relock works in 2024, deadlocks in 2021; `match` holds the guard in all editions | 11.3 | — |
| Lock ordering by id (8 threads, no deadlock); non-reentrant Mutex (Java port trap); shared-map E0499 vs Java 7 / Go failures | 11.3 | — |
| UnsafeCell: the only legal mutation through `&T` (3 effects); Miri "SharedReadOnly" without it; MyCell on UnsafeCell miri-ok | 11.4 | XV.3 |
| OnceLock (init once under race, one Acquire load after) vs LazyLock (poisoned forever after a panicking init); `get_or_try_init` unstable (E0658) | 11.4 | XIV (orderings) |
| Per-request Cells folded into global atomics in Drop (4 RMWs per request) — Chapter 4.1's promise | 11.4 | — |
| Interior-mutability family sizes (RefCell 16, OnceCell<String> 24, Mutex<()> 8, RwLock<()> 12, parking_lot::Mutex<()> 1, Mutex<[u8;64]> 72) | 11.4 | — |
| Read path: Mutex<Arc> 219 ns, RwLock<Arc> 259 ns, ArcSwap 7.8 ns, plain 2.1 ns (4 readers, one run); arc-swap debts | 11.4 | XX |
| ArcSwap route table + dropper thread waiting for the last reference (0 torn reads, 50/50 freed on dropper) | 11.4 | — |
| Channels move ownership (E0382 after send); disconnection = end of stream; Empty vs Timeout vs Disconnected; try_send/SendError hand the value back | 11.5 | XIII.3 |
| std mpsc = crossbeam port since 1.67 (list/array/zero flavors, stamps with Release/Acquire, parking); Sender: Sync since 1.72; Receiver !Sync (E0277); std `mpmc` unstable (E0658) | 11.5 | — |
| crossbeam select + shutdown by dropping a Sender; owner-thread pattern with one-shot reply channels | 11.5 | — |
| Unbounded channel memory byte-exact: 72 B per 64-B message, 31-slot blocks, 7,058 KiB per 100K; bounded(1000) refuses 99,000 | 11.5 | — |
| Channel cost: ~18–24 ns/item buffered, ~5.7–7 µs lockstep (`sync_channel(1)`), ~5 ns batched, 0.4 ns no channel (one run each) | 11.5 | XX |
| `thread::scope` soundness by control flow; `thread::scoped` Leakpocalypse rebuilt → Miri dangling reference; stable 1.63 | 11.6 | XV.1 |
| Rayon: deques, LIFO owner / FIFO thieves, adaptive splitting, `join`; parallel logstat exact merge (81.9 → 36.2 scoped / 27.9 rayon ms, one run) | 11.6 | — |
| Parallel BFS level-synchronous with CAS claims (363.9 → 196.5 ms); par_iter break-even ~10K–100K cheap elements (tens of µs) | 11.6 | XX |
| Rayon closures must be `Fn + Send + Sync` (E0596 for captured mutation) vs Java parallelStream lost updates | 11.6 | — |
| Counter ranking, 5 runs: no sharing 0.3–0.6 < padded 2.9–6.7 < false sharing 5–23 ≈ shared atomic 9–19 < parking_lot 26–35 < std Mutex 38–181 ns; CachePadded = 128 B | 11.7 | XIV.4, XX.5 |
| Concurrent maps, 5 runs (4 threads, 95% reads): Mutex 160–272, RwLock 92–154, 16-shard Mutex 74–76, 16-shard RwLock 76–81 ns, owner thread 6.5–19.4 µs | 11.7 | — |
| I/O under a lock: 943 vs 3,687 req/s (3.9×); the ~1,000/s ceiling derived | 11.7 | XX.7 |
| Single-writer counter `store(load + 1)` compiles to `inc` without `lock` (verified asm) vs `fetch_add` = `lock inc` | 11.7 | XIV |
| Decision procedure + seven-design twelve-criteria matrix; Java concurrency API mapped row by row with analogy limits | 11.7 | — |
| Project L3: WorkerPool<T> (bounded queue, `try_submit -> Result<(), T>`, catch_unwind per item, Drop = graceful shutdown), HTTP/1.1 parsing with limits, 503 admission control, keep-alive, idle timeout, self-connect shutdown trick | L3 | XIII (Ferrite v2), XXI |
| Project L4 Ferrite v1: `KvStore` (&self, owned values, dyn-compatible, infallible in v1), ShardedStore (16 RwLock shards, RandomState router), recover-count-clear poisoning, line protocol + pipelining flush rule, L3 pool reused unchanged | L4 | XIII (v2), XXIII (v3), XXIV (v4, v5) |
| Ferrite v1 measured: 98.6K (debug) / 184K (release) cmds/s over loopback TCP with 8 clients; 2.6M / 18.8M ops/s in-process (one run); shard spread 347–411 of 375 ideal | L4 | XIII (compare with v2) |
| Review capstone: check-then-act over-admission bounded by limit + threads − 1 (26–27 of 20, five runs); lock-order inversion; poison → double-panic abort chain; fixed design (one critical section, act after, bounded audit writer with rollback, per-window billing) | Part XI review | — |

## Part X concepts introduced

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

- ~~**Part X (Iterators):** `for x in vec` vs `&vec` vs `&mut vec`; iterator chain adapter by adapter; closure types
  per generic instance; fn item vs pointer vs closure; HRTB closures; lending iterators/GATs; in-place `collect`;
  zero-cost claims of 1.3~~ **KEPT** (10.1–10.4).
- ~~**Part XI (Concurrency):** Cell→atomics, Arc swap for catalogs/route tables, parallel `logstat`, poisoning policy,
  pools that survive panics, Ferrite v1 panic policy, interior mutability, the decision matrix, Send/Sync manual impls,
  disjoint capture, concurrent maps measured, rayon/parallel BFS, channels, atomic vs mutex vs no-sharing measured,
  Java mapping, Ferrite v1 contract~~ **KEPT** (11.1–11.7, Projects L3 and L4). Small gap: Part XIV didn't compare
  seqlocks with Java's `StampedLock` (11.3 §8 promised it); pick it up in XX.7 or XXIV if natural.
- ~~**Part XII (Async):** async recursion needs `Box::pin`, "no runtime", stackless future sizes, RFC 230, Go
  G-M-P/netpoller vs stackless, `async fn` in traits and dyn compatibility, RPIT capture for async return types,
  `AsyncFn*`, `gen` blocks vs coroutine lowering~~ **KEPT** (12.1–12.6; `Box<dyn Error>` across `.await` delivered early
  in 12.3).
- ~~**Part XIII (Tokio):** cancellation = dropping a future, frame split across reads, no locks across `.await`,
  `Box<dyn Error>` across `.await`, Ferrite v2 `-ERR <CODE>` (BUSY, shutting down), `spawn_blocking`, Drop can't await,
  `JoinError::is_panic`, Rayon off the executor, `Semaphore`/`Notify`, scheduler internals (LIFO slot, budget, timing
  wheel, eventfd waker, blocking pool), `tokio::sync::Mutex`, "no reactor running", cancellation safety, Ferrite v2
  connection limit / write timeout / unflushed-bytes cap / `Arc<[u8]>` values, slow clients cost KiB~~ **KEPT**
  (13.1–13.5, Project L5). Not kept, carried forward: the async tail service reusing the logstat library (→ XXII), buffer
  pools with capacity caps (9.1, only touched in L5 §5). Correction found by verification: Tokio's `bind` listens with a
  backlog of 128 (via mio), not 1,024.
- ~~**Part XIV (Memory model):** publish-via-counter counterexample, Relaxed justified by join, JLS 17.7, MESI analogy,
  false sharing measured~~ **KEPT** (14.1–14.5).
- **Part XV (Unsafe), partially kept:** ~~crossbeam-epoch `Local::element_of` SB-vs-TB (14.4), `unsafe impl Send/Sync`
  obligations and `MaybeUninit` slots (Part XIV), Stacked/Tree Borrows (4.2, 4.6), drop check + `#[may_dangle]` +
  PhantomData's drop-check row (3.5, 5.3), `split_at_mut`/`get_disjoint_mut` internals (4.1), validity invariants and
  niches (2.3, 5.2), `set_len`, privacy as memory-safety boundary (2.7), soundness depending on same-module safe code
  (12.4)~~ **KEPT** (15.1–15.3). **Still open, owed by the unwritten 15.4–15.6:** leak amplification (`Vec::drain`), ZST
  handling (5.3), `try_reserve`/fallible allocation (9.1), `GlobalAlloc` and the counting allocator explained (Part
  III), intrusive linked lists (9.4), Miri in CI with both aliasing models (4.6, 12.x); verified listings for 15.4 exist
  (`listings/part-15/ch04-*.rs`).
- ~~**Part XVI (FFI):** `extern "C"` ABI, `catch_unwind` at entry points, `C-unwind`, `cdylib` exports, the fraud library
  via FFM (codes 0/-1/-99, concrete `score_batch`), JNI vs FFM strings, `Option<&T>`/`repr(transparent)`, `repr(C)` and
  `repr(C, u32)` enums, thread-affine handles, `unsafe extern` with `safe` items, raw-parts ownership transfer, exposed
  provenance, `conv: Rust` vs `extern "C"`~~ **KEPT** (16.1–16.4, narrower scope: no UB demonstrations). Partly kept:
  loading a `cdylib` plugin: `dlopen`/`dlsym` and the `Symbol<'lib>` lifetime are verified, the `libloading` load is a
  labeled sketch. Promises XVI made to XIX/XX, which were written in parallel: XIX covers `nm -D`, `@GOTPCREL`,
  `RTLD_LOCAL`/`RTLD_GLOBAL`, interposition, `dlopen` and `libgcc_s` unwinding; **still open:** the `cdylib` version
  script, `-rdynamic`, reading a JVM `hs_err` log with native frames, measuring FFM downcall overhead vs batch size
  (JMH), `qsort_r` vs `sort_by` + `total_cmp`.
- ~~**Part XVII (Compilers):** SSA/φ/register allocation, unification-based inference, definite assignment as
  dataflow, LICM/loop → `memcpy`/loop collapse and their license, closure compilation of an interpreter~~ **KEPT**
  (17.1–17.8). The Part's language **Ore** is specified in `notes/part-17-report.md` for Part XXVI to grow.
- ~~**Part XVIII (rustc):** trait solver, vtable layout, collector/CGUs/shared generics/function merging/Cranelift,
  exhaustiveness and match lowering, `?` desugaring, borrowck on MIR and Polonius status, post-mono errors, privacy
  phases, drop elaboration, `noalias`, queries/incremental, hygiene, HIR desugarings, `let x = foo();` traced,
  `getelementptr inbounds nuw`~~ **KEPT** (18.1–18.7). **Gaps:** the coroutine MIR state transform, prefix/overlap
  layout, and `async fn` arguments stored twice (12.3 → 18.4) were not covered (18.2 shows only the HIR side of async
  lowering); "intercrate mode" appears only via the next-gen solver (18.3). **New fact:** NLL problem case #3 is
  rejected on stable 1.98.1 and beta but **accepted on nightly 1.100** with no flag (18.5, verified), so Chapter 4.2's
  statement is true for stable and the change is coming.
- ~~**Part XIX (Binary/OS):** mmap input for logstat, linking in depth, unwind tables and symbolization, exit
  statuses, RSS, guard pages and probes, musl, spawn/futex syscalls, thread stacks, pointer tagging, GOT/relaxation, v0
  symbols, `lang_start`, `personality`, allocator shims, proc-macro dylibs, `split-debuginfo`/`strip`, relocations,
  frames/red zone/alignment, page-fault mechanics~~ **KEPT** (19.1–19.6). New facts: the Playground container has
  `rustc`, `gcc` 13.3 and binutils (readelf, objdump, nm, strip) with a writable `/tmp`, so listings can build and
  inspect real binaries; `RUSTC_BOOTSTRAP=1 rustc -Z …` works inside it; `std`'s weak `pidfd` references make
  `GLIBC_2.39` a hard requirement, so the build machine's glibc sets the floor. Promises XIX made to XX (written
  earlier) stay open as exercises: GOT-indirection cost vs relaxation, frame-pointer cost, syscall cost in containers.
- ~~**Part XX (Performance):** every "measure it in Part XX" promise from Parts I–XVIII~~ **KEPT or explicitly labeled**
  (20.1–20.7): the Part XX README has a table mapping each promise to the section that pays it off or to the local
  command that would (perf, PGO/BOLT, NUMA, Arm, `-Z self-profile` are labeled, not run). Measured corrections to
  earlier estimates: the fraud feature-vector change saves ~1.4% CPU per score (Part IX estimated ~3%), mainly removing
  408 allocations per score and improving the tail; the access-log fix pays off in the 4-thread tail, not CPU; the
  order-book arena gives 1.4× throughput and half the p99 of the `Rc<RefCell>` design; THP was requested but mostly not
  granted on the Playground host. New fact: two "identical" sums measured 1.7× apart with identical asm; reordering the
  closure definitions removed it (code layout bias, 20.2).
- **Part XXI (Networking):** harden the Meridian gateway (Part I review); timeouts, retries, circuit breakers, retry
  budgets, deadline propagation (8.4); HashDoS and network input (9.3); Netty ByteBuf analogy (4.3, 4.5); io::ErrorKind
  by connection phase (8.4); owned-buffer completion I/O (io_uring) for servers, socket-buffer tuning at scale (12.1);
  TLS for L3/L4 (21.3); HTTP/2 streams and chunked bodies (L3, 21.2); request-smuggling strictness (duplicate
  `Content-Length`), per-request deadlines against slowloris (L3 §5); the whole-gateway load test and canary numbers
  deferred by 20.1 §9; syscall and TLS cost per request measured with Part XX's tools; wrk2 load curves; hedging with
  budgets and cancellation (21.4); pool sizing with Little's law (21.5); EMFILE handling in accept loops and
  descriptor budgets (19.6); HTTP/1.1 `Connection: close` and HTTP/2 GOAWAY in graceful drains (13.4); the Nagle /
  `TCP_NODELAY` mechanism L5 refers to; retry budgets and circuit breakers on top of 13.5's shedding; TLS for Ferrite.
- **Part XXII (Ecosystem):** serde zero-copy (`Cow<'a, str>`, `DeserializeOwned`) (4.3, 4.5); serde `transparent` /
  `try_from` at boundaries (5.3); tagged enums (2.6); money as strings in JSON (2.3); clap; thiserror/anyhow in crate
  choice (8.2); axum `IntoResponse for PaymentError` (8.4, unverified sketch); tower Retry policy by error class; Tower
  `BoxService` hybrid (6.5); tracing observability (8.4); clippy `result_large_err` (8.2); cargo-hack / cargo-deny /
  `cargo tree -d` / cargo-semver-checks, `#[non_exhaustive]` semver (2.2, 5.1, 6.2, 6.3); fuzzing the header parser;
  serde_json recursion limit (Interlude); concurrency testing in CI: loom (`cfg(loom)`), TSan, Miri seeds, aarch64
  stress runners (14.x → 22.7); deterministic simulation testing (turmoil, madsim) and Miri in CI (12.5); publishing
  `WorkerPool` as a crate (L3 review Q7, 22.1); Tower (22.3); `cargo-semver-checks` for lost auto traits (11.2); a
  binary-safe framing for Ferrite (L4 omissions → Project L6); proc-macro allowlist and `cargo expand` (18.2);
  compile-time SQL checking trade-off (18.2 → 22.4); Tower `BoxService` to cap type depth (18.3 → 22.3); `cargo test
  --release` in CI and reverse-dependency builds for shared crates (18.5, 18.6 → 22.7); semver checks for `Drop` and
  lifetime additions; fuzzing lexers and parsers ("never panics, every byte covered by one token"), differential
  testing and shadow mode, library CI that recompiles dependents (17.2–17.4 → 22.7); criterion/divan and
  cachegrind-based instruction-count CI, the benchmark template from 20.2 §10, dhat-rs and continuous profiling, tracing
  spans for per-request latency (20.x → 22.5, 22.7); `bindgen`/`cbindgen` in build scripts, `-sys` crate conventions
  and the `links` key, cross-language LTO, `jextract` in the Java build, `staticlib` for cgo (16.x); reproducible builds
  (`--remap-path-prefix`, build-id comparison job), a release gate / artifact audit in CI (Part XIX review); `tracing`
  spans as request context (13.2); tower `Buffer`/`LoadShed`/`ConcurrencyLimit`/`RateLimit` in depth (13.5); exporting
  Ferrite v2's `Stats` (22.5); `tokio-console`; the async tail service reusing the logstat library (carried from XIII).
- **Part XXIII (Storage):** transaction guard → real transactions (3.5); commit with unknown outcome (8.3); persistent
  encodings instead of in-memory layout, padding (5.2); DB compare-and-set for state transitions (5.4); Ferrite v3
  `FerriteError` and a fallible store API (8.2 design exercise: reconcile with the brief's contract by adding a
  fallible trait or adapter, keeping v1's `KvStore` usable); zero-copy `RequestRef<'a>` (1.4); Ferrite v3 adds the
  fallible trait **next to** `KvStore` (L4 §3) and reuses `store_semantics` as a conformance suite (L4 §4);
  `update`/`INCR` logs the result, not the closure; transactions and cross-shard atomicity (23.6); zero-copy formats
  with `zerocopy`/`bytemuck`, byte order in the type (15.2 §9 → 23.5); arena-per-batch memory for memtables and
  compaction (20.4), RSS vs live heap in a storage engine, page-cache effects measured with `getrusage`/faults; an
  explicit `mmap` policy for Ferrite's segment files vs the WAL and the SIGBUS contract (19.5).
- **Part XXIV (Distributed):** idempotency + reconciliation for ambiguous outcomes, "did it happen?" (8.4); generational
  arena / LRU reused (3.6); deterministic simulation for Ferrite's replication tests (12.5); Ferrite v5 replaces
  `RandomState` routing with a stable hash or range partitioning behind a versioned partition map (L4 §2); the leader
  executes read-modify-writes and replicates results; partitioning large state (11.7 §5); hedged reads to replicas,
  tail at scale for scatter-gather queries, thread-per-core for Ferrite shards, Little's law for replication
  pipelines (20.x); deterministic simulation on a paused Tokio clock (as used throughout 13.x) for replication tests;
  outbox/saga for cancellation-safe multi-step operations (13.4 §7); idempotent requeue and per-merchant sequence
  numbers (Part XIII review); graceful handover with `SO_REUSEPORT` (L5).
- **Part XXVI (Language):** specialization and language-design trade-offs (6.3); `#[non_exhaustive]` and exhaustiveness
  (5.1); mirror rustc's architecture in the toy compiler (queries/HIR/MIR stages as the reference design, borrow
  checking on a MIR-like IR in 26.5–26.6); local type inference, hygiene, and Polonius-style analyses as language-design
  choices (18.x); optionally the coroutine state transform left open by XVIII (12.3); grow **Ore** (Part XVII's
  language, spec in `notes/part-17-report.md`) with structs, enums, patterns, ownership, and a borrow checker on a
  MIR-like IR; reuse the XVII pipeline listings; 26.7 targets LLVM; the Sieve rule language as a DSL-sized second
  example; the lexer class table and spill-aware register allocation in the toy compiler (20.5, 17.8).

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
| Gateway log sampler | config rules like `status>=500 \|\| path^=/payments && latency>250`; closure compilation chosen (20-line `compile`), `Pred` in `ArcSwap`; interpreter would cost ~4.6 ms CPU/s at 400K req/s; analytics pipeline compiles fixed rules to Rust | 10.1 §9 |
| Retry counter incident | `move` over `Copy` `attempts` in the gateway's retry helper → access log reported 0 attempts; retry-budget alert stayed flat while upstream traffic tripled during a processor brownout; fix: helper returns `(Result, attempts)` | 10.1 §10 |
| Market-data replay tool | capture files replayed through `Frames<'a>`; 2 GB segments, 0 allocations per frame; truncated trailing frames are items; `FusedIterator` required for chained segments | 10.2 §9 |
| Ingestion batcher incident | `by_ref().zip(0..size)` batching lost 1 event per batch from a channel (batch 500 → 1 in 501); 0.2% gap in nightly reconciliation, dismissed for a week; fix `take(n)` + tests with non-exact sources | 10.2 §10 |
| Settlement batch job guidelines | ~40M rows/night, CSV + ISO 20022 XML; `Box<dyn Iterator>` per file format (~65 ms dispatch/night, accepted); generic per-row work; no mid-pipeline collects; the real cost was a `String` per CSV field | 10.3 §9 |
| Fraud blocklist memory incident | refresh every 10 min; ~8M candidates (32 B) → ~2% active; in-place collect kept ~256 MB per list; OOM kills at the 1 GB limit; fix `shrink_to_fit` + capacity gauge | 10.3 §10 |
| Merchant statement job (Rust port) | moved from Java; `BTreeMap` for stable order, integer `fold` accumulator, `try_fold` over `Result<Txn, LedgerError>` rows | 10.4 §9 |
| Fee schedule duplicate-key incident | partner CSV export; Java `toMap` threw on duplicates; Rust port's `collect` kept the last row → m-100 charged 190 bps instead of 290 for three days; porting checklist per `Collectors` method | 10.4 §10 |
| Settlement report PR | Part X review capstone (settlement batch job): 14 defects — duplicate fee row silently wins, `unwrap` on partner input, `move` copy of a counter (compiler warned), audit side effect in `map` cut short by `any` (4 of 50,000 audited), discarded suspicious list, `zip` batching dropped payouts (10 of 12 merchants paid), `f64` fee truncation, in-place capacity kept, mid-pipeline collects, per-transaction `String` clones, `fees[&m]` panic, side effect in lazy `map`, `HashMap` order in bank batches, CI accepted warnings; fixed listing with 5 tests | Part X review |
| Gateway drain handshake | per-worker `in_flight` flags + global `draining` flag = SB/Dekker pattern; first draft Release/Acquire rejected; all four accesses `SeqCst` with a documented comment; ~2 ns `xchg` per request | 14.1 §9 |
| Settlement-reconciliation worker (Rust batch) | `static mut STOP: bool` read by value, hoisted in release; pods ignored SIGTERM, SIGKILL after 30 s grace; a killed batch left a partial report file consumed downstream; fix `AtomicBool` `Relaxed`; rule: "a `static mut` in review is a question" | 14.1 §10 |
| Gateway process-wide metrics | `requests_total`, `bytes_out_total`, `panics_total` `AtomicU64` `Relaxed`, scraped every 15 s; no cross-counter consistency (documented); per-tenant billing counter pair uses `Mutex<(u64, u64)>` per shard | 14.2 §9 |
| Fraud library `Symbol` hash cache | ported Java `String.hashCode()`-style racy cache; caught by the Miri CI job added after Chapter 4.6; fix `AtomicU64` `Relaxed` (identical asm) | 14.2 §10 |
| Risk-limits snapshots | `RwLock` reader-count contention → immutable snapshots via `ArcSwap` | 14.3 §9 |
| Access-log shipper on Graviton | per-worker 256-slot batch, `len` `Relaxed` ("x86 is TSO"); correct for 8 months on x86; after a third of the gateway pool moved to Graviton: stale lines in 0.002% of batches, two cross-tenant exposures disclosed; fixes: Release/Acquire both directions, Miri test, aarch64 CI runners, review rule on `Relaxed` comments | 14.3 §10 |
| Gateway per-worker counters | Graviton profile: `requests_total.fetch_add` a top hot spot; first redesign `Vec<AtomicU64>` false-shared; shipped per-worker `CachePadded` blocks summed on scrape | 14.4 §9 |
| Risk-limits reservation slots | naive lock-free index free list → ABA double-booking at 3× load test; one-week investigation, reproduced only under oversubscription; shipped `Mutex<Vec<u32>>`, then per-worker slot ranges next quarter; rule: lock-free needs ABA + reclamation argument, Miri test, benchmark | 14.4 §10 |
| Fraud library concurrency port | translation sheet: `volatile Model` + DCL → `ArcSwap<Model>` (hourly reload); static-init map → `LazyLock`; `LongAdder` → per-worker `CachePadded`; `AtomicLong lastModelLoadMillis` → `AtomicU64` `Relaxed`; `ConcurrentHashMap` feature cache → sharded `Mutex<HashMap>`; `synchronized` → `Mutex` | 14.5 §9 |
| Model-reload flag incident | Java `volatile boolean reloadRequested` ported with `read_volatile`/`write_volatile`; Miri job (mandatory for `unsafe impl Sync` since the hash-cache incident) caught it; fix `AtomicBool` Release/Acquire; `clippy.toml` `disallowed-methods` bans `read_volatile`/`write_volatile` | 14.5 §10 |
| Market-data fan-out SPSC ring PR | Part XIV review capstone: feed-handler → fan-out thread ring replacing a crossbeam channel; x86 CI green, Miri red; 10 defects (both handovers `Relaxed`, `Sync` without `T: Send`, SPSC not enforced by types, no `Drop`, index overflow, `%` division, false sharing, "x86 is TSO" justification, capacity 0) | Part XIV review |
| Design exercises | latency histogram (64 buckets + count + sum, scraped every 15 s); settlement MPMC work queue (16 workers, p99 enqueue < 10 µs); 64-worker config snapshot; Meridian porting guide (concurrency section); multi-architecture test policy | 14.1–14.5 |
| merchant-notify | Pushes payment/refund/payout events to merchant dashboards and POS terminals; ~600,000 concurrent connections at peak, >95% idle; Java on Netty today: 24 pods × ~25,000 connections; Rust pilot (2026) targets 100,000 per pod | 12.1, README |
| merchant-notify 2019 reconnect storm (Java) | Blocking sockets, thread per connection, ~8,000 connections per pod; terminal firmware bug reconnected without closing; >30,000 connections per pod; `OutOfMemoryError: unable to create native thread` with 40% heap; real limit `vm.max_map_count`; fixes: per-IP caps + 90 s idle reaping, Netty (2020), admission control | 12.1 §10 |
| payments-core in-flight coalescing (2026) | Duplicate `POST /charges` with the same key on the same instance now waits for the first attempt (bounded by its deadline) instead of 409; `WaitFor` leaf future; other instances still get 409 | 12.2 §9 |
| merchant-notify `AckWait` incident (March 2026) | Register-once waker; after moving deliveries into spawned tasks, ~3% of deliveries showed exactly 30 s ACK latency (client resend timeout); fixes: refresh waker, two-waker test, review rule for hand-written futures | 12.2 §10 |
| merchant-notify future-size budget | `connection_loop` held `[u8; 65536]` + `[u8; 16384]` across awaits: ~80 KiB per connection, RSS ~8 GB at 100,000; fixes: heap buffers starting at 512 B, CI `size_of_val < 2048`, `Box::pin` in debug tests | 12.3 §9 |
| Marketplace payouts reservation incident | Timeout dropped the payout future between reserve and pay-out; 212 sellers' payouts stuck in "reserved" until next-morning reconciliation; fix: reservation guard with `Drop` | 12.3 §10 |
| merchant-notify merchant fan-out | Per-merchant `Mutex<Vec<Waker>>` replaced by `tokio::sync::Notify` + per-merchant event sequence numbers; review rules: no hand-written projections in app crates, Miri (SB + TB) in CI for primitives | 12.4 §9 |
| payments-core `Deadline` incident (February 2026) | `impl<F> Unpin for Deadline<F>` added to satisfy `Pin::new`; later `swap_remove` in a batching `Vec` moved polled futures; `SIGSEGV` in the allocator about once a day per pod; two weeks of investigation; found with Miri + a self-borrowing inner future; fixed with `pin-project-lite` | 12.4 §10 |
| payments-core simulation executor | Cache fast path (check, await fraud score, record key) double-charged in 183 of 1,000 seeds; v2 reserves first; first simulator lacked wake dedup and took minutes on a 1,000-merchant settlement fan-in | 12.5 §9–§10 |
| Concurrency-model review (2026) | merchant-notify → Rust async (Tokio); ledger stays on a platform-thread pool (concurrency capped by its DB pool); partner webhook dispatcher (Java, thousands of concurrent outbound calls taking seconds) → Java virtual threads with a per-partner `Semaphore`; fraud feature library stays synchronous (no async) | 12.6 §9 |
| merchant-notify reconnect-handshake incident (May 2026) | 4 pilot pods at up to 100,000 connections, Tokio multi-thread with 8 workers; ~150 ms password hash inside the async handshake; ~20,000 terminals reconnected to one pod; ≈53 handshakes/s; dashboards drop after 3 missed 5 s heartbeats; pod drained; fixes: `spawn_blocking` behind a 4-permit semaphore, resumption tokens, handshake admission with retry-after + jitter, long-poll guard in load tests | 12.6 §10 |
| merchant-notify PR #412 | Part XII review capstone: ACK-based at-least-once delivery path with 17 defects; fixed version with injected `Clock`, register-before-send, `Outcome` enum, 5 tests | Part XII review |
| Webhook dispatcher | Java: platform thread per outbound call; a partner's 40-min outage → ~28,000 threads → `OutOfMemoryError: unable to create native thread`, took healthy partners down. Rust: fixed pool per partner tier (named threads), bounded queue per partner spilling to a durable retry table, `Builder::spawn` errors fail startup, `stack_size(512 KiB)` (measured frame size × 4), pool size by Little's law | 11.1 §9 |
| Settlement batch upload | Detached uploader thread; `main` returned; upload never ran; partner reconciliation flagged a missing batch next morning; fix: join what you spawn | 11.1 §10 |
| Merchant-portal report renderer | Design exercise: 300 req/s, ~40 ms ledger I/O + 15 ms PDF CPU, 8 cores | 11.1 §14 |
| Gateway geo-blocking middleware | `RefCell` cache rejected (E0277) in the shared middleware chain; chose an immutable GeoIP prefix table published with `ArcSwap`; the Java version had shipped with an unsynchronized `HashMap` | 11.2 §9 |
| Market-data fan-out FeedStats | Rust port: `Cell<u64>` counters + `unsafe impl Sync` ("only a metric"); gap counter under-reported ~half; caught when Miri-in-CI was adopted; fix `AtomicU64` Relaxed + rule: every `unsafe impl Send/Sync` needs a SAFETY comment naming the synchronization | 11.2 §10 |
| Fraud native scoring engine | Design exercise: thread-affine FFI handle, owner-thread submission | 11.2 §14 |
| Ingestion pipeline backpressure | Bounded blocking queues between decode→validate→enrich→publish (50,000 events/s × 0.2 s = 10,000 slots); `close()` drives shutdown; `producer_waits` and depth metrics | 11.3 §9 |
| Account-service balance cache | Java `ReentrantReadWriteLock` nested read ported to std `RwLock` → deadlock every few hours under refresh load; fixes: pass data down, `ArcSwap` snapshots, `read_recursive` as documented exception; review rule "no lock in a helper of a locked type" | 11.3 §10 |
| Ledger client connection pool | Design exercise: 64 connections, 400 worker threads | 11.3 §14 |
| Payment-provider config | `static LazyLock<ProviderConfig>` from env; misspelled variable → first request panicked, then every request panicked ("previously been poisoned") → 100% errors until restart; health checks passed; fix: eager config in `main`, pass `Arc<Config>` | 11.4 §10 |
| Pricing catalog (concurrent) | Design exercise: ~200 MB, 32 quote threads, 50K quotes/s, updates every few seconds | 11.4 §14 |
| payments-core audit trail | One writer thread owns the durable sink; batches 500 records or 20 ms, one fsync per batch; bounded crossbeam channel of 10,000; `send_timeout(50 ms)` → Transient 503 ("no audit, no charge"); metrics via separate `try_send` drop-and-count channel (`metrics_dropped_total`) | 11.5 §9 |
| Gateway access-log shipper | Unbounded `mpsc` link; collector 20-min outage; ~7,300 lines/s/pod, ~250 B each, ~2 MB/s/pod heap → OOM kills after ~15 min; fix `bounded(200_000)` (~30 s, ~60 MB) + `try_send` drop-and-count (`access_log_dropped_total`), depth exported | 11.5 §10 |
| Merchant notification service | Design exercise: ~2,000 events/s, e-mail provider 500 calls/s, 40,000 webhook endpoints, inbox DB write ~2 ms | 11.5 §14 |
| Settlement reconciliation (parallel) | ~40M records on a peak day; 70 min single-threaded → ~14 min on 8 cores with rayon; partition by merchant (largest ~4% of volume), 8× more partitions than cores, integer sums, sorted mismatches, byte-for-byte comparison on a week of production files | 11.6 §9 |
| Merchant-portal statement pages | `par_iter` over blocking fetches (30–50 docs, 20–200 ms): p50 better, p99 2.5 s → 9+ s; fix async fetches with a concurrency limit, dedicated rayon pool for CPU work, review rule | 11.6 §10 |
| Fraud-model backfill | Design exercise: ~1.2B rows, 90 days, daily files of 10–20 GB, 32 cores, 128 GB RAM | 11.6 §14 |
| Gateway per-route metrics | ~5,000 routes; Mutex<HashMap> → atomics in a startup-built map (hot lines + false sharing) → per-worker single-writer counters (`inc` without `lock`), summed on the 10 s scrape | 11.7 §9 |
| Risk-limits audit under shard lock | Audit write inside a shard's critical section; network volume fsync stalls 20–50 ms; 16 shards → one stall blocked ~6% of traffic; p99 0.4 ms → 40+ ms; fix: I/O after the guard + compensating rollback; lock hold > 1 ms logs the call site | 11.7 §10 |
| Session cache (Rust) | Design exercise: 2M sessions, 100K lookups/s, 5K updates/s, 32 threads, "touch" on 60% of lookups, expiry sweep once a minute | 11.7 §14 |
| Gateway partner-quota tracker | Part XI review capstone: process-wide per-partner quotas per window; PR with 16 defects (over-admission 26–27 of 20, 53 alerts, fail-open default 100, lost billing batch, lock-order inversion, poison → double-panic abort); redesign listing | Part XI review |
| Gateway workspace unsafe policy | `#![forbid(unsafe_code)]` in gateway-core, gateway-io, gateway bin; only `gateway-proto::headers` may contain unsafe (inline `HeaderBlock<16>` for upstream response headers, capped at 16 by proxy policy); clippy `undocumented_unsafe_blocks` + `missing_safety_doc`; `deny(unsafe_op_in_unsafe_fn)`; CODEOWNERS; Miri nightly; ~120 lines of unsafe module vs ~40,000 lines of gateway code | 15.1 §9 |
| `HeaderBlock::restore` incident | safe `restore(checkpoint)` added with no `unsafe` keyword; stale checkpoint from a pooled request context → len beyond initialized slots → allocator crashes, once a leaked header value; fix: shrink-only restore that drops; review rule changed to "PRs touching a module that contains unsafe" | 15.1 §10 |
| Market-data ingest frame parsing | C++-style raw cast proposal rejected (alignment UB + wrong byte order); safe `from_be_bytes` parser adopted for the 8-byte header; `zerocopy` derives approved for larger structures; raw byte-to-struct casts banned in the service's unsafe policy | 15.2 §9 |
| Risk-limits self-transfer incident | Rust port keeps per-merchant exposure buckets in `Vec<i64>`; hand-written `two_mut` (predates `get_disjoint_mut`) checked bounds, not distinctness; self re-route rule → two `&mut` → release reported headroom 30 lower than stored → spurious declines; found by daily reconciliation; fix: `get_disjoint_mut` + explicit `i == j` no-op, Miri in both models, ban on hand-written disjointness code | 15.2 §10 |
| Gateway pooled read buffers | 16 KiB pooled buffers; `memset` from `clear()+resize()` seen in a profile; `set_len` PR rejected (UB + ~115 ns per read ≈ 0.02% of ~500 µs CPU/request); shipped `PooledBuf` kept initialized (zeroed once), exposing only `[..filled]`; test that a short read after a long one leaks no stale bytes | 15.3 §9 |
| payments-core warm-up leak | fixed per-worker set of card-processor connections built in a `MaybeUninit` array; processor maintenance → 3rd connect failed → `expect` panic caught by the task supervisor, retried every few seconds; each attempt leaked 2 open connections (no Drop); processor's per-client connection limit filled; outage outlasted maintenance until a rolling restart; fix: drop guard, then a safe `Vec` + `?` + `try_into()` rewrite that removed the unsafe | 15.3 §10 |
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
| payments-core serializer swap (April 2026) | Microbenchmark (one response, one thread, mean) 2.1× faster; per-thread scratch buffer grew and shrank; p99 ~3 ms → ~11 ms within an hour; fix: fixed-size scratch buffer; review rule: performance PRs show p50/p99/p99.9/max on a recorded mix at production concurrency | 20.1 §10 |
| Rust gateway capacity method | Component microbenchmarks with CI budgets (HMAC ≤ 0.3 µs, head parse ≤ 0.3 µs, 0 allocs/request after warm-up); open-loop load test at 30/50/70%; canary 2% of traffic on 3 pods vs Java pods in the same AZ; the canary is the only capacity number | 20.1 §9 |
| gateway-core CI benchmark policy | Allocation counts + instruction counts (+2%) gate merges; asm diff of five hot functions asks for review; nightly criterion on a pinned runner (investigate if >5% for three nights); pre-release open-loop load test at 60% | 20.2 §9 |
| Market-data decoder "0.3 ns" PR (2026) | Benchmark discarded results (40× claimed); production decode unchanged; new error path formatted a `Box<dyn Error>` per malformed frame from one flaky feed; fix: benchmark template (black_box, 1/1000 malformed inputs, bytes/s printed) | 20.2 §10 |
| Gateway canary profiling procedure | Profiling variant (same opts, line-tables-only, force-frame-pointers); 99 Hz × 60 s on-CPU at peak, diffed against the capacity model; per-thread user/system metric; 30 s off-CPU; then one change at a time | 20.3 §9 |
| Risk-limits merged-function profile (2026) | Flame graph blamed `<ExposureBucket as Drop>::drop` (35%); it was merged drop glue of a new per-request `Vec<Reservation>` clone; two days lost; runbook: `-Z merge-functions=disabled` for implausible leaves, trust callers | 20.3 §10 |
| Access-log and fraud-vector justifications, measured | Access log: ~124 ns/line saved (≈0.05 cores at 400K req/s), p99.9 9.5 → 1.0 µs on 4 threads; fraud: 41 µs → 0.8 µs per score (~1.4% of 3 ms, ≈1.7 of ~120 cores), 408 → 0 allocs/score (~20M/s fleet-wide), p99.9 300 → 7.7 µs | 20.4 §9 |
| payments-core reconciliation job OOM (2026) | Nightly, batches of 500,000 charges, 2 GiB limit; live heap ~400 MB, RSS 1.9 GiB → OOM on 3rd batch after a processor format change; fix: per-batch arena, RSS + live-heap export, mimalloc (measured); rule: jobs with limit < 3× live heap need an RSS-vs-live dashboard and a batch memory test | 20.4 §10 |
| Order-book redesign decision | `ch05-07`-shaped benchmark on the production stream: arena 1.4× throughput, p99 120 vs 246 ns, 0.82 → 0.22 allocs/op, then price levels preallocated for the band; report reads a 1 s columnar snapshot | 20.5 §9 |
| Session cache THP incident (2026) | `transparent_hugepage=always`: median slightly better, p99.9 spikes of several ms (synchronous compaction, khugepaged); fix: madvise mode, `defrag=defer`, `MADV_HUGEPAGE` only on the session table, `AnonHugePages` exported | 20.5 §10 |
| Fleet SIMD policy | Baseline `x86-64` for the general pool, `aarch64` for Graviton; `target-cpu=native` banned in CI; libraries → auto-vectorization with asm check → explicit kernels with runtime dispatch; every kernel has a scalar reference and a differential test on x86-64 and aarch64 CI; Miri runs scalar paths | 20.6 §9 |
| Access-log shipper AVX2 line counter (2026) | Kernel skipped the <32-byte remainder; under-reported ~1 line per few thousand batches; nightly "lines lost" reconciliation alerts for two weeks; its benchmark used a 16 MiB (multiple-of-32) buffer | 20.6 §10 |
| Merchant-portal statement pages, hedged | ~40 document fetches per page; 1% slow → ~33% of pages; hedge at the document service's p95, ≤ 1 hedge per fetch, 5% global hedge budget, cancel the loser | 20.7 §9 |
| Gateway 85% utilization target (2026) | Raised from 60% to 85% for cost; peak p99 3–4× worse; AZ failover pushed survivors > 95% → 8% of requests shed for 15 min; fix: size so the pool after losing a zone stays ≤ 75% at peak, autoscale on queue depth and p99, load shedding | 20.7 §10 |
| Router cache PR #2291 | Part XX review capstone: last-hit cache behind a global `Mutex`; PR benchmark (one key) claims 6.0×; realistic mix: 2.4% hit rate, 1.36× slower on 1 thread, 4.3× slower on 4, ~2 allocs/lookup; the one-line fix `get(path)` is 1.85×/2.1× faster with 0 allocs | Part XX review |
| Fraud library ABI governance | one `cbindgen`-generated, committed header; boundary types only `repr(C)`/`repr(transparent)` with `offset_of!`/`size_of` const assertions; incoming enums as `u32` codes (`DecisionCode`); `meridian_abi_version()` handshake at Java load; `extern "C"` + `catch_unwind`; `panic = "unwind"` | 16.1 §9 |
| `MeridianTxn` reorder incident (spring 2026) | Rust side reordered 24 B → 16 B largest-first; Java kept the old layout; no crash; the canary scored against wrong merchants' histories until the score-distribution alert fired; fixed by the governance rules | 16.1 §10 |
| Vendor scoring engine `libvse` | second opinion for transactions the in-house model flags for review (a small fraction of traffic); thread-affine handles; bound as `sys` (bindgen) + safe `Engine` (`!Send`, `Drop` → `vse_close`, `&mut self` + copy for `vse_last_error`); `vse_version` declared `safe` | 16.2 §9 |
| Model-loader errno incident (2026) | pod failed to start; log said "Is a directory (os error 21)"; real error ENOENT; a logging call clobbered errno; ~30 minutes lost on the volume mount; rule: `last_os_error()` is the first statement of a failure branch | 16.2 §10 |
| Fraud library C API v3 | `meridian_abi_version()` = 3; `meridian_scorer_new(cfg, **out)`/`meridian_scorer_free` (NULL no-op); `meridian_score_batch(s, ids, n, out)` thread-safe (`Sync` asserted), `ids`/`out` must not overlap; `MeridianConfig { block_at, review_at }`; codes 0 / -1 / -99, plus -2 = buffer too small (16.4) | 16.3 §3 |
| Fraud FFM binding (Java) | `FraudLibrary implements AutoCloseable`; `jextract` bindings in the repo, regenerated in CI; library loaded once into `Arena.global()`; ABI check at startup; batches ≤ 256 IDs per downcall, per-call confined arenas; one scorer per process, hourly model reload inside Rust (`ArcSwap<Model>`); -1 → `IllegalArgumentException` + metric, -99 → `IllegalStateException` + alert + circuit breaker to fallback rules | 16.3 §9 |
| Rule plugin host | rule `cdylib`s loaded once at startup, never unloaded (documented at `FfiRule`'s `'static` vtable) | 16.3 §9 |
| Backfill null out-pointer crash | the Rust backfill job linked the `rlib` and called the (then safe) exported `meridian_score` with `null_mut()` for `out` in warm-up; segfault in a crate with no `unsafe`; fix: `unsafe extern "C" fn` + null check → -1; Clippy `not_unsafe_ptr_arg_deref` enabled | 16.3 §10 |
| Vendor engine owner threads | a small pool of owner threads, one engine each (vendor allows several per process, each thread-bound), least-loaded dispatcher, bounded queues, one-shot replies; `EngineDown` → restart + metric | 16.4 §9 |
| `meridian_version_string` allocator incident (2026) | returned `CString::into_raw`, header said "free with free()"; a C++ tool worked for years under the `System` allocator; the fraud library adopted mimalloc in 2026 and the tool crashed intermittently in `free()`; fix: `MeridianBuf` + `meridian_buf_free`, every pointer-returning export has a `*_free`, Java adopts buffers via `reinterpret(..., cleanup)` | 16.4 §10 |
| Explain API (risk console) | Part XVI review capstone: PR adding `init`/`explain`/`last_explanation` (one lint warning, 16 defects); rewrite: `meridian_explainer_new/free`, `meridian_explain` → `MeridianBuf`, `MeridianExplainOptions { u32 top, u32 format, u8 include_negative }`, ABI version 4 | review |
| Geolocation library `libgeo` | design exercise: risk team's IP geolocation via a C library; reload semantics | 16.2 §14 |
| Tokenization library rewrite | design exercise: Rust library called from Java (FFM), Go (cgo), and Rust | 16.3 §14 |
| Rust symbol pipeline | release profile `debug = "line-tables-only"`; `objcopy --only-keep-debug` in CI; debug files uploaded to an internal symbol server keyed by build-id; stripped binaries shipped | 19.1 §9 |
| Market-data ingest build-id mismatch | panic with stripped backtrace; upload had failed; rebuild on a different runner had a different build-id (paths in DWARF); fixes: `--remap-path-prefix` for workspace and `$CARGO_HOME`, upload as release gate, weekly rebuild-compare job, "never symbolize with a rebuild" | 19.1 §10 |
| Linking policy per artifact | edge services musl static + mimalloc FROM scratch (reasons written down); payments-core glibc built on oldest-fleet-glibc builder, distroless; fraud FFM cdylib built on oldest glibc, CI checks `nm -D --defined-only` against the header; CLIs musl; never `prefer-dynamic` or `LD_LIBRARY_PATH` in production | 19.2 §9 |
| Settlement job glibc-floor incident | CI moved to glibc 2.39 images; settlement VMs on glibc 2.31; job failed at 02:00 with `GLIBC_2.34` / `GLIBC_2.39` not found; settlement 4 h late; fixes: pinned builder, CI floor audit, musl for batch jobs, staging OS parity | 19.2 §10 |
| Observability build policy | continuous profiler walks frame pointers fleet-wide; `strip = "debuginfo"`; `-C force-frame-pointers=yes` for services (cost measured on the gateway before rollout); `panic = "unwind"`, `extern "C"` at FFI unless design review approves `C-unwind` | 19.3 §9 |
| Fraud flame graph incident | frame-pointer profiler on a build without frame pointers blamed `memcpy`; a sprint wasted; real hot path the `HashMap<String, f64>` lookup (9.5); fixes: frame pointers, know prebuilt-std limits, profiler canary | 19.3 §10 |
| Statement export startup | ~2M CLI starts per night; static + clean env saves ~15 CPU-minutes (from one noisy run); real fix `--batch` mode long-lived workers | 19.4 §9 |
| Empty statements incident | `export-cli | gzip` without pipefail; a panic (legacy currency code) masked as success; 1,200 customers got empty files; fixes: `set -euo pipefail` lint, exit-code contract 0/1/2/101, runbook exit-status table | 19.4 §10 |
| Session cache "nightly leak" | 03:00 purge of ~1.2M sessions; RSS stayed near peak; 85% alert; fragmentation, not a leak; fixes: graph allocator in-use next to RSS, limit from peak + retention headroom, moved to mimalloc after a canary | 19.5 §9 |
| logstat sidecar SIGBUS | mmap fork of logstat; logrotate `copytruncate` at midnight → SIGBUS (135) crash loop; handler rejected; buffered reader restored; mmap only for exclusively owned files; logrotate `create` | 19.5 §10 |
| Gateway descriptor budget | soft limit raised to hard at startup, hard limit in pod spec; connection limit below fd limit; EMFILE → pause accepting, rate-limited log; fd count metric, alert at 80% | 19.6 §9 |
| Port 9100 inherited-socket incident | vendored C metrics library opened its listener without `SOCK_CLOEXEC`; `system()` notify hook inherited it; hung notify held port 9100; restarted gateway got EADDRINUSE; readiness stalled rollout; fixes: patch to SOCK_CLOEXEC, startup CLOEXEC assertion, `Command` instead of `system()`, metrics out of readiness | 19.6 §10 |
| payments-core release PR (review capstone) | PCI-segment VMs on glibc 2.35; watchdog uses pidfd_open; PR flags target-cpu=native, relocation-model=static, -z lazy, -z execstack, absolute CI rpath, debug = 2; audit: PR fails 7 of 9, fixed passes all | Part XIX review |
| merchant-notify runtime configuration (after May 2026) | `worker_threads(8)` set explicitly (8-CPU pods); named threads + startup config log; `max_blocking_threads(16)`; `RLIMIT_NOFILE` 1,048,576; `num_alive_tasks` gauge compared with connection count; `tokio_unstable` in a canary; heartbeat-lateness probe in load tests | 13.1 §9 |
| merchant-notify heartbeat burst (June 2026) | synchronous DNS resolver in an async fn → 2.3 s stall on one pod; per-connection 100 ms flush `interval` with default Burst → 23 catch-up flushes × ~90,000 connections; ~400 ms worker saturation; ~6,000 terminals reconnected; recovered in ~1 min; fixes: Skip, review rule "every interval states its missed-tick policy", stall-injection load test | 13.1 §10 |
| Ledger TLS sidecar | design exercise: 2 cores, 2,000 client TLS connections, 16 connections to the JVM, 1–5 ms per batch, ~1 ms CPU per handshake | 13.1 §14 |
| payments-core fraud scoring | fraud library called in-process, ~3 ms CPU per score; Black Friday forecast 2,000 charges/s ≈ 6 cores; inline scoring at 1,500/s raised unrelated p99 from 3 to 90 ms; shipped: rayon pool of 6 threads + `Semaphore(64)` (20 ms wait → `PAY_OVERLOADED`, 503 + Retry-After) + `oneshot` bounded by the request deadline; `spawn_blocking` rejected (grew to 180 threads) | 13.2 §9 |
| settlement-api merchant-ID exposure (2026) | Rust port kept Java's MDC idiom via `thread_local!`; tests ran on the current-thread runtime; production 8 workers; support's log export for merchant m-2208 contained payment IDs of three other merchants (reported as data exposure); fixes: `tracing` spans, CI lint on `thread_local!`, multi-thread tests | 13.2 §10 |
| Statement-export service | design exercise: ~50 exports/min, ledger query + ~200 ms PDF CPU + ~50 ms compression + ~500 ms upload; 12-month exports = 60× work; 4-core pods | 13.2 §14 |
| Risk-limits service (async, 2026) | 16 shard actors, mailbox 1,024 each, callers `try_send` (fail closed); policies via `watch<Arc<Policies>>`; supervisor `JoinSet` restarts a panicked shard from snapshot; load test 100K ops/s ≈ 1.6 cores, p99 0.4 ms | 13.3 §9 |
| payments-core FX-rate convoy (2026) | `tokio::sync::Mutex<HashMap<CurrencyPair, Rate>>` held across the FX fetch; rates expire after 60 s; FX provider 10 ms → ~800 ms; cross-currency p99 12 ms → >4 s; gateway 5 s timeout returned 504s; 40 min looking at "blocked threads"; fixes: std Mutex around map ops, single-flight per pair (watch), serve stale up to 5 min, review rule | 13.3 §10 |
| Ledger event bus (sidecar) | design exercise: ~3K postings/s to settlement batcher (must see all; pauses 30 s on deploy), fraud feature store (may skip, must know), balance cache (latest per account) | 13.3 §14 |
| Gateway deploy shutdown | K8s SIGTERM + 30 s grace; unready → keep accepting 5 s → cancel root token (`Connection: close` / GOAWAY) → drain ≤ 20 s → abort stragglers → explicit flushes (2 s timeouts); "requests aborted by shutdown" tracked per deploy | 13.4 §9 |
| Market-data ingest `select!` corruption (2026) | `read_frame` (two `read_exact`) raced with a 100 ms stale-quote tick; split snapshot frames → `HeaderError::BadMagic` → reconnect → bigger snapshots; feed flapped 25 min at market open; fixes: `FramedRead` codec for the 0xCAFE header, stale check in its own task, split-at-every-offset CI test, review checklist line | 13.4 §10 |
| merchant-notify admission control | per-queue policy table: `listen(4096)` + somaxconn; `Semaphore(110,000)` connections with jittered retry-after 5–15 s; handshake `Semaphore(4)` + 200-slot wait queue; per-connection `mpsc(256)` with resync marker; 256 KiB unflushed cap + `SO_SNDBUF` 64 KiB + 10 s write timeout; `broadcast(4,096)` per merchant with `Lagged` resync | 13.5 §9 |
| payments-core processor slowdown (2026) | processor 150 ms → 1.8 s for ~4 min; unbounded channel before processor calls, ~40,000 queued at peak; 11 more minutes to drain → 15-min outage; no double charges thanks to idempotency keys; fixes: deadline header (refuse with < 400 ms left: `PAY_DEADLINE_EXCEEDED`), bounded queue of 200 + `try_send` → 503 Retry-After, skip-if-expired at dequeue, 10% retry budget; July 2026 repeat: 30% shed, admitted p99 < 2.2 s, immediate recovery | 13.5 §10 |
| payout-relay (marketplace payouts team, new Rust service) | pushes payout status events to merchant webhooks; review capstone PR: `tokio::sync::Mutex` held across delivery, unbounded channel and spawn, no timeouts, limits, or accounting → 8 of 300 delivered by t = 5 s, m1 4,040 ms, 292 lost at the deploy, 0 of 8 audit lines durable; redesign: per-merchant queue 32 + 4 in flight + 500 ms × 3 attempts with backoff + spill/dead-letter/requeue, event id as idempotency key → 300 of 300 accounted for | Part XIII review |

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
- 2026-09-26: **`tools/verify.ps1` false-PASS bug fixed** (found by the Part XIX writer): when a Playground request
  failed or timed out, `Invoke-WebRequest` threw and the check was scored on the *previous* check's response. Now each
  check resets its state, retries once, and reports `FAIL … request failed` if the request still fails. Audit: none of
  the integrator's independent re-verification logs for Parts V–XX contained a request error, and Parts I–IV, XVI and
  XIX were re-run with the fixed script. `verify.ps1` also sets UTF-8 console output so redirected logs keep non-ASCII.
- The Playground container has `rustc`, `gcc` 13.3 and binutils (readelf, objdump, nm, strip) with a writable `/tmp`;
  listings can build and inspect real binaries or link C and Rust (assert every inner build's exit status).
  `RUSTC_BOOTSTRAP=1 rustc -Z …` works inside it. No clang/llc, strace, perf, gdb or valgrind.
