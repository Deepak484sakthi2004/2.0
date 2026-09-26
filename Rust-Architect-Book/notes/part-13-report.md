# Part 13 report

Part XIII was finished by a resuming writer. An earlier writer stalled on a network outage after writing 31 listings and
no prose. This pass verified and fixed those listings (one failed: `ch01-01` raced exiting worker threads), added 13
listings, and wrote the README, 13.1–13.5, Project L5, the review, the answer key, and this report. At the coordinator's
request, Ferrite v2's value-type decision uses Part XX's measurement (`listings/part-20/ch07-05-ferrite-shards.rs`,
Chapter 20.7) instead of a new one.

## SUMMARY.md lines

Replace the Part XIII draft block with:

```markdown
- [Part XIII Overview](part-13-tokio/README.md)
  - [13.1 Tokio's Architecture: Scheduler, I/O Driver, Timers](part-13-tokio/ch01-tokio-architecture.md)
  - [13.2 Tasks, Spawning, and Blocking Code](part-13-tokio/ch02-tasks-blocking.md)
  - [13.3 Async Channels and Synchronization](part-13-tokio/ch03-channels-sync.md)
  - [13.4 Cancellation and Structured Concurrency](part-13-tokio/ch04-cancellation.md)
  - [13.5 Backpressure, Timeouts, and Load Shedding](part-13-tokio/ch05-backpressure.md)
  - [Project Level 5: An Async TCP Server (Ferrite v2)](part-13-tokio/project-05-ferrite-v2.md)
  - [Part XIII Review: The payout-relay PR & Interview Mode](part-13-tokio/review.md)
```

Appendix entry, after "Part XII Answers":

