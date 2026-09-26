# Chapter 20.1 — Performance as a Measurement Discipline

> **Where this sits:** Part XX · Performance Engineering · chapter 1 of 7
> **Prerequisites:** Chapter 1.4 (the scale lens: per-operation budgets and fleet arithmetic), Chapter 11.7 (the first
> measured ranking), Chapter 12.6 (tail latency from long polls). Everything else in this Part builds on this chapter.
> **After this chapter you can:** name the eight metrics a performance claim can be about and say how each is measured
> in Rust and in Java; explain why one number (a mean, a "speedup") hides the distribution that matters; predict the
> ceiling of a parallel speedup with Amdahl's law and explain why real speedups fall short of it; and turn a
> microbenchmark into a capacity estimate without overclaiming.

---

## Pass 1 · User level — *What exactly are we claiming?*

### 1. Problem

Every Part of this book has made performance claims, and every one has come with a measurement, an attribution, or
the label "predicted from mechanism" plus an exercise. That was a rule of the book. This Part explains why it has to be
the rule of your team.

The Meridian API gateway is the running example. Its Java version handles about 400,000 requests per second at peak on
~330 cores (55 pods), with a p99.9 latency of about 45 ms and two GC-related SLO misses a quarter (Chapters 1.2, 1.4,
Part I review). The Rust rewrite has been justified since Part I on two grounds: fewer cores, and a tail that doesn't
depend on a garbage collector. Both are performance claims. Neither is a number yet.

Three things tend to go wrong between "Rust is faster" and a number you can put in a capacity plan:

- **The wrong metric.** "Faster" can mean lower latency, higher throughput, fewer cores, a lower p99, or less memory.
  Improving one often makes another worse (Chapter 9.5's structure-of-arrays incident: the report got 6× faster and
  the matcher's p99 got worse).
- **The wrong statistic.** A mean, or a single "speedup" number, hides the distribution. The requests that break SLOs
  live in the tail.
- **The wrong experiment.** A benchmark measures what it measures, which is often not what you think (Chapter 20.2 is
  a catalogue of ways that happens).

Donald Knuth's line is usually quoted as "premature optimization is the root of all evil". The full sentence (Knuth,
*Structured Programming with go to Statements*, ACM Computing Surveys, 1974) is more useful: "We should forget about
small efficiencies, say about 97% of the time: premature optimization is the root of all evil. Yet we should not pass
up our opportunities in that critical 3%." The discipline this Part teaches is **finding the 3% by measurement**, and
then proving that a change helped, also by measurement.

### 2. Mental model

**A performance claim has four parts: a metric, a statistic, a workload, and a comparison.** "The new router is
faster" has none of them. "At 400K req/s with the production request mix, p99 CPU time per request in the router drops
from 54 ns to 29 ns, measured as the median of 31 interleaved samples" has all four, and it can be checked.

The metrics you'll meet in this Part (the brief's list, with how each is measured here):

