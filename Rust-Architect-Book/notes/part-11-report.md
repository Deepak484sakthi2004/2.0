# Part 11 report

Status: **complete.** Chapters 11.1–11.7 and Project L3 were written by the original Part XI writer, which then stopped
on an API/network error, and a second writer was stopped by a coordination request before adding anything. A resuming
writer (this report) re-verified every listing, checked the existing chapters (all 14 template sections present, every
`rust`/`rust,compile_fail` block a verbatim substring of a verified listing, no broken links), and wrote Project L4's
chapter (its listing already existed and was verified), the Part README, the review with its two capstone listings,
the full answer key, and this report. Finished chapters were not rewritten.

## SUMMARY.md lines

Replace the Part XI draft block with:

```markdown
- [Part XI Overview](part-11-concurrency/README.md)
  - [11.1 Threads and the OS](part-11-concurrency/ch01-threads-and-the-os.md)
  - [11.2 Send and Sync](part-11-concurrency/ch02-send-and-sync.md)
  - [11.3 Arc, Mutex, RwLock, and Condvar](part-11-concurrency/ch03-arc-mutex-rwlock-condvar.md)
  - [11.4 Interior Mutability: Cell, RefCell, OnceCell, UnsafeCell](part-11-concurrency/ch04-interior-mutability.md)
  - [11.5 Channels and Message Passing](part-11-concurrency/ch05-channels.md)
  - [11.6 Scoped Threads and Data Parallelism with Rayon](part-11-concurrency/ch06-scoped-threads-rayon.md)
  - [11.7 The Concurrency Decision Matrix](part-11-concurrency/ch07-decision-matrix.md)
  - [Project Level 3: A Multithreaded HTTP Server from Raw TCP](part-11-concurrency/project-03-http-server.md)
  - [Project Level 4: A Concurrent Key-Value Store (Ferrite v1)](part-11-concurrency/project-04-ferrite-v1.md)
  - [Part XI Review: The Partner-Quota PR & Interview Mode](part-11-concurrency/review.md)
```

Appendix entry, after "Part X Answers":

