# Appendix A — Answer Key: Part XX

> Model answers. Write yours first. Where several answers are defensible, the key says so. Measurements quoted here are
> the chapters' Playground runs (one run each on a shared 4-vCPU machine, noisy); "predicted" marks reasoning you should
> check by measuring.

---

## Chapter 20.1 — Performance as a Measurement Discipline

### Interview & architecture questions

**1. "3× faster."** Four things: the **metric** (latency, throughput, CPU per request, p99, memory?), the **statistic**
(mean, median, p99, max?), the **workload** (which inputs, what mix, what concurrency, warm or cold?), and the
**comparison** (against what baseline, how many samples, what spread, measured how?). Without them the sentence can be
true and irrelevant (a 3× faster mean with a worse p99, on one input nobody sends).

**2. Three averages.** 60.3 ns is wall time ÷ count: it includes the cost of timing and recording every push. 29.0 ns
is the mean of the recorded samples: it includes the timer's own cost (~20 ns) and is pulled up by the rare slow
pushes. 20 ns is the median, which equals the empty-region measurement: the clock's resolution, not the push. For a
design doc, none of them alone: report the batched per-push cost (time 1,000 pushes, divide) for throughput, and the
tail (p99.9 1.6 µs from page faults, max 778 µs from reallocation) for latency.

**3. Fleet p99.** A percentile of a union isn't the mean of the parts' percentiles: a host with a few very slow
requests and a host with none average to a p99 that no request distribution has. Correct: merge the hosts'
histograms (HdrHistogram supports adding them; so do Prometheus native histograms and t-digests) and read the p99 of
the merged distribution, or compute it from the raw samples.

**4. Amdahl.** Speedup on N workers ≤ 1 / (f + (1 − f)/N), with f the serial fraction; the ceiling is 1/f. Real
speedups fall short because the parallel part doesn't scale perfectly: thread wake-up and work-splitting overhead,
shared resources (memory bandwidth for the memory-bound job: 2.8× on 4 threads for the parallel part), and virtual CPUs
that share physical cores with other tenants.

**5. Little's law.** L = λW: the average number in the system equals arrival rate times average time in the system,
for any system in steady state. So concurrency (connections, in-flight requests, pool sizes), throughput, and latency
are linked: if latency doubles at the same throughput, twice as many requests are in flight, and every pool sized for
the old number becomes a queue.

**6. 60%, not 90%.** Queueing: waiting time grows like ρ/(1 − ρ), and the tail grows faster (Chapter 20.7: p99 22× the
service time at 80%, 43× at 90%). Headroom also absorbs bursts, noisy neighbours, and failover (a pool that loses a zone
must absorb its load). 60% at peak is a common compromise; the right number comes from the p99 target and the failure
you size for.

**7. Allocation rate.** Java: allocation is a TLAB pointer bump, cheap at the site; its cost is GC work (young-gen
collections scale with allocation rate; promotion and old-gen work with survival), and pause risk. Reducing it buys
fewer and shorter collections. Rust: each allocation is a `malloc`/`free` pair (~11 ns local, `ch04-04`), paid
immediately by the request that allocates; reducing it buys CPU on that request, less allocator contention, and fewer
tail events (cross-thread frees, trims). No pause either way.