| Metric | What it is | Measured in Rust with | Java counterpart |
|---|---|---|---|
| **Latency** | Time for one operation, start to finish | `Instant` around the operation, recorded into a histogram | JMH `SampleTime`, `System.nanoTime()` |
| **Throughput** | Operations completed per unit of time | Operations / wall time over a long enough window | JMH `Throughput` mode |
| **Tail latency** | High percentiles (p99, p99.9, max) of the latency distribution | `hdrhistogram` (on the Playground) | HdrHistogram (same design, same author) |
| **CPU utilization** | Fraction of available CPU time spent running (user vs system) | `getrusage` (`ch03-03`), `/proc/self/stat`, `perf stat` | JFR CPU load events, `top -H` |
| **Memory utilization** | What the process holds vs what the OS has given it (live heap vs RSS) | A counting allocator + `/proc/self/status` (`ch04-05`) | Heap used/committed, NMT, RSS |
| **Allocation rate** | Allocations (count and bytes) per operation or per second | The counting allocator (Part III's instrument) | JFR allocation events, `-Xlog:gc` |
| **GC pressure** | Collector work caused by allocation and live-set size | Not applicable; the Rust analog is allocator cost plus fragmentation (Chapter 20.4) | GC logs, pause-time percentiles |
| **Lock contention** | Time threads spend waiting to acquire locks, vs holding them | Wait/hold histograms (`ch07-03`), `perf lock`, off-CPU profiles | JFR `JavaMonitorEnter`, async-profiler lock mode |

Two relationships tie them together, and both return in Chapter 20.7:

- **Little's law** [LANG-independent, a theorem]: the average number of requests in a system equals the arrival rate
  times the average time each spends in it (L = λW). Throughput, latency, and concurrency aren't independent knobs.
- **Utilization drives the tail.** As a resource approaches 100% busy, waiting time grows without bound, long before the
  mean service time changes. That's why capacity plans target 50–70% CPU, not 95%.

The working loop is the scientific one: **define the metric → measure a baseline → form a hypothesis → change one thing
→ measure again → keep or revert.** Every step has a way to cheat yourself, and this Part names them.

### 3. Rust code

**One number hides the distribution** (listing `ch01-01-one-number-lies.rs`, release). Time every single `Vec::push` of
4 million `u64`s and record each latency in an HdrHistogram:

```rust,ignore
// excerpt of ch01-01-one-number-lies.rs
let mut hist = Histogram::<u64>::new_with_bounds(1, 10_000_000_000, 3).unwrap();
let mut v: Vec<u64> = Vec::new();
let start = Instant::now();
for i in 0..n {
    let t = Instant::now();
    v.push(black_box(i));
    hist.record(t.elapsed().as_nanos().max(1) as u64).unwrap();
}
let total = start.elapsed();
```

One run's output:

```text
4000000 pushes, final capacity 4194304
wall time / n               60.3 ns   (the number a 'benchmark' usually reports)
p50                           20 ns
p99                           30 ns
p99.9                       1600 ns
p99.99                      4263 ns
max                       778239 ns
mean of samples             29.0 ns
empty timed region p50        20 ns   (timer overhead included in every sample above)
slowest 0.01% of pushes = 4% of the summed sample time
```

Read it carefully, because every line teaches something:

- **The "average push" is three different numbers** depending on how you compute it: 60.3 ns (wall time divided by
  count, which includes the cost of timing and recording each push), 29.0 ns (the mean of the recorded samples), and
  20 ns (the median). None of them is wrong. They answer different questions.
- **The median is the timer.** An *empty* timed region also reads 20 ns. A push is cheaper than the clock that measures
  it, so per-operation timing can't resolve it (Chapter 20.2 measures the clock itself). To time something this small,
  time a batch and divide.
- **The tail is made of different mechanisms.** The p99.9 (1.6 µs) isn't a slow push: it's a push that touches a
  memory page for the first time. A 4 KiB page holds 512 `u64`s, so one push in 512 (0.2%, the p99.9 band) takes a
  page fault while the kernel maps and zeroes a fresh page (Chapter 20.2 measures page faults directly). The max
  (778 µs) is a *reallocation*: when the vector is full, `push` asks for twice the capacity and moves everything
  (Chapter 9.1); the last one moves 16 MiB. There are only 20 reallocations in 4 million pushes, so neither mechanism
  shows in the mean or the median, yet the slowest 0.01% of pushes are 4% of the total time. In a latency-sensitive
  service, those are the requests that page you.

**Amdahl's law, measured** (listing `ch01-02-amdahl.rs`, release). A job with a parallel part followed by a serial part
(a dependent chain that no second thread can help with), on 1, 2, and 4 rayon threads, for a compute-bound and a
memory-bound parallel part:

```text
compute-bound parallel part (64 rounds per element, 3.2 MB):
  1 thread(s):  27.7 ms = parallel  24.4 + serial  3.3   speedup 1.00x (Amdahl predicts 1.00x)
  2 thread(s):  15.9 ms = parallel  12.6 + serial  3.3   speedup 1.74x (Amdahl predicts 1.79x)
  4 thread(s):  10.8 ms = parallel   7.4 + serial  3.3   speedup 2.57x (Amdahl predicts 2.95x)
  serial fraction f = 0.12 on 1 thread; Amdahl's ceiling 1/f = 8.4x
memory-bound parallel part (1 round per element, 192 MB):
  1 thread(s):  23.1 ms = parallel  19.9 + serial  3.3   speedup 1.00x (Amdahl predicts 1.00x)
  2 thread(s):  14.0 ms = parallel  10.7 + serial  3.3   speedup 1.66x (Amdahl predicts 1.75x)
  4 thread(s):  10.4 ms = parallel   7.1 + serial  3.3   speedup 2.22x (Amdahl predicts 2.80x)
  serial fraction f = 0.14 on 1 thread; Amdahl's ceiling 1/f = 7.0x
```

Gene Amdahl's 1967 argument (AFIPS conference) is arithmetic: if a fraction f of the work can't be parallelized, the
speedup on N workers is at most 1 / (f + (1 − f)/N), and never more than 1/f however many cores you buy. Here f is 12–14%,
so the ceiling is 7–8×. Two observations matter more than the formula:

- **Real speedups fall short of Amdahl's prediction**, because the formula assumes the parallel part scales perfectly.
  It doesn't. The compute-bound part went from 24.4 ms to 7.4 ms on 4 threads (3.3×, not 4×): thread wake-ups, work
  splitting, and 4 *virtual* CPUs whose physical cores are shared with other tenants. The memory-bound part did worse
  (19.9 → 7.1 ms, 2.8×), because 4 threads streaming from DRAM share one memory system: its bandwidth, not the core
  count, is the limit.
- **The serial part is untouched.** 3.3 ms on every row. Optimizing the parallel part further has diminishing returns;
  once the parallel part is fast, the serial 12% *is* the job. That's Amdahl's real lesson for an architect: measure
  the fractions before you pick what to optimize.

**A capacity model from a microbenchmark** (listing `ch01-03-gateway-hot-path.rs`, release). A *model* of the Rust
gateway's per-request application work, on one thread: parse the request head with `httparse`, verify an HS256 bearer
token (HMAC-SHA256 with `ring`), decode the claims with `serde_json`, route on method and path prefix among ~1,000
routes, rate-limit per API key with a token bucket, and write the upstream request head and an access-log line into
reused per-worker buffers. It deliberately leaves out TLS, socket system calls, the upstream call, and the response
body. The request has 13 headers (Chapter 9.1's "11–14 headers typical"):

```rust,ignore
// excerpt of ch01-03-gateway-hot-path.rs: per-worker state, reused across requests
struct Worker {
    key: hmac::Key,
    routes: HashMap<(&'static str, &'static str), u32>,
    buckets: HashMap<u64, TokenBucket>,
    json_buf: Vec<u8>,
    sig_buf: Vec<u8>,
    upstream_head: String,
    log: String,
}
```

```text
outcome: Forward { route: 250 }; request head 638 bytes
per request (200 samples x 1000): min 0.96 us, median 0.97 us, p99 of samples 1.82 us
heap allocations in 200000 requests after warm-up: 0
  of which HMAC-SHA256 verify (144 bytes): median 0.24 us
  of which httparse of the head:           median 0.23 us
cores for 400K req/s at 60% utilization, this model only: 0.6
same arithmetic with the Java gateway's ~500 us CPU per request: 333
```

This is the kind of number that ends up in a slide deck as "550× fewer cores". It must not. §9 explains what the number
does and doesn't mean.

---

## Pass 2 · Systems level — *What the numbers are made of*

### 4. Under the hood

**What a timestamp costs.** [RUNTIME][OS] `Instant::now()` on Linux calls `clock_gettime(CLOCK_MONOTONIC)`, which the
kernel serves from the vDSO (a page of kernel code mapped into every process) without a real system call. With the
`tsc` clocksource (this machine's, per `ch02-04`) it reads the CPU's timestamp counter and scales it. It still costs
~25 ns per call here (Chapter 20.2 measures it) and it has a resolution floor: consecutive readings on this machine
differ by at least 20 ns. That's why `ch01-01`'s median is 20 ns for a push *and* for an empty region: you're looking at
the ruler, not the object.

**How an HdrHistogram stores a distribution.** [LIB] Gil Tene's HdrHistogram (the Rust crate is a port) records values
into buckets whose width grows with magnitude, so that every value is kept to a fixed number of significant digits
(3 here: 0.1% precision) from 1 ns to 10 s in a few tens of KB. Recording is a couple of arithmetic operations and an
increment: cheap enough to record every operation, which is the point. Percentiles are then read from the counts. Two
consequences:

- You can **merge** histograms (per-thread histograms added at the end, as `ch04-02` and `ch04-03` do), which you can't
  do with averages of percentiles. Averaging p99s across hosts is a classic dashboard error: the p99 of the fleet is
  not the mean of the hosts' p99s.
- The **max** is exact and the percentiles are within 0.1%. The histogram can't tell you *which* request was slow; for
  that you need tracing (Chapter 22.5).

**Why the tail is a different mechanism.** [LIB][OS] `ch01-01`'s max of 778 µs is the last reallocation (Chapter 9.1's
growth policy: capacity doubles, so the final growth from 2M to 4M elements moves 16 MiB). Copying 16 MiB at the
~20 GB/s one core can stream is ~0.8 ms, the observed max. (For blocks this large, glibc may move the pages with
`mremap` instead of copying, Chapter 9.1's unverified inference; then the cost moves to the page faults that follow.)
The p99.9 band is page faults: `ch02-03` counts them one per page and measures about 2 µs each on this machine (32,770
faults in 68.6 ms, zeroing included).
Most performance problems in production look like this: a rare event with a different cost structure (a reallocation,
a rehash, a page fault, a GC pause, a lock convoy, a retry) that the average can't see.

### 5. Memory

**Allocation rate is a first-class metric in Rust too**, but it means something different from Java's. In Java, an
allocation is a pointer bump in a thread-local allocation buffer (TLAB): nearly free at the site, and paid for later by
the collector in proportion to allocation rate and live-set size. In Rust, each allocation is a `malloc` call: ~10–20 ns
at the site with glibc for small sizes (Chapter 20.4 measures ~11 ns per allocate-plus-free pair in `ch04-04`) and
nothing afterwards. So in Rust:

- an allocation on the hot path costs CPU **on that request**, and
- removing it helps the same request, not "the GC later".

`ch01-03` reports **zero** heap allocations in 200,000 requests after warm-up, because every buffer is owned by the
worker and reused (Chapter 9.2's access-log fix, Chapter 3.2's "don't clone per request"). Zero is a meaningful number
to put in a CI test; Chapter 20.4 shows how.

**Memory utilization is two numbers.** What your program holds (live heap) and what the OS has given it (resident set
size, RSS). They diverge after frees: `ch04-05` frees every one of a million small objects and RSS doesn't move until
the allocator is asked to trim (Chapter 20.4; the mechanism is Chapter 19.5's). A dashboard that shows only RSS will
tell you a leak is happening when it's fragmentation, and one that shows only live heap will miss the fragmentation.

### 6. CPU / OS

**CPU utilization is user time plus system time**, and they're worth separating. User time is your code; system time
is the kernel working on your behalf (system calls, page faults). `ch03-03` shows the same 200,000 log lines costing
43 ms of user and 87 ms of system time unbuffered, and 1.1 ms and 0.1 ms through a `BufWriter`: the fix was in the
kernel's column. `getrusage(RUSAGE_SELF)` gives you both without any profiler.

**Why capacity plans target ~60%, not 95%.** [CPU][OS] Because queueing delay grows sharply as utilization approaches
1 (Chapter 20.7 simulates it: at 90% utilization, the p99 is ~43× the service time; at 99%, ~300×). The headroom also
absorbs bursts, failover (losing one of 55 pods raises every other pod's load by ~2%), and noisy neighbours. `ch01-03`'s
arithmetic divides by 0.6 for that reason, and the Java figure (400K × 500 µs / 0.6 ≈ 333 cores) matches the ~330 cores
Meridian actually runs.

**The machine you measure on is part of the result.** [CPU] The Playground runs on 4 virtual CPUs of an AMD EPYC server
(Part XIV observed "AMD EPYC 9R14"). Its cores' clocks change with load and temperature (turbo), other tenants share
the physical machine, and a vCPU can be descheduled by the hypervisor (steal time, which `ch02-08` reads from
`/proc/stat`). Every timing in this Part is therefore labeled "one run, noisy", and the rankings, not the absolute
numbers, are what to keep.

---

## Pass 3 · Architect level — *Where to spend optimization effort*

### 7. Trade-offs

**When is a performance problem worth solving?** A simple decision procedure, in order:

| Question | If the answer is no | Tool |
|---|---|---|
| Is there a requirement (SLO, cost target, capacity limit) that is at risk? | Stop. It's a curiosity, not a problem. | SLOs, capacity model |
| Do you know which metric is failing (latency, throughput, tail, memory, cost)? | Measure before touching code. | Dashboards, the table in §2 |
| Do you know where the time or memory goes? | Profile (Chapter 20.3) or count allocations (20.4). | Profiler, counting allocator |
| Is the hot spot a large enough fraction to matter (Amdahl)? | Optimize elsewhere or not at all. | The fractions from the profile |
| Can you measure the change reliably (Chapter 20.2)? | Build the benchmark first. | Harness, histograms |
| Is the gain worth the complexity (unsafe, less readable code, new dependencies)? | Keep the simpler code. | Review, budgets |

**Premature optimization vs premature pessimization.** Knuth's warning is about small efficiencies bought with
complexity. Its mirror image (Sutter and Alexandrescu's *C++ Coding Standards*, 2004, call it "premature pessimization")
is writing needlessly slow code when the efficient version is equally clear: cloning a `HashMap` per request
(Chapter 3.2), `format!` per log field (Chapter 9.2), querying a `HashMap<String, _>` with `&path.to_string()` instead of
`path` (this Part's review). Those aren't optimizations. They're defaults, and reviews should enforce them without
benchmarks.

**The cost of an optimization is paid forever.** Every unsafe block, specialized code path, or cache adds review
burden, test surface, and failure modes. The Part XV rule applies here too: an optimization that requires `unsafe`
must come with a benchmark that justifies it, and with the Miri test that proves it sound.

### 8. Java comparison

| Concern | Java | Rust |
|---|---|---|
| Microbenchmark harness | JMH: forks, warm-up iterations, blackholes, modes (throughput, average, sample time) | criterion (not on the Playground; this Part uses a small harness, `ch02-09`), `std::hint::black_box` |
| Warm-up | Essential: interpreter → C1 → C2, with deoptimization possible later | Still needed (caches, page faults, branch predictors, `ch02-03`), but no JIT: code is final at build time |
| Allocation cost | Near zero at the site (TLAB bump), paid by GC later | `malloc` at the site (~10–20 ns small, glibc), nothing later |
| "GC pressure" | Allocation rate × object lifetime drives young/old collections and pauses | No GC. The analog is allocator CPU, cross-thread frees, fragmentation (Chapter 20.4) |
| Tail latency sources | GC pauses, safepoints, JIT deopt, lock inflation | Reallocation, page faults, allocator trims, lock convoys, blocking in async tasks |
| Profilers | async-profiler, JFR, VisualVM | `perf` + flame graphs, `samply`, heaptrack/dhat (Chapter 20.3–20.4) |

> **Analogy limit.** "Java's allocation is free" and "Rust's allocation costs ~15 ns" are both true and both misleading
> in isolation. A Java service that allocates 1 GB/s pays for it in GC CPU and pause risk; a Rust service that allocates
> the same pays malloc CPU but has no pause. Compare *total* CPU per request and the *tail*, not allocation cost per
> call.

> **Why not just trust the JIT-vs-AOT folklore?** Because both directions are sometimes true. A JIT can inline across
> virtual calls using runtime profiles (Chapter 7.2) and beat static code; AOT code has no warm-up and no
> deoptimization cliffs. The only way to know for *your* workload is a measurement on *your* workload.

### 9. Production scenario

**The gateway capacity model, done honestly.** The rewrite team's first draft said: "The Rust hot path takes ~1 µs per
request; at 400K req/s that's 0.6 cores instead of 330." The review rejected the conclusion, not the measurement:

1. **The model excludes most of the work.** TLS record encryption and decryption, two to four system calls per request
   (read, write, sometimes epoll), the upstream connection's own I/O, and response-body copying are all outside
   `ch01-03`. Each is a separate measurement (Parts XIX and XXI), and some are larger than the application work.
2. **The model is single-threaded and warm.** Production runs 8+ workers per pod sharing caches, the allocator, and
   the NIC queues (Chapter 20.7).
3. **The Java figure is total CPU per request; the Rust figure is a slice.** Dividing one by the other is comparing a
   whole to a part.

What the team did instead:

- They kept `ch01-03`-style microbenchmarks for **components**, each with a budget in CI (HMAC ≤ 0.3 µs, head parse
  ≤ 0.3 µs, zero allocations per request after warm-up).
- They built a **load test** of the whole binary (open-loop, Chapter 20.2) on the production instance type with a
  recorded request mix, and measured CPU per request at 30%, 50%, and 70% utilization.
- They ran a **canary**: 2% of traffic to 3 Rust pods, comparing CPU per request and the p99.9 against Java pods in the
  same availability zone.

The canary is the only number in the capacity plan. (The rewrite's final numbers belong to the Part XX review and
Part XXI; this chapter's point is the method.)

### 10. Failure scenario

**The payments-core serializer swap (April 2026).** payments-core (Chapter 8.4) builds a JSON response per charge. An
engineer replaced the serializer with a faster library after a microbenchmark: 10,000 serializations of one response,
single thread, mean time 2.1× lower. The change shipped behind no flag.

Under production load, p99 latency rose from ~3 ms to ~11 ms within an hour, while mean latency improved slightly.
The postmortem found three gaps, each of which this Part's discipline would have caught:

- **Statistic.** The benchmark reported a mean. The new library kept a per-thread scratch buffer that it grew to the
  largest response it had seen and occasionally shrank (freeing and re-allocating a large block). Those rare calls were
  slow, invisible in a mean of 10,000, and 1–2% of production calls.
- **Workload.** One response, repeated: the scratch buffer never needed to grow after the first call. Production
  responses vary from 300 bytes to 40 KB.
- **Concurrency.** One thread. With 16 workers, the large blocks came from and went back to the allocator's shared
  arena, and the p99 of those calls rose further (Chapter 20.4's cross-thread cost).

The fix wasn't to revert the library but to configure a fixed-size scratch buffer, and to change the review rule:
**performance PRs must show a latency histogram (p50, p99, p99.9, max) for a recorded production mix, at production
concurrency, before and after.** That rule is the one Meridian's Part XVIII rule ("attach the right artifact") extends
to performance.

---

## Practice

### 11. Interview & architecture questions

1. A PR description says "the new parser is 3× faster". List the four things you need to know before that sentence
   means anything.
2. `ch01-01` reports 60.3 ns (wall/n), 29.0 ns (mean of samples), and 20 ns (p50) for the same pushes. Explain each
   number and say which one you'd put in a design doc.
3. Why is the p99 of a fleet not the average of the hosts' p99s? How do you compute it correctly?
4. State Amdahl's law. `ch01-02` measured 2.57× on 4 threads where Amdahl predicted 2.95×. Give two reasons real
   speedups fall short of the formula.
5. What is Little's law, and why does it mean you can't set throughput, latency, and concurrency independently?
6. Why do capacity plans target ~60% CPU utilization rather than 90%?
7. Compare "allocation rate" as a metric in a Java service and in a Rust service. What does reducing it buy you in each?
8. A dashboard shows RSS growing steadily for a Rust service. What two very different causes could produce that, and
   which additional metric tells them apart?
9. Your team wants a single number for "gateway performance" on the weekly dashboard. Argue for what it should be, and
   against what it shouldn't.
10. What's the difference between premature optimization and premature pessimization? Give one example of each from
    earlier Parts of this book.

### 12. Exercises

- **Beginner.** Run `ch01-01` with `Vec::with_capacity(n)` instead of `Vec::new()`. Predict what happens to p99.9 and
  max before you run it, then explain the result.
- **Intermediate.** Change `ch01-01` to time batches of 1,000 pushes instead of single pushes. What happens to the
  median per push, and why is batching the right way to time operations cheaper than the clock?
- **Advanced.** In `ch01-02`, make the serial part parallelizable (replace the dependent chain with an associative
  reduction) and re-measure. What's the new ceiling, and does the memory-bound job reach it?
- **Systems.** Extend `ch01-03` with a TLS record encryption step (use `ring`'s AES-GCM on the 638-byte head) and a
  `write` system call to `/dev/null`. Re-derive the "cores at 400K req/s" figure and say which component now dominates.
- **Architecture.** Write the one-page capacity-model template Meridian should require for any rewrite: metrics,
  workload source, measurement method, exclusions, and the canary plan.

### 13. Debugging exercise

A teammate's benchmark of a new rate limiter prints:

```text
mean latency: 41 ns   (1,000,000 calls, 1 thread)
p50: 40 ns   p99: 40 ns   max: 40 ns
```

The same limiter, in production, shows a p99 of 3 µs in its tracing spans. List at least three reasons the benchmark
and production disagree, say which experiment would confirm each one, and point out the line of the output that
should have made the teammate suspicious immediately.

### 14. Design exercise

Design the **performance contract** for gateway-core, the library at the heart of the Rust gateway: which metrics it
commits to (per component and end to end), at what workload, how each is measured in CI and in production, what budget
triggers a failing build, and who can approve an exception. Keep it to a page. Chapter 20.2 will give you the harness;
Chapters 20.3–20.4 the profiling and allocation tools.