```markdown
  - [Part XI Answers](appendix/answers-part-11.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| std::thread = 1:1 OS thread; spawn = pthread_create → clone + 2 MiB mmap stack + 4 KiB `---p` guard page, seen in `/proc/self/maps` and `/proc/self/task` (TID = PID for main) | 11.1 | XIX.4, XIX.6 |
| `'static` on spawn as an ownership statement; JoinHandle = result moved back (`Ok(T)` or panic payload `&str`/`String`); main returning kills other threads (verified: detached uploader never printed) | 11.1 | — |
| spawn+join ~28–34 µs vs hand-off ~5.7 µs (one run); oversubscription: same total, mean completion 73 vs 57 ms (64 threads vs pool of 4) | 11.1 | XX |
| Thread memory: virtual vs resident stack, kernel per-thread cost, limits (ulimit -u, threads-max, pids.max, max_map_count); Little's law pool sizing; no `interrupt()`, cooperative cancellation | 11.1 | XIX.5 |
| Send/Sync defined via aliasing XOR mutation; auto-trait table read from the compiler (inherent-const probe trick); E0277 notes as the proof chain; std's on_unimplemented RwLock suggestion | 11.2 | — |
| Disjoint capture changes which types are checked (2018 E0277 vs 2024 OK; non-move `stats.hits` bypassing a wrong `unsafe impl Sync`) | 11.2 | — |
| False `unsafe impl Sync` on Cell counters → Miri "Data race detected"; AtomicU64 version miri-ok; rusqlite `Connection` Send-not-Sync from its RefCells | 11.2 | XV |
| Rc vs Arc clone asm (`inc` vs `lock inc`); Rc 1.1 / Arc 3.5 / shared Arc 57.2 ns (4 threads) / per-thread Arc 3.6 ns | 11.2 | XIV.1 |
| Auto traits as implicit public API (a private field is semver-major); `unsafe impl` justification table; bounds for APIs (`Box<dyn Error + Send + Sync>`, `Arc<dyn Trait + Send + Sync>`) | 11.2 | XXII (semver-checks) |
| Mutex owns data; guard = run-time `&mut`; `Arc::try_unwrap(..).into_inner()` / `get_mut` need no locking | 11.3 | — |
| Futex mutex asm (`lock cmpxchg` / `xchg`, state 0/1/2, poison byte at +4, data at +8; `Mutex<u64>` = 16 B); futex-based since 1.62 | 11.3 | XIX.6 |
| Poisoning: three policies (propagate / ignore / repair + `clear_poison` 1.77) with verified output; per-lock policy table | 11.3 | — |
| Condvar bounded queue (`wait_while` consumes/returns the guard; spurious wakeups; close + drain) | 11.3 | XIII.3 |
| Linux std RwLock prefers writers: recursive read while a writer waits deadlocks (verified via `try_read`); parking_lot same, `read_recursive` | 11.3 | — |
| Guard lifetime = temporary lifetime: `if let` else-branch relock works in 2024, deadlocks in 2021; `match` holds the guard in all editions | 11.3 | — |
| Lock ordering by id (8 threads, no deadlock); non-reentrant Mutex (Java port trap); shared-map E0499 vs Java 7 / Go failures | 11.3 | — |
| UnsafeCell: the only legal mutation through `&T` (3 effects); Miri "SharedReadOnly" without it; MyCell on UnsafeCell miri-ok | 11.4 | XV.3 |
| OnceLock (init once under race, one Acquire load after) vs LazyLock (poisoned forever after a panicking init); `get_or_try_init` unstable (E0658) | 11.4 | XIV (orderings) |
| Per-request Cells folded into global atomics in Drop (4 RMWs per request) — Chapter 4.1's promise | 11.4 | — |
| Interior-mutability family sizes (RefCell 16, OnceCell<String> 24, Mutex<()> 8, RwLock<()> 12, parking_lot::Mutex<()> 1, Mutex<[u8;64]> 72) | 11.4 | — |
| Read path: Mutex<Arc> 219 ns, RwLock<Arc> 259 ns, ArcSwap 7.8 ns, plain 2.1 ns (4 readers, one run); arc-swap debts | 11.4 | XX |
| ArcSwap route table + dropper thread waiting for the last reference (0 torn reads, 50/50 freed on dropper) | 11.4 | — |
| Channels move ownership (E0382 after send); disconnection = end of stream; Empty vs Timeout vs Disconnected; try_send/SendError hand the value back | 11.5 | XIII.3 |
| std mpsc = crossbeam port since 1.67 (list/array/zero flavors, stamps with Release/Acquire, parking); Sender: Sync since 1.72; Receiver !Sync (E0277); std `mpmc` unstable (E0658) | 11.5 | — |
| crossbeam select + shutdown by dropping a Sender; owner-thread pattern with one-shot reply channels | 11.5 | — |
| Unbounded channel memory byte-exact: 72 B per 64-B message, 31-slot blocks, 7,058 KiB per 100K; bounded(1000) refuses 99,000 | 11.5 | — |
| Channel cost: ~18–24 ns/item buffered, ~5.7–7 µs lockstep (`sync_channel(1)`), ~5 ns batched, 0.4 ns no channel (one run each) | 11.5 | XX |
| `thread::scope` soundness by control flow; `thread::scoped` Leakpocalypse rebuilt → Miri dangling reference; stable 1.63 | 11.6 | XV.1 |
| Rayon: deques, LIFO owner / FIFO thieves, adaptive splitting, `join`; parallel logstat exact merge (81.9 → 36.2 scoped / 27.9 rayon ms, one run) | 11.6 | — |
| Parallel BFS level-synchronous with CAS claims (363.9 → 196.5 ms); par_iter break-even ~10K–100K cheap elements (tens of µs) | 11.6 | XX |
| Rayon closures must be `Fn + Send + Sync` (E0596 for captured mutation) vs Java parallelStream lost updates | 11.6 | — |
| Counter ranking, 5 runs: no sharing 0.3–0.6 < padded 2.9–6.7 < false sharing 5–23 ≈ shared atomic 9–19 < parking_lot 26–35 < std Mutex 38–181 ns; CachePadded = 128 B | 11.7 | XIV.4, XX.5 |
| Concurrent maps, 5 runs (4 threads, 95% reads): Mutex 160–272, RwLock 92–154, 16-shard Mutex 74–76, 16-shard RwLock 76–81 ns, owner thread 6.5–19.4 µs | 11.7 | — |
| I/O under a lock: 943 vs 3,687 req/s (3.9×); the ~1,000/s ceiling derived | 11.7 | XX.7 |
| Single-writer counter `store(load + 1)` compiles to `inc` without `lock` (verified asm) vs `fetch_add` = `lock inc` | 11.7 | XIV |
| Decision procedure + seven-design twelve-criteria matrix; Java concurrency API mapped row by row with analogy limits | 11.7 | — |
| Project L3: WorkerPool<T> (bounded queue, `try_submit -> Result<(), T>`, catch_unwind per item, Drop = graceful shutdown), HTTP/1.1 parsing with limits, 503 admission control, keep-alive, idle timeout, self-connect shutdown trick | L3 | XIII (Ferrite v2), XXI |
| Project L4 Ferrite v1: `KvStore` (&self, owned values, dyn-compatible, infallible in v1), ShardedStore (16 RwLock shards, RandomState router), recover-count-clear poisoning, line protocol + pipelining flush rule, L3 pool reused unchanged | L4 | XIII (v2), XXIII (v3), XXIV (v4, v5) |
| Ferrite v1 measured: 98.6K (debug) / 184K (release) cmds/s over loopback TCP with 8 clients; 2.6M / 18.8M ops/s in-process (one run); shard spread 347–411 of 375 ideal | L4 | XIII (compare with v2) |
| Review capstone: check-then-act over-admission bounded by limit + threads − 1 (26–27 of 20, five runs); lock-order inversion; poison → double-panic abort chain; fixed design (one critical section, act after, bounded audit writer with rollback, per-window billing) | Part XI review | — |

