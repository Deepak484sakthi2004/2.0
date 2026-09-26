# Chapter 20.3 — CPU Profiling and Flame Graphs

> **Where this sits:** Part XX · Performance Engineering · chapter 3 of 7
> **Prerequisites:** Chapter 20.1 (metrics), Chapter 8.3 (unwind tables and landing pads), Chapter 7.3 and Chapter 18.6
> (LLVM merges identical functions), Chapter 14.4 (seqlocks).
> **After this chapter you can:** explain how a sampling profiler works from the timer interrupt to the flame graph; read
> a flame graph without the classic misreadings; choose between on-CPU, off-CPU, and wall-clock profiles for a given
> question; make Rust binaries profilable (symbols, frame pointers, inlined frames, merged functions); and run the
> standard Linux commands (`perf stat`, `perf record`, flame graphs, PGO, BOLT, compiler self-profiling) knowing what
> each one can and can't tell you.

---

## Pass 1 · User level — *Where does the time go?*

### 1. Problem

Chapter 20.2 was about comparing two versions of code you already suspected. Most performance work starts one step
earlier: the service uses too much CPU, or its p99 is too high, and nobody knows *where*. Guessing is how teams spend a
week optimizing a function that was 2% of the profile. The tool for "where" is a **profiler**, and the standard way to
look at its output is a **flame graph**.

This chapter has an unusual constraint. The Rust Playground won't let a program use the CPU's performance counters:

```text
perf_event_paranoid = 4
perf_event_open(cycles, this thread) = -1: Operation not permitted (os error 1)
```

(listing `ch03-01-no-perf-here.rs`, which calls `perf_event_open` directly). That's normal for containers and shared
hosts, and it's also the situation you'll often be in on production machines. So this chapter does two things: it gives
you the real commands, marked "not verified here", and it **rebuilds each tool's core idea in-process** so the idea is
verified even where the tool can't run.

### 2. Mental model

**A sampling profiler is a statistician with a stopwatch.** Many times per second, it interrupts the program and records
*where it was*: the full stack of function calls. After thousands of samples, the fraction of samples in which a
function appears estimates the fraction of time spent in it (and everything it called). It never times anything; it
counts.

```text
  timer / counter overflow ──► interrupt ──► walk the stack ──► ["main","handle","auth","hmac"]  ×1
                                                                ["main","handle","parse"]        ×1
  ... thousands of times ...                                    aggregate identical stacks
                                                                        │
  flame graph: one box per frame, width = number of samples containing it, stacked by depth
  ┌─────────────────────────────────────────────── main ───────────────────────────────────────────┐
  ┌──────────────────────────────────────────────── handle ─────────────────────────────────────────┐
  ┌──────────── parse ─────────┐┌─────────────────── auth ───────────────────┐┌──── serialize ──────┐
                                ┌──────────────── hmac ──────────────┐
```

Three rules for reading one (Brendan Gregg introduced flame graphs in 2011, and these are his rules):

1. **Width is the only thing that matters.** A wide box is a lot of samples. The x-axis is *not* time: boxes are sorted
   alphabetically so that identical stacks merge.
2. **Look for wide plateaus at the top.** A function whose box is wide and has nothing above it spends its time in its
   own code ("self time"). A wide box with children spends it in callees.
3. **Know which clock you sampled.** An **on-CPU** profile samples only while the thread runs, so it shows what burns CPU
   and hides waiting. An **off-CPU** profile samples blocked time (locks, I/O, sleeps). A **wall-clock** profile samples
   everything: it answers "why is this request slow?", which is usually a different question from "why is this CPU
   busy?".

### 3. Rust code

