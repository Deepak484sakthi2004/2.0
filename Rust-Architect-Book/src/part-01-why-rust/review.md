# Part I Review — Architecture Review & Interview Mode

> Consolidate Part I, then use it: review a real adoption proposal as a principal engineer, answer senior-level interview
> questions without notes, and write the Part's capstone ADR. Answers are in **Appendix A, Part I**.

---

## Part I on one page

```text
THE PROBLEM          every allocation has a lifecycle: allocate → initialize → use → free
                     every memory-safety bug violates it:
                     SPATIAL · TEMPORAL · INITIALIZATION · CONCURRENCY · TYPE
      │
      ▼
THE QUESTION         Who frees this memory, and how do we know nobody still uses it?
      │
      ├── manual (C) .................. the programmer's reasoning ........ UB when wrong
      ├── RAII + smart pointers (C++) . convention + destructors .......... UB when wrong
      ├── tracing GC (Java, Go) ....... the runtime proves reachability ... memory + CPU headroom
      ├── reference counting .......... counters .......................... cycles, atomic traffic
      └── ownership (Rust) ............ the COMPILER proves it ............ rejects some correct programs
                                              │
                                              ▼
RUST'S MECHANISMS    ownership · destructive moves · aliasing XOR mutation · lifetimes (erased)
                     Send/Sync = auto traits + lifetime bounds + LIBRARY signatures
                     encapsulated unsafe · monomorphized zero-cost abstractions · no runtime
      │
      ▼
NOT COVERED          leaks · deadlocks · race conditions (TOCTOU) · panics · overflow in release
                     logic errors · bugs in the trusted base (unsafe code, C libraries, compiler)
      │
      ▼
THE COST             learning · compile time · graph-shaped data · lifetime propagation
                     async complexity · ecosystem and hiring
      │
      ▼
THE DECISION         workload arithmetic first (per-op budget, fleet cores, I/O share),
                     then team, ecosystem, boundaries: adopt PER COMPONENT, measure, set exit criteria
```

## Ten ideas to carry into the rest of the book

1. **Rust's safety lives mostly in the type system, not in runtime machinery.** `Vec` has the same bytes and the same
   reallocation as `std::vector`. The difference is which programs you're allowed to write.
2. **Borrow checking is local and signature-based.** That makes it modular and scalable, and it also makes it
   conservative.
3. **Aliasing XOR mutation** is a single rule, and it prevents iterator invalidation, prevents data races, and gives
   the optimizer `noalias`.
4. **Moves are destructive**, which is how Rust gets exactly-once freeing without runtime bookkeeping.
5. **Destructors manage resources. They are never relied on for soundness**, because leaking is safe (the 2015
   scoped-thread lesson).
6. **Thread safety is a library-level achievement** built on auto traits and lifetime bounds, which means other
   libraries can extend it.
7. **"Zero-cost" is relative to hand-written code, in optimized builds.** It's paid for at compile time.
8. **Safe Rust promises no undefined behavior, relative to a trusted base.** It doesn't promise no bugs.
9. **Store positions, not addresses.** Indices and IDs are the systems pattern that keeps coming back.
10. **Do the workload arithmetic before arguing about languages.**

---

## Architecture review: Meridian's gateway proposal

The platform team submits this proposal. You're the reviewing principal engineer.

> **Proposal: Rewrite the API gateway in Rust.**
> The current gateway is Java 21 on Netty. It terminates TLS for ~400K req/s at peak, validates JWTs, routes to ~120
> upstream services, enforces per-API-key rate limits, and writes an access log line per request. It runs on ~330 cores
> across 55 pods with 6 GB heaps (ZGC). Measured: ~0.5 ms CPU per request; p99.9 latency 45 ms at peak, with spikes
> correlated to allocation stalls during traffic bursts.
>
> Plan: rewrite on Tokio + Hyper + rustls over six months. Expected results: 40% fewer cores, p99.9 below 10 ms, 1 GB
> per pod. Cut over in a single weekend.

Answer each question as you would in the review meeting. Specific beats generic.

1. **Bottlenecks.** Where does the 0.5 ms of CPU per request most likely go: TLS handshakes vs session resumption, JWT
   signature verification, header parsing, routing, or access-log serialization? Which of those does a language change
   affect, and which are dominated by cryptography that would cost the same in any language?
