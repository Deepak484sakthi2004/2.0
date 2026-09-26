# Part 20 report

Written by a resuming writer: the original Part XX writer stalled on a network outage after writing 17 listings
(through `ch04-02`) and no prose. The resuming writer verified those 17 (18 checks, all pass), added 28 listings, and
wrote every chapter, the README (with the promise table), the review, the answer key, and this report.

## SUMMARY.md lines

Replace the Part XX draft block with:

```markdown
- [Part XX Overview](part-20-performance/README.md)
  - [20.1 Performance as a Measurement Discipline](part-20-performance/ch01-measurement-discipline.md)
  - [20.2 Benchmarking Without Lying to Yourself](part-20-performance/ch02-benchmarking.md)
  - [20.3 CPU Profiling and Flame Graphs](part-20-performance/ch03-cpu-profiling.md)
  - [20.4 Allocation and Memory Profiling](part-20-performance/ch04-allocation-memory.md)
  - [20.5 Caches, Branch Prediction, and False Sharing](part-20-performance/ch05-caches-branches-false-sharing.md)
  - [20.6 SIMD and Vectorization](part-20-performance/ch06-simd-vectorization.md)
  - [20.7 Contention, NUMA, and Tail Latency](part-20-performance/ch07-contention-numa-tail.md)
  - [Part XX Review: The Router Cache PR & Interview Mode](part-20-performance/review.md)
```

Appendix entry, in Part order among the answer keys:

```markdown
  - [Part XX Answers](appendix/answers-part-20.md)
```

## PROGRESS concepts

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

## Promises to later Parts

- **Part XXI:** the whole-gateway load test and canary numbers (20.1 §9 defers them); syscalls and TLS cost per request
  measured with this Part's tools; wrk2 load curves; hedging with budgets and cancellation as a network mechanism
  (21.4); pool sizing with Little's law (21.5).
- **Part XXII:** criterion/divan and cachegrind-based instruction-count CI (22.7); the benchmark template from 20.2 §10;
  dhat-rs and continuous profiling; tracing spans for per-request latency (22.5).
- **Part XXIII:** arena-per-batch memory patterns (20.4's reconciliation job) for memtables and compaction; RSS vs live
  heap in a storage engine; page-cache effects measured with `getrusage`/faults.
- **Part XXIV:** hedged reads to replicas; tail at scale for scatter-gather queries; thread-per-core as a design option
  for Ferrite shards; Little's law for replication pipelines.
- **Part XIII (Ferrite v2):** `ch07-05`'s measurement for the value-type decision (`Arc<[u8]>`/`Bytes` vs copying under
  the shard lock).
- **Part XIX:** the mechanisms behind 20.4/20.5's effects (page faults ~2 µs each, brk/mmap/trim, THP, RSS).
- **Part XXVI:** the lexer class table and spill-aware register allocation in the toy compiler.

## Promises kept

All Part XX promises in PROGRESS.md's open-promises list, itemized in the README's "Where each earlier promise is paid"
table (20.x section per promise, and whether measured, explained, or given as unverified commands), plus the four
promises Part XVII added while this Part was being written (closed-form loops, spills, lexer tables, LLVM pass cost) and
the `StampedLock` gap noted in Part XI. Measured payoffs: tagged vs niche (5.2), AoS/SoA/hot-cold (5.2, 9.5), access-log
p99 (9.2), fraud feature vector (9.5; the "3% CPU" estimate was 2× high), order book (9.4), allocator contention (9.x),
THP (9.x), chunked f64 sum (7.x), panic vs Result at failure rates (8.x), noalias in loops (15.2), zeroing cost (15.3),
Ferrite shards (L4), lock wait/hold (11.7), pinning (11.x), lexer tables (17.2). Not measurable on the Playground and
labeled with commands: perf stat/record/c2c/sched, off-CPU, PGO/BOLT, opt-level and profile experiments, compiler
self-profiling, Arm/Graviton, NUMA, wrk2.

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
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

## Verification