## Promises to later Parts

- **Part XII / XIII:** Ferrite v2 on Tokio keeps the v1 protocol and `ShardedStore` behind `Arc`; removes the
  one-worker-per-connection limit; adds a **write timeout** and a cap on unflushed reply bytes (L4 §5 gap); decide
  whether `get` returns shared immutable values (`Arc<[u8]>`/`Bytes`) so no lock is held across `.await` (L4 review Q2).
  Async I/O with per-request concurrency limits instead of rayon for fetches (11.6 §10); the "don't block the executor"
  rule mirrors rayon's (11.6 → 13.2); `tokio::sync::mpsc` for async stages (11.5 → 13.3); cancellation by dropping a
  future (11.1 §8 → 13.4); slow clients cost KiB instead of a thread (L3 §5).
- **Part XIV:** define happens-before for `join` (11.1 §4), channels' Release/Acquire stamps (11.5 §4), `OnceLock`'s
  Release/Acquire (11.4 §4), and why `Relaxed` suffices in the BFS CAS (11.6 §6); seqlocks vs `StampedLock` (11.3 §8);
  what lock-free costs to build (11.7 §7 box); single-writer counters (11.7 §9).
- **Part XV:** Miri and loom for structures with `unsafe impl Send/Sync` (11.2 §7); Stacked Borrows' "SharedReadOnly"
  (11.4 §4); the Leakpocalypse as the origin of safety-invariant discipline (11.6 §4).
- **Part XVI:** the fraud library's thread-affine native scoring handle and the owner-thread design for FFI (11.2 design
  exercise).
- **Part XIX:** `strace` of spawn (`clone`, `mmap`, `munmap`) and virtual memory for thread stacks (11.1 §4–§5); futex
  system calls (11.3, 11.7).
- **Part XX:** lock-wait/hold-time measurement, `perf sched`, off-CPU flame graphs, `perf c2c` for false sharing (11.7
  §4, §6, debugging exercise); pinning threads for contention benchmarks and the thread-placement hypothesis (11.7 §4);
  NUMA and thread-per-core (11.1 §6); `RwLock` vs `Mutex` shards for large values in Ferrite (L4 Systems extension);
  `wrk` load curves for L3 (L3 Systems extension).
- **Part XXI:** TLS (L3/L4 omissions, 21.3); HTTP/2 streams and chunked bodies (L3, 21.2); request-smuggling strictness
  (duplicate `Content-Length`), per-request deadlines against slowloris (L3 §5).
- **Part XXII:** publishing `WorkerPool` as a crate (L3 review Q7, 22.1); Tower (22.3); `cargo-semver-checks` for lost
  auto traits (11.2 §7); a binary-safe framing for Ferrite (L4 omissions; Project L6).