2. **Allocations.** Where does the Java gateway allocate per request? Which of those could the Rust design make
   zero-copy, and what lifetime cost (Chapter 1.4) would that impose on the code?
3. **Contention.** Per-API-key rate limiting is shared state touched by every core on every request. What designs avoid a
   global lock? (You'll revisit this in Parts XIV and XXIV. Reason from Chapter 1.3's counter example for now.)
4. **Ownership boundaries.** What owns a connection, a request, and a streaming response body? Where would you use IDs
   instead of references?
5. **Failure model.** What happens when a request handler panics? When an upstream hangs? When the process hits its
   memory limit? Compare with what the Java gateway does in each case today.
6. **`unsafe`.** Where, if anywhere, would `unsafe` be required? How would you audit the dependency tree's `unsafe` code
   and C libraries?
7. **10x and 100x load.** Which resource saturates first: CPU, memory, file descriptors, upstream connection pools, or
   the rate limiter's shared state?
8. **Benchmarking.** How would you check the 40% claim *before* committing to a six-month rewrite? Define the benchmark:
   workload mix, TLS resumption rate, payload sizes, and the metrics you'd compare.
9. **Observability.** What do you lose compared with JFR and heap dumps, and how do you compensate?
10. **Rollout.** Critique "cut over in a single weekend." Propose a safer migration.
11. **Changing priorities.** How would the proposal change if **latency** became the sole priority? If **throughput per
    dollar** did? If **developer velocity** did?

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. Why does Rust have ownership at all? Answer without using the word "safety."
2. Why are Rust references never null, and what does that cost at run time?
3. What does "undefined behavior" mean? How can safe Rust promise no UB while still allowing panics?
4. What is the difference between a data race and a race condition? Which does Rust prevent, and how?

### Compiler

5. How can the borrow checker reject a program without knowing what `Vec::push` does internally?
6. What does "lifetimes are erased" mean, and what would change if they weren't?
7. What does monomorphization trade away, and what does it buy?

### Concurrency

8. Why isn't `Rc<T>` `Send`? What does `Arc<T>` change at the CPU level, and what does that cost?
9. Why is "the lock owns the data" (`Mutex<T>`) a stronger design than Java's `synchronized`?
10. Why was `Ordering::Relaxed` enough for the shared counter in Chapter 1.3?

### Async (preview)

11. Rust removed green threads before 1.0 and added `async`/`await` in 2019. Why is async/await compatible with
    "no mandatory runtime" when green threads weren't?

### Performance

12. Define "zero-cost abstraction" precisely, and give two common misinterpretations of it.
13. Where does Java pay for the optimization work that Rust pays for at compile time?
14. What's the per-operation budget at 100M ops/s on one core, and what does it rule out?

### Architecture

15. When would you choose Rust over Java? Over Go? When would you choose neither?
16. How would you introduce Rust into a Java organization with the least risk?
17. A team's Rust rewrite turned out slower than the Java original. How do you diagnose it?

---

## Part I capstone: ADR-001

Write **ADR-001** for Meridian's gateway decision, using the proposal and your review answers. Use this template:

```text
ADR-001: <decision title>
Status:          proposed
Context:         workload shape (with numbers), current pain (with numbers), team, ecosystem constraints
Options:         at least three, INCLUDING "tune the existing Java gateway"
                 (e.g. allocation reduction, TLS session resumption, ZGC tuning, horizontal scaling)
Decision:        what, and the scope (the whole gateway? one path? a library?)
Consequences:    positive / negative / risks
Validation:      benchmark plan, success metrics, exit criteria, decided BEFORE any code is written
Revisit when:    the conditions that would reverse this decision
```

Rules:

- The "tune Java" option must be argued **fairly**, as its strongest advocate would argue it.
- Every claimed benefit needs either a number from the proposal or a stated way to measure it.
- One page. A principal engineer's ADR is short because the thinking was done before writing it.

Appendix A has a model ADR outline. It's one reasonable answer, not *the* answer.

---

## Looking ahead: Part II

Part II starts writing Rust, through the compiler lens set up in Part I. It covers what `let x = 10;` turns into, whether
`x` exists at run time at all, what debug and release builds actually change, and how `cargo`, crates, features, and
build profiles (`panic = "abort"`, `overflow-checks`, `lto`) shape the binary you deploy. It ends with **Project Level
1**, a production-grade CLI tool.