**8. RSS grows, two causes.** A real leak (live heap grows too) or fragmentation/retention (live heap flat, RSS grows:
freed memory not returned, `ch04-05`). The metric that separates them is live heap from a counting allocator (or the
allocator's own statistics) plotted next to RSS.

**9. One dashboard number.** Argue for the p99 (or p99.9) latency at the gateway edge per minute, plus CPU per request,
both from production. Against a mean latency (hides the tail) and against peak throughput (a capacity number, not a
health number). If forced to one: the SLO burn rate, which combines latency and errors against the objective.

**10. Premature optimization vs pessimization.** Optimization: complexity bought for speed nobody measured a need for
(an `unsafe` buffer trick in cold code; the review's router cache). Pessimization: needlessly slow code where the
efficient version is equally clear: cloning a `HashMap` per request (3.2), `format!` per log field (9.2),
`get(&path.to_string())` instead of `get(path)` (the review).

### Debugging exercise (the rate limiter: 40 ns everywhere)

Reasons the benchmark and production disagree: (a) **the timer**: p50 = p99 = max = 40 ns means every sample reads
the same clock quantum: the operation is below the clock's resolution, and the numbers are the ruler; batch the timing.
(b) **One thread, one key, warm**: production has 16 threads contending on the limiter's map or lock (wait time,
Chapter 20.7) and many keys (cache misses, hash-map growth). (c) **No contention or preemption**: a 3 µs p99 is typical
of lock waits or a descheduled holder. (d) **Different code path**: production calls may include the refill arithmetic
for idle keys, map growth on new keys, or a metrics update. Experiments: time batches (a); run with 16 threads and a
realistic key distribution and record wait times (b, c, `ch07-03`); trace a slow request's spans (d). The suspicious
line is `max: 40 ns`: in a million real operations on a shared machine, *something* is always slower; a max equal to
the median means the instrument can't see.

### Selected exercises

- **Beginner.** With `Vec::with_capacity(n)`, the 20 reallocations disappear, so the max drops from ~778 µs to a page
  fault or a preemption (a few µs to tens of µs). The p99.9 stays in the page-fault band (predicted: capacity reserves
  virtual memory; pages are still faulted in on first touch, one per 512 pushes), unless you also touch the memory
  first.
- **Intermediate.** Batched timing divides the clock's ~20 ns cost across 1,000 pushes, so the per-push number drops
  to the real cost (a few ns: a compare, a store, an increment, Chapter 9.1's fast path). Batching is right whenever the
  operation is cheaper than the clock; the price is that you lose the per-operation distribution, so record batch
  latencies in the histogram instead.
- **Systems.** Adding AES-GCM on 638 bytes (~0.3–0.5 µs, predicted from typical AES-NI throughput) and a `write` system
  call (`ch03-03` measured 200,000 writes to `/dev/null` in 130 ms, ~0.65 µs each; a socket write does more work and
  costs more) roughly doubles or triples the model's per-request cost, and the kernel's share now rivals the
  application's. The lesson: the application work was the cheap part.

### Design exercise (gateway-core performance contract): model answer

One page, five sections. **Metrics**: per component (head parse, auth, route, rate-limit, log): ns per operation at
p50/p99 on the recorded mix, allocations per request (target 0 after warm-up); end to end: CPU per request and p99/p99.9
at 60% load. **Workload**: a recorded, anonymized production request mix refreshed monthly; concurrency = production
worker count. **Measurement**: CI runs allocation-count tests and instruction-count benchmarks (gate); a nightly
criterion run on a pinned runner (inform); a pre-release open-loop load test (gate). **Budgets**: component budgets
from the capacity model; a CI failure when allocations > 0 or instruction counts +2%; release blocked if CPU per request
+5% or p99 +10% at 60% load. **Exceptions**: approved by the gateway tech lead with the evidence attached; recorded in
the contract's changelog.

---

## Chapter 20.2 — Benchmarking Without Lying to Yourself

### Interview & architecture questions

**1. Five lies and countermeasures.** Dead code (black_box the result); constant folding and closed forms (black_box
inputs, use data not ranges); cold start (warm up, or measure cold deliberately); order effects (interleave, rotate);
layout bias (compare assembly; several builds; don't trust small deltas); plus closed-loop load (open-loop generators)
and happy-path-only inputs (include failures at production rates).

**2. `black_box`.** On 1.98 it's an empty inline-assembly statement that takes the value, so the optimizer must assume
it was used and possibly modified. The std docs say it's a best-effort hint: implementations may treat it as the
identity, so it must never be relied on for correctness, and it can't be relied on to disable every optimization.

**3. A billion in 1.2 ns.** Scalar evolution recognized an arithmetic series and replaced the loop with n(n−1)/2
(`mul`, `shld`: a 128-bit product halved). Legal because the result is identical for every `n` (wrapping semantics
included) and a loop has no observable behaviour besides its result.

**4. Identical functions 11× apart.** Each was timed once on a freshly allocated buffer; whichever ran first took all
32,768 page faults (and cold caches). The fix: warm up, then interleave many samples and compare medians with a spread
(`ch02-05`'s last line: 1.41 vs 1.41 ms).

**5. Layout bias.** Performance changes caused by where code lands in memory (fetch and micro-op-cache boundaries,
predictor aliasing), not by what it does. To tell: emit the assembly of the hot function before and after; if the
instructions are the same, the difference is placement. Also rebuild with an unrelated change or a different link
order and see if the difference moves; tools like STABILIZER randomize layout to average it out. Treat single-digit
deltas from refactorings as unproven.

**6. Coordinated omission.** A closed-loop tool waits for each reply before sending the next request, so when the
server stalls it stops sending, and it records one slow request instead of the hundreds real users would have sent
during the stall. An open-loop generator sends on a fixed schedule regardless of replies and measures latency from the
*intended* send time; in `ch02-06` that turns a reported p99 of 1.0 ms into the true 1.5 s.

**7. Panic vs `Err`.** When failures are very rare and the stack is deep, unwinding's zero-cost happy path can beat
`Result`'s per-level checks (117.6 vs 206.7 ns at depth 100 with no failures). It's still wrong for business errors:
break-even is ~0.6% failures at depth 100, ~0.2% at depth 10, and each failure costs 2–15 µs; and panics are for bugs
(Chapter 8.3): they skip the error-type contract, may abort under `panic=abort`, and can't be matched on.

**8. Build profiles need load tests.** Opt-level, LTO, and codegen units change inlining and code layout across the
whole binary, which changes i-cache and branch-predictor behaviour under the real mix of work; a microbenchmark of one
function sees neither, and its differences are within layout noise.

**9. Instruction counts vs timing.** Instruction counts are deterministic (no noise, no layout jitter), so they catch
small regressions in CI reliably; they're blind to cache misses, mispredictions, and contention, so a change that adds
instructions but removes misses looks worse than it is. Timing sees everything and is noisy.

**10. Minimum evidence.** The claim in four parts; a benchmark with black-boxed inputs/outputs, warm-up, interleaving,
≥30 samples, medians and spread; a histogram at production concurrency on a recorded mix; allocation counts; the
generated assembly of the hot function (Part XVIII's artifact rule); and a correctness test.

### Debugging exercise (the JSON extractors)

Defects: (1) results discarded: `extract_v2` was deleted by the optimizer (0.9 ns for a whole loop is less than a cycle
per call); (2) the input is a string literal, a compile-time constant, so even kept results could be precomputed;
(3) one sample each, no spread; (4) no warm-up, v1 runs first and pays cold costs; (5) one input, which isn't the
production mix; (6) no failure inputs (malformed JSON); (7) no correctness check that v1 and v2 agree. The 0.9 ns: the
loop was optimized away and the timer measured two `Instant::now()` calls, minus nothing. Rewrite: inputs from a
`black_box`ed `Vec<String>` sampled from real payloads (with ~1% malformed), `compare(&mut [("v1", …), ("v2", …)], 3,
31, 1000)` from `ch02-09` with each closure doing `black_box(extract(black_box(&inputs[i])))` over the whole vector,
plus a test asserting both return the same results on every input.

### Selected exercises

- **Beginner.** With a `const` input, `sum_to(N)` becomes a constant: the benchmark reports the cost of the timer and
  `black_box`; the assembly of the caller contains the literal answer.
- **Intermediate.** Expect `fold` to compile to the same vectorized loop as `sum()`. Whichever timing it gets depends
  on where it lands; the verdict may change between placements. That's the answer: "the same, with a different layout."
- **Systems.** A slow clocksource (HPET: roughly half a microsecond per read on typical hardware, attributed/order of
  magnitude) makes every `Instant::now()` expensive, inflates per-operation timings, and even perturbs the application
  itself if it timestamps a lot. Check `current_clocksource` before trusting timings on a new machine.

### Design exercise (gateway-core benchmark suite): model answer

Microbenchmarks (criterion) for the five hot components with realistic inputs drawn from the recorded mix; instruction-
count gates (cachegrind) for the same five, ±2%; allocation tests (counting allocator) per request path; an assembly
diff check for three hot functions. Load test: wrk2/k6 open loop at 30/50/70% of pod capacity, 10 minutes each, request
mix recorded from production weekly with PII stripped, latencies measured from intended send time. Locally: `cargo
bench -- <component>` and the allocation tests before a PR. Layout noise: never gate on timing; require a difference to
persist across three nightly runs and exceed 5%; when a timing moves without an instruction-count change, check
placement before investigating.

---

## Chapter 20.3 — CPU Profiling and Flame Graphs

### Interview & architecture questions

**1. Sampling.** Interrupt the program at regular intervals (time or hardware events), record the call stack, and
count identical stacks. The fraction of samples containing a function estimates the fraction of time spent in it; with
thousands of samples the estimate is good for anything that matters (a function at 1% of time needs ~10,000 samples for
±10% relative accuracy, roughly).

**2. Flame graph axes.** Width = number of samples (share of time) containing that frame; y = stack depth; x order is
alphabetical, not time. The classic misreading is treating the x-axis as a timeline ("first it parses, then …") or
reading the *height* of a tower as cost.

**3. Profile types.** On-CPU for CPU cost (the gateway's cores: which code burns them). Off-CPU for blocking (the
statement page: what fetches or locks it waits on). Wall-clock for request latency end to end (why a charge takes
400 ms: mostly the processor call, `ch03-02`'s "wait_upstream 83%").

**4. Aliasing.** A fixed sampling period in step with a periodic workload samples the same phases repeatedly, biasing
the result (`ch03-02`: 28/50/22 vs true 20/50/30). Sampling at an odd frequency (99 Hz rather than 100) or with random
jitter breaks the lockstep.

**5. Misattribution in release builds.** Inlining removes frames (time appears in the caller, e.g. `main`): fix with
debug info (DWARF inline records) and a profiler that reads it. Function merging makes several symbols share one body
(time appears under an unrelated name): check the assembly for aliases, look at callers, or build with
`-Z merge-functions=disabled`. Missing frame pointers break stacks (`[unknown]`): use `-C force-frame-pointers=yes` or
DWARF unwinding.

**6. Stack walking.** Frame pointers: cheap and reliable when every function keeps them (costs a register and a few
instructions per call). DWARF: no rebuild needed, but copies stack per sample and post-processes: heavy. LBR: hardware,
cheap, shallow (≈32 branches), not always available in VMs. Distributions enabled frame pointers because continuous,
fleet-wide profiling needs cheap reliable stacks, and the measured runtime cost was judged small.

**7. IPC.** Instructions retired per cycle: how much useful work the core does per clock. A vectorized sum over cached
data runs at high IPC (2–4, predicted); a pointer chase runs near zero (each load waits ~170 ns, hundreds of cycles, for
a handful of instructions).

**8. `getrusage`.** User vs system time (is the cost in my code or the kernel?), voluntary vs involuntary context
switches (am I blocking, or being preempted?), page faults. A CPU flame graph of your code can't show kernel time
attributable to your syscall pattern as clearly, or waiting at all.

**9. PGO and BOLT.** PGO feeds a representative execution profile into compilation (inlining, block layout,
indirect-call promotion, branch weights). BOLT rewrites the linked binary's layout by profile (hot code together, cold
code out of the way). The risk: an unrepresentative training workload optimizes for the wrong paths, and profile data
from production may contain sensitive information if collected carelessly.

**10. Slow CI.** `cargo build --timings` (critical path; which crates; front end vs codegen), `-Z self-profile` with
`summarize` (which queries: trait solving, monomorphization, LLVM), `-Z time-passes` and LLVM's `-time-passes` (which
passes); Cranelift for debug builds if LLVM dominates debug codegen. Split crates if the front end of one crate is the
critical path; reduce generics (Chapter 7.3) if monomorphization and LLVM time dominate.

### Debugging exercise (`__memmove_avx_unaligned_erms` from `[unknown]`)

`[unknown]` means the profiler couldn't walk from the leaf to its callers: the stack walk broke at a frame without a
frame pointer (glibc's memmove and the Rust code both omitted them) or without unwind info it could use. Fixes: build
with `-C force-frame-pointers=yes` (and, ideally, a libc with frame pointers), or record with `perf record
--call-graph dwarf` (bigger samples), and keep debug info (`debug = "line-tables-only"`, not stripped, or split debug
info available to the profiler). Rust operations that become large `memmove`s: moving large values by value (big
structs, arrays), `Vec` growth (reallocation copies), `clone` of large `Vec`/`String`, `copy_from_slice`/`extend` of
large slices, and `VecDeque::make_contiguous`.

### Selected exercises

- **Beginner.** At 1 ms you get ~5× fewer samples (less precision, same bias pattern if still in lockstep); at 50 µs, 4×
  more samples and more sampler overhead (the sampler thread competes for a vCPU). Accuracy improves with sample count
  only while the bias (aliasing) doesn't dominate.
- **Advanced.** With `#[inline(never)]`, all three functions appear in the release backtrace (they have frames). A
  frame-pointer profiler without `-C force-frame-pointers=yes` may still lose frames where functions don't set up
  `rbp`; with the flag, each function pushes `rbp`, and the chain is walkable.

### Design exercise (profiling platform): model answer

Build variants: release with `debug = "line-tables-only"` and frame pointers for all services (measure the cost once;
accept ~1–2% if measured), split debug info uploaded to a symbol server keyed by build ID. Collection: a continuous
eBPF-based profiler at ~19–49 Hz on every host (on-CPU), on-demand 99 Hz on-CPU + off-CPU captures triggered by an
engineer or by an alert (p99 regression). Storage: folded stacks per service × version × 10 s, 30-day retention.
Deploy comparison: automatic differential flame graph of the canary vs baseline, with the top growing frames attached to
the deploy. PGO: training profiles generated from the load-test replay of the recorded mix (never from raw production
payloads), stored as build artifacts per release.

---

## Chapter 20.4 — Allocation and Memory Profiling

### Interview & architecture questions

**1. Four facts.** Site (where to change code), count (CPU at ~11 ns per pair, contention), bytes (memory, cache
footprint, page faults for large ones), lifetime (per-request → reuse or arena; per-process → presize, intern, pack;
cross-thread → recycle to producer). Many small short-lived → arena; few large per-request → pooled buffer with a cap.

**2. Three sizes.** Virtual size: everything mapped, including reserved-but-untouched ranges. RSS: pages resident in
RAM. Live heap: bytes held through the allocator. The OOM killer (and cgroup memory limits) act on resident memory
(RSS plus page cache charged to the cgroup), not on live heap.

**3. `ch04-05`.** Freed chunks were scattered between live ones, so no whole page became free, and glibc only returns
memory from the heap's top (or whole free pages during a trim). After everything was freed, small chunks still sat in
per-thread caches and fast bins, unconsolidated. `malloc_trim(0)` consolidates the free lists and releases every free
page to the kernel (`MADV_DONTNEED`), including pages in the middle of the heap.

**4. `ch04-04`.** Local malloc/free hits glibc's per-thread cache (tcache): no shared state, so it scales. Cross-thread
frees overflow the freeing thread's cache and go back to the allocating thread's arena under its lock, while the
allocating thread (whose cache is always empty) takes the same lock to allocate: contention on the arena, ~118 ns per
free measured.

**5. Bump arenas.** Right for many small allocations sharing a lifetime (a request, a batch, a parse tree). You give up
freeing individual objects (memory is reclaimed only at reset) and, with bumpalo's default API, `Drop` for the values it
holds (types that own other resources need care).

**6. The fraud numbers.** CPU: ~1.4% of a 3 ms score, ~1.7 of ~120 cores: modest. Allocations: 408 → 0 per score, ~20M
fewer allocations per second fleet-wide, less allocator contention; tail: p99.9 300 µs → 7.7 µs under four threads.
The allocation count and tail are properties the design reliably changes; the CPU share depends on everything else a
score does.

**7. Zeroing cost.** Small buffers: the copy is cheap and cache-resident, so a `memset` of the same size doubles the
work. Large buffers: both the memset and the copy stream through memory, limited by bandwidth; zeroing adds a pass but
the copy's cost dominates less clearly (~1.5×).

**8. Fragmentation.** Rust: objects never move, so holes left by frees persist until neighbours are freed; RSS can stay
high with low live heap (`ch04-05`, the reconciliation job). Java with a compacting collector: live objects are moved
together, so fragmentation is repaired at collection time, at the cost of GC work and pauses.

**9. Allocation rate in production.** Export a counter from a counting global allocator (cheap, always on); or use the
allocator's statistics (jemalloc/mimalloc stats); or attach a bpftrace uprobe on `malloc` for a few seconds (counts and
stacks without redeploying).

**10. Allocator swap evidence.** A load test with the production mix at production concurrency before and after: CPU
per request, p99/p99.9, RSS over hours (fragmentation behaviour), and peak RSS under the batch-shaped workloads;
plus build-size and startup-time checks, and a canary.

### Debugging exercise (RSS climbing 50 MB/h with flat live heap)

heaptrack finds allocations that are never freed; here live heap is flat, so there's probably no leak to find. Likely
mechanisms: (1) fragmentation (long-lived allocations interleaved with short-lived ones pin pages; new size classes
can't reuse old holes); (2) retention in the allocator (freed memory kept in caches, arenas that never trim; with many
threads, many arenas each holding free memory). One-line experiment: call `malloc_trim(0)` (or send the service a
signal that does): if RSS drops to near live heap, it was retained free memory; if it doesn't, it's fragmentation
around live objects. Long-term fixes: arenas or pools for the workload that creates the churn, fewer glibc arenas
(`MALLOC_ARENA_MAX`), periodic trims, or an allocator with decay-based purging (jemalloc/mimalloc), measured.

### Selected exercises

- **Beginner.** `Arc<HashMap>` removes the 81 allocations per order (a clone is one atomic increment): 27 per order
  remain (24 decode + 3 log, predicted).
- **Intermediate.** A borrowing struct allocates nothing for fields that are plain JSON strings without escapes; what
  remains is the `items` array (a `Vec` of structs) and any escaped strings. The exact count depends on the struct; a few
  per order instead of 24 (predicted, measure it).
- **Advanced.** Returning buffers to the producer turns cross-thread frees into local reuse: the producer's allocations
  come from its returned buffers, and throughput should approach the "token only" control (predicted; the return
  channel adds its own cost).

### Design exercise (allocation budget system): model answer

Each hot path declares `#[alloc_budget(allocs = 0, bytes = 0)]`-style metadata (in practice, a test per path): CI runs
each path's test under a counting allocator and fails on budget overrun. Production: the global counting allocator
exports allocations and bytes per request class (sampled attribution through a thread-local "current path" as in
`ch04-01`), alerting on drift. Exceptions: a PR label plus a note in the budget file with an owner and an expiry date.
Third-party crates: the budget tests run on dependency updates (Renovate/Dependabot PRs), so a crate update that adds
allocations fails CI; pin or patch as needed.

---

## Chapter 20.5 — Caches, Branch Prediction, and False Sharing

### Interview & architecture questions

**1. Latencies.** On this host: L1 ~1.6 ns (~5 cycles), L3 ~16 ns, DRAM ~130–170 ns. Pointer chasing makes each load's
address depend on the previous load, so nothing overlaps and the full latency is exposed. A scan's addresses are known in
advance; prefetchers fetch ahead and many misses are in flight at once, so it runs at bandwidth.

**2. The 16 MiB jump.** The L3 is shared by all cores (and tenants), so a 16 MiB working set doesn't fit in what this
core can use; a random walk over 16 MiB also exceeds the TLB's reach, adding page walks. So "L3-sized" behaves like DRAM.

**3. Misprediction.** ~6 ns (~20 cycles), measured as (random − sorted) / miss rate in `ch05-02`. The compiler removes
the branch when the body is simple enough to compute unconditionally: `cmov`/`select`, or vectorized masks (`count_big`'s
`psrlw`/`pand`). To force immunity: branch-free formulations, sorting or grouping by the condition, or lookup tables
(`ch05-08`).

**4. Niche vs tag.** In cache, the tagged loop does more instructions per element (extracting tags, masking); the niche
loop just adds, so the gap is compute (4.4×). From DRAM, both loops wait on bandwidth and the tagged slice has twice the
bytes, so the gap approaches 2× (2.2×).

**5. Layouts.** AoS: random access to whole records (one or two lines each). SoA: scans over few fields (bytes touched,
vectorization). Both: hot/cold split for the record path plus a columnar snapshot for scans (the matcher's fix).

**6. False sharing.** Two variables used by different threads sit in the same 64-byte line; the hardware keeps
coherence per line, so a write to one variable invalidates the other's line in every other core. Readers slow down
because their cached copy is invalidated by each write to the neighbour, so their next read misses and fetches the line.

**7. MLP.** Memory-level parallelism: a core can have many cache misses outstanding. `ch05-05`'s reads don't depend on
each other, so ~17 misses overlap and each costs ~10 ns of throughput, while `ch05-01`'s dependent loads can't overlap
(~170 ns each).

**8. Huge pages.** Improve TLB reach (fewer misses and page walks) and cut page faults 512×. Risks: memory bloat (a
touched byte commits 2 MiB), latency spikes from synchronous compaction and `khugepaged`. Verify with `AnonHugePages` in
`/proc/<pid>/smaps_rollup` (or `smaps` per mapping).

**9. The arena book.** Fewer allocations (a slab reuses slots; 0.82 → 0.22 per operation); contiguity (orders adjacent
in one array share lines and prefetch well); less indirection and bookkeeping (no `RcBox` with two counts and a
`RefCell` flag per order, no refcount traffic on clone/drop).

**10. i-cache pressure.** `perf stat -e L1-icache-load-misses,iTLB-load-misses` on the service; compare hot-function
sizes (`cargo llvm-lines`, symbol sizes); an experiment that calls N monomorphized copies round-robin (the Systems
exercise); and a BOLT-optimized build: if BOLT helps a lot, front-end pressure was significant.

### Debugging exercise (`Registry`)

**Problem 1 (new): a hot write in the read-mostly line.** `requests_total` sits right after `schema_version`, and the
vector's header follows: all three are in the registry struct's first cache line. Every request now *writes* that line
(`requests_total.fetch_add`), and every metric operation *reads* it (`schema_version`, the vector's pointer and length).
So every metric operation on every core misses on a line another core just modified: `ch05-06`'s slowed readers, on
the hottest path in the service. **Problem 2 (made worse): contiguous hot counters.** The counters are 8 per 64-byte
line; the dozen incremented on every request share a couple of lines, which bounce between all cores that handle
requests (writers false-sharing among themselves, Chapter 14.4). The release added one more per-request write, and with
16 cores, the lines spend more time in transit. On 2-core hosts there's little to bounce between, so neither shows.
Fixes without padding every counter: (1) move `requests_total` out of the struct into per-worker counters (or at least
put the read-mostly fields, `schema_version` plus the vector, behind one `CachePadded` so no written field shares their
line); (2) make the per-request counters per-worker blocks (one `CachePadded` block of counters per worker thread,
summed on scrape, Chapter 14.4's design), leaving the ~190 rarely incremented counters contiguous and unpadded.

### Selected exercises

- **Beginner.** The L1 edge is at 32 KiB (the plateau holds through 32 KiB and rises by 128 KiB); an 8 KiB row matches
  16 KiB, and 64 KiB sits between L1 and L2 (predicted; run it).
- **Intermediate.** With `acc += x as u64` in the body, LLVM typically removes the branch (select or vectorized mask),
  and random order costs the same as sorted (predicted; the assembly decides).
- **Systems.** The per-call time should step up once the total size of the hot copies exceeds the L1i (32 KiB) and again
  at the micro-op cache's capacity; 1 and 8 copies of a small function likely fit, 64 may not (predicted).

### Design exercise (route table): model answer

An immutable route table built off the request path and published through `ArcSwap` every 10 minutes (readers do one
atomic load per request, no writes); the old table dropped on a dedicated dropper thread (Chapter 3.1). Hot fields for
lookup (a perfect hash or a `HashMap<&str, RouteId>` over interned prefixes, plus a dense `Vec<HotRoute>` indexed by
`RouteId` with upstream ID and limits) separate from cold metadata (`Vec<ColdRoute>`: descriptions, owners, audit
data). Per-route counters are per-worker (`CachePadded` blocks per worker indexed by `RouteId`), summed by the exporter
once a minute. Measurements: `ch05-04` (hot/cold layout), `ch05-06` (readers vs writers in shared lines), `ch07-05`
(sharing vs copying), and `ch02-09`'s harness for the lookup itself.

---

## Chapter 20.6 — SIMD and Vectorization

### Interview & architecture questions

**1. `sd` vs `pd`.** "s" = scalar (one element), "p" = packed (a full register of elements); "d" = double precision.
A loop full of `addsd` isn't vectorized; `addpd`/`paddd` means it is.

**2. `f64` sums.** Vectorizing reorders additions, and IEEE addition isn't associative, so the result could change;
LLVM may not do that without permission, and stable Rust never grants it implicitly. Write the order you want: several
accumulators (`chunks_exact(8)`, one per lane) and a final combine. You give up bit-for-bit equality with the source-
order sum (and must document the tolerance).

**3. `saxpy`.** Slices: `&mut` and `&` are `noalias`, so LLVM knows stores to `y` can't change `x`, and vectorizes
directly. Raw pointers: they might overlap, so LLVM tests for overlap at run time and falls back to a scalar loop. The
overlapping case was 46× slower because each iteration read the value the previous one wrote: a true loop-carried
dependency through memory that nothing can vectorize.

**4. AVX2.** The default target is the x86-64 baseline (SSE2), so the compiler may only emit instructions every x86-64 CPU
has. Use AVX2 with runtime dispatch (`is_x86_feature_detected!` + a `#[target_feature(enable = "avx2")]` function), or
build for `-C target-cpu=x86-64-v3` when the whole fleet supports it.

**5. Detection cost.** One `cpuid` on first use, then a cached load and test. Put the check outside the hot loop and the
whole loop inside the feature-enabled function (a feature-enabled function can't be inlined into a caller without the
feature).

**6. `unsafe` call.** Executing AVX2 instructions on a CPU without AVX2 is undefined behaviour (in practice SIGILL).
The function's safety depends on a property of the machine that the type system can't check, so the *caller* must
assert it (`unsafe` block, after a runtime check); inside code that already has the feature, the call is safe.

**7. `std::simd`.** Portable: one source for any target, no `unsafe`, readable lane types and masks. But it's unstable
(nightly-only on 1.98), and many production crates require stable toolchains.

**8. At bandwidth?** Compare bytes processed per second with the bandwidth of the level the data lives in (measured with
a streaming copy/sum over the same size). If they're close, a faster kernel can't help; reduce bytes instead.

**9. −0.0.** Negative zero is the true additive identity in IEEE 754: −0.0 + x = x for every x, including −0.0. Starting
from +0.0, the sum of an empty-but-negative-zero input like `[-0.0]` would be +0.0, a (tiny) wrong answer.

**10. Java vs Rust.** Java's JIT compiles on the machine that runs the code, so it uses the host's vector width
automatically; risks: warm-up and deoptimization, and less control. Rust decides at build time: baseline binaries leave
width unused unless you dispatch; `target-cpu=native` risks SIGILL on other hosts.

### Debugging exercise (small `#[target_feature]` helper)

Two reasons it's slower: (1) **inlining**: a function with `target_feature(enable = "avx2")` can't be inlined into a
caller that doesn't have AVX2 enabled, so each 32-byte chunk pays a real call (argument passing of `__m256i` values
through memory under the SysV ABI for a caller without AVX, plus call overhead); (2) **where the feature is enabled**:
the loop itself is compiled for the baseline, so the loads, loop control, and accumulation stay scalar or SSE2, and only
the tiny helper uses AVX2. Restructure: make the whole loop (or the whole function containing it) the
`#[target_feature(enable = "avx2")]` function, do the runtime check once before calling it, and let the helper be an
ordinary `#[inline]` function called from inside.

### Selected exercises

- **Beginner.** In release, `+` on `u32` wraps (overflow checks off), so the loop still vectorizes (`paddd`). In debug,
  overflow checks add a branch per addition and nothing is vectorized (no optimization at all in debug by default).
- **Intermediate.** `get_unchecked` with index arithmetic typically compiles to the same loop as `chunks_exact`
  (the bounds are provable either way): the `unsafe` buys nothing (predicted; compare the assembly).
- **Advanced.** An AVX-512 path compares 64 bytes per instruction; it can beat AVX2 on in-cache data, and may not beat
  memchr at bandwidth (predicted: the 16 MiB buffer is partly memory-bound).
- **Systems.** Check 8 elements at a time with `c.iter().any(|&x| x == needle)`, which LLVM vectorizes as a compare and
  mask test (predicted), then locate the index within the matching chunk: a few times faster than the scalar loop for
  needles near the end.

### Design exercise (redaction filter): model answer

Use `memchr`-style byte searches (memchr3/memmem) for rare trigger bytes (`@` for e-mails), a 256-entry class table for
the digit-run scanner (card numbers: digits are common in logs, so skip-search doesn't pay, Chapter 9.2), and
auto-vectorized loops for simple passes (ASCII checks). Explicit kernels only for the digit-run detector if it measures
below target, with dispatch across x86-64 v1/v3/v4 and NEON on aarch64, and a scalar reference with a differential test
(random lengths, alignments, contents with planted card numbers and e-mails). Measure GB/s per core against a streaming
read of the same buffer: at ~2 GB/s on in-cache lines the filter is far from bandwidth, so the target is achievable with
a table-driven scanner; if it plateaus near memory bandwidth, reduce passes (fuse the scanners into one).

---

## Chapter 20.7 — Contention, NUMA, and Tail Latency

### Interview & architecture questions

**1. Little's law.** L = 20,000 × 0.015 = 300 requests in flight on average. The service needs at least ~300
concurrent slots end to end (connections, worker capacity, pool size to its dependencies, if each request holds one for
its duration), plus headroom for bursts; a pool of 100 would be a queue that turns into latency.

**2. Why latency rises early.** Random arrivals bunch; during a bunch the server falls behind, and at high utilization
it rarely catches up before the next bunch. Mean waiting grows like ρ/(1 − ρ). Kingman: waiting also grows with the
variability (squared coefficients of variation) of arrivals and service times, so reducing variability (smoothing
arrivals, bounding work per request) shortens queues without adding capacity.

**3. 40 backends.** If each call is slow with probability 1%, 1 − 0.99^40 ≈ 33% of requests see a slow call. To keep
the request's p99 at 50 ms, you need P(all 40 fast) ≥ 0.99, so each backend must meet 50 ms at the 0.99^(1/40) ≈
99.975th percentile.

**4. Hedging.** Send a second copy of a request that hasn't answered by roughly its p95 and use whichever answers
first. Safe for idempotent requests (reads, or writes with idempotency keys). Keep it from worsening overload with a
budget (hedge at most a few percent of calls; stop when the budget is spent), cancel the losing request, and don't hedge
to a backend that is known to be overloaded.

**5. Contention and hold times.** A hold-time profile shows the critical section's own cost, which doesn't change with
contention (180 ns in every `ch07-03` row). Contention is time spent *waiting* to acquire, which appears as off-CPU time
(parked threads) or spin time outside the critical section. Measure wait-time histograms, off-CPU profiles, or lock
statistics.

**6. `RwLock` vs `Arc` values.** Every `RwLock` read acquisition still writes the lock word (reader count), so the lock's
line moves between cores on every GET; with a 4 KiB copy inside, the critical section is short enough that this cost
matters and long enough that readers overlapping helps only a little (10%). `Arc<[u8]>` values remove the copy from the
critical section entirely (a refcount increment instead), which is 2.8× even on one thread.

**7. Core-to-core.** ~56–70 ns round trip here (one line transfer each way). Paid by every contended atomic RMW, lock
hand-off, channel send/receive between cores, and every read of a line another core just wrote (false sharing,
`Arc` refcounts on shared values).

**8. First touch.** Linux places a physical page on the NUMA node of the thread that first writes it. A buffer
initialized by one thread on socket 0 and then used by workers on socket 1 is remote for them: every miss pays the
cross-socket latency and consumes interconnect bandwidth.

**9. Thread-per-core vs work-stealing.** Thread-per-core: no sharing, no locks, predictable caches and latency;
gives up load balancing (a hot shard saturates its core while others idle) and requires partitioning the data.
Work-stealing: balances load automatically and tolerates skew; pays for cross-core task migration, shared queues, and
synchronization.

**10. `StampedLock`.** Optimistic reads are a seqlock: read a stamp, read the fields, validate the stamp; readers write
nothing shared. In Rust the fields must be read atomically (a racy non-atomic read is undefined behaviour, whereas
Java's memory model merely allows a torn or stale value that validation then rejects), so a Rust seqlock needs atomic
fields and careful fences (Chapter 14.4); for anything larger than a few words, `ArcSwap` snapshots are simpler.

### Debugging exercise (cliff at 1,300 req/s, CPU at 65%)

Three likely causes, invisible to CPU and on-CPU profiles: (1) **a saturated non-CPU resource**: a connection pool,
semaphore, or downstream concurrency limit sized for ~1,000 req/s (Little's law); confirm with pool wait-time metrics
and in-flight counts; fix by resizing to L = λW with headroom, or adding backpressure. (2) **lock contention / convoys**:
threads parked waiting, not burning CPU; confirm with an off-CPU profile or lock wait histograms; fix by shortening or
sharding the critical section. (3) **a single-threaded bottleneck** (one hot thread, an actor, an accept loop, a
logging thread) at 100% while the average CPU is 65%; confirm with per-thread CPU (`top -H`, `/proc/<pid>/task/*/stat`);
fix by parallelizing or offloading it. (Also possible: a downstream at its own cliff; confirm with per-hop latencies.)

### Selected exercises

- **Beginner.** Deterministic service times (M/D/1) halve the mean queueing delay relative to M/M/1 (Pollaczek–Khinchine:
  the waiting-time factor is (1 + C_s²)/2, with C_s = 0 for constant service), and shrink the p99 considerably; the
  cliff moves right but doesn't disappear (arrivals are still random).
- **Intermediate.** Tied requests double backend load (100% extra calls) and cut the tail even more than hedging
  (both copies start immediately); hedging gets most of the benefit for ~1% extra by waiting until a call is already
  slow. Tied requests make sense only with spare capacity and cancellation (Dean and Barroso discuss both).
- **Advanced.** Expect a modest spin (≈100 iterations with `spin_loop`) to cut wait p99 at 2 threads (waiters rarely
  park), 0 spins to cost a park/unpark per contended acquisition, and 10,000 spins to waste CPU and hurt throughput at 4
  threads on 4 vCPUs (spinners occupy the CPU the holder needs), predicted from mechanism.

### Design exercise (payments-core tail budget): model answer

Budget (p99, 800 ms end to end): gateway 20 ms, payments-core own work 30 ms, fraud score 50 ms (its p99 < 5 ms SLO
leaves slack for the network), processor 600 ms (the dominant, external hop), 100 ms reserve for retries and queueing.
Hedging: allowed on the fraud score (idempotent read, cheap, internal) with a 3% budget; **forbidden** on the processor
charge (a duplicate charge is a correctness failure; the idempotency key protects correctness but hedging would double
processor load and cost; use a single attempt with a deadline, then reconciliation, Chapter 8.4). Utilization targets:
payments-core ≤ 60% so its own queueing stays under ~20× its service time; fraud ≤ 60%; gateway sized for zone loss at
≤ 75%. Dashboards: per-hop p50/p99, queue depth and pool wait time per hop, in-flight counts (Little's law check),
hedge rate vs budget; alerts on SLO burn rate and on any hop exceeding its budget for 10 minutes.

---

## Part XX Review — Capstone: the router cache PR

### The benchmark (at least six defects)

1. **One key, a million times** (Chapter 20.2, input distribution): the cache hits 100% of the time. On a realistic mix
   the last-hit cache hits **2.4%** of the time (`review-02`'s first line).
2. **Results discarded, inputs constant** (lies 1–2): nothing is `black_box`ed; the literal path could be folded. (Here
   the `Mutex` and allocation kept the work alive, but the benchmark doesn't guarantee it.)
3. **One sample, no spread, no warm-up, old first** (lies 3–5): the first loop pays cold costs; there's no way to tell
   6.0× from noise.
4. **One thread** (Chapter 20.7): the gateway runs 16 workers against one router; a global `Mutex` is invisible
   single-threaded.
5. **No allocation count** (Chapter 20.4): the PR allocates on every miss.
6. **Mean only** (Chapter 20.1): no histogram, so lock convoys and allocator slow paths can't show.
7. **No correctness check**: "no functional change" isn't tested.
8. **"2× to be conservative"**: a number that was never measured isn't conservative; it's made up. The PR should report
   what it measured, with the workload.

### The code (at least five problems)

1. **A global `Mutex` on every lookup**: all workers serialize on one lock and one cache line, even on hits; a
   read-only path now writes shared memory (Chapters 11.7, 20.5, 20.7).
2. **Two allocations per miss**: `path.to_string()` for the cache entry plus main's `to_string()` inside
   `inner.lookup` (1.95 allocations per lookup measured).
3. **A last-hit cache doesn't match the traffic**: it needs *consecutive* identical requests, and 16 workers interleave
   their requests into one cache, so even hot routes rarely hit.
4. **Inherited from main**: `self.routes.get(&path.to_string())` allocates on every lookup, though a
   `HashMap<String, u32>` can be queried with `&str` (`String: Borrow<str>`, Chapter 9.3). This is the real win in the PR's
   neighbourhood.
5. **A new failure mode**: `lock().unwrap()` on the hot path; a panic while holding it would poison the router for
   every request (Chapters 8.3, 11.3).
6. **Unbounded claims about "most traffic"**: the design rests on a traffic assumption nobody measured; the recorded
   mix says otherwise.

### What to ask for instead, and the corrected benchmark

The one-line fix on main: `self.routes.get(path).copied()`. If a cache is still wanted after that, it must be per worker
(no lock, no shared writes) and justified by a measured hit rate on the recorded mix. Evidence: `review-02`'s shape:
recorded mix, one change at a time, interleaved samples with spread, production-like concurrency, allocation counts, a
correctness test, and (per Part XVIII's rule) the release assembly of the lookup.

`review-02-route-bench-fixed.rs`, one run:

```text
last-hit cache hit rate on the realistic mix: 2.4%
wall ns per lookup (per thread's stream), median of 31 [p10 .. p90]; release; one Playground run, noisy
  1 thread(s)  main (String per lookup)      54.2  [51.4 .. 64.6]   allocs/lookup 1.00
  1 thread(s)  main + borrowed lookup        29.3  [27.5 .. 35.3]   allocs/lookup 0.00
  1 thread(s)  PR: last-hit cache            73.8  [66.5 .. 203.0]   allocs/lookup 1.95
  4 thread(s)  main (String per lookup)     134.3  [121.2 .. 245.7]   allocs/lookup 1.00
  4 thread(s)  main + borrowed lookup        64.3  [56.4 .. 213.8]   allocs/lookup 0.00
  4 thread(s)  PR: last-hit cache           574.0  [554.8 .. 663.0]   allocs/lookup 1.96
```

Read within each thread count (each sample includes spawning its threads, which inflates the four-thread rows for every
variant alike). On one thread the PR is **1.36× slower** than main and 2.5× slower than the one-line fix, and allocates
twice per lookup. On four threads it's **4.3× slower** than main and **8.9× slower** than the fix: the `Mutex`. The fix
is 1.85× faster than main on one thread and 2.1× on four, with zero allocations. The PR's "6×" measured a workload the
gateway never sees. The test in `review-02` (`every_router_gives_the_same_answers`) passes for all three, so the
problem was never correctness: it was evidence.

---

## Part XX Review — Interview mode

**1. "3× faster".** Metric, statistic, workload, comparison (20.1 §2); then: the benchmark's construction (black_box,
warm-up, interleaving, samples, spread), the generated code (is the work still there?), production-like concurrency and
input mix, allocation counts, and a canary result before believing it about production.

**2. Fleet p99.** Percentiles don't average. Merge histograms (HdrHistogram's `add`) or compute from raw samples;
Prometheus histograms need identical buckets and give approximations.

**3. Coordinated omission.** Closed loop: a 2 s stall records one slow request and a p99 of ~1 ms. Open loop at 500
req/s: ~1,000 requests arrive during the stall, and the p99 is ~1.5 s (`ch02-06`).

**4. Nothing measured.** Dead code (black_box the result), constant inputs (black_box the input), closed forms (use data,
not ranges; check the asm). `black_box` is documented as a best-effort hint, not a guarantee.

**5. Same instructions, 1.7×.** Code placement (front-end effects). Protect the suite by gating on instruction counts
and allocations, not timings; comparing assembly before attributing timing changes; requiring differences to persist
across builds and exceed a threshold.

**6. Vectorization.** `f64` sums: order change not allowed; write lanes. `saxpy_raw`: possible aliasing; slices carry
`noalias`, so no check.

**7. Hierarchy.** ~1.6 / ~5 / ~16 / ~150 ns (L1/L2/L3/DRAM here); measure with a pointer chase through a random cycle
at growing working-set sizes (`ch05-01`).

**8. Branches.** ~6 ns per misprediction; make hot loops immune with branch-free code (selects, masks, tables) or by
sorting/grouping by the condition.

**9. Readers slowed.** Their line was invalidated by a neighbour's writes (false sharing), so every read missed.

**10. Live vs RSS.** Allocator retention and fragmentation: freed memory stays mapped unless whole pages at the right
places are free and the allocator returns them (`ch04-05`).

**11. 85%.** p99 at 80% is ~22× service time vs ~9× at 50%; losing a zone raises survivors' load by 1.5×, which turns 85%
into >100%. Size so the pool after zone loss stays under ~75% at peak; autoscale on queue depth and p99.

**12. Fan-out.** 1 − 0.99^40 ≈ 33% of pages hit a slow backend; hedge at ~p95 with a budget and cancellation, tighten
backend p99.9s, reduce fan-out (batch documents). Risks: extra load under overload (hence the budget), non-idempotent
calls (never hedge those).

**13. Thread-per-core.** When data partitions cleanly (by key, by connection), load is roughly uniform, and tail latency
matters more than elastic load balancing: e.g., a sharded cache or database node; not for request-heterogeneous
services with skewed work.

**14. Evidence policy.** CI gates: allocation tests, instruction counts, assembly diffs of hot functions. Nightly:
timing benchmarks on pinned runners, trend alerts. Pre-release: open-loop load test on the recorded mix at 30/50/70% of
capacity (CPU per request, p99, p99.9). Rollout: a canary at 1–5% with per-version comparison of CPU per request and
p99, with automatic rollback on regression.