- **Part XXIII:** Ferrite v3 adds a **fallible trait next to** `KvStore` rather than changing it (L4 §3), reusing
  `store_semantics` as a conformance suite (L4 §4); `update`/`INCR` logs the result, not the closure (L4 extension);
  transactions and cross-shard atomicity (23.6).
- **Part XXIV:** Ferrite v5 replaces `RandomState` routing with a stable hash or range partitioning behind a versioned
  partition map (L4 §2, review Q5); the leader executes read-modify-writes and replicates results (L4 extension);
  partitioning large state (11.7 §5).

## Promises kept

- **Cell → atomics transition (4.1):** 11.4 §3, per-request `Cell`s folded into global atomics in `Drop`
  (`requests=10000 lookups=40000 ...`).
- **Arc swap for catalogs and route tables (3.1, 3.3):** 11.4 §9, `ArcSwap` + dropper thread (0 torn reads, 50/50 old
  tables freed on the dropper), and the answer to Chapter 3.3's catalog design exercise.
- **Parallel `logstat` with per-thread Summary merge (L1 review Q7):** 11.6 §3, three strategies producing identical
  summaries.
- **Poisoning policy per lock, pools that survive job panics, Ferrite v1 panic policy (8.3):** 11.3 §7's table; L3's
  per-item `catch_unwind` (test `workers_survive_panics_and_drop_drains_the_queue`); L4's recover-count-clear policy
  with test `a_poisoned_shard_is_recovered_once`.
- **Interior mutability in full (4.1):** 11.4.
- **The concurrency decision matrix (Part I):** 11.7 §7.
- **Send/Sync manual impls and why they're unsafe; disjoint capture (5.3, 6.4):** 11.2 §4, §7 (Miri data race; 2018 vs
  2024 capture).
- **Concurrent maps measured (9.3), parallel `chunks_mut`/rayon and level-synchronous parallel BFS (9.1, Interlude),
  channels as cross-thread queues (9.4):** 11.7 §3, 11.6 §3 and §6, 11.5.
- **Measure atomic vs mutex vs no-sharing (1.3):** 11.7 §3, five runs, ranking confirmed with two refinements.
- **2 MiB default stack and guard page (1.2, Part IX interlude):** 11.1 §4 (`/proc/self/maps`).
- **Rc !Send (3.6), Cell !Sync with the RwLock/AtomicU32 suggestion (4.1):** 11.2 §3 table and E0277.
- **Shared-map failure across languages (1.2):** 11.3 §8 (E0499).
- **Mutex<T> owns data vs `synchronized` (1.3); TOCTOU and deadlock (1.3):** 11.3 §2, §7 (lock ordering listing); the
  review capstone's check-then-act race.
- **Leakpocalypse (1.3 box):** 11.6 §4, rebuilt and caught by Miri.
- **Java synchronized / ReentrantLock / volatile / AtomicInteger / ConcurrentHashMap / ExecutorService mapping:**
  11.7 §8 (and 11.3 §8, 11.5 §8).
