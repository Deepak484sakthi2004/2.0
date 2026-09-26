# Chapter 20.2 — Benchmarking Without Lying to Yourself

> **Where this sits:** Part XX · Performance Engineering · chapter 2 of 7
> **Prerequisites:** Chapter 20.1 (metrics and distributions), Chapter 1.3 (`black_box` and the verified "loop ==
> iterator chain" assembly), Chapter 8.3 (panic cost, measured once).
> **After this chapter you can:** recognize the eight standard ways a benchmark measures the wrong thing, and show each
> one happening; use `std::hint::black_box` correctly and know its limits; build a warm, interleaved, many-sample
> comparison with a verdict that respects the noise; load-test without coordinated omission; and benchmark failure paths
> at production failure rates.

---

## Pass 1 · User level — *A benchmark is an experiment*

### 1. Problem

A benchmark is an experiment: you change one thing, hold everything else constant, and observe an effect. Every way to
get that wrong has happened to a team you know. Here is what the optimizer, the operating system, the CPU, and the
benchmark author can each do to the result, all demonstrated on the Playground in this chapter:

| # | Lie | What happens | Listing |
|---|---|---|---|
| 1 | Dead code | The result is unused, so the compiler deletes the work | `ch02-01` |
| 2 | Constant folding | The input is known at compile time, so the compiler computes the answer in advance (or in closed form) | `ch02-01`, `ch02-02` |
| 3 | Cold start | The first run pays for page faults, cache misses, and branch-predictor training | `ch02-03`, `ch02-05` |
| 4 | The clock | Timing costs ~25 ns and can't resolve anything shorter | `ch02-04` |
| 5 | Order | Whatever runs first pays the cold-start costs for everyone | `ch02-05` |
| 6 | Code layout | The same instructions at different addresses run at different speeds | `ch02-09`, `ch02-11` |
| 7 | Closed-loop load | The load generator slows down when the server does, so the stall is never measured | `ch02-06` |
| 8 | The happy path | Only successes are benchmarked; failures cost differently and happen in production | `ch02-07` |

Noise on a shared machine (`ch02-08`) is the ninth, and it's why every comparison needs many samples and a spread.

### 2. Mental model

**Control every variable except the one you're testing, then ask whether the difference is larger than the noise.**
In practice that means five habits, which this chapter's harness (`ch02-09`) builds in:

1. **Make the work observable.** Pass inputs through `std::hint::black_box` so the compiler can't specialize on them,
   and pass results through it so it can't delete them.
2. **Warm up.** Run everything a few times untimed, so page faults, caches, and predictors reach steady state.
3. **Interleave candidates.** Run A, B, A, B … (rotating who goes first) rather than all of A then all of B, so drift
   hits both equally.
4. **Take many samples and report a spread.** Median plus p10..p90 (or min..max), never one number.
5. **Refuse small differences.** If the ranges overlap, the verdict is "no detectable difference", not "A is 3% faster".

And one rule for load tests: **send requests on a schedule, not when the previous one returns** (open loop), and measure
latency from the time the request *should* have been sent.

### 3. Rust code

**Lies 1 and 2: the optimizer deletes or precomputes the work** (listing `ch02-01-dce-const-fold.rs`, release):

```rust,ignore
// excerpt of ch02-01-dce-const-fold.rs
let a = ns_per_call(1_000, || {
    sum_slice(&v); // result unused
});
let b = ns_per_call(1_000, || {
    black_box(sum_slice(&v)); // result kept, input visible to the optimizer
});
let c = ns_per_call(1_000, || {
    black_box(sum_slice(black_box(&v))); // input and result opaque
});
```

```text
Summing 100,000 u64 (800 KB), ns per call:
  result discarded                            0.0
  black_box(result)                        8664.1
  black_box(input) and black_box(result)   8245.7
Summing the range 0..n, ns per call:
  n =          1000     1.21   (a loop of n additions would take ~n/4 ns or more)
  n =       1000000     1.20   (a loop of n additions would take ~n/4 ns or more)
  n =    1000000000     1.21   (a loop of n additions would take ~n/4 ns or more)
```

Summing 800 KB "took" 0.0 ns: the loop was deleted because nothing used its result. And summing the first billion
integers took the same 1.2 ns as summing the first thousand, because there is no loop in the compiled code at all. Here
is `sum_to` from `ch02-02-closed-form-asm.rs` (release assembly via `tools/emit.ps1`):

```text
playground::sum_to:
	test	rdi, rdi
	je	.LBB1_1
	lea	rax, [rdi - 1]
	lea	rcx, [rdi - 2]
	mul	rcx                  ; (n - 1) * (n - 2), 128-bit product
	shld	rdx, rax, 63         ; ... / 2
	lea	rax, [rdi + rdx]
	dec	rax                  ; + n - 1   = n(n-1)/2, computed without a loop
	ret
```

LLVM recognized the loop as an arithmetic series and replaced it with Gauss's formula. It does the same for a sum of
squares: `sum_squares_to` in the same listing compiles to a closed form with no loop, whose division by 3 is done by
multiplying with the constant `6148914691236517206` (0x5555555555555556), a division-by-multiplication trick like the
÷10,000 in Chapter 7.2. Neither function is "fast". The benchmark stopped measuring them.

**Lie 3: the first pass over memory is a different experiment** (listing `ch02-03-warmup-page-faults.rs`, release). A
freshly allocated 128 MiB buffer, touched one byte per 4 KiB page, three times:

```text
pass 1:   68.58 ms,  32770 minor page faults
pass 2:    0.82 ms,      0 minor page faults
pass 3:    0.57 ms,      0 minor page faults
pages in the buffer: 32768
```

Same code, same data, **83× slower** on the first pass: every page faulted in once (32,768 pages, 32,770 faults).
`vec![0u8; n]` doesn't touch memory; the kernel maps zero pages lazily and pays when you first write (Chapter 19.5 has
the mechanism).

