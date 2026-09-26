# Chapter 20.7 — Contention, NUMA, and Tail Latency

> **Where this sits:** Part XX · Performance Engineering · chapter 7 of 7 (the review follows)
> **Prerequisites:** Chapter 20.1 (Little's law, utilization), Chapter 11.7 (the measured ranking of sharing
> strategies), Chapter 14.4 (false sharing), Chapter 12.6 (long polls stall a worker), Project L4 (Ferrite v1's
> `ShardedStore`).
> **After this chapter you can:** predict how latency grows with utilization and explain why capacity plans keep
> headroom; compute the tail amplification of a fan-out and decide when hedged requests pay off; measure lock contention
> as wait time rather than hold time; choose between `Mutex`, `RwLock`, and immutable sharing from measurements; reason
> about core-to-core distance, pinning, and NUMA; and turn a p99 target into design constraints.

---

## Pass 1 · User level — *Why the tail is where systems fail*

### 1. Problem

Most of this Part so far has been about making one operation cheaper. This chapter is about what happens when many
operations compete: for a CPU, a lock, a cache line, a backend. That competition barely shows in averages and dominates
the tail, and the tail is what users and SLOs see. A gateway request that touches 30 services is as slow as the slowest
of the 30; a lock held by one thread for 2 µs costs every waiting thread 2 µs each; a CPU at 90% busy makes a 100 µs
operation wait for milliseconds.

Four questions organize the chapter, each with a simulation or measurement:

1. How does latency grow as utilization rises? (`ch07-01`, a queueing simulation)
2. How does fan-out amplify rare slowness, and what do hedged requests buy? (`ch07-02`)
3. What does lock contention cost, and how do you see it? (`ch07-03`, and Ferrite's shards in `ch07-05`)
4. How far apart are two cores, and what does placement change? (`ch07-04`)

### 2. Mental model

```text
 Little's law      L = λ · W        requests in the system = arrival rate × time each spends in it
 M/M/1 queue       W = S / (1 − ρ)  mean time in system, service time S, utilization ρ (random arrivals and services)
                                    ρ = 0.5 → 2S,  0.8 → 5S,  0.9 → 10S,  0.99 → 100S (and the p99 is several times more)
 Fan-out           P(slow) = 1 − (1 − p)^N     N calls, each slow with probability p
                                    p = 1%: N = 10 → 9.6%,  N = 50 → 39.5%,  N = 100 → 63.4%
 Contention        cost = wait time, not hold time; wait grows with (holders × hold time × arrival rate)
 Distance          moving a cache line between cores costs tens of ns; between sockets, more
```

Two named results help in reviews. **Kingman's formula** (1961) extends the queueing picture beyond random arrivals:
waiting time grows with utilization/(1 − utilization) *and* with the variability of arrivals and service times, so
smoothing either one (batching, admission control, bounded work per request) shortens queues. And Neil Gunther's
**Universal Scalability Law** models throughput on N workers with two penalties: contention (the serial part, Amdahl's
f) and coherence (the cost of keeping shared state consistent, which grows with N²), which is why some systems get
*slower* as you add cores.

### 3. Rust code

**The utilization cliff** (listing `ch07-01-queueing.rs`). A single-server FIFO queue with random (exponential) arrival
and service times, mean service time 100 µs, simulated in virtual time with a fixed seed:

```rust,ignore
// excerpt of ch07-01-queueing.rs
for _ in 0..n {
    t_arrive += rng.exp(arrival_mean_us);
    let start = t_arrive.max(server_free);
    let done = start + rng.exp(service_mean_us);
    server_free = done;
    let w = done - t_arrive; // time in system: queueing + service
    sum_w += w;
    h.record(w.max(1.0) as u64).unwrap();
}
```

```text
 util   p50 (us)   p99 (us)      p99.9  mean W (us)  theory W (us)
  50%        137        904       1341          198            200   L = 0.99, lambda*W = 0.99
  70%        226       1473       2131          325            333   L = 2.27, lambda*W = 2.27
  80%        337       2183       3339          482            500   L = 3.84, lambda*W = 3.84
  90%        659       4319       6627          942           1000   L = 8.45, lambda*W = 8.45
  95%       1290       8091       9951         1820           2000   L = 17.24, lambda*W = 17.24
  99%       5811      30847      34751         8058          10000   L = 79.54, lambda*W = 79.55
```

The mean follows the M/M/1 formula (within sampling error; the 99% row hasn't converged in 400,000 customers). The
point is the p99 column: **9× the service time at 50% utilization, 22× at 80%, 43× at 90%, 308× at 99%.** Nothing about
the work changed. Only the queue did. And Little's law holds to the second decimal in every row, because it isn't a
model: it's an identity about any system that's in steady state.

**Fan-out and hedging** (listing `ch07-02-tail-at-scale.rs`). Each backend call takes 1–3 ms, except 1% of calls that
take 50–60 ms. A request fans out to N backends and waits for all of them. The hedged version sends one backup call to
any backend that hasn't answered within 3 ms and takes whichever answer comes first:

```text
fan-out  p50 (ms)  p99 (ms)   slow reqs     1-0.99^N   hedged p99 / extra calls
      1       2.0      50.1        1.0%         1.0%         4.0 ms /  1.02%
     10       2.9      59.0        9.5%         9.6%         5.8 ms /  0.99%
     50       3.0      59.8       39.4%        39.5%         6.0 ms /  0.99%
    100      53.1      59.9       63.3%        63.4%        50.2 ms /  1.00%
```

With 50 backends, 39% of requests hit at least one slow call, exactly `1 − 0.99^50`: a 1% problem per backend became
a 39% problem per request, and at 100 backends the *median* request is slow. Hedging after 3 ms (about the backends'
p99 of fast calls) cuts the p99 from ~60 ms to ~6 ms for fan-outs up to 50, for **1% more backend calls**. At fan-out
100 it stops being enough: with 100 hedges per request, the chance that some hedge is also slow (1% of 1% × 100 ≈ 1%)
puts the p99 back at 50 ms. Jeff Dean and Luiz André Barroso's *The Tail at Scale* (Communications of the ACM, 2013)
described this effect and these remedies at Google's scale; the simulation reproduces the arithmetic.

**Contention is waiting** (listing `ch07-03-lock-wait-hold.rs`). Each thread takes a lock, does ~0.2 µs of work inside
it and ~1 µs outside, 20,000 times. Measured per acquisition: time spent *waiting* for the lock, and time *holding* it:

```text
setup                   hold p50  wait p50  wait p99  wait max    M ops/s
1 thread(s), 1 lock(s)       180        30        30       150       1.08
2 thread(s), 1 lock(s)       180        50        60     30223       2.12
4 thread(s), 1 lock(s)       180       200       240     46175       3.44
4 thread(s), 16 lock(s)      180        50        60     32895       4.20
(ns; hold/wait measured with Instant, ~20-25 ns per reading on this machine: see ch02-04)
```

The hold time is the same 180 ns in every row: the critical section didn't get slower. The wait grew from ~30 ns (the
cost of the uncontended acquisition plus the clock) to 200–240 ns at four threads on one lock, more than the hold time
itself, and throughput scaled 3.2× on four threads instead of 4×. Sharding the same work over 16 locks brought the wait
back to ~50 ns and throughput to 4.2M ops/s. The maxima (30–46 µs) are a thread that was descheduled while holding or
waiting: the lock turns one thread's preemption into every waiter's latency. **Profiles of hold time don't show
contention; wait-time histograms do.**

A second run is a useful reminder of what "one run, noisy" means. The hold times were identical and the four-thread
single-lock **p99** wait was the same 240 ns, but the p50 wait was 50 ns, the maximum reached 2 ms, and sharding gave
only 3.16 vs 3.02M ops/s. With the critical section a fifth of each iteration, four threads only sometimes collide, so
the median wait is sensitive to how the vCPUs were scheduled that run, while the tail isn't. That's another reason to
read contention in percentiles: the p99 was stable across runs; the median and the throughput gain weren't.

**Ferrite's shards: lock type vs value type** (listing `ch07-05-ferrite-shards.rs`, Project L4's open question). 16
shards, 4,096 keys, 4 KiB values, 95% GET / 5% SET. Ferrite v1's `get` returns an owned copy, so a GET copies 4 KiB
while holding the shard lock:

```text
M ops/s, 95% GET of 4 KiB values:
  1 thread(s):  Mutex+Vec   3.03   RwLock+Vec   3.38   RwLock+Arc<[u8]>   8.44
  4 thread(s):  Mutex+Vec  10.19   RwLock+Vec  11.27   RwLock+Arc<[u8]>  19.15
```

Switching the shard lock from `Mutex` to `RwLock` bought 10%. Changing what's stored (an `Arc<[u8]>`, so a GET clones a
pointer under the lock and the value is read after the lock is released) bought 1.9× on four threads and 2.8× on one.
The copy, not the lock, was the cost: on one thread there's no contention at all, and the `Arc` version is still 2.8×
faster. With the copy gone, each critical section is a hash lookup and a reference-count increment, short enough that
16 shards rarely collide.

---

## Pass 2 · Systems level — *Where the waiting happens*

### 4. Under the hood

**Why queues explode.** [math, not a layer] Variability, not load alone, creates queues: if requests arrived exactly
every 111 µs and each took exactly 100 µs (90% utilization), nobody would ever wait. Random arrivals bunch up, and while
a bunch is served, later arrivals wait. As utilization approaches 1, the server never catches up between bunches. That
is Kingman's formula in words, and it's why the p99 in `ch07-01` grows faster than the mean: the tail is the unlucky
bunches.

**What a contended `std::sync::Mutex` does.** [LIB][OS] On Linux, std's `Mutex` is a futex-based lock: an uncontended
lock and unlock are one atomic operation each on a word in the mutex. When the lock is taken, a waiter spins briefly
(a bounded number of iterations, [LIB] detail that can change between versions), then sleeps in the kernel with
`futex(FUTEX_WAIT)`; the unlocking thread wakes one waiter with `futex(FUTEX_WAKE)`. So a short wait costs a few tens of
nanoseconds of spinning and a long one costs two system calls and a scheduler wake-up (microseconds). A **lock convoy**
forms when the holder is preempted: every thread that arrives queues behind it, and they're woken one at a time.

**Why `RwLock` barely helped.** [LIB][CPU] Readers don't exclude each other, but each read acquisition still *writes*
the lock's state word (to count readers) and each release writes it again. With four threads reading the same shard,
that word's cache line moves between cores on every GET, the false-sharing cost of Chapter 20.5 on a line that is
genuinely shared. `RwLock` pays off when critical sections are long relative to that transfer (tens of nanoseconds); a
4 KiB copy is in between, which is why it bought 10%. Part XIV's risk-limits service hit the same wall and moved to
immutable snapshots behind `ArcSwap` (Chapter 14.3), where readers don't write any shared line at all.

**Fan-out is a maximum.** [math] A request that waits for N independent calls takes max(T₁ … T_N). The probability
that the maximum exceeds a threshold is 1 − P(each call is under it)^N, which is why per-backend p99s must be much
tighter than the request's p99 in a fan-out architecture: to keep a 50-way fan-out's p99 at the backends' p99 you need
each backend's p99.98.

### 5. Memory