- **Rayon vs Java parallel streams (10.4 forward):** 11.6 §8.
- **Ferrite v1 contract (AUTHORING-BRIEF):** implemented exactly (trait signatures, `ShardedStore` type, keyed
  `hash(key) % N`, the v1 protocol and replies), served by the L3 pool.

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Webhook dispatcher | Java: platform thread per outbound call; a partner's 40-min outage → ~28,000 threads → `OutOfMemoryError: unable to create native thread`, took healthy partners down. Rust: fixed pool per partner tier (named threads), bounded queue per partner spilling to a durable retry table, `Builder::spawn` errors fail startup, `stack_size(512 KiB)` (measured frame size × 4), pool size by Little's law | 11.1 §9 |
| Settlement batch upload | Detached uploader thread; `main` returned; upload never ran; partner reconciliation flagged a missing batch next morning; fix: join what you spawn | 11.1 §10 |
| Merchant-portal report renderer | Design exercise: 300 req/s, ~40 ms ledger I/O + 15 ms PDF CPU, 8 cores | 11.1 §14 |
| Gateway geo-blocking middleware | `RefCell` cache rejected (E0277) in the shared middleware chain; chose an immutable GeoIP prefix table published with `ArcSwap`; the Java version had shipped with an unsynchronized `HashMap` | 11.2 §9 |
| Market-data fan-out FeedStats | Rust port: `Cell<u64>` counters + `unsafe impl Sync` ("only a metric"); gap counter under-reported ~half; caught when Miri-in-CI was adopted; fix `AtomicU64` Relaxed + rule: every `unsafe impl Send/Sync` needs a SAFETY comment naming the synchronization | 11.2 §10 |
| Fraud native scoring engine | Design exercise: thread-affine FFI handle, owner-thread submission | 11.2 §14 |
| Ingestion pipeline backpressure | Bounded blocking queues between decode→validate→enrich→publish (50,000 events/s × 0.2 s = 10,000 slots); `close()` drives shutdown; `producer_waits` and depth metrics | 11.3 §9 |
| Account-service balance cache | Java `ReentrantReadWriteLock` nested read ported to std `RwLock` → deadlock every few hours under refresh load; fixes: pass data down, `ArcSwap` snapshots, `read_recursive` as documented exception; review rule "no lock in a helper of a locked type" | 11.3 §10 |
| Ledger client connection pool | Design exercise: 64 connections, 400 worker threads | 11.3 §14 |
| Payment-provider config | `static LazyLock<ProviderConfig>` from env; misspelled variable → first request panicked, then every request panicked ("previously been poisoned") → 100% errors until restart; health checks passed; fix: eager config in `main`, pass `Arc<Config>` | 11.4 §10 |
| Pricing catalog (concurrent) | Design exercise: ~200 MB, 32 quote threads, 50K quotes/s, updates every few seconds | 11.4 §14 |
| payments-core audit trail | One writer thread owns the durable sink; batches 500 records or 20 ms, one fsync per batch; bounded crossbeam channel of 10,000; `send_timeout(50 ms)` → Transient 503 ("no audit, no charge"); metrics via separate `try_send` drop-and-count channel (`metrics_dropped_total`) | 11.5 §9 |
| Gateway access-log shipper | Unbounded `mpsc` link; collector 20-min outage; ~7,300 lines/s/pod, ~250 B each, ~2 MB/s/pod heap → OOM kills after ~15 min; fix `bounded(200_000)` (~30 s, ~60 MB) + `try_send` drop-and-count (`access_log_dropped_total`), depth exported | 11.5 §10 |
| Merchant notification service | Design exercise: ~2,000 events/s, e-mail provider 500 calls/s, 40,000 webhook endpoints, inbox DB write ~2 ms | 11.5 §14 |
| Settlement reconciliation (parallel) | ~40M records on a peak day; 70 min single-threaded → ~14 min on 8 cores with rayon; partition by merchant (largest ~4% of volume), 8× more partitions than cores, integer sums, sorted mismatches, byte-for-byte comparison on a week of production files | 11.6 §9 |
| Merchant-portal statement pages | `par_iter` over blocking fetches (30–50 docs, 20–200 ms): p50 better, p99 2.5 s → 9+ s; fix async fetches with a concurrency limit, dedicated rayon pool for CPU work, review rule | 11.6 §10 |
| Fraud-model backfill | Design exercise: ~1.2B rows, 90 days, daily files of 10–20 GB, 32 cores, 128 GB RAM | 11.6 §14 |
| Gateway per-route metrics | ~5,000 routes; Mutex<HashMap> → atomics in a startup-built map (hot lines + false sharing) → per-worker single-writer counters (`inc` without `lock`), summed on the 10 s scrape | 11.7 §9 |
| Risk-limits audit under shard lock | Audit write inside a shard's critical section; network volume fsync stalls 20–50 ms; 16 shards → one stall blocked ~6% of traffic; p99 0.4 ms → 40+ ms; fix: I/O after the guard + compensating rollback; lock hold > 1 ms logs the call site | 11.7 §10 |
| Session cache (Rust) | Design exercise: 2M sessions, 100K lookups/s, 5K updates/s, 32 threads, "touch" on 60% of lookups, expiry sweep once a minute | 11.7 §14 |
| Gateway partner-quota tracker | Part XI review capstone: process-wide per-partner quotas per window; PR with 16 defects (over-admission 26–27 of 20, 53 alerts, fail-open default 100, lost billing batch, lock-order inversion, poison → double-panic abort); redesign listing | Part XI review |

