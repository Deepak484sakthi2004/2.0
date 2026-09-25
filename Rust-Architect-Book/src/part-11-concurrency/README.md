# Part XI — Concurrency

> **Part question:** *When several threads touch the same program, what does the compiler prove, what does the hardware
> charge, and how do you choose, for each piece of state, between not sharing it, sharing it immutably, and sharing it
> mutably, with numbers rather than habit?*

Chapter 1.3 made two promises in its first concurrency example. It said the compiler would reject a data race at
compile time, and it ranked three fixes (no sharing, an atomic, a mutex) by predicted cost, "from mechanism", without
measuring them. Part XI keeps both promises, starting from what the kernel sees.

It begins with **threads as the OS provides them**: a `clone` with a 2 MiB stack and a guard page, read out of
`/proc`, and a `JoinHandle` that carries ownership back. Then come the two traits that make thread safety a property of
types, **`Send` and `Sync`**, derived from aliasing XOR mutation and read out of the type checker, with Miri catching the
one `unsafe impl` that lied. The **locks** follow: `Mutex` down to its `lock cmpxchg` and futex call, a poisoning policy
per lock, `Condvar` queues, and the recursive-read deadlock that bites every Java port. **Interior mutability** comes
next: one primitive (`UnsafeCell`), many strategies, and lock-free publication with `ArcSwap`, measured 28× faster than
a lock on the read path. Then **channels**, where ownership moves instead of being shared, and capacity is an admission
policy with a failure mode attached. **Scoped threads and rayon** borrow across threads safely, because of a control-flow
guarantee that the pre-1.0 destructor-based API couldn't give (the Leakpocalypse, rebuilt and caught by Miri). The Part
closes with **the decision matrix**: Chapter 1.3's ranking measured five times, the concurrent-map options measured
(Part IX's promise), and the one-line change that multiplied a service's throughput by four.

Two projects assemble it: an **HTTP/1.1 server from raw TCP** with a worker pool that survives panics and refuses work
it can't queue, and **Ferrite v1**, the concurrent key-value store the rest of the book builds on.

## Chapter map

```text
11.1 Threads and the OS                         spawn = clone + 2 MiB mmap + guard page (seen in /proc); join moves
                                                the result back; main exits = process exits; spawn ~30 µs vs
                                                hand-off ~5 µs; oversubscription hurts mean completion time
      │
11.2 Send and Sync                              the auto-trait table read from the compiler; E0277 notes as a proof;
                                                disjoint capture across editions; an unsound `unsafe impl Sync`
                                                caught by Miri; `lock inc` is all Arc pays over Rc (16× contended)
      │
11.3 Arc, Mutex, RwLock, and Condvar            the lock owns the data; futex mutex in asm; poisoning policies;
                                                Condvar bounded queue; writer-preferring RwLock and the recursive-
                                                read deadlock; guard lifetime = temporary lifetime (2021 vs 2024)
      │
11.4 Interior Mutability                        UnsafeCell as the one primitive (Miri without it); OnceLock/LazyLock
                                                and poisoning; per-request Cells folded into atomics; ArcSwap route
                                                table with a dropper thread; sizes of the family
      │
11.5 Channels and Message Passing               send = move; disconnection ends loops; bounded = backpressure;
                                                crossbeam select + shutdown by drop; owner thread; 18 ns vs 7 µs per
                                                item; unbounded channel memory, byte-exact
      │
11.6 Scoped Threads and Rayon                   thread::scope borrows (and why thread::scoped was unsound); rayon's
                                                work stealing; parallel logstat that merges exactly; parallel BFS;
                                                the break-even size of par_iter
      │
11.7 The Concurrency Decision Matrix            no sharing < padded atomic < false sharing ≈ shared atomic < mutex,
                                                five runs; sharded maps win; I/O under a lock costs 3.9×; the
                                                procedure and the twelve-criteria matrix; Java mapped row by row
      │
Project L3: HTTP Server from Raw TCP            bounded worker pool (Result<(), T> hands refused work back), panic
                                                containment, limits, keep-alive, 503 admission control, shutdown
      │
Project L4: Ferrite v1                          the KvStore contract (&self, owned values); 16 RwLock shards with a
                                                keyed router; recover-and-count poisoning; line protocol with
                                                pipelining; the L3 pool reused unchanged
      │
Part XI Review                                  the partner-quota PR: over-admission, alert storms, lock-order
                                                inversion, lost billing events; a verified redesign; interview mode
```

## What you'll be able to do after Part XI

- Explain what `thread::spawn`, `join`, and a thread's stack cost in the kernel, and size pools with Little's law.
- Derive `Send`/`Sync` for any type from its fields, read any thread-safety E0277 down to the offending field, and
  decide when an `unsafe impl` is justified and how to prove it.
- Choose among `Mutex`, `RwLock`, atomics, `OnceLock`, `ArcSwap`, channels, scoped threads, and rayon from three
  questions (who writes, how often, how many threads), and back the choice with a measurement.
- Write a poisoning policy for every lock, and prevent the four classic lock bugs structurally.
- Design admission control (bounded queues, refusal paths, drop-and-count) so overload is visible where it happens.
- Build and review a thread-per-connection server and a sharded concurrent store, and state their limits precisely.

## Listings

`listings/part-11/`: 55 files, 69 checks, all verified on rustc 1.98.1 (edition 2024), including edition 2018 and 2021
comparisons, ten intended compile errors, and five runs under **Miri** (three reporting the undefined behavior they
demonstrate: a false `unsafe impl Sync`, a "cell" without `UnsafeCell`, and the Leakpocalypse). Compiler artifacts
(release assembly of `Rc` vs `Arc` clones, of a `Mutex` lock/unlock, and of single-writer vs `fetch_add` counters) come
from `tools/emit.ps1`. Both projects run real TCP servers on `127.0.0.1` inside one Playground program. Timings are one
run each on the shared Playground machine, or several runs reported as a range, and are labeled as noisy.
