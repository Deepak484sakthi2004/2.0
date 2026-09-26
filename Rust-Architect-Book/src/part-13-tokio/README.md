# Part XIII — Tokio and Production Async

> **Part question:** *What is a production async runtime made of, where does it end and the operating system begin, and
> how do you place work, share state, stop tasks, and refuse load so that a Rust service stays fast and correct when
> its dependencies, its clients, and its own deploys misbehave?*

Part XII built every piece of an async runtime by hand and measured what each costs. Part XIII studies the one Meridian
(and most of the Rust ecosystem) runs in production: **Tokio**. It opens the runtime (workers, queues, the LIFO slot, the
cooperative budget, the I/O driver, the timer wheel, the blocking pool) and reads its defaults straight out of Tokio's
source at the version the book verifies against (1.53.1). It then turns each mechanism into a production rule: where
blocking and CPU-bound work go, which synchronization primitive fits which communication shape, how cancellation really
behaves and how to make code safe under it, and how to put a bound and a policy on every queue between a client and a
database.

The Part's project is **Ferrite v2**: the key-value store from Project L4, served over Tokio with a connection limit, idle
and write timeouts, a cap on unflushed replies, stable error codes, frames split across reads, graceful shutdown with a
drain deadline, and per-server metrics. It serves 1,000 concurrent connections on five OS threads. Two of its lessons
came from its own first runs: an idle timeout shorter than a client's think time, and a listen backlog of 128 that made
connections wait for SYN retransmissions.

## Chapter map

```text
13.1 Tokio's Architecture          workers, epoll + eventfd + signal socketpair (read from /proc); worker loop, LIFO slot,
                                   work stealing; coop budget 128; 1 ms timer wheel, missed-tick policies; task = 88 B +
                                   future; spawn ~300 ns vs ~800 ns; every default read from Tokio's source
      │
13.2 Tasks, Spawning, Blocking     Send + 'static and why no scoped spawn; JoinHandle semantics; thread_local! vs
                                   task_local! (1,489 of 1,600 wrong); blocking stalls measured; spawn_blocking vs rayon
                                   vs block_in_place; the blocking pool's limits
      │
13.3 Channels and Synchronization  mpsc as backpressure (send vs try_send vs reserve), the actor pattern, broadcast
                                   Lagged, watch, Notify's lost wakeup and enable(), Semaphore; std vs tokio Mutex
                                   (4 ns vs 22 ns) and the convoy (500 ms vs 10 ms); channel memory
      │
13.4 Cancellation                  cancel = drop at the current .await; cancel safety from Tokio's docs; select! losing
                                   bytes; JoinSet, TaskTracker, CancellationToken; graceful shutdown with a deadline;
                                   Drop can't .await; runtime drop and blocking tasks
      │
13.5 Backpressure and Shedding     TCP pushes back (2.5 MiB, then Pending), UDP doesn't (92 of 20,000); the metastable
                                   simulation (goodput 294 vs 1,020); tower layer order; one propagated deadline; the
                                   kernel's accept queue (bind uses 128) and what overflowing it looks like
      │
Project L5: Ferrite v2             v1's store (now with shared Arc<[u8]> values, per Chapter 20.7) and grammar on Tokio:
                                   codes, codec, limits, timeouts, lingering close, graceful shutdown; 11 tests;
                                   1,000 connections on 5 threads
      │
Part XIII Review                   the payout-relay PR (8 of 300 delivered, 292 lost at a deploy); redesign: 300 of 300
                                   accounted for; interview mode
```

## What you'll be able to do after Part XIII

- Describe a Tokio runtime's moving parts and the exact OS resources behind them, and read any default from the source.
- Predict where a task runs, when a ready task waits anyway, and what the cooperative budget does and doesn't protect.
- Place every kind of work correctly (async I/O, blocking I/O, CPU-bound) and prove it with a heartbeat measurement.
- Choose and size Tokio's synchronization primitives, and avoid the convoy and the lost wakeup.
- Write code that is correct when cancelled at any `.await`, and build graceful shutdown that finishes what it started.
- Put a bound and an overload policy on every queue in a service, including the kernel's, and propagate deadlines.

## Meridian systems in this Part

`merchant-notify` (runtime configuration after the May 2026 incident; the June heartbeat burst; admission control),
`payments-core` (fraud scoring on a CPU pool; the FX-rate convoy; the processor slowdown that outlived itself),
`settlement-api` (the thread-local merchant-ID exposure), the risk-limits service (sharded actors), the market-data ingest
(the `select!` that corrupted a feed), the gateway (deploys without dropped requests), and the new `payout-relay` (the
review capstone).

## Listings

`listings/part-13/`: 44 files, 47 checks, all verified on rustc 1.98.1 (edition 2024) with Tokio 1.53.1, tokio-util
0.7.19, and tower on the Playground. Most asynchronous timing listings run on Tokio's paused clock (`start_paused`), so
their numbers are exact virtual times. The rest (real sockets, real CPU) are one run each on a shared 4-vCPU machine and
are labeled that way. Several listings read Tokio's, mio's, and tokio-util's own source files at run time to back the
[LIB] facts quoted in the text.