**Lies 4 and 5: the clock, and whoever goes first** (listings `ch02-04-timer-cost.rs`, `ch02-05-order-bias.rs`):

```text
clocksource: tsc
Instant::now()     25.1 ns per call
SystemTime::now()  24.3 ns per call
back-to-back readings: smallest non-zero step 20 ns, 0 of 1,000,000 pairs read equal
```

```text
naive, A then B:  A  15.15 ms   B   1.42 ms   -> "B is 11x faster"
naive, B then A:  A   1.40 ms   B  15.04 ms   -> "A is 11x faster"
warm, interleaved: A median 1.41 ms [1.40..1.49]   B median 1.41 ms [1.40..1.53]
```

`variant_a` and `variant_b` in `ch02-05` are **identical functions**. Timed once each on a fresh buffer, whichever runs
first pays for the page faults and "loses" by 11×. Warm and interleaved, they're indistinguishable.

**The harness** (listing `ch02-09-harness.rs`). The book's stand-in for criterion (which isn't on the Playground): warm-up
rounds, then samples in which every candidate runs `iters` calls in rotating order, then median and p10..p90 per
candidate, and a verdict that only declares a winner when the ranges don't overlap:

```rust,ignore
// excerpt of ch02-09-harness.rs
fn compare(cands: &mut [(&str, &mut dyn FnMut())], warmup: u32, samples: usize, iters: u32) -> Vec<Stats> {
    for _ in 0..warmup {
        for (_, f) in cands.iter_mut() {
            f();
        }
    }
    let n = cands.len();
    let mut per: Vec<Vec<f64>> = vec![Vec::with_capacity(samples); n];
    for s in 0..samples {
        for k in 0..n {
            let i = (s + k) % n; // rotate who goes first
            let f = &mut cands[i].1;
            let t = Instant::now();
            for _ in 0..iters {
                f();
            }
            per[i].push(t.elapsed().as_nanos() as f64 / iters as f64);
        }
    }
    per.into_iter().map(stats).collect()
}

fn verdict(a: (&str, &Stats), b: (&str, &Stats)) -> String {
    if a.1.p90 < b.1.p10 {
        format!("{} is faster: {:.2}x at the median", a.0, b.1.median / a.1.median)
    } else if b.1.p90 < a.1.p10 {
        format!("{} is faster: {:.2}x at the median", b.0, a.1.median / b.1.median)
    } else {
        "no difference detectable at this noise level (the p10..p90 ranges overlap)".to_string()
    }
}
```

