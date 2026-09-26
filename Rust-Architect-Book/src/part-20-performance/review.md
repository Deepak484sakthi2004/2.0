# Part XX Review — The Router Cache PR & Interview Mode

> Consolidate Part XX, then use it: review a performance PR whose own benchmark claims a 6× speedup, and answer
> senior-level questions without notes. Answers are in **Appendix A, Part XX**.

---

## Part XX on one page

```text
 A CLAIM              metric (latency, throughput, tail, CPU, memory, allocation rate, contention) + statistic (p50,
                      p99, p99.9, max; never a lone mean) + workload (production mix, production concurrency) +
                      comparison (interleaved, many samples, spread, verdict that respects noise)
        │
 BENCHMARK LIES       dead code · closed forms · cold start (page faults: 83×) · the clock (~25 ns) · order ·
                      LAYOUT (same asm, 1.7×) · closed-loop load (coordinated omission) · happy path only · noise
        │
 WHERE TIME GOES      sampling profiles (on-CPU vs off-CPU vs wall-clock) · flame graphs: width only · user vs system
                      (getrusage) · inlined frames need debug info · merged functions mislabel leaves · PGO/BOLT
        │
 WHERE MEMORY GOES    allocation site × count × bytes × lifetime · malloc ~11 ns local, ~118 ns freed cross-thread ·
                      live heap ≠ RSS (fragmentation, retention, trim) · zeroing ~2× small buffers
        │
 THE MACHINE          L1 1.6 ns · L2 ~5 · L3 ~16 · DRAM ~130–170 (this host) · misprediction ~6 ns · cache-line
                      round trip 56–70 ns · false sharing slows readers too · huge pages: requested ≠ granted
        │
 SIMD                 baseline x86-64 = SSE2 · vectorization needs noalias, a chosen FP order, no early exit ·
                      dispatch at run time (AVX2 7× from a baseline binary) · always a scalar reference + diff test
        │
 THE TAIL             p99 = 9× S at 50% busy, 22× at 80%, 43× at 90% · fan-out: 1 − (1 − p)^N · hedge at ~p95 for
                      ~1% extra · contention = WAIT time · the value type decides what a lock protects
```

## Ten ideas to carry forward

1. **A performance claim without a metric, statistic, workload, and comparison is not a claim.** Ask for all four.
2. **The tail is a different mechanism from the median.** Reallocations, page faults, trims, lock convoys, and slow
   backends don't show in means; histograms show them.
3. **The compiler is the first thing your benchmark measures.** `black_box` inputs and outputs, check the asm, and
   sanity-check the result against physics (bytes per second, cycles per operation).
4. **Identical instructions can run 1.7× apart.** Layout bias is real; a small difference after an unrelated code change
   is probably placement.
5. **Measure open-loop, from the intended send time.** Closed-loop load tests hide exactly the stalls you care about.
6. **Choose the profile by the question.** On-CPU for "what burns cores", wall-clock for "why is this request slow",
   off-CPU for "what are we waiting on".
7. **In Rust, allocation costs at the call site and fragmentation lasts.** Reuse, arenas, indices, and sharing (`Arc`)
   come before swapping the allocator.
8. **Data layout follows access pattern**, and most systems have two patterns: keep records together for the hot path
   and give the scans a snapshot.
9. **Build for the baseline, dispatch above it, and test every SIMD path against a scalar reference.**
10. **Size for the tail and for failure.** Utilization targets come from queueing, fan-out budgets from `1 − (1 − p)^N`,
    and capacity from the pool that survives losing a zone.

---

## Capstone: review the router cache PR

The Rust gateway's router maps a request's path to a route ID on every request (Chapter 20.1's hot-path model; about
1,000 routes). A PR to gateway-core says:

> **PR #2291 — router: add a last-hit cache in front of the route map. 2x faster lookups.**
> Most traffic goes to a handful of routes, so remembering the last lookup skips the hash map most of the time. The
> benchmark below shows 6x on my machine; I've written "2x" to be conservative. No functional change.

Here are the PR's code and its benchmark (listing `review-01-route-pr-bench.rs`, verified):

