# Part 12 report

Note: the original Part XII writer died on an API/network error, and a resuming writer was stopped in a coordination
handover; together they had written the README, Chapters 12.1–12.5, and all listings through `ch06-*` and `review-*`.
A finishing writer (this report's author) verified every listing, checked the existing chapters (all 14 template
sections, clean endings, `rust`/`rust,compile_fail` blocks verbatim from verified listings), labeled one unlabeled
excerpt in 12.3, added a paragraph + listing to 12.3 for the RPIT-capture promise, wrote Chapter 12.6 (with one new
listing), the review, the full answer key (with one new listing), corrected the README's listing counts, and wrote this
report.

## SUMMARY.md lines

Replace the Part XII draft block with:

```markdown
- [Part XII Overview](part-12-async/README.md)
  - [12.1 Why Async Exists: From C10K to C10M](part-12-async/ch01-why-async.md)
  - [12.2 The Future Trait and poll](part-12-async/ch02-future-poll.md)
  - [12.3 async fn Becomes a State Machine](part-12-async/ch03-async-state-machine.md)
  - [12.4 Pin and Self-Referential Futures](part-12-async/ch04-pin.md)
  - [12.5 Wakers and Executors: Build One from Scratch](part-12-async/ch05-wakers-executors.md)
  - [12.6 OS Threads vs Green Threads vs Async Tasks](part-12-async/ch06-threads-green-async.md)
  - [Part XII Review: The merchant-notify PR & Interview Mode](part-12-async/review.md)
```

Appendix entry, after "Part XI Answers" (or after the last answers line present):