Its two built-in experiments, one run:

```text
sum of 100,000 u64 (ns per call)
  iter().sum()             median    8007.6 ns   p10..p90 [7949.0 .. 8469.5]   min 7925.6  max 8768.0
  index loop               median   13544.5 ns   p10..p90 [13524.5 .. 13992.0]   min 13523.0  max 15421.5
  verdict: iter().sum() is faster: 1.69x at the median
sort 10,000 u32 (ns per call, copy included in both)
  sort (stable)            median   97090.2 ns   p10..p90 [95290.4 .. 99144.4]   min 94892.2  max 105452.2
  sort_unstable            median   79398.2 ns   p10..p90 [78270.2 .. 81078.2]   min 77876.2  max 85138.2
  verdict: sort_unstable is faster: 1.22x at the median
```

The sort result is expected: `sort_unstable` avoids the stable sort's merge buffer. The sum result is not. Chapter 10.3
measured an index loop and an iterator chain at the same speed, and this harness says 1.69×, with ranges far apart and
the same result on a second run (1.76×). **That's the sixth lie, and the most unsettling one** (§4).

---

## Pass 2 · Systems level — *Why each lie happens*

### 4. Under the hood

**Dead-code elimination and `black_box`.** [RUSTC][LANG] LLVM deletes any computation whose result is unused and that
has no side effects. `std::hint::black_box(x)` returns `x` unchanged but tells the compiler to assume *anything* might
have happened to it: on 1.98.1 it's an empty inline-assembly statement that takes the value (Chapter 4.1 showed the
`#APP`/`#NO_APP` markers). The std documentation is explicit that it's a best-effort hint, not a guarantee: programs
must not rely on it for correctness, and a future compiler could see through it. Two placements matter:

- `black_box(result)` stops deletion of the work that produces `result`.
- `black_box(input)` stops the compiler from specializing on a known input, including computing the answer at compile
  time. `ch02-01`'s second row still ran the loop because the input was a runtime `Vec`; with a constant input it could
  have been folded.

**Closed forms.** [RUSTC] LLVM's scalar-evolution analysis models how loop variables change per iteration. For a loop
that adds an induction variable (0, 1, 2, …) it can compute the final value directly: Gauss for `sum_to`, a cubic for
`sum_squares_to`. Chapter 17.7 showed the same analysis from the compiler's side, in Part XVII's listing
`ch07-03-llvm-does-it.rs`: `triangle` becomes a formula, and `guarded_sum` (a loop that adds `b / a` when `a != 0`)
becomes constant time with at most one division, through loop versioning, loop-invariant code motion, and induction
simplification. That's a legitimate optimization in real code and a
disaster in a benchmark: timing `triangle(n)` for growing `n` shows a flat line, which is a measurement of one
multiplication. Benchmark inputs should come through `black_box` and be data, not ranges.

**Layout bias.** [CPU][RUSTC] The two sum closures in `ch02-09` compile to the same instructions. Here they are, pulled
out of the harness binary with `tools/emit.ps1 -CrateType bin` (release, hot loops only):

```text
playground::main::{closure#0}:            ; iter().sum()
.LBB20_6:
	movdqu	xmm2, xmmword ptr [rcx + 8*rsi]
	paddq	xmm1, xmm2
	movdqu	xmm2, xmmword ptr [rcx + 8*rsi + 16]
	paddq	xmm0, xmm2
	add	rsi, 4
	cmp	rdx, rsi
	jne	.LBB20_6

playground::main::{closure#1}:            ; index loop
.LBB23_6:
	movdqu	xmm2, xmmword ptr [rcx + rdi]
	paddq	xmm1, xmm2
	movdqu	xmm2, xmmword ptr [rcx + rdi + 16]
	paddq	xmm0, xmm2
	add	rdi, 32
	cmp	rsi, rdi
	jne	.LBB23_6
```