## Verification

- `listings/part-11/`: **55 files, 69 checks, all PASS** on rustc 1.98.1 stable, edition 2024 (final full run saved to
  scratch `part-11/verify-final.txt`). Outcomes: 46 `ok` (16 of them release), 5 `test`, 3 `build` (release, asm listings),
  10 intended compile errors (E0277 ×5, one of them the edition-2018 check `debug@2018` of `ch02-07`; E0658 ×2;
  E0382; E0499; E0596), 3 `miri` (UB reported: data race from a false `unsafe impl Sync`, write through `&T` without
  `UnsafeCell` ("SharedReadOnly"), Leakpocalypse dangling reference), 2 `miri-ok` (AtomicU64 counter, MyCell).
- Both projects run real TCP servers on `127.0.0.1` inside one Playground program (ephemeral port via `bind(":0")`).
- **Artifacts** (`tools/emit.ps1`, release asm): `clone_rc`/`clone_arc` (`ch02-08`), `bump` = Mutex lock/unlock
  (`ch03-07`), single-writer vs `fetch_add` counter (`ch07-04`).
- **Measurements:** one run each and labeled noisy, except 11.7's counter ranking and map options (five runs, reported
  as ranges). The review capstone's over-admission was run five times (26 or 27 against 20).
- Every `rust`/`rust,compile_fail` block in the Part XI chapters (15) was machine-checked as a verbatim substring of a
  verified listing. `rust,ignore` blocks are named excerpts, the std `scope` signature quote [LIB], or debugging/failure
  code meant to be diagnosed. Answer-key sketches are labeled "Unverified sketch".
- **Unverifiable here, labeled with commands:** `strace`, `perf stat`, `perf sched`, `perf c2c`, `taskset`/core
  pinning, `wrk` (exercises); the thread-placement explanation for false-sharing variance (11.7 §4, "not verified
  here"); `dashmap` (not on the Playground); JDK facts attributed to the JDK docs; kernel facts tagged [OS].

## Word count

| File | Words |
|---|---|
| README.md | 900 |
| ch01-threads-and-the-os.md | 3,839 |
| ch02-send-and-sync.md | 4,186 |
| ch03-arc-mutex-rwlock-condvar.md | 4,778 |
| ch04-interior-mutability.md | 3,621 |
| ch05-channels.md | 5,045 |
| ch06-scoped-threads-rayon.md | 4,443 |
| ch07-decision-matrix.md | 5,016 |
| project-03-http-server.md | 3,215 |
| project-04-ferrite-v1.md | 4,421 |
| review.md | 1,954 |
| answers-part-11.md | 13,386 |
| **Total** | **54,804** |

(`wc -w`, code included.)

## Tooling notes

- **Localhost TCP works on the Playground.** `TcpListener::bind("127.0.0.1:0")` plus in-process client threads runs
  full client/server tests in one program (both projects). A blocking `accept` can be woken for shutdown by connecting
  to the server itself after setting a flag.
- **Make races reliable before quoting them.** A sleep inside the race window (review-01's 1 ms audit write) turned
  over-admission into the common case (26–27 of 20 in five runs), and the text quotes the run count. Bound the outcome
  analytically where possible (here: limit + threads − 1).
- **`try_lock` as a stand-in for `lock`** lets a listing report "this would deadlock" instead of hanging the Playground
  (`ch03-03`, `ch03-09`).
- **Unused `pub` items in a bin crate still warn** (dead code). Exercise them in `main`, or `verify.ps1 -ShowStderr`
  will show warnings on an `ok` listing.
- **Don't inject `\n` into Rust string literals with bash `sed`**: it becomes a real newline inside the literal. Use the
  Edit tool for escapes.
- **Verbatim-block check**: a small PowerShell script (scratch `part-11/blockcheck.ps1`) extracts `rust`,
  `rust,compile_fail`, and `rust,no_run` blocks, strips `// verify:` lines from listings, normalizes CRLF, and checks
  substring containment. It's worth promoting to `tools/` (integrator's decision).
- Contention benchmarks varied up to 5× in absolute terms between Playground runs while rankings held: run them five
  times and report ranges.