```markdown
  - [Part XII Answers](appendix/answers-part-12.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| Thread per connection measured: `EAGAIN` at connection 503 (sandbox task limit); 2,064 KiB virtual / ≈9.5 KiB resident per thread; glibc malloc arenas (64 MiB each, up to 8 × cores) explain the first-100-threads jump | 12.1 | XIX.5 |
| Nonblocking spin (19,617 `read` calls for 4 bytes); select/poll/epoll/kqueue table; epoll level- vs edge-triggered verified natively and under Miri (Miri models epoll + socketpairs) | 12.1 | XXI.1 |
| mio C10K: 10,000 connections, 1 server thread, 8,807 `epoll_wait` wakeups, RSS +264 KB; `RLIMIT_NOFILE` raised via libc; kernel sockstat | 12.1 | XIII.1 |
| Completion I/O (io_uring, IOCP) needs owned buffers because drop = cancel (`tokio-uring`, `glommio`, `monoio`) | 12.1 | XXI, XXIII.1 |
| Kernel limits for threads (`pids.max`, `vm.max_map_count`, `threads-max`/`pid_max`, `RLIMIT_NPROC`, `RLIMIT_NOFILE`); Java "unable to create native thread" | 12.1 | XIX.6 |
| RFC 230 → `Future` in `core`, async as a library; timeline 2014–2025 (async-std discontinued 2025 [VERSION]); C10K (Kegel 1999), C10M (Graham 2013), thread-per-core runtimes | 12.1 | — |
| `Future` contract table (Pending obligation, no blocking, no poll after Ready, spurious polls); lazy futures (16-byte `charge` future); `unused_must_use` on futures as an error | 12.2 | — |
| Lost wakeup three ways (NeverRegisters / RegistersOnce hang, RefreshesEveryPoll ok); `Waker::will_wake`; check-and-publish atomically | 12.2 | XIII.3 |
| `join`/`select` by hand; cancellation = drop (verified with a `Drop` reporter); `CompletableFuture` comparison (`cancel(true)` doesn't interrupt) | 12.2 | XIII.4 |
| In-flight waiter coalescing (`WaitFor`): wake after releasing the lock; never hold a lock across `.await` | 12.2 | XIII.3 |
| Future sizes (1 B `nothing` … 1,026 B array across await, 8 B dead before it, 1,028 B sequential vs 2,072 B joined, 16 B boxed); hand-written enum 24 B vs compiler 40 B | 12.3 | — |
| Real coroutine MIR (`coroutine layout`, variants Unresumed/Returned/Panicked/SuspendN, `storage_conflicts`), resume `switchInt`, "`async fn` resumed after completion" panic; release asm jump table + waker vtable call `[rcx + 16]` | 12.3 | XVIII.4 |
| `async fn` arguments stored twice (prefix upvar + saved local): 64 B vs 40 B (`answers-ch03-sizes.rs`) [RUSTC] | 12.3 answers | XVIII.4 |
| `!Send` futures verified: `Rc`, `MutexGuard`, `Box<dyn Error>` across `.await` (error has no code: `error:future` needle); fixes | 12.3 | XIII.2 |
| E0733 async recursion; `Box::pin` = 1 × 16-byte allocation per level (counting allocator) | 12.3 | — |
| `async fn` in traits (1.75): E0038 for `dyn`; Send bound problem (E0277 + compiler's `-> impl Future + Send` suggestion); RTN (RFC 3654) unstable; async closures (1.85) with `AsyncFn` | 12.3 | XXII.3 |
| Returned future captures all argument lifetimes (E0502 before polling, `ch03-18`); RPITIT always captures, edition 2024 extends to free fns | 12.3 | — |
| 1 MiB future overflows a 512 KiB stack (debug + release); cancellation safety + guard pattern (`payout_v1`/`v2`) | 12.3 | XIII.4 |
| Self-referential struct dangling under Miri; pinned version miri-ok (Stacked + Tree Borrows); E0277 "cannot be unpinned"; every async future `!Unpin` (even `async { 1 + 1 }`) | 12.4 | XV.2 |
| Pin guarantee (no move + drop guarantee); `Pin::new` / `pin!` / `Box::pin` / `new_unchecked` table; `Pin<&mut F>` and `Pin<Box<F>>` 8 B | 12.4 | — |
| Intrusive waiter list (40-byte node inside the future), miri-ok under both models; without the `Drop` unlink → use-after-free under Miri | 12.4 | XIII.3, XV.4 |
| Hand-written pin projection (`WithBudget`) + the five structural-pinning obligations; one safe `impl Unpin` → UAF (`ch04-06`); `pin-project-lite` + E0119 on a manual `Unpin` | 12.4 | XV.1 |
| `&mut T` for `T: !Unpin` loses `noalias` and `dereferenceable` in LLVM IR; `UnsafePinned` (RFC 3467) [VERSION] | 12.4 | XVIII.6 |
| `block_on` with the park/unpark token (Miri-clean); `RawWaker` vtable by hand (16 B; clone/wake/wake_by_ref/drop accounting, Miri) | 12.5 | — |
| Executor: run queue + `queued` dedup flag cleared before the poll; timer thread + `BinaryHeap` + `Condvar`; `JoinHandle` as a future | 12.5 | XIII.1 |
| Reactor on mio: 100 clients, 1 thread, 201 tasks, 302 polls, 2 `epoll_wait` calls (derived from the code) | 12.5 | XIII.1 |
| Spawn cost: ≈158 ns / 2 allocations per task vs ≈36 µs per OS thread spawn + join (one run) | 12.5 | XX |
| Deterministic simulation executor (virtual time, seeded scheduling): check-then-act double charge in 183 of 1,000 seeds, replayable; FoundationDB attribution; turmoil/madsim | 12.5 | XXII.7, XXIV |
| Wake dedup measured (n = 1,000: 252,000 vs 2,500 child polls); `futures::join_all` switches to `FuturesOrdered` above 30 children [LIB] | 12.5 | XIII |
| Memory per unit measured: thread 2,064 KiB virtual / 9.6 KiB resident vs task 247 B heap / 0.28 KiB resident (34× resident, not 8,500×); `LocalPool` spawn-queue high-water mark (3 MiB) retained, not leaked | 12.6 | XIX.5 |
| Round trip: threads 7.7–9.6 µs vs async tasks 115–118 ns (three runs) | 12.6 | XX.7 |
| Blocking on the executor: 194 ms heartbeat stall vs 1 ms with `spawn_blocking`; CPU-bound: 143 ms vs 1 ms with `yield_now` every ≈140 µs (1,025 yields, no measurable cost); coop budget doesn't cover pure computation | 12.6 | XIII.2 |
| Stackful runtimes: Go (copying stacks 1.3, 2 KiB 1.4, adaptive start 1.19, netpoller, P handoff, async preemption 1.14, cgo stack switch); Java VT (JEP 444 mount/unmount, heap stack chunks, no time-slicing, pinning, JEP 491 in JDK 24); Erlang reductions; why M:N failed in C and in pre-1.0 Rust | 12.6 | — |
| Function coloring (E0728); `Handle::block_on` inside a runtime panics ("Cannot start a runtime from within a runtime"); ways-across table (`spawn_blocking`, `block_in_place`) | 12.6 | XIII.2 |
| Cancellation compared: Rust drop, Java interrupt (`Thread.stop` throws since JDK 20), Go `context`, Erlang exit signals | 12.6 | XIII.4 |
| Five-model × ten-criterion concurrency decision matrix; "choose by where the waiting is and who owns the code that waits" | 12.6 | XXI, XXIV |

## Promises to later Parts

- **Part XIII:** `tokio::sync::Semaphore` / `Notify` compared with the 12.2 and 12.4 design exercises (waiter lists,
  lost-permit bug); Tokio's LIFO slot, cooperative budget, timing wheel, eventfd waker, `block_in_place`, blocking pool
  (default cap 512); `tokio::sync::Mutex` across `.await` "and why you usually still shouldn't" (12.3); the "there is no
  reactor running" panic (12.1); per-method cancellation safety (12.3); Ferrite v2's connection limit and graceful
  shedding (12.1 design exercise and §10); merchant-notify admission control and handshake budget (12.6 §10).
- **Part XV:** unsafe code whose soundness depends on safe code in the same module (12.4 §4, `ch04-06`); Miri in CI
  with both aliasing models for primitives.
- **Part XVIII (18.4):** the coroutine MIR state transform, prefix/overlap layout, and arguments stored twice (12.3).
- **Part XX:** tail latency from long polls; `perf stat -e context-switches,cpu-migrations` on the ping-pong (12.6
  Systems exercise); allocator effects on per-task memory.
- **Part XXI:** owned-buffer completion I/O (io_uring) for network servers; socket-buffer tuning at scale (12.1).
- **Part XXII (22.7):** deterministic simulation testing (turmoil, madsim) and Miri in CI (12.5 §9).
- **Part XXIV:** deterministic simulation for Ferrite's replication tests (12.5 §9 states it).

## Promises kept

- **PROGRESS "Part XII (Async)":** async recursion needs `Box::pin` (12.3, `ch03-08`/`ch03-09`, also the IX interlude
  promise); why async/await fits "no runtime" (12.1 §7); stackless future sizes (12.3 §3, measured); RFC 230 green
  threads removal (12.1 §7); Go G-M-P / netpoller vs stackless (12.6 §4); `async fn` in traits and dyn compatibility
  (12.3, promised by 6.4); RPIT capture rules for async return types (12.3, new paragraph + `ch03-18`, promised by 6.1 and
  7.3).
- **Chapter 8.2 → XIII (delivered early):** `Box<dyn Error>` without `Send + Sync` held across `.await` makes a future
  `!Send` (12.3, `ch03-06`).
- **Chapter 1.2 / Part I:** stackful vs stackless concurrency, Java virtual threads compared properly (12.6 §8).
- **Chapter 11.1:** "Different mechanism: stackful vs stackless (Chapter 12.6)" (12.6 §2, §4).
- **Internal forward references in 12.1–12.5** to 12.6 (memory per unit ≈250 B, switch cost ≈8 µs vs 0.1 µs, blocking
  measured, `block_on` never from async code, virtual threads, `spawn_blocking`, CPU-bound tasks can't be interrupted):
  all delivered with the numbers 12.1 already quoted (247 B; 7.7–9.6 µs vs 0.12 µs).

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| merchant-notify | Pushes payment/refund/payout events to merchant dashboards and POS terminals; ~600,000 concurrent connections at peak, >95% idle; Java on Netty today: 24 pods × ~25,000 connections; Rust pilot (2026) targets 100,000 per pod | 12.1, README |
| merchant-notify 2019 reconnect storm (Java) | Blocking sockets, thread per connection, ~8,000 connections per pod; terminal firmware bug reconnected without closing; >30,000 connections per pod; `OutOfMemoryError: unable to create native thread` with 40% heap; real limit `vm.max_map_count`; fixes: per-IP caps + 90 s idle reaping, Netty (2020), admission control | 12.1 §10 |
| payments-core in-flight coalescing (2026) | Duplicate `POST /charges` with the same key on the same instance now waits for the first attempt (bounded by its deadline) instead of 409; `WaitFor` leaf future; other instances still get 409 | 12.2 §9 |
| merchant-notify `AckWait` incident (March 2026) | Register-once waker; after moving deliveries into spawned tasks, ~3% of deliveries showed exactly 30 s ACK latency (client resend timeout); fixes: refresh waker, two-waker test, review rule for hand-written futures | 12.2 §10 |
| merchant-notify future-size budget | `connection_loop` held `[u8; 65536]` + `[u8; 16384]` across awaits: ~80 KiB per connection, RSS ~8 GB at 100,000; fixes: heap buffers starting at 512 B, CI `size_of_val < 2048`, `Box::pin` in debug tests | 12.3 §9 |
| Marketplace payouts reservation incident | Timeout dropped the payout future between reserve and pay-out; 212 sellers' payouts stuck in "reserved" until next-morning reconciliation; fix: reservation guard with `Drop` | 12.3 §10 |
| merchant-notify merchant fan-out | Per-merchant `Mutex<Vec<Waker>>` replaced by `tokio::sync::Notify` + per-merchant event sequence numbers; review rules: no hand-written projections in app crates, Miri (SB + TB) in CI for primitives | 12.4 §9 |
| payments-core `Deadline` incident (February 2026) | `impl<F> Unpin for Deadline<F>` added to satisfy `Pin::new`; later `swap_remove` in a batching `Vec` moved polled futures; `SIGSEGV` in the allocator about once a day per pod; two weeks of investigation; found with Miri + a self-borrowing inner future; fixed with `pin-project-lite` | 12.4 §10 |
| payments-core simulation executor | Cache fast path (check, await fraud score, record key) double-charged in 183 of 1,000 seeds; v2 reserves first; first simulator lacked wake dedup and took minutes on a 1,000-merchant settlement fan-in | 12.5 §9–§10 |
| Concurrency-model review (2026) | merchant-notify → Rust async (Tokio); ledger stays on a platform-thread pool (concurrency capped by its DB pool); partner webhook dispatcher (Java, thousands of concurrent outbound calls taking seconds) → Java virtual threads with a per-partner `Semaphore`; fraud feature library stays synchronous (no async) | 12.6 §9 |
| merchant-notify reconnect-handshake incident (May 2026) | 4 pilot pods at up to 100,000 connections, Tokio multi-thread with 8 workers; ~150 ms password hash inside the async handshake; ~20,000 terminals reconnected to one pod; ≈53 handshakes/s; dashboards drop after 3 missed 5 s heartbeats; pod drained; fixes: `spawn_blocking` behind a 4-permit semaphore, resumption tokens, handshake admission with retry-after + jitter, long-poll guard in load tests | 12.6 §10 |
| merchant-notify PR #412 | Part XII review capstone: ACK-based at-least-once delivery path with 17 defects; fixed version with injected `Clock`, register-before-send, `Outcome` enum, 5 tests | Part XII review |

## Verification

- `listings/part-12/`: **59 files, 75 checks, all pass** on rustc 1.98.1 (edition 2024). The final full-folder run
  (scratch `part-12/verify-final.txt`) covered 58 files / 74 checks; `ch03-18-future-borrows-args.rs`, added while it
  ran, was verified individually (E0502).
- Outcomes: 40 `ok` (incl. 2 for the answer-key sizes listing, debug and release), 3 `build`, 1 `test` (5 tests),
  2 `crash` (stack overflow), 2 `panic`, 13 compile errors (E0038, E0119, E0277 × 4, E0502, E0728, E0733,
  `error:future` × 3, `error:unused_must_use`), 12 Miri runs (9 `miri-ok` incl. 2 under Tree Borrows, 3 `miri`
  catching UB: dangling/use-after-free).
- Artifacts (from the earlier writers, via `tools/emit.ps1`): debug MIR of the `step` coroutine, release asm of its
  resume function, release LLVM IR of `bump_plain` vs `bump_pinned`.
- Measurements, one run each on the shared Playground unless stated: thread/task memory (`ch06-01`), round trip
  (`ch06-02`, three runs: 7.7–9.6 µs vs 115–118 ns), blocking stall (`ch06-03`), CPU starvation (`ch06-06`, calibrated
  at run time), spawn cost (`ch05-05`), sizes (exact).
- Unverifiable here, labeled in text: Go / JVM switch and memory figures (attributed to release notes / JEP 444 /
  Erlang Efficiency Guide, or order of magnitude); `tokio-console` (not on the Playground); `perf stat` (exercise with
  the local command); the `LocalPool` spawn-queue explanation of the retained 3 MiB (an inference from the `futures`
  source); `turmoil`/`madsim` (not on the Playground).
- All `rust`/`rust,compile_fail` blocks in the Part's chapters and review were machine-checked as verbatim substrings
  of verified listings (script: scratch `part-12/blockcheck.pl`). `rust,ignore` blocks are labeled excerpts, debugging
  exercise code, or (answer key) one labeled unverified sketch.

## Word count

README 708 · 12.1 4,516 · 12.2 3,803 · 12.3 5,251 · 12.4 5,302 · 12.5 4,906 · 12.6 6,269 · review 1,913 · answers
9,196 → **41,864** (`wc -w`, code included).

## Tooling notes

- **Calibrate CPU-bound async loops with the async function itself.** The same arithmetic loop ran 2.6× slower inside an
  `async fn` (its locals live in the future's memory) than in a plain loop, so a calibration done outside overshot a
  150 ms target to 389 ms (`ch06-06`).
- **`futures::executor::LocalPool` keeps its spawn-queue capacity** (3 MiB after 100,000 spawns before the first run).
  Account for it, or measure after a warm-up batch, when computing per-task memory.
- **Timing listings with two very different magnitudes (µs vs ns): run three times and quote the range** (`ch06-02`
  varied 7.7–9.6 µs; the task side was stable at 115–118 ns).
- **`tokio::runtime::Handle::block_on` inside a runtime** panics with "Cannot start a runtime from within a runtime";
  use that as the `panic` needle. `futures::executor::block_on` doesn't detect it (it deadlocks or blocks instead).
- **"future cannot be sent between threads safely" has no error code**: use `error:future`.
- Localhost TCP and mio work on the Playground; raise `RLIMIT_NOFILE` with `libc::setrlimit` for 10,000 connections;
  the sandbox caps a process at about 500 threads. Miri models epoll and socketpairs, but not TCP.
- The heartbeat pattern (a task that sleeps 10 ms in a loop and records its worst lateness) is a cheap, reliable probe
  for executor stalls; see `ch06-03` and `ch06-06`.