Seven instructions each, the same vectorized structure (two 128-bit accumulators, four `u64` per iteration); only the
addressing mode differs. As standalone functions (`ch02-10-sum-codegen.rs`) they're equally alike. So where does 1.69×
come from? Listing `ch02-11-layout-bias.rs` is `ch02-09` with **one change: the two closures are defined in the opposite
order**, which moves their machine code to different addresses:

```text
sum of 100,000 u64 (ns per call)          (ch02-11: same code, closures defined in the other order)
  iter().sum()             median   13552.0 ns   p10..p90 [13540.0 .. 13998.0]   min 13535.5  max 14049.5
  index loop               median   13576.0 ns   p10..p90 [13543.0 .. 14236.0]   min 13541.5  max 15103.0
  verdict: no difference detectable at this noise level (the p10..p90 ranges overlap)
```

Run twice each, the original order gave 1.76× and 1.60× in favour of `iter().sum()`, and the swapped order gave "no
difference" both times, with `iter().sum()` at the slower 13.5 µs. The instructions didn't change; **their placement
did**, and one placement was 1.7× faster. The likely mechanism is the CPU front end: how a hot loop falls relative to
instruction-fetch and micro-op-cache boundaries, and how its branches alias in the predictor's tables [CPU; the
Playground doesn't show final addresses, so this attribution is not verified here: `objdump -d` on the binary plus
`perf stat -e` front-end counters would confirm it locally]. Researchers documented this years ago: Mytkowicz, Diwan,
Hauswirth, and Sweeney (*Producing Wrong Data Without Doing Anything Obviously Wrong!*, ASPLOS 2009) showed that link
order and even the size of the environment variables could swing measured performance by more than the effect being
studied, and Curtsinger and Berger's STABILIZER (ASPLOS 2013) randomizes layout to make such comparisons statistically
sound. The practical consequence: **a benchmark that changes code and finds a 5–20% difference hasn't necessarily found
an effect of the change.** Check the generated code; if it's the same, you measured layout.

**Coordinated omission.** Gil Tene named this one (talks and the wrk2 load generator, from about 2013 onward). A
closed-loop load generator sends a request and waits for the reply before sending the next. When the server stalls, the
generator stalls with it, so it sends almost no requests during the stall and records almost no slow ones. Listing
`ch02-06-coordinated-omission.rs` simulates it deterministically: a server that answers in 1 ms except for one 2-second
freeze, and a test that intends to send a request every 2 ms for 100 s:

```text
closed loop (reply-paced client)             n= 49001  p50     1.0 ms  p99     1.0 ms  p99.9     1.0 ms  max  2001.9 ms
open loop (latency from intended send time)  n= 50000  p50     1.0 ms  p99  1501.2 ms  p99.9  1952.8 ms  max  2001.9 ms
closed loop + record_correct(interval)       n= 50000  p50     1.0 ms  p99  1001.5 ms  p99.9  1903.6 ms  max  2001.9 ms
requests the closed-loop client never sent during the stall: 999
```

The closed-loop test reports a p99 of **1.0 ms** for a system that, for real users arriving at 500 req/s, had a p99 of
**1.5 s**: during the 2-second freeze, 1,000 users were waiting, and the test sent one request. HdrHistogram's
`record_correct` back-fills the missing samples from the expected interval (1,001.5 ms), better but still an
approximation. The real fix is an open-loop generator (wrk2, k6's constant-arrival-rate executor, Gatling's open
injection profiles) that measures from the intended send time.

### 5. Memory

**Warm-up is about memory state, not only about "JIT".** Rust has no JIT, and `ch02-03` still shows an 83× first-pass
penalty: page faults. The same applies to allocator state (the first allocations of each size class grow the heap),
CPU caches, the TLB, and branch predictors. A benchmark should either measure steady state (warm up, then measure) or
measure the cold path deliberately (fresh process per sample), and say which.

**The allocator carries state between candidates.** If candidate A leaves the heap fragmented, candidate B pays for it.
Interleaving spreads that effect over both; so does resetting state (fresh buffers per sample, as `ch02-09`'s sort
experiment does by copying the input into a pre-allocated buffer inside both timed regions).