```rust,ignore
// excerpt of review-01-route-pr-bench.rs
/// The router on main today.
pub struct Router {
    routes: HashMap<String, u32>,
}

impl Router {
    pub fn lookup(&self, path: &str) -> Option<u32> {
        self.routes.get(&path.to_string()).copied()
    }
}

/// The PR: remember the last path looked up and its answer.
pub struct CachedRouter {
    inner: Router,
    last: Mutex<Option<(String, Option<u32>)>>,
}

impl CachedRouter {
    pub fn lookup(&self, path: &str) -> Option<u32> {
        let mut last = self.last.lock().unwrap();
        if let Some((p, id)) = last.as_ref() {
            if p == path {
                return *id; // cache hit
            }
        }
        let id = self.inner.lookup(path);
        *last = Some((path.to_string(), id));
        id
    }
}

fn main() {
    // ... build `old` and `new` over the same 1,000 routes ...
    let t = Instant::now();
    for _ in 0..1_000_000 {
        old.lookup("/v1/svc0001/items");
    }
    let a = t.elapsed();

    let t = Instant::now();
    for _ in 0..1_000_000 {
        new.lookup("/v1/svc0001/items");
    }
    let b = t.elapsed();
    // ... print both and the speedup ...
}
```

The PR's benchmark output, from the Playground:

```text
old router:    26.347051ms for 1M lookups
cached router: 4.417963ms for 1M lookups
speedup: 6.0x
```

**Your task.** Write the review. Cover three things, each with the chapter that justifies it:

1. **The benchmark.** Every reason its 6.0× might not describe production (there are at least six).
2. **The code.** Every performance and design problem in `CachedRouter` under the gateway's real conditions: 16 worker
   threads sharing one router, ~1,000 routes, a request mix with a few hot routes and a long tail, 2% unknown paths
   (at least five problems, including one on main that the PR inherits).
3. **What you'd ask the author to do instead**: the smallest change that captures whatever real win exists, and the
   evidence the PR should carry.

Then run listing `review-02-route-bench-fixed.rs` (verified: a release run and a test), which is the benchmark the
review should have asked for, and compare its output with your predictions. The model review, the corrected benchmark's
output, and its discussion are in the answer key.

---

## Interview mode

Answer aloud, without notes, in two to four minutes each. Model answers are in Appendix A, Part XX.

### Measurement

1. A PR says "3× faster". Walk through the questions you ask before you believe it, in order.
2. Explain why the p99 of a fleet isn't the average of the hosts' p99s, and how HdrHistogram lets you compute it.
3. What is coordinated omission? Describe a load test that has it and one that doesn't, and what each reports for a
   2-second stall.

### Compiler and code generation

4. Name three ways the optimizer can make a benchmark measure nothing, and show how `black_box` placement addresses
   each. What does the std documentation promise about `black_box`?
5. Two functions compile to the same seven instructions and run 1.7× apart. How is that possible, and how do you
   protect a benchmark suite from it?
6. Why won't LLVM vectorize a plain `f64` sum, and why did `saxpy` over raw pointers need a runtime check that the slice
   version didn't?

### Hardware and OS

7. Walk me down the memory hierarchy with approximate latencies, and tell me how you'd measure them on a new machine.
8. What does a branch misprediction cost, and how would you make a hot loop immune to data-dependent branches?
9. Why did `ch05-06`'s readers slow down 4× when they never wrote anything?
10. What's the difference between live heap and RSS, and why can freeing memory leave RSS unchanged?

### Architecture

11. Your gateway runs at 60% CPU. Finance asks to run it at 85%. What do you tell them, with numbers?
12. A page fans out to 40 backends. The page's p99 is bad even though every backend meets its own p99. Explain, and
    design the fix, including what could go wrong with it.
13. When would you choose thread-per-core over a work-stealing runtime for a Rust service?
14. Design the performance-evidence policy for Meridian's Rust services: what CI gates, what nightly jobs inform, what
    a load test must show before a release, and what a canary must show before full rollout.

---

## Looking ahead: Part XXI

Part XXI takes the gateway the rest of the way: sockets and TCP behaviour, HTTP/1.1 and HTTP/2 framing, TLS, and the
timeouts, retries, circuit breakers, pools, and load balancers that sit between services. Every one of those has a
performance side (syscalls per request, handshake cost, head-of-line blocking, pool starvation, retry amplification),
and every one is measured with this Part's tools: open-loop load tests, latency histograms, `getrusage`, and the
utilization and fan-out arithmetic of Chapter 20.7.