**The value representation decides what the lock protects.** `ch07-05`'s lesson generalizes: a lock should protect a
*decision* (which value is current), not a *copy* (moving the bytes). Storing values behind `Arc` (or `Bytes`) makes the
critical section a pointer clone; the reader then reads the bytes without any lock, and the writer builds the new value
before taking the lock (`Arc::from(v)` outside the guard in `ch07-05`'s `put`). The cost is an allocation per write and
reference-count traffic per read, both measured cheaper than the copy here.

**NUMA.** [CPU][OS] A server with two (or more) sockets has memory attached to each socket; a core reaches its own
socket's memory faster than the other socket's (a remote access typically costs on the order of 1.5–2× a local one on
two-socket x86 servers, as vendors and measurement papers report). Linux allocates physical memory on the node of the
thread that first touches it ("first touch"), so a buffer initialized by one thread and used by threads on another
socket pays remote latency on every miss. The Playground is a single NUMA node (`ch07-04` below reports node `0` only),
so none of that can be measured here. On your own hardware, not verified here:

```text
numactl --hardware                        # nodes, CPUs per node, distances
numastat -p $(pidof service)              # the process's memory per node
numactl --cpunodebind=0 --membind=0 ./service   # confine a process to one node
```

For a latency-sensitive service on a multi-socket host, the common answers are one process per socket (each pinned to
its node) or sizing instances so each one fits on one socket; both make first-touch placement automatic.

### 6. CPU / OS

**Core-to-core distance** (listing `ch07-04-core-to-core.rs`). Two threads bounce one cache line (ping-pong on an
atomic), pinned to chosen CPUs with `sched_setaffinity`:

```text
NUMA nodes online: 0
cpu0: core_id 0, package 0, SMT siblings 0
cpu1: core_id 1, package 0, SMT siblings 1
cpu2: core_id 2, package 0, SMT siblings 2
cpu3: core_id 3, package 0, SMT siblings 3
main thread starts on cpu 3
round trip, not pinned:          60.8 ns
round trip, cpu0 <-> cpu1:        56.2 ns
round trip, cpu0 <-> cpu2:        66.9 ns
round trip, cpu0 <-> cpu3:        70.4 ns
```

A cache-line round trip between two cores costs ~56–70 ns here: a line transfer each way. That's the unit cost behind
every contended atomic (Chapter 11.7's 9–19 ns per shared `fetch_add` is several threads sharing that cost), every lock
hand-off, and `ch05-06`'s slowed readers. The pairs differ by up to 25% even on one socket, because cores sit at
different distances on the chip's interconnect (on AMD's chiplet designs, cores in different core complexes don't share
an L3); these are virtual CPUs, so which physical cores they map to is the hypervisor's choice. On two sockets, the
cross-socket round trip is several times larger (attributed; not measurable here).

**Pinning and thread-per-core.** [OS] Pinning threads to cores removes migrations (a thread moved to another core leaves
its cache behind) and lets you place communicating threads close together. Taken to its conclusion, it's the
**thread-per-core** architecture: one thread per core, each owning a shard of the data and its own event loop, with no
shared mutable state and no work stealing (ScyllaDB's Seastar framework is the best-known example; Glommio and monoio
bring the model to Rust). It removes contention by construction and gives up load balancing: a hot shard stays on its
core. Tokio's work-stealing scheduler (Part XIII) makes the opposite trade.

**Spinning, `pause`, and backoff** (Chapter 14.4's promise). [CPU] A spin-wait loop should use
`std::hint::spin_loop()` (the `pause` instruction on x86, `yield`/`isb`-style hints on Arm) so the core yields
execution resources to its SMT sibling and doesn't flood the memory system with speculative loads; `ch07-04`'s
ping-pong uses it. How long to spin before parking, and how fast to back off, depends on the CPU (`pause` latency changed
by an order of magnitude between Intel generations, per Intel's optimization manuals) and on the hold times you expect.
The effect of those settings is **predicted from mechanism and not measured here**; the exercise measures it.

**On Arm** (Chapter 14.4's Graviton counters, 14.x's ordering costs). Contended atomics and cache-line transfers have
different costs on Graviton's Neoverse cores than on this x86 machine, and Acquire/Release orderings compile to
different instructions (`ldar`/`stlr`). None of it is measurable on the Playground (x86-64 only); the measurement plan is
to run `ch07-03`, `ch07-04`, and Chapter 11.7's `ch07-01` on a Graviton instance and compare rankings, not absolute numbers.

---

## Pass 3 · Architect level — *Designing for the tail*

### 7. Trade-offs

| Problem | Remedies | What they cost | Evidence |
|---|---|---|---|
| Latency rises with load | Headroom (target 50–70%), autoscaling with lead time, admission control and load shedding (Chapter 13.5) | Idle capacity; rejected requests | `ch07-01`: p99 22× S at 80%, 43× at 90% |
| Fan-out amplifies rare slowness | Hedged requests after ~p95, tied requests, tighter per-backend SLOs, fewer backends per request | Extra load (≈ hedge rate); cancellation plumbing | `ch07-02`: p99 60 → 6 ms for 1% extra calls |
| Lock contention | Shorten critical sections (copy outside), shard, per-thread + merge, immutable snapshots (`ArcSwap`), owner thread | Complexity; staleness for snapshots | `ch07-03`, `ch07-05`, Chapter 11.7 |
| Coherence traffic on shared lines | Single-writer data, padding, per-core counters | Memory; merge cost | `ch05-06`, Chapter 14.4 |
| Preemption of lock holders | Fewer threads than cores, no blocking under locks, `spawn_blocking` for slow work | Lower peak parallelism | `ch07-03`'s maxima; Chapter 11.7's audit-under-lock incident |
| Topology and NUMA | Pin communicating threads together; one process per socket; thread-per-core | Load imbalance; operational complexity | `ch07-04` |

**Hedging isn't free, and it isn't always safe.** A hedge duplicates work, so it's only for idempotent reads (or writes
with idempotency keys, Chapter 8.4), it needs a cap (the "extra calls" column must stay a small percentage, or the hedges
themselves raise utilization and the tail with it), and it should cancel the loser.

### 8. Java comparison

| | Java | Rust |
|---|---|---|
| Striped counters | `LongAdder` (per-cell counters, summed on read) | Per-worker `CachePadded` counters (Chapter 14.4), merge on scrape |
| Optimistic reads | `StampedLock.tryOptimisticRead()` + `validate(stamp)` | A seqlock (Chapter 14.4; `ch03-02`'s sampler uses one) or `ArcSwap` snapshots |
| Read-mostly maps | `ConcurrentHashMap` (lock-free reads) | Sharded `RwLock<HashMap>`, `ArcSwap<HashMap>` for rarely changing maps, `Arc` values |
| Contention diagnostics | JFR `JavaMonitorEnter` / `ThreadPark` events, async-profiler lock mode | Wait-time histograms (`ch07-03`), `perf lock`, off-CPU profiles (Chapter 20.3) |
| NUMA | `-XX:+UseNUMA` (NUMA-aware allocation for some collectors) | First-touch placement; `numactl`; per-socket processes |
| Tail sources | GC pauses, safepoints, deoptimization, lock inflation, virtual-thread pinning | Queueing, lock convoys, blocking in async tasks (Chapter 12.6's 143 ms stall), deferred drops (Chapter 3.1) |

`StampedLock` deserves its own note (Chapter 11.3 promised it). Its optimistic read is a seqlock: read a version
("stamp"), read the fields, then check the version hasn't changed; readers write nothing shared, which is exactly what
`RwLock` readers can't avoid (§4). The Rust equivalent needs the same care Chapter 14.4 described: the reads of the
fields must be atomic (or the data race is undefined behaviour in Rust, where Java's memory model merely allows a stale
value), which is why Rust code usually reaches for `ArcSwap` snapshots instead unless the data is a few words.

> **Analogy limit.** Java's tail is often dominated by the runtime (GC and safepoints), so Java teams learn to tune the
> JVM first. A Rust service has no runtime pauses, so the tail you see is your queueing, your locks, and your fan-outs.
> The remedies in §7 are the same in both languages; in Rust they're usually the *only* remedies.

### 9. Production scenario

**Hedging the merchant-portal statement pages.** The statement page (Chapter 11.6's incident) fetches 30–50 documents
from a document service whose calls take 20–200 ms, with a slow tail when a document isn't cached. After Chapter 11.6's
fix (async fetches with a concurrency limit), the page's p99 was still dominated by fan-out: with ~40 fetches per page
and 1% of fetches slow, `1 − 0.99^40 ≈ 33%` of pages hit at least one slow fetch.

The team applied `ch07-02`'s remedy with the service's real distribution: a hedge for any fetch not finished by the
document service's p95, at most one hedge per fetch, a global hedge budget of 5% of fetches (hedging stops if the
budget is exhausted, so a slow document service doesn't get double the load at the worst moment), and cancellation of
the losing request. Document fetches are idempotent reads, so duplicates are harmless. They measured two numbers after
rollout, as `ch07-02` does: the page p99 and the extra-call rate.

**Ferrite v2's value type.** `ch07-05` is the measurement behind a design question Project L4 left open and Part XIII's
Ferrite v2 inherits: storing values as `Arc<[u8]>` (or `Bytes`) makes a GET a pointer clone under the shard lock,
1.9–2.8× faster than copying 4 KiB under it, and lets an async server release the lock before any `.await`. The lock type
(`Mutex` vs `RwLock`) turned out to be a second-order choice.

### 10. Failure scenario

**The 85% utilization target (2026).** To cut infrastructure cost, a planning exercise raised the gateway pool's target
CPU utilization from 60% to 85%, reasoning from the mean: at 85%, mean request latency was predicted (and measured in a
test) to rise by only ~20%, well inside the SLO.

Two weeks later, the daily peak produced p99 latencies 3–4× the old ones, and an availability-zone failover (losing a
third of the pods for 15 minutes) pushed the survivors above 95% busy: queues grew faster than they drained, upstream
timeouts fired, retries added load, and the gateway shed 8% of requests until the zone returned. The postmortem used
`ch07-01`'s table to explain it: the mean grows as 1/(1 − ρ), but the p99 was already 22× the service time at 80% and
81× at 95%, and the failover arithmetic (each surviving pod takes 1.5× its load) turns 85% into more than 100%.

The fix wasn't to go back to 60% blindly, but to size for failure: target utilization such that **the pool after losing
one zone** stays under 75% at peak, with autoscaling triggered on queue depth and p99 rather than on mean CPU, and load
shedding (Chapter 13.5) so that overload degrades to rejected requests instead of a queueing collapse.

---

## Practice

### 11. Interview & architecture questions

1. State Little's law and use it: a service handles 20,000 req/s with a mean latency of 15 ms. How many requests are in
   flight on average? What does that imply for its connection pool?
2. Why does latency rise sharply before utilization reaches 100%? What is Kingman's insight about variability?
3. A request fans out to 40 backends, each with a p99 of 50 ms. What fraction of requests see at least one 50 ms+ call?
   What per-backend percentile must you control to keep the request's p99 at 50 ms?
4. What are hedged requests, when are they safe, and how do you keep them from making overload worse?
5. Why is lock contention invisible in a CPU profile of hold times? What do you measure instead?
6. In `ch07-05`, why did `RwLock` help by only 10% while `Arc<[u8]>` values helped by ~2×?
7. What does a core-to-core round trip cost, and which Rust operations pay it?
8. What is NUMA first-touch placement, and how can it hurt a service that initializes buffers on one thread?
9. Compare thread-per-core with a work-stealing runtime. What does each give up?
10. How do Java's `StampedLock` optimistic reads map to Rust, and why is the Rust version harder to write correctly?

### 12. Exercises

- **Beginner.** In `ch07-01`, replace exponential service times with a constant 100 µs (M/D/1). How much do p50 and p99
  change at 90%? Relate the result to Kingman's formula.
- **Intermediate.** Add a "tied request" variant to `ch07-02`: send two copies of every call and cancel the loser when
  one finishes. Compare its p99 and extra load with hedging.
- **Advanced.** Add a spin-then-park lock with configurable spin iterations (with and without `spin_loop()`) to
  `ch07-03`, and measure wait p99 and throughput at 2 and 4 threads for 0, 100, and 10,000 spins.
- **Systems.** Run `ch07-04` with three threads passing the line around a ring (0 → 1 → 2 → 0) and with the threads
  unpinned. How do round trips and `getrusage` context switches change?
- **Architecture.** For Ferrite v2 (Part XIII), write the performance section of the design: value representation,
  shard count, lock type, and the three benchmarks (from this chapter's listings) that justify each choice.

### 13. Debugging exercise

A service's p99 is 4 ms at 1,000 req/s and 40 ms at 1,300 req/s, with CPU at 65% in both cases. Its flame graph
(on-CPU) looks the same at both rates. List the three most likely causes of a latency cliff that CPU and on-CPU profiles
can't see, the measurement that confirms each, and the design change you'd expect for each.

### 14. Design exercise

Design the **tail-latency budget** for a payments-core charge request (gateway → payments-core → fraud score →
processor, Chapter 8.4) with an end-to-end p99 target of 800 ms: allocate a per-hop latency budget, decide where to hedge
and where hedging is forbidden (and why), set utilization targets per tier using `ch07-01`'s table, and define the
dashboards and alerts (queue depth, wait time, p99 per hop) that tell you which hop is spending the budget.