```markdown
  - [Part XIII Answers](appendix/answers-part-13.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| Runtime seen from `/proc`: 4 `tokio-rt-worker` threads, one epoll instance (plus its `dup`), an eventfd, and the signal driver's process-global socketpair (still open after the runtime is dropped) | 13.1 | XIX.6 |
| `#[tokio::main]` expansion (real `-Zunpretty=expanded`): `Builder::new_multi_thread().enable_all().build().block_on(body)`; the body runs on the main thread | 13.1 | — |
| Tokio 1.53.1 defaults read from its own source at run time: coop budget 128, local queue 256 (steal half), LIFO ≤ 3 polls, event_interval 61 ("copied from golang"), global queue 31 / self-tuned ~10 ms, blocking pool 512 + 10 s keep-alive, nevents 1024, timer wheel 6 × 64 (MAX_DURATION 2^36−1 ms), BOX_FUTURE_THRESHOLD 2,048 debug / 16,384 release | 13.1 | technique reusable everywhere |
| LIFO slot: 1,000 of 1,000 children on the parent's worker; can't be stolen: heartbeat waited 300 ms vs 28 µs after one more spawn | 13.1 | XX.7 |
| Coop budget: recv loop heartbeat 2 ms late; `unconstrained` 38 ms; always-ready non-Tokio futures 399 ms; `yield_now` fixes it | 13.1 | — |
| Timers: 1 ms ticks, rounded up (1 µs sleep ≈ 1.06 ms); 100,000 timers registered at once (worst lateness 24 ms); `MissedTickBehavior` Burst vs Skip | 13.1 | — |
| Task memory: one allocation, 88 B + the future; futures above the threshold boxed first (2 allocations) | 13.1 | XX.4 |
| Costs: spawn+join ~300 ns (current_thread) / ~800 ns (4 workers); round trip ~180 ns / 250–400 ns; OS-thread round trip ~9 µs | 13.1 | XX |
| Stable `RuntimeMetrics` (num_workers, num_alive_tasks, global_queue_depth, worker_total_busy_duration, park counts) vs `tokio_unstable` (E0599 for worker_steal_count) | 13.1 | XXII.5 |
| `spawn` needs `Send + 'static` (E0373; "future cannot be sent ... used across an await"); no safe scoped spawn (leak argument); `LocalSet` for `!Send` | 13.2 | — |
| `JoinHandle`: drop detaches, `abort` at next `.await`, panic → `JoinError::is_panic`, abort after completion is a no-op | 13.2 | — |
| `thread_local!` under task migration: 1,489 of 1,600 reads saw another request's ID; `task_local!` 0 | 13.2 | XXII.5 (tracing spans) |
| Blocking measured: 1 of 4 workers blocked 1.9 ms, 4 of 4 196 ms, spawn_blocking 1.1 ms; CPU work inline 444 ms vs spawn_blocking 2.5 ms vs rayon + oneshot 17 ms | 13.2 | XX.7 |
| Blocking pool: grows to the cap, unbounded queue, keep-alive shrink, running closure can't be aborted; `block_in_place` panics on current_thread | 13.2 | — |
| mpsc as backpressure (sends at the consumer's pace); `send` vs `try_send` vs `reserve`; closure signals | 13.3 | — |
| Actor pattern: limits actor grants exactly 66 of 100; a panicked actor is visible to callers | 13.3 | XXIV |
| broadcast `Lagged(6)`; watch latest-only; Notify permit semantics, the lost wakeup, `enable()` | 13.3 | — |
| Channel memory: mpsc(1,000,000) 800 B empty, ~9 B per queued u64; broadcast(1,000) preallocates 41,096 B | 13.3 | — |
| Semaphore (limit / shed / close); std Mutex 4 ns vs tokio Mutex 22 ns; convoy 500 ms vs 10 ms | 13.3 | — |
| Cancellation = drop: which steps ran under 100/60/20 ms timeouts; JoinSet completion order and abort on drop | 13.4 | — |
| Tokio's per-method cancel-safety statements (read from source) | 13.4 | — |
| `select!` + `read_exact` lost 8 bytes and glued frames; `FramedRead` intact | 13.4 | — |
| Graceful shutdown: CancellationToken + TaskTracker + drain deadline | 13.4 | XXI, XXIV |
| `Drop` can't await: BufWriter dropped lost 3,520 bytes; `Handle::try_current` backstop; runtime drop waits for blocking tasks (280 ms) vs `shutdown_timeout` | 13.4 | — |
| TCP backpressure (2.5 MiB accepted, then `Pending`) vs UDP (92 of 20,000 kept; 9,000-byte datagram truncated to 2,048) | 13.5 | XXI.1 |
| Metastable simulation: unbounded goodput 294 (906 wasted), bounded + shed 1,020, skip-if-expired 1,049 (p50 493 ms) | 13.5 | XXI.4, XXIV |
| Tower layer order: `Timeout` doesn't time `poll_ready`; `Buffer` moves the wait into `call` | 13.5 | XXII.3 |
| Deadline propagation vs per-hop timeouts (110 ms of wasted processor work vs a refusal at 110 ms) | 13.5 | XXI.4 |
| Listen backlog: Tokio's `bind` → mio → `listen(128)` ("same as std"); overflow: `connect()` returns but `accept` waits for retransmits (243–609 of 1,000 accepted in 0.5 s vs all with 2,048) | 13.5 | XIX.6, XXI.1 |
| tokio-util 0.7.19: FramedRead + FramedWrite = 16,384 B per idle connection (1.64 GB at 100,000) | 13.5, L5 | XX.4 |
| Ferrite v2: task per connection; Semaphore shed with BUSY; `-ERR <CODE>`; LineCodec (split frames, linear scan); flush rule; `max_unflushed` + write timeout; lingering close; JoinSet shutdown with drain; per-server stats; configurable backlog; `Arc<[u8]>` values with a defaulted `KvStore::get_shared` | L5 | XXIII (v3) |

## Promises to later Parts

- **Part XIX:** `strace -f` of a Tokio server (epoll_wait timeouts, eventfd writes, futex wakes: 13.1 Systems exercise);
  `ss -ltn` Recv-Q and `/proc/net/netstat` `ListenOverflows` for the accept queue (13.5 §6); `tcp_wmem`/`tcp_rmem`/`tcp_mem`
  and kernel socket memory at scale (13.5 §5).
- **Part XX:** tokio-console and `tokio_unstable` poll-time histograms for long polls (13.1, 13.2); LIFO slot on/off
  measurements (13.1 Advanced); zstd level cost (13.2 debugging exercise); Ferrite v2 memory per idle connection with
  `FramedRead::with_capacity` (L5 Systems extension); buffer pools with capacity caps (the 9.1 promise, only touched by L5
  §5: carried forward).
- **Part XXI:** HTTP/1.1 `Connection: close` and HTTP/2 GOAWAY in graceful drains (13.4 §9); Nagle and `TCP_NODELAY`
  mechanism (L5 refers to 21.1); retry budgets and circuit breakers on top of 13.5's shedding; TLS for Ferrite.
- **Part XXII:** `tracing` spans as request context (13.2 §10); tower's `Buffer`, `LoadShed`, `ConcurrencyLimit`,
  `RateLimit` in depth (13.5); exporting Ferrite v2's `Stats` (22.5); Project L6's binary-safe framing (L5 omission);
  `tokio-console`.
- **Part XXIII:** Ferrite v3 keeps disk I/O off the workers (`spawn_blocking`, a dedicated I/O thread, or an async store
  trait) and adds the fallible trait next to `KvStore`, keeping v2's defaulted `get_shared`; `store_semantics` as the
  conformance suite; the on-disk value representation.
- **Part XXIV:** deterministic simulation on a paused clock (used throughout 13.x) for Ferrite's replication tests;
  outbox/saga for cancellation-safe multi-step operations (13.4 §7); idempotent requeue and per-merchant sequence numbers
  (review capstone); graceful handover with `SO_REUSEPORT` (L5 Architecture extension).
- **Not kept, carried forward:** "an async tail service reusing the logstat library" (PROGRESS Part XIII bullet): Part
  XIII didn't build it. Suggest Part XIX (with the mmap input for logstat) or Part XXII.

## Promises kept

- Cancellation = dropping a future mid-flight (1.3, 1.4): 13.4, measured step by step (`ch04-01`).
- Frame split across two network reads (2.4 design exercise): L5's `LineCodec` + `frames_split_across_reads_are_reassembled`,
  and 13.4 §10's feed-corruption scenario.
- No locks across `.await` (8.2, 12.3): 13.3 §6 (convoy measured), L5 (no guard outlives a store call).
- Ferrite v2 `-ERR <CODE>` errors with busy and shutting-down codes (8.4): L5.
- `spawn_blocking` for CPU-heavy work (Part IX answers), and why a core-sized pool is often better: 13.2.
- `Drop` can't await (3.5): 13.4 §5 (`ch04-05`: 3,520 bytes lost).
- `JoinError::is_panic` (8.3): 13.2 (`ch02-04`) and L5's panic test.
- Rayon doesn't belong on executor threads (10.4): 13.2 (rayon + oneshot, heartbeat measurements).
- `Semaphore` / `Notify` vs the 12.2 and 12.4 design exercises (waiter lists, lost wakeups): 13.3.
- LIFO slot, cooperative budget, timing wheel, eventfd waker, `block_in_place`, blocking pool cap 512: 13.1–13.2, from
  source.
- `tokio::sync::Mutex` across `.await` "and why you usually still shouldn't" (12.3): 13.3.
- The "there is no reactor running" panic (12.1): 13.4 §5.
- Per-method cancellation safety (12.3): 13.4 §4, quoting Tokio's docs from source.
- Ferrite v2 connection limit and graceful shedding; merchant-notify admission control and handshake budget (12.1, 12.6):
  L5 and 13.5 §9.
- Ferrite v2 keeps the v1 protocol and `ShardedStore` behind `Arc`, adds a write timeout and an unflushed-bytes cap (L4
  §5), and decides the value type (L4 review Q2): `Arc<[u8]>`, justified by Chapter 20.7's measurement.
- Per-request concurrency limits instead of rayon for fetches (11.6); "don't block the executor" (11.6); `tokio::sync::mpsc`
  for async stages (11.5); slow clients cost KiB, not a thread (L3 §5): 13.2, 13.3, 13.5, L5.
- SPEC Part XIII: runtime, worker threads, tasks, scheduling, channels, timers, TCP, UDP (13.5), synchronization,
  cancellation, backpressure, and "where Tokio ends and the OS begins" (13.1 §3, §6; 13.5 §3, §6).

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| merchant-notify runtime configuration (after May 2026) | `worker_threads(8)` set explicitly (8-CPU pods); named threads + startup config log; `max_blocking_threads(16)`; `RLIMIT_NOFILE` 1,048,576; `num_alive_tasks` gauge compared with connection count; `tokio_unstable` in a canary; heartbeat-lateness probe in load tests | 13.1 §9 |
| merchant-notify heartbeat burst (June 2026) | synchronous DNS resolver in an async fn → 2.3 s stall on one pod; per-connection 100 ms flush `interval` with default Burst → 23 catch-up flushes × ~90,000 connections; ~400 ms worker saturation; ~6,000 terminals reconnected; recovered in ~1 min; fixes: Skip, review rule "every interval states its missed-tick policy", stall-injection load test | 13.1 §10 |
| Ledger TLS sidecar | design exercise: 2 cores, 2,000 client TLS connections, 16 connections to the JVM, 1–5 ms per batch, ~1 ms CPU per handshake | 13.1 §14 |
| payments-core fraud scoring | fraud library called in-process, ~3 ms CPU per score; Black Friday forecast 2,000 charges/s ≈ 6 cores; inline scoring at 1,500/s raised unrelated p99 from 3 to 90 ms; shipped: rayon pool of 6 threads + `Semaphore(64)` (20 ms wait → `PAY_OVERLOADED`, 503 + Retry-After) + `oneshot` bounded by the request deadline; `spawn_blocking` rejected (grew to 180 threads) | 13.2 §9 |
| settlement-api merchant-ID exposure (2026) | Rust port kept Java's MDC idiom via `thread_local!`; tests ran on the current-thread runtime; production 8 workers; support's log export for merchant m-2208 contained payment IDs of three other merchants (reported as data exposure); fixes: `tracing` spans, CI lint on `thread_local!`, multi-thread tests | 13.2 §10 |
| Statement-export service | design exercise: ~50 exports/min, ledger query + ~200 ms PDF CPU + ~50 ms compression + ~500 ms upload; 12-month exports = 60× work; 4-core pods | 13.2 §14 |
| Risk-limits service (async, 2026) | 16 shard actors, mailbox 1,024 each, callers `try_send` (fail closed); policies via `watch<Arc<Policies>>`; supervisor `JoinSet` restarts a panicked shard from snapshot; load test 100K ops/s ≈ 1.6 cores, p99 0.4 ms | 13.3 §9 |
| payments-core FX-rate convoy (2026) | `tokio::sync::Mutex<HashMap<CurrencyPair, Rate>>` held across the FX fetch; rates expire after 60 s; FX provider 10 ms → ~800 ms; cross-currency p99 12 ms → >4 s; gateway 5 s timeout returned 504s; 40 min looking at "blocked threads"; fixes: std Mutex around map ops, single-flight per pair (watch), serve stale up to 5 min, review rule | 13.3 §10 |
| Ledger event bus (sidecar) | design exercise: ~3K postings/s to settlement batcher (must see all; pauses 30 s on deploy), fraud feature store (may skip, must know), balance cache (latest per account) | 13.3 §14 |
| Gateway deploy shutdown | K8s SIGTERM + 30 s grace; unready → keep accepting 5 s → cancel root token (`Connection: close` / GOAWAY) → drain ≤ 20 s → abort stragglers → explicit flushes (2 s timeouts); "requests aborted by shutdown" tracked per deploy | 13.4 §9 |
| Market-data ingest `select!` corruption (2026) | `read_frame` (two `read_exact`) raced with a 100 ms stale-quote tick; split snapshot frames → `HeaderError::BadMagic` → reconnect → bigger snapshots; feed flapped 25 min at market open; fixes: `FramedRead` codec for the 0xCAFE header, stale check in its own task, split-at-every-offset CI test, review checklist line | 13.4 §10 |
| merchant-notify admission control | per-queue policy table: `listen(4096)` + somaxconn; `Semaphore(110,000)` connections with jittered retry-after 5–15 s; handshake `Semaphore(4)` + 200-slot wait queue; per-connection `mpsc(256)` with resync marker; 256 KiB unflushed cap + `SO_SNDBUF` 64 KiB + 10 s write timeout; `broadcast(4,096)` per merchant with `Lagged` resync | 13.5 §9 |
| payments-core processor slowdown (2026) | processor 150 ms → 1.8 s for ~4 min; unbounded channel before processor calls, ~40,000 queued at peak; 11 more minutes to drain → 15-min outage; no double charges thanks to idempotency keys; fixes: deadline header (refuse with < 400 ms left: `PAY_DEADLINE_EXCEEDED`), bounded queue of 200 + `try_send` → 503 Retry-After, skip-if-expired at dequeue, 10% retry budget; July 2026 repeat: 30% shed, admitted p99 < 2.2 s, immediate recovery | 13.5 §10 |
| payout-relay (marketplace payouts team, new Rust service) | pushes payout status events to merchant webhooks; review capstone PR: `tokio::sync::Mutex` held across delivery, unbounded channel and spawn, no timeouts, limits, or accounting → 8 of 300 delivered by t = 5 s, m1 4,040 ms, 292 lost at the deploy, 0 of 8 audit lines durable; redesign: per-merchant queue 32 + 4 in flight + 500 ms × 3 attempts with backoff + spill/dead-letter/requeue, event id as idempotency key → 300 of 300 accounted for | Part XIII review |

## Verification

- `listings/part-13/`: **44 files, 47 checks, all PASS** on rustc 1.98.1 stable with Tokio 1.53.1, tokio-util 0.7.19,
  mio 1.2.3. The final full-folder run used the fixed `verify.ps1` (retry, then FAIL with "request failed") and had
  zero "request failed" lines (saved to scratch `part-13/verify-final3.txt`). Outcomes: 37 `ok` (16 release), 3
  compile errors (E0373, `error:future`, E0599 for an unstable metric), 1 `panic` (`block_in_place` on current_thread),
  and 2 test runs (`project-05-ferrite-v2.rs`: 11 tests; `review-02-webhook-relay-fixed.rs`: 4 tests). `project-05`
  runs in debug, test, and release.
- **Deterministic timing:** 15 listings run on Tokio's paused clock (`start_paused`), so their times are exact and
  repeatable. The others (real sockets, real CPU) are one run each on a shared 4-vCPU machine, labeled "one run, noisy",
  with ranges where rerun (`ch01-05` three runs; `ch05-07` two runs: 243–609 accepted; Ferrite v2 two runs per version).
- **[LIB] facts read from source at run time:** `ch01-06` (Tokio defaults), `ch04-07` (cancel-safety docs), `ch05-06`
  (mio's backlog 128). Plus probes (scratch) for metrics stability, `consume_budget`'s path, and tokio-util's 8 KiB buffers
  (backed by `ch05-08`'s measurement).
- **Artifact:** `#[tokio::main]` macro expansion via `tools/emit.ps1 -Target expand -CrateType bin` (listing `ch01-07`).
- **Block check:** every `rust` / `rust,compile_fail` block (4) is a verbatim substring of a listing (scratch
  `part-13/blockcheck.ps1`, from Part XI). `rust,ignore` blocks are excerpts naming their listing, the Tokio `spawn`
  signature (quoted from `task/spawn.rs`, whose lines 174–176 appear in `ch02-02`'s verified error output), the macro
  expansion, or sketches labeled "not a verified listing" / "Unverified sketch".
- **Corrections made during verification:** `ch05-02` counted skipped requests (dropped `oneshot` → `Err`) as goodput;
  fixed (1,200 → 1,049). The "backlog 1,024" claim was wrong: the source says 128 (fixed; became a listing and a
  measurement). `ch01-01` failed when worker threads exited during `/proc` reads (fixed).
- **Unverifiable here, labeled with commands or attribution:** `strace`/`ss`/`perf`/tokio-console (exercises), JDK facts
  (JEP 491, 505, 506; `ManagedBlocker`; Netty `HashedWheelTimer`), Seastar/Glommio/Monoio (a "Why not" box), Alice
  Ryhl's 2020 blocking guidance (attributed), kernel SYN/ACK behavior under backlog overflow ([OS], observed as
  numbers in `ch05-07`), and zstd level-19 throughput ("predicted from mechanism; measure it").

## Word count

README 803 · 13.1 5,684 · 13.2 4,536 · 13.3 4,465 · 13.4 4,426 · 13.5 4,344 · Project L5 5,095 · review 2,032 ·
answers 8,838 → **40,223** (`wc -w`, code included).

## Tooling notes

- **The Playground can read its own registry sources at run time**:
  `/playground/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/<crate>-<version>/src/...` (tokio-1.53.1,
  tokio-util-0.7.19, mio-1.2.3 seen). A listing that prints the relevant lines is the cleanest evidence for a [LIB]
  constant or doc statement.
- **Tokio's paused clock** (`#[tokio::main(start_paused = true)]` or `Builder::start_paused(true)`) makes timing listings
  exact, but never combine it with real sockets: auto-advance treats a task waiting on a socket as idle and fires
  timeouts early. Ferrite v2's tests use real time with 100–300 ms bounds instead.
- **The Playground kills programs after a few seconds** ("The operation timed out: deadline has elapsed"). Anything that
  can wait on kernel retransmits (1 s, 2 s, 4 s...) must bound its observation window (`ch05-07` uses 4.5 s).
- **1,000 localhost connections** need `RLIMIT_NOFILE` raised (`libc::setrlimit`; the hard limit reported 524,288) *and*
  an explicit listen backlog (`TcpSocket::listen(2048)`): with `bind`'s 128, connects "succeed" but accepts stall.
- **Simulations with `oneshot` replies:** a dropped sender resolves the receiver with `Err(RecvError)`, so
  `timeout(d, rx).await.ok()` counts it as success. Match `Ok(Ok(_))` explicitly.
- **`error:future`** is the needle for "future cannot be sent between threads safely" (no error code).
- Deprecated-looking paths: `tokio::task::consume_budget` and `tokio::task::coop::consume_budget` both compile on 1.53.1.
