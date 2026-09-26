# Part XX — Performance Engineering

> **Part question:** *How do you find out where a Rust system spends its time and memory, prove that a change made it
> faster, and design for the tail rather than the average?*

Every Part of this book has made performance claims, and each came with a measurement, an attribution, or the label
"predicted from mechanism" plus an exercise to measure it. Many of those exercises said "Part XX". This Part is where
they're paid: it teaches performance as a **measurement discipline** (the brief's words), and it measures most of what
earlier Parts predicted, on the same Playground machine, with the listings in `listings/part-20/`.

Two constraints shape it. The Playground can't run `perf` (the kernel refuses performance counters to unprivileged
containers, `ch03-01` shows it), so each profiling tool appears twice: as the exact command to run on your own Linux
machine, labeled "not verified here", and as an in-process experiment that demonstrates the tool's core idea and *is*
verified. And every timing is one run on a shared 4-vCPU cloud machine: the book reports rankings and ratios, shows the
spread where it matters, and says so when a second run disagreed (Chapter 20.7 has an example).

## Chapter map

```text
20.1 Performance as a Measurement      the eight metrics; one number hides the distribution (4M pushes: p50 20 ns, max
     Discipline                        778 µs); Amdahl measured; a gateway capacity model and why it isn't a fleet plan
      │
20.2 Benchmarking Without Lying        eight lies, each demonstrated: dead code, closed forms (Gauss in the asm), cold
     to Yourself                       starts (83× first pass), the clock, order bias (identical fns "11× faster"),
                                       LAYOUT BIAS (same instructions, 1.7× apart), coordinated omission, happy paths
      │
20.3 CPU Profiling and Flame Graphs    a sampling profiler rebuilt in-process (on-CPU vs wall-clock, aliasing);
                                       user vs system time; inlined frames; perf, eBPF, PGO/BOLT, compiler profiling
      │
20.4 Allocation and Memory Profiling   an allocation-site profiler in 40 lines; malloc costs (local ~11 ns, cross-thread
                                       ~118 ns); live heap vs RSS; zeroing cost; Part IX's two estimates measured
      │
20.5 Caches, Branch Prediction, and    the latency ladder (1.6 ns → 171 ns); ~6 ns per misprediction; niche vs tag;
     False Sharing                     hot/cold vs SoA by access pattern; readers slowed by a neighbour; huge pages;
                                       the order-book redesign benchmarked
      │
20.6 SIMD and Vectorization            what LLVM vectorizes and why not (aliasing, FP order, early exits); runtime
                                       dispatch (AVX2 7× from a baseline binary); std::simd; the target-cpu levels
      │
20.7 Contention, NUMA, and Tail        the utilization cliff (p99 43× service time at 90%); fan-out and hedging;
     Latency                           contention as wait time; Ferrite's shards; core-to-core distance; NUMA
      │
Part XX Review                         a performance PR whose benchmark claims 6×, and the corrected benchmark
```

## What you'll be able to do after Part XX

- State a performance claim with its metric, statistic, workload, and comparison, and reject claims that lack one.
- Build a benchmark that survives dead-code elimination, cold starts, order effects, layout bias, and noise, and a load
  test that doesn't hide stalls (coordinated omission).
- Find where CPU time goes (on-CPU, off-CPU, wall-clock) and where allocations come from, with or without `perf`.
- Explain a performance number from the hardware up: cache levels, TLB, branch prediction, coherence, SIMD width.
- Choose data layouts, lock strategies, and value representations from measurements.
- Size a system for its tail: utilization headroom, fan-out budgets, hedging, and failure capacity.

## Where each earlier promise is paid