### 6. CPU / OS

**Noise.** [CPU][OS] Listing `ch02-08-noise.rs` runs the same 2M-element dependent loop 31 times:

```text
31 runs of the same 2M-element dependent loop (ms):
  first run 1.636   min 1.632   median 1.644   max 1.665
  spread (max - min) / median = 2%
  /proc/stat during the runs: 21 ticks total, 0 ticks of steal (time the hypervisor gave our vCPUs to someone else)
```

A 2% spread on a quiet run; Chapter 11.7 saw 5× between runs of a contended benchmark. Sources, roughly in order of
size on shared cloud machines: other tenants (memory bandwidth, shared L3, steal time), frequency scaling (turbo drops
when more cores are busy), interrupts, and timer resolution. On your own hardware you can reduce them (pin the process
with `taskset`, fix the frequency governor, disable turbo, isolate cores); on shared hardware you can only sample more
and compare ranges.

**Failure paths cost differently** (Part VIII's promise). Listing `ch02-07-failure-path.rs` compares `Result`
propagation with panic + `catch_unwind` at call depths 1, 10, and 100, at the failure rates production might see:

```text
ns per call (release, best of 5 runs of 20000 calls; one Playground run, noisy)
 depth  fail%       Result      panic+catch
     1     0%          3.5              2.7
     1     1%          4.1             20.2
     1    10%          3.8            180.9
    10     0%         18.6             12.4
    10     1%         18.7             43.0
    10    10%         18.8            319.4
   100     0%        206.7            117.6
   100     1%        207.4            272.2
   100    10%        207.0           1653.6
```

Three lessons, all invisible to a happy-path benchmark:

- **Unwinding is free until it happens.** [RUNTIME] With no failures, the panic version is *faster* (117.6 vs 206.7 ns at
  depth 100): the happy path has no checks at all, because the unwind tables and landing pads (Chapter 8.3) sit outside
  the executed code. `Result` pays a tag check and branch at every level, every time.
- **Unwinding is expensive when it happens:** about (180.9 − 2.7) / 0.1 ≈ 1.8 µs per failure at depth 1 and
  ≈ 15 µs at depth 100 (the unwinder walks and decodes each frame), against a few nanoseconds for an `Err`.
- **The break-even failure rate is low.** At depth 100, panics lose once more than ~0.6% of calls fail
  (117.6 + r × 15,360 = 206.7 → r ≈ 0.58%); at depth 10, above ~0.2%. Business errors (declines, validation failures,
  "not found") happen far more often than that, which is the performance half of Chapter 8.3's rule that panics are for
  bugs. The other half (panics are for bugs because they're bugs) matters more.

---

## Pass 3 · Architect level — *Benchmarks as part of the engineering process*

### 7. Trade-offs

| Kind of measurement | Good for | Weak at | Tooling |
|---|---|---|---|
| Microbenchmark (ns per call) | Comparing implementations of one component; catching regressions in hot functions | Anything involving the rest of the system; layout and noise can exceed small effects | criterion (locally), this chapter's harness, `divan` |
| Instruction-count benchmark | Stable CI numbers (no timing noise) | Doesn't see cache misses, stalls, or contention: can move opposite to wall time | iai-callgrind / cachegrind (Valgrind-based; not verified here) |
| Load test (whole binary) | Throughput, latency distribution, saturation point, CPU per request | Slow to run; needs a realistic request mix and an open-loop generator | wrk2, k6, Gatling, vegeta |
| Production canary | The only measurement of production | Takes time, needs safe rollout and comparison groups | Metrics, tracing, feature flags |

**Build-profile experiments are system benchmarks, not microbenchmarks.** Part II promised a measurement of
`opt-level = 2` vs `3`; Part XVIII's architecture exercise asked about release-profile choices (`lto`, `codegen-units`,
`panic`, `target-cpu`). The Playground offers only its fixed debug and release profiles, so these can't be verified here,
and more to the point, they shouldn't be judged by a microbenchmark: inlining and code-size changes move the whole
binary's instruction-cache behaviour. The method is an A/B load test of two binaries built from the same commit:

```text
# not verified here: requires a local toolchain and your service's load test
CARGO_PROFILE_RELEASE_OPT_LEVEL=2 cargo build --release && cp target/release/gateway gateway-o2
CARGO_PROFILE_RELEASE_OPT_LEVEL=3 cargo build --release && cp target/release/gateway gateway-o3
# run the same open-loop load test (same request mix, same rate) against each, 3+ times, interleaved
```

Report CPU per request and p99 at a fixed rate, with the spread across runs. Differences under ~3% are usually layout.

**Load curves** are the load-test version of a benchmark, and Project L3's HTTP server was promised one. The method, not
verified here (it needs wrk2 and a local build of the L3 server): run an open-loop generator at a series of fixed rates
and record the latency distribution at each.

```text
# wrk2 keeps a constant request rate (-R) and corrects for coordinated omission.
# PORT: the port you bind the L3 server to (its tests bind 127.0.0.1:0, an ephemeral port).
for rate in 1000 2000 5000 10000 15000 20000; do
  wrk2 -t4 -c64 -d30s -R$rate --latency http://127.0.0.1:PORT/health > l3-$rate.txt
done
```

Plot p50, p99, and p99.9 against the offered rate. The curve is flat, then bends sharply near saturation: the knee is
Chapter 20.7's utilization cliff, and the rate at the knee (not the maximum throughput) is the server's capacity for
planning purposes.

### 8. Java comparison

JMH (Aleksey Shipilëv's harness, part of OpenJDK) was built around the same lies, and its features map almost
one-to-one:

| Lie | JMH's answer | Rust answer |
|---|---|---|
| Dead code | `Blackhole.consume`, returning results from `@Benchmark` | `std::hint::black_box` on results |
| Constant folding | Inputs in `@State` fields, not constants | `black_box` on inputs |
| Warm-up / JIT | `@Warmup` iterations; JIT tiers make warm-up mandatory | Warm-up rounds (caches, page faults, predictors) |
| Run-to-run variance | `@Fork` several JVMs and aggregate (JIT profiles and layouts differ per fork) | Several process runs; layout effects (`ch02-11`) are the AOT analog |
| Timer overhead | Batch timing in `Throughput`/`AverageTime` modes | Time batches, not single calls (`ch01-01`) |
| Coordinated omission | Out of scope for JMH (it's a microbenchmark harness); use wrk2/Gatling | Same: open-loop load generators |

> **Analogy limit.** JMH forks exist largely because each JVM run can JIT-compile differently (different inlining
> decisions from different profiles). A Rust binary is compiled once, so repeated runs of the *same binary* vary only
> with the environment. But a *rebuild* after an unrelated change can move code and change timings (`ch02-11`), so the
> AOT world has its own run-to-run variance, just at build time rather than at run time.

### 9. Production scenario

**Benchmarks in Meridian's CI.** After the serializer incident (Chapter 20.1) and with Part XVIII's rule that
performance PRs attach the right artifact, gateway-core adopted a three-layer policy:

1. **Allocation counts and instruction counts gate merges.** These are deterministic. A test with the counting allocator
   asserts zero allocations per request on the hot path (Chapter 20.4). An instruction-count benchmark (cachegrind-based,
   run in CI's Linux container; not verified here) fails the build when a hot function's count grows by more than 2%.
   An **artifact-regression check** (Part XVIII's design exercise) diffs the release assembly of five hot functions
   (the `ch02-10` technique) and asks for a reviewer when it changes.
2. **Timing benchmarks inform, they don't gate.** criterion runs nightly on a dedicated, pinned runner; results are
   plotted with their confidence intervals. A change is investigated only when it persists across three nights and
   exceeds 5%.
3. **Load tests gate releases.** An open-loop load test at 60% of pod capacity with a recorded request mix, before and
   after, comparing CPU per request and p99/p99.9.

The policy is explicit about what each layer can't see: instruction counts miss cache effects, timing benchmarks have
layout noise, and load tests are slow. Together they cover each other's blind spots.

### 10. Failure scenario

**The market-data decoder that took 0.3 ns (2026).** A PR to the market-data ingest service (Chapter 8.2's fan-out
rewrite) claimed a new frame decoder was "40× faster": 0.3 ns per message, down from 12 ns. The benchmark decoded the
same frame in a loop and discarded the result. Reviewers approved it because the code was also simpler.

A week later, a profile of production showed decode time unchanged, and the new decoder's error path (which returned
`Box<dyn Error>` with a formatted message) was now allocating on every malformed frame from one flaky exchange feed.
The postmortem's first finding was the simplest: **0.3 ns is about one CPU cycle, and a decoder that reads a 24-byte
header and validates four fields can't run in one cycle.** A speed-of-light check (bytes touched ÷ bandwidth, or
instructions ÷ ~4 per cycle) would have caught it in review. The benchmark was measuring an empty loop.

The fix to the process was a benchmark template that passes inputs and results through `black_box`, includes one
malformed input per 1,000, and prints bytes processed per second next to ns per call, so that impossible numbers look
impossible.

---

## Practice

### 11. Interview & architecture questions

1. Name five ways a microbenchmark can measure something other than the code under test, and the countermeasure for each.
2. What does `std::hint::black_box` do on rustc 1.98, and why does the std documentation say you can't rely on it?
3. `ch02-01` summed the first billion integers in 1.2 ns. Explain what the compiler did and why it's legal.
4. Two identical functions measured 11× apart (`ch02-05`). Explain why, and design the experiment that shows they're
   equal.
5. What is layout bias? How would you tell whether a 7% improvement from a refactoring is real or a layout effect?
6. Explain coordinated omission to a colleague who has only used closed-loop load tools. What does an open-loop
   generator measure differently?
7. When is it cheaper to panic than to return `Err` on a hot path, and why is that still usually the wrong design?
8. Why should build-profile choices (opt-level, LTO, codegen-units) be evaluated with a load test rather than a
   microbenchmark?
9. What can an instruction-count benchmark catch that a timing benchmark can't, and vice versa?
10. Design the minimum evidence you'd require on a PR that claims a hot-path speedup.

### 12. Exercises

- **Beginner.** In `ch02-01`, make `sum_to` take its `n` from a `const` instead of `black_box(n)`. What does the
  benchmark report now, and what does the assembly show?
- **Intermediate.** Add a third candidate to `ch02-09`: `v.iter().fold(0, |a, &x| a + x)`. Run it several times and
  record which placements it gets. Is it faster, slower, or "the same with a different layout"?
- **Advanced.** Extend `ch02-06` with a load test whose inter-arrival times are random (Poisson) instead of fixed. How do
  the closed- and open-loop percentiles change?
- **Systems.** Measure `Instant::now()` inside a loop that's otherwise empty on your own machine with different
  clocksources (`tsc`, `hpet` if available; `/sys/devices/system/clocksource/`). What does a slow clocksource do to
  every benchmark on that machine?
- **Architecture.** Write the checklist a reviewer uses on a performance PR at Meridian. Every item should name the lie
  it prevents.

### 13. Debugging exercise

A teammate compares two JSON field extractors and posts:

```rust,ignore
// not a book listing: the teammate's benchmark, for you to review
let t = Instant::now();
for _ in 0..1_000_000 { extract_v1(r#"{"amount":12999,"currency":"EUR"}"#); }
println!("v1 {:?}", t.elapsed());
let t = Instant::now();
for _ in 0..1_000_000 { extract_v2(r#"{"amount":12999,"currency":"EUR"}"#); }
println!("v2 {:?}", t.elapsed());
```

```text
v1 38.2ms
v2 0.9ns
```

Find every defect (there are at least four), explain the 0.9 ns, and rewrite the benchmark using `ch02-09`'s harness.

### 14. Design exercise

Design gateway-core's benchmark suite: which components get microbenchmarks, which get instruction-count gates, what
the load test's request mix is recorded from and how often it's refreshed, how the suite avoids coordinated omission,
and what a developer runs locally before opening a PR. Include how you'll keep layout effects from generating false
alarms.