- `listings/part-20/`: **45 files, 48 checks, all PASS** on rustc 1.98.1 (edition 2024), no compiler warnings: 40
  `release ok`, 3 `release build` (asm sources), 1 `debug ok` (ch03-04's debug half), 1 `release error:E0133`, 1
  `release+nightly ok` (std::simd), 1 `debug miri-ok` (runtime dispatch), 1 `debug test` (review-02). Final full-folder
  run saved to scratch `part-20/verify-final.txt`.
- Artifacts via `tools/emit.ps1` (release asm): closed forms (`ch02-02`), the two sum loops (`ch02-10` and the harness
  closures from `ch02-09` via `-CrateType bin`), `branchy`/`count_big` (`ch05-02`), all six `ch06-01` functions, the
  SSE2 plain-loop and AVX2 kernels (`ch06-03`), and the nightly `std::simd` loop (`ch06-05`, `-Channel nightly`).
- A script confirmed the one `rust,compile_fail` block is verbatim from `ch06-04`, and every `rust,ignore` block is
  labeled as an excerpt of a named listing or as "not a book listing".
- All timings are one run on the shared Playground machine unless stated; `ch07-03` was run twice and the text reports
  both (median and throughput moved; p99 stable). The layout-bias finding was reproduced twice per ordering.
- Unverifiable on the Playground, labeled with commands: perf stat/record/c2c/sched/lock, eBPF off-CPU, PGO/BOLT
  (cargo-pgo), opt-level/profile A/B, `--timings`, `-Z self-profile`, `-Z time-passes`, LLVM `-time-passes`/
  `-print-after-all`, Cranelift, `-Z threads`, heaptrack/dhat/bpftrace, jemalloc/mimalloc swaps, NUMA (single node),
  Arm/Graviton, wrk2 load curves, cachegrind CI. Predicted-from-mechanism claims (i-cache pressure from duplicated
  code, spin/backoff tuning, TLB share of DRAM latency, THP benefit when granted) are labeled and paired with exercises.

## Word count

README 1,416 · 20.1 4,414 · 20.2 4,431 · 20.3 3,798 · 20.4 3,759 · 20.5 4,474 · 20.6 4,176 · 20.7 4,092 · review 1,373 ·
answers 7,882 → **≈ 39,800** (`wc -w`, before final small edits).

## Tooling notes

- **Layout bias is real on the Playground.** `ch02-09`'s two sum closures compile to the same 7-instruction loop, yet
  measured 1.6–1.8× apart; swapping their definition order (`ch02-11`) made them equal. Never attribute a timing
  difference to a code change without comparing the assembly.
- `perf_event_open` fails with EPERM (`perf_event_paranoid = 4`); `sched_setaffinity` works; `/proc/self/*`,
  `getrusage`, `/proc/stat` (steal), `/sys/devices/system/cpu/*/cache` and `/topology` are readable. 4 vCPUs, one NUMA
  node, each vCPU its own SMT sibling. CPU flags include avx2, avx512f/bw (Zen 4-class EPYC).
- Memory: ~300 MB of allocations in one listing worked (`ch05-01`: 256 MiB + a 32 MB index). THP is `madvise` with
  `defrag=madvise`, and `MADV_HUGEPAGE` mostly doesn't get huge pages (fragmented host).
- `Instant::now()` ≈ 25 ns, clocksource `tsc`, smallest step 20 ns: time batches, not single operations.
- `tools/emit.ps1 -Channel nightly` works for nightly listings (`release+nightly ok` verify mode works too).
- `is_x86_feature_detected!` under Miri reports only `sse2`: a `debug miri-ok` check exercises the scalar path and the
  dispatch logic of SIMD code; shrink inputs under `cfg!(miri)`.
- `#[target_feature(enable = "avx2")]` doesn't enable POPCNT: `count_ones()` compiles to a bit-trick sequence; use
  `"avx2,popcnt"`.
- `thread::scope` inside each timed sample adds thread-spawn cost to wall time; compare rows with the same thread count
  only (review-02 says so).
- Contention benchmarks on 4 shared vCPUs vary run to run in the median and throughput; the p99 was the stable number.
  Run twice and report both when a conclusion depends on it.
- glibc's dynamic mmap threshold makes repeated large `vec![0; n]` allocations come from the heap after the first free,
  so "fresh buffer" costs no page faults in a loop; don't assume large allocations always mean fresh pages.