| Promise (made in) | Paid in | How |
|---|---|---|
| PGO / BOLT (2.2, 7.2) | 20.3 §7 | Workflow and commands; not verified here (local toolchain) |
| Runtime CPU-feature detection, multiversioning (2.1) | 20.6 §3, §6 | Measured: AVX2 via dispatch from a baseline binary (`ch06-03`); target-cpu levels |
| `opt-level` 2 vs 3; release-profile experiments (Part II, 18.6) | 20.2 §7 | Method (A/B load test of two binaries) and commands; not verified here (the Playground's profiles are fixed) |
| Atomic vs mutex vs no-sharing ranking (1.3) | Measured in 11.7; refined in 20.7 | Wait-time histograms (`ch07-03`), core-to-core cost (`ch07-04`) |
| The gateway benchmark (Part I) | 20.1 §3, §9 | Component model measured (`ch01-03`: ~0.97 µs, 0 allocations); whole-service load test belongs to Part XXI |
| False sharing (5.2, 9.5) | 20.5 | Measured for readers (`ch05-06`); writers measured in 14.4 and 11.7; `perf c2c` command |
| Tagged vs niche in memory (5.2 Systems exercise) | 20.5 §3 | Measured (`ch05-03`): 4.4× in cache, 2.2× from DRAM |
| AoS/SoA and hot/cold splitting (5.2, 9.5) | 20.5 §3 | Measured for both access patterns (`ch05-04`) |
| `perf stat -e branch-misses` for dispatch (6.5) | 20.3 §6, 20.5 §3 | Command (not verified here) and an in-process proxy: ~6 ns per misprediction (`ch05-02`) |
| Pointer chasing (7.x) | 20.5 §3 | The latency ladder (`ch05-01`) |
| i-cache effects of duplicated hot code (7.x) | 20.5 §6 | Predicted from mechanism; Systems exercise measures it |
| Chunked `f64` sum (7.x) | 20.6 §3 | Measured: 8× faster, 9 × 10⁻¹⁶ relative difference (`ch06-02`) |
| Code-size budget in CI (7.x) | 20.2 §9 | Policy: instruction counts, assembly diffs (`ch02-10`'s technique) |
| Panic cost vs depth; Result-vs-exception failure rates (8.1, 8.3) | 20.2 §6 | Measured at depths 1/10/100 and 0/1/10% failures (`ch02-07`) |
| Gateway access-log p99 (9.2) | 20.4 §3, §9 | Measured (`ch04-02`): p99.9 9.5 µs → 1.0 µs on four threads |
| Fraud feature-vector p99 (9.5) | 20.4 §3, §9 | Measured (`ch04-03`): 41 µs → 0.8 µs per score; the "3% CPU" estimate was 2× high |
| THP / huge pages (9.x) | 20.5 §5 | Measured: requested, mostly not granted on this host (`ch05-05`) |
| Order-book redesign benchmark (9.4) | 20.5 §5, §9 | Measured (`ch05-07`): 1.4× throughput, 2× p99, 0.82 → 0.22 allocations/op |
| Allocator contention (9.x) | 20.4 §4 | Measured (`ch04-04`): local malloc scales; cross-thread frees ~118 ns each |
| Deferred drop (3.1) | 20.7 §8 | Referenced as a tail source (measured in 3.1) |
| `next()` vs `fold()`, `dyn Iterator` under `perf`; vectorization details (10.3) | 20.3 §6, 20.6 | `perf stat` commands (not verified here); vectorization in real asm (`ch06-01`) |
| `perf c2c`, ordering costs on Arm, `pause`/backoff, Graviton counters (14.x) | 20.5 §4, 20.7 §6 | `perf c2c` command; Arm not measurable (x86-only Playground); backoff predicted + exercise |
| Tail latency from long polls; `perf stat` context switches on the ping-pong; per-task memory (12.6) | 20.7 §8, 20.3 §6, 20.4 §5 | Referenced; command + `getrusage`/`sched_getcpu` proxies; allocator chunk overhead measured |
| Lock wait/hold time, `perf sched`, off-CPU flame graphs (11.7) | 20.7 §3, 20.3 §6 | Measured (`ch07-03`); commands; wall-clock proxy (`ch03-02`) |
| Thread pinning and placement; NUMA and thread-per-core (11.1, 11.7) | 20.7 §5–§6 | Pinning measured (`ch07-04`); NUMA explained (single-node Playground) |
| `RwLock` vs `Mutex` shards for Ferrite (L4) | 20.7 §3, §9 | Measured (`ch07-05`): value type matters more than lock type |
| `wrk` load curves for L3 (L3) | 20.2 §7 | wrk2 method and commands; not verified here |
| `noalias` in loops (15.2) | 20.6 §3 | Measured and in asm (`ch06-01`, `ch06-06`) |
| Zeroing cost vs buffer size (15.3) | 20.4 §4 | Measured (`ch04-06`) |
| `-Z self-profile`, `--timings`, `-Z merge-functions`, Cranelift vs LLVM, `-Z threads`, artifact-regression check (18.x) | 20.3 §4, §7, §10; 20.2 §9 | Commands (not verified here); merged-function misattribution as the failure scenario |
| Java `StampedLock` vs seqlocks (11.3, noted as a gap) | 20.7 §8 | Comparison |
| Closed-form loops as a benchmark lie (`triangle`, `guarded_sum`, 17.7) | 20.2 §3–§4 | Measured with this Part's own closed forms (`ch02-01`, `ch02-02` asm) and cross-referenced |
| Spill costs and register pressure (17.8 §6) | 20.6 §4 | Explained with 6.4's real spill and 20.6's accumulators; how to spot spills in asm |
| Lexer throughput: byte-class table vs comparison chain (17.2 Systems exercise) | 20.5 §3 | Measured (`ch05-08`): table 6–7× faster on unpredictable input |
| LLVM pass-pipeline cost (`-time-passes`, `-print-after-all`, 17.7) | 20.3 §7 | Commands; not verified here (local toolchain) |

## Listings

`listings/part-20/`: 45 files and 48 checks, all verified on rustc 1.98.1 (edition 2024) with `tools/verify.ps1`: timing listings in
release mode, one intended compile error (E0133), one test module, one run under **Miri**, and one on nightly
(`std::simd`). Compiler artifacts (assembly) come from `tools/emit.ps1`. Timings are one run on a shared machine unless
the text says otherwise.