**A sampling profiler, rebuilt in-process** (listing `ch03-02-phase-sampler.rs`). A worker thread handles "requests"
with four phases (parse, auth, a 300 µs wait on an "upstream", serialize) and keeps a tiny stack of phase IDs in
atomics, protected by a seqlock (Chapter 14.4's pattern) so the sampler never reads a half-updated stack. A sampler
thread wakes up every 200 µs, reads the stack, and counts it, separately for on-CPU samples (the worker isn't blocked)
and wall-clock samples (all of them). The output uses the **folded-stack** format (`frame;frame;frame count`) that
`flamegraph.pl` and `inferno` turn into a flame graph:

```rust,ignore
// excerpt of ch03-02-phase-sampler.rs: the sampler's read side of the seqlock
fn read_stack() -> Option<(String, bool)> {
    let s0 = SEQ.load(Acquire);
    if s0 % 2 == 1 {
        return None;
    }
    let d = (DEPTH.load(Relaxed) as usize).min(4);
    let ids: Vec<u8> = (0..d).map(|i| STACK[i].load(Relaxed)).collect();
    let off = OFF_CPU.load(Relaxed);
    fence(Acquire);
    if SEQ.load(Relaxed) != s0 {
        return None;
    }
    // ...
}
```

One run (fixed interval first, then a randomly jittered one):

```text
== sampling interval: fixed 200 us ==
on-CPU samples: 733 samples over 2543 requests
  idle 1    (0%)
  main;handle;auth 369    (50%)
  main;handle;parse 205    (28%)
  main;handle;serialize 158    (22%)
wall-clock samples: 4435 samples over 2543 requests
  idle 1    (0%)
  main;handle;auth 369    (8%)
  main;handle;parse 205    (5%)
  main;handle;serialize 158    (4%)
  main;handle;wait_upstream 3702    (83%)
== sampling interval: random 50..350 us ==
on-CPU samples: 786 samples over 2524 requests
  idle 1    (0%)
  main;handle;auth 425    (54%)
  main;handle;parse 157    (20%)
  main;handle;serialize 203    (26%)
wall-clock samples: 4471 samples over 2524 requests
  ...
  main;handle;wait_upstream 3685    (82%)
direct timing: parse 22 us (20%), auth 55 us (50%), serialize 33 us (30%) of on-CPU time
```

Two lessons are visible in these numbers:

- **The two views answer different questions.** On-CPU: auth is half the CPU, so optimize auth to save cores. Wall
  clock: 83% of a request's time is waiting for the upstream, so optimizing auth barely changes latency. A team that
  looks at the wrong view optimizes the wrong thing.
- **A fixed sampling interval can lie.** The ground truth (timed directly) is parse 20%, auth 50%, serialize 30%. The
  fixed 200 µs sampler said 28/50/22: it over-counted parse and under-counted serialize, because the sampler and the
  request loop ran in near-lockstep and kept landing in the same phases. The jittered sampler (20/54/26) is closer.
  This is **aliasing**, and it's why `perf record` is usually run at an odd frequency such as `-F 99` rather than 100 Hz:
  it avoids sampling in lockstep with timers and periodic work (Gregg's recommendation).

**User time vs system time** (listing `ch03-03-user-vs-system-time.rs`), the cheapest profile there is:
`getrusage(RUSAGE_SELF)` before and after, which needs no permissions:

```text
unbuffered (1 syscall per line) wall   130.4 ms   user   43.3 ms   system   87.1 ms   ctx switches: 0 voluntary, 1 involuntary
BufWriter 64 KiB       wall     1.2 ms   user    1.1 ms   system    0.1 ms   ctx switches: 0 voluntary, 0 involuntary
```

Two-thirds of the unbuffered version's time was in the kernel: 200,000 `write` system calls. No CPU profile of *your*
functions would have shown that as clearly as the system-time column. (Project L1 chose `BufWriter` for this reason;
this is the measurement.)

**What a stack sampler can see** (listing `ch03-04-inlined-frames.rs`). Three small functions call each other; the
innermost captures a backtrace and we print the frames from this crate, in a debug and a release build:

```text
debug:   4 frames from this crate:
           playground::validate_amount
           playground::build_refund
           playground::handle_refund
           playground::main
release: 1 frames from this crate:
           playground::main
```

In release, `validate_amount`, `build_refund`, and `handle_refund` were inlined into `main`. **Their frames don't exist
at run time**, so a profiler that only walks frames attributes all their time to `main`. Profilers recover inlined
functions from debug information (DWARF records which source function each instruction came from), which is why the
profiling build needs debug info even when it's optimized.

---

## Pass 2 · Systems level — *From interrupt to flame graph*

### 4. Under the hood

**Sampling on Linux.** [OS][CPU] `perf record` asks the kernel, through `perf_event_open`, for an interrupt every N
events: every N CPU cycles (hardware counter overflow), or every 1/F seconds of CPU time (software clock). At each
interrupt the kernel records the instruction pointer and, depending on the options, the call stack. `perf_event_paranoid`
controls who may do this: level 4 (a Debian/Ubuntu extension) forbids unprivileged users entirely, which is what
`ch03-01` hit; containers usually add a seccomp filter on top. On your own Linux machine you'd lower it
(`sysctl kernel.perf_event_paranoid=1`) or run with `CAP_PERFMON`.

**Walking the stack, three ways.** [CPU][RUSTC]

| Method | How | Cost | Rust notes |
|---|---|---|---|
| Frame pointers | Follow the `rbp` chain | Cheap per sample | rustc omits frame pointers by default on x86-64; enable with `-C force-frame-pointers=yes`. The precompiled std doesn't have them unless rebuilt (`-Z build-std`, nightly) [VERSION] |
| DWARF unwinding | Copy a chunk of stack (8 KiB by default) per sample, unwind later with `.eh_frame` (the same tables panics use, Chapter 8.3) | Expensive: large samples, post-processing | Works without recompiling; `perf record --call-graph dwarf` |
| Hardware branch records (LBR on Intel) | The CPU records recent branches | Cheap, but shallow stacks | `perf record --call-graph lbr`; not available on every CPU or VM |

Some distributions have moved towards frame pointers everywhere for exactly this reason (Fedora 38 in 2023 and Ubuntu
24.04 in 2024 built their packages with frame pointers, as reported by both projects), accepting a small runtime cost for
reliable profiles.

**Symbolization.** [RUSTC] Turning addresses into names needs the symbol table (function names; Rust's are v0-mangled
on 1.98.1, Chapter 7.1, and need a recent `perf` or `rustfilt` to demangle) and DWARF line tables for file:line and
inlined frames. `debug = "line-tables-only"` in the release profile (the Meridian gateway's choice, Chapter 2.2) is the
usual compromise: enough for profiles and backtraces, a fraction of full debug info's size. `strip` and
`split-debuginfo` decide where that information lives in the shipped artifact (Chapter 19.3's topic).

**Merged functions confuse attribution.** [RUSTC] In release builds LLVM merges functions with identical machine code
into one body and makes the other symbols aliases (Chapters 7.3 and 18.6 saw `a = b` lines in the assembly). A profiler
attributes all samples of the shared body to *one* name, possibly a function that isn't even on the hot path. For a
profiling build you can turn merging off (`-Z merge-functions=disabled`, nightly; not verified here), or check the
assembly for aliases before trusting a surprising name.

### 5. Memory

**Profiling has a memory and I/O cost.** DWARF call graphs copy 8 KiB of stack per sample: at 99 Hz × 64 threads,
that's ~50 MB per second of `perf.data`. Frame-pointer stacks are a few hundred bytes per sample. Continuous profilers
(Parca, Pyroscope, and Google-Wide Profiling, described by Ren et al., IEEE Micro 2010) sample at low frequency on every machine and
aggregate centrally; they rely on frame pointers or eBPF-based unwinding to keep that cost small.

**The profiler's own allocations** are a reason to prefer out-of-process profilers (perf, eBPF) for Rust services: an
in-process profiler that allocates while sampling can perturb exactly the allocator behaviour you're trying to see.
`ch03-02`'s sampler allocates (`String`s for the folded stacks), which is fine for a teaching tool and not for
production.

### 6. CPU / OS

**Counting instead of sampling: `perf stat`.** When the question is *why* code is slow rather than *where*, hardware
counters answer it. The commands below are the ones earlier Parts deferred to this chapter (not verified here: they
need a Linux machine with `perf` and `perf_event_paranoid ≤ 1`):

```text
# IPC and the big four
perf stat -e cycles,instructions,branches,branch-misses,cache-references,cache-misses ./target/release/bench

# Chapter 6.5's dispatch benchmark: is mixed dyn dispatch slow because of branch mispredictions?
perf stat -e branch-misses,instructions ./dispatch-bench mixed
perf stat -e branch-misses,instructions ./dispatch-bench grouped

# Chapter 10.3: next() vs fold() over Chain, and dyn Iterator: instructions per element tell the story
perf stat -e instructions,cycles ./iter-bench chain-next
perf stat -e instructions,cycles ./iter-bench chain-fold

# Chapter 12.6's ping-pong: how many switches and migrations per round trip?
perf stat -e context-switches,cpu-migrations ./pingpong
```

How to read them: **IPC** (instructions ÷ cycles) near 3–4 on a modern core means compute-bound and well-predicted;
below 1 usually means stalls (cache misses or mispredictions). For the dispatch benchmark, Chapter 6.5 measured
3.3 ns per element mixed vs 2.0 ns grouped, and predicted from mechanism that the difference is mispredicted indirect
calls; `branch-misses` per element should be near 0.5 in the mixed case and near 0 when grouped. Chapter 20.5 measures a
direct-branch version of the same effect in-process (`ch05-02`): ~6 ns per misprediction.

**In-process proxies when `perf` isn't available.** `getrusage` gives user/system time and voluntary/involuntary context
switches (`ch03-03`); `sched_getcpu` shows migrations (`ch07-04`); `/proc/self/status` gives RSS and context switches;
`/proc/stat` shows steal (`ch02-08`). None of them replaces counters, but they answer "kernel or user?", "blocked or
preempted?", and "did the machine take my CPU?" without privileges.

**Off-CPU profiling** (Chapter 11.7's promise). When the wall-clock view says "waiting", you need to know *on what*:
a lock, a disk, a network read, the scheduler. The Linux tools, not verified here:

```text
perf sched record -- ./service ; perf sched latency           # run-queue delay per task
offcputime-bpfcc -f -p $(pidof service) 30 > off.folded       # BCC: blocked stacks, folded format
flamegraph.pl --color=io --title="Off-CPU" off.folded > off.svg
```

`ch03-02`'s wall-clock view is the same idea: it sampled the worker while it slept, and "wait_upstream" came out as 83%.

---

## Pass 3 · Architect level — *Profiles in production and in the build*

### 7. Trade-offs

| Tool | Answers | Overhead | Needs |
|---|---|---|---|
| `perf record -F 99 -g` + flame graph | Where on-CPU time goes, with stacks | ~1–5% | perf permissions, symbols, frame pointers or DWARF |
| `cargo flamegraph` / `samply` | Same, with less typing (`samply` also reads macOS/Windows profiles) | Same | Same |
| `perf stat` | Why: IPC, misses, mispredictions, switches | ~0 in counting mode | perf permissions, a PMU (often limited in VMs) |
| eBPF (`offcputime`, `profile`) | Off-CPU and on-CPU stacks, continuously | Low | Root or CAP_BPF, kernel headers/BTF |
| Continuous profiler (Parca, Pyroscope) | Fleet-wide trends, before/after deploys | Very low per host | An agent on every host |
| `tracing` spans (Chapter 22.5) | Latency of named operations per request | Proportional to spans | Instrumented code |
| In-process tricks (`getrusage`, counters, phase stacks) | Coarse attribution without privileges | Near zero | Code changes |

**Profile-guided optimization (PGO) and BOLT** turn a profile into a faster binary rather than a report. PGO compiles
twice: an instrumented build collects branch and call counts on a representative workload, and the final build uses
them for inlining, block layout, and indirect-call promotion (the static answer to Chapter 7.2's JIT type profiles). BOLT
(Meta's post-link optimizer, described by Panchenko et al., CGO 2019) rearranges a finished binary's code by profile to
improve instruction-cache and branch-predictor behaviour, the same layout effect `ch02-11` exposed by accident. Not
verified here; the usual commands:

```text
# PGO with cargo-pgo (a cargo subcommand wrapping -Cprofile-generate / -Cprofile-use)
cargo pgo build                       # instrumented binary
./target/.../gateway < recorded-load  # run a representative workload
cargo pgo optimize                    # rebuild using the collected profile
# BOLT on top of PGO
cargo pgo bolt build --with-pgo && cargo pgo bolt optimize --with-pgo
```

The Rust project builds its own Linux compiler with PGO (and has used BOLT for parts of the toolchain), and reported
measurable compile-time gains from both in 2022–2023. For a service, the gain is workload-specific: measure it with
Chapter 20.2's A/B load test, and keep the training workload representative, or you'll optimize for the benchmark.

**Your build is a program too** (Part XVIII's promises). Compile time has its own profilers, all local-only and not
verified here:

```text
cargo build --release --timings                  # HTML report: per-crate time, codegen vs front end, critical path
cargo +nightly rustc --release -- -Z self-profile  # query-level profile; analyze with `summarize` (measureme)
cargo +nightly rustc -- -Z time-passes           # coarse per-pass timings
RUSTFLAGS="-Z threads=8" cargo +nightly build    # parallel front end (nightly)
RUSTFLAGS="-Zcodegen-backend=cranelift" cargo +nightly build   # Cranelift for debug builds (nightly component)
cargo rustc --release -- -C llvm-args=-time-passes         # LLVM's own per-pass timings
cargo rustc --release -- -C llvm-args=-print-after-all 2> passes.ll   # every pass's IR (huge: one function at a time)
```

The last two look inside LLVM's optimization pipeline (Chapter 17.7's passes, run for real): `-time-passes` shows which
of LLVM's passes cost the most compile time on your crate, and `-print-after-all` dumps the IR after each one, which is
how you find the pass that vectorized, inlined, or deleted something. Use `-C llvm-args=-filter-print-funcs=<symbol>` to
limit the dump to one (mangled) function, or it will be gigabytes.

Chapter 7.3's gateway workspace diet used `--timings` to find that the binary crate's code generation was the critical
path. `-Z self-profile` on a trait-heavy crate usually shows trait solving and monomorphization near the top (Chapters
18.3 and 18.6). Cranelift trades runtime speed for compile speed in debug builds; measure it on your workspace, because
the gain depends on how much of the build is LLVM.

### 8. Java comparison

| | Java | Rust |
|---|---|---|
| Standard sampling profiler | async-profiler (uses `perf_events` plus the JVM's `AsyncGetCallTrace`), JFR | `perf`, `samply`, `cargo flamegraph` |
| Safepoint bias | Classic JVM profilers could only sample at safepoints, so hot loops without safepoints were invisible (Nitsan Wakart's writing popularized the problem); async-profiler and JFR avoid it | No safepoints: samples land on any instruction |
| Symbols for generated code | JIT-compiled code needs the JVM's help (perf-map-agent, `-XX:+DumpPerfMapAtExit`) | Symbols are in the binary (v0-mangled) |
| Inlining | JIT inlines by profile; async-profiler reports inlined Java frames from JVM metadata | LLVM inlines at build time; recovered from DWARF |
| Off-CPU and locks | JFR thread-park, monitor-enter events | `offcputime`, `perf lock`, `ch07-03`-style wait histograms |
| Profile → optimization | The JIT does PGO continuously and for free | PGO/BOLT are explicit build steps |

> **Analogy limit.** A Java flame graph mixes interpreted, C1, and C2 frames for the same method across the run, and the
> profile can change after a deoptimization. A Rust flame graph shows one fixed binary: what you see is what runs, but
> you have to supply the debug info and frame pointers the JVM would have given you for free.

### 9. Production scenario

**Profiling the Rust gateway canary.** During Chapter 20.1's canary, the gateway team needed to know why CPU per request
was higher than the component budgets predicted. The procedure they wrote down (the numbers of that investigation are
in Part XXI; the procedure is the point here):

1. **Build a profiling variant** of the release binary: same `opt-level`, LTO, and codegen units; `debug =
   "line-tables-only"` (already the default for the gateway); `-C force-frame-pointers=yes`. Ship it to one canary pod.
2. **Record on-CPU at 99 Hz for 60 s at peak** (`perf record -F 99 -g -p <pid> -- sleep 60`), fold the stacks, render a
   flame graph, and **diff it** against the component model: every wide box must map to a line in the capacity model
   or become one.
3. **Check the kernel column first.** `getrusage`-style user/system split per worker thread (they exported it as a
   metric). A high system share points to syscalls per request, not to Rust code.
4. **Record off-CPU for 30 s** with `offcputime` to see what workers block on: the async runtime shouldn't block at
   all, so any blocked worker stack is a bug (Chapter 12.6's handshake incident).
5. **Only then optimize**, one change at a time, each with a Chapter 20.2 measurement.

### 10. Failure scenario

**The risk-limits profile that blamed the wrong function (2026).** The risk-limits service (100K ops/s, p99 < 1 ms,
Chapter 1.2) regressed after a release, and a flame graph showed 35% of CPU in
`<ExposureBucket as Drop>::drop`, a trivial function in a module nobody had changed. The owning team spent two days on
it.

The function wasn't hot. LLVM had merged it with a byte-identical function: the drop glue of a new per-request
`Vec<Reservation>` that a release had started cloning into every request context. Both bodies were "free a buffer if
its capacity is non-zero"; the profiler attributed the shared body's samples to whichever symbol it picked. The
investigation that found it was the one from Chapter 18.7's rule: emit the release assembly, see the `=` alias line,
and look at *callers* in the flame graph instead of the leaf's name. The callers pointed straight at the clone.

The team's profiling runbook now has two lines added: "profiling builds use `-Z merge-functions=disabled` when a leaf
looks implausible", and "trust callers over leaf names".

---

## Practice

### 11. Interview & architecture questions

1. Explain how a sampling profiler estimates time spent in a function without timing anything.
2. What do the x-axis and the width of a flame graph box mean? Name the most common misreading.
3. When do you want an on-CPU profile, an off-CPU profile, and a wall-clock profile? Give a Meridian example of each.
4. `ch03-02`'s fixed-interval sampler misattributed parse and serialize. Explain aliasing and how `perf` users avoid it.
5. Why does a release build's profile sometimes attribute time to `main` or to a function you know is trivial? Give two
   mechanisms and their fixes.
6. Compare frame-pointer, DWARF, and LBR stack walking. Why have distributions started compiling with frame pointers?
7. What does IPC tell you, and what would you expect it to be for a pointer-chasing loop vs a vectorized sum?
8. What can `getrusage` tell you that a CPU flame graph can't?
9. What do PGO and BOLT each optimize, and what's the risk in how you collect the training profile?
10. Your CI takes 40 minutes. Which tools tell you whether to split crates, reduce generics, or switch the debug
    backend?

### 12. Exercises

- **Beginner.** Change `ch03-02`'s sampling interval to 1 ms and to 50 µs. How do sample counts and accuracy change?
- **Intermediate.** Add a fifth phase to `ch03-02` that takes a `Mutex` held by another thread. Which view shows it, and
  how would you label it in a real off-CPU profile?
- **Advanced.** Add `#[inline(never)]` to the three functions in `ch03-04` and rerun in release. Then emit the assembly
  and explain which frames a frame-pointer profiler would see with and without `-C force-frame-pointers=yes`.
- **Systems.** On a Linux machine you control, profile listing `ch07-05-ferrite-shards.rs` with
  `perf record -F 99 -g` and render a flame graph. Where does the `Mutex + Vec` variant spend its time, and does the
  profile agree with Chapter 20.7's explanation?
- **Architecture.** Write the gateway's continuous-profiling proposal: which profiler, sampling rate, symbol handling
  (split debug info, symbol server), retention, and how deploys are compared.

### 13. Debugging exercise

A flame graph of a Rust batch job shows 60% of samples in `__memmove_avx_unaligned_erms` with `[unknown]` as its only
caller, and the rest spread thinly. Explain what `[unknown]` means, list the likely reasons the stacks are broken, and
give the commands (build flags and `perf` options) that would produce usable stacks. What Rust-level operations commonly
turn into large `memmove` calls?

### 14. Design exercise

Design Meridian's **profiling platform** for Rust services: build variants and flags, how profiles are collected
(continuous low-rate plus on-demand high-rate), how they're symbolized and stored, how a deploy is automatically
compared with the previous one, and how PGO training profiles are produced from it without leaking customer data.
