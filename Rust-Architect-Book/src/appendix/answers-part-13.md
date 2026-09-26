# Appendix A — Answer Key: Part XIII

> Model answers. Write yours first. Where several answers are defensible, the key says so. Outputs quoted here come from
> the verified listings in `listings/part-13/` (rustc 1.98.1, Tokio 1.53.1); "predicted" marks reasoning you should check.

---

## Chapter 13.1 — Tokio's Architecture: Scheduler, I/O Driver, Timers

### Interview & architecture questions

**1. What the kernel sees.** `N` worker threads (named `tokio-rt-worker` by default), plus any blocking-pool threads
that exist at the moment; one epoll instance (possibly visible twice, as a descriptor and its `dup`); one eventfd used to
wake a worker blocked in `epoll_wait`; the 10,000 socket descriptors, each registered with the epoll instance; and, with
the signal driver enabled, a process-wide socketpair (listing `ch01-01-os-view.rs`). Memory for 10,000 task cells and
futures is ordinary heap to the kernel. It sees no tasks, no queues, and no timers (timers become the `epoll_wait`
timeout).

**2. From "readable" to "running".** Kernel: the socket receives data, the TCP stack queues it, and the socket's epoll
entry becomes ready. Tokio: a worker polls the I/O driver (between tasks every 61 ticks, or when it's idle and parked in
`epoll_wait`), receives the event, finds the socket's `ScheduledIo` record, and calls the wakers of the tasks waiting for
read readiness. Each wake pushes a task onto a run queue (the waking worker's LIFO slot or local queue). A worker pops the
task and polls it, and the task's `read` future now finds data (a `read(2)` that returns bytes instead of `EAGAIN`). The
kernel's part ends at "this descriptor is ready".

**3. The LIFO slot.** Helps message passing: a task that wakes another and then waits (request/response between tasks)
gets the woken task run next on the same core, with its data still in cache. Listing `ch01-04` measured 1,000 of 1,000
children on the parent's worker. The failure: the slot can't be stolen, so if the task that filled it then blocks its
worker, the woken task waits for the whole block (300 ms in experiment 3), while three workers sit idle. One more spawn
pushed it into the stealable queue, and it ran after 28 µs.

**4. Which starves.** The `recv` loop doesn't (for long): each `recv().await` spends cooperative budget, and after 128
operations it returns `Pending`, so the task yields (listing `ch01-03` row A: heartbeat 2 ms late). The loop over
`std::future::ready` futures does starve its neighbors: those futures know nothing about Tokio's budget, never return
`Pending`, and the whole loop runs as one poll (row C: 399 ms). Fix it with `tokio::task::coop::consume_budget().await` or a
periodic `yield_now()`.

**5. `sleep(200 µs)`.** Tokio's timer has 1 ms ticks and rounds deadlines up, and the worker has to wake from
`epoll_wait`, so each sleep lasts at least ~1 ms (listing `ch01-02`: 1 µs → 1.06 ms, 500 µs → 1.59 ms). The loop can't
exceed ~1,000 iterations per second. Alternatives: pace by *batch* (do 5 items per 1 ms tick); compute deadlines from a
start time with `sleep_until(start + n × period)` and let several items share a tick; spin with `std::hint::spin_loop` on a
dedicated thread for true sub-millisecond pacing; or use a token bucket that permits bursts.

**6. Current-thread in production.** When the service is small and mostly waits (a sidecar, an agent, a CLI); when
`!Send` state is central and you'd otherwise need a `LocalSet` anyway; in thread-per-core designs where you run one
current-thread runtime per core and partition work across them (with the load-imbalance risk that brings); and when you
need determinism (tests, simulation). It also has the cheapest spawn and wakeup (listing `ch01-05`: ~300 ns vs ~800 ns).

**7. Why multi-thread spawns cost more.** A spawn on 4 workers can land the task on another core: atomic queue operations
instead of single-threaded ones, cross-core cache-line transfers for the task cell, and sometimes a parked worker to wake
(a futex or eventfd syscall). It's a good trade because it buys parallelism: a CPU-heavy mix uses every core, and one
stuck task doesn't stop the rest. At ~800 ns per spawn, a service can spawn a million tasks a second per core before
spawning matters.

**8. `MissedTickBehavior::Burst`.** After a stall longer than a period, the interval fires the missed ticks immediately,
one after another, to keep the long-run *count* of ticks right (listing `ch01-02`: three ticks at 46 ms). Right for
"sample N times per second on average" where each tick is cheap and independent. Wrong for heartbeats, flushes, and
anything idempotent or expensive, where one catch-up tick is enough: use `Skip` (realign to the schedule) or `Delay`
(restart the period from now).

**9. Boxing large futures.** `spawn` moves the future by value through several function frames before it reaches the heap
allocation. A 100 KiB future would need 100 KiB of worker stack per frame in that call chain, and a debug build could
overflow the stack. So Tokio boxes futures larger than `BOX_FUTURE_THRESHOLD` (16,384 bytes in release, 2,048 in debug,
listing `ch01-06`) first, costing a second allocation (listing `ch01-08`: 2.00 allocations for the 16 KiB future). What
it tells you: futures that large hold big buffers across `.await`s. Chapter 12.3's advice applies: move the buffer to the
heap yourself, and put a `size_of_val` budget in CI.

**10. Netty ports.** The assumption "my handler always runs on the same thread". Netty pins a channel to one event loop,
so handler state is thread-confined and thread-locals are per-connection. A Tokio task migrates between workers at any
`.await`, so state must be `Send` (the compiler checks), and `thread_local!` values change under the task (the compiler
doesn't check). Chapter 13.2 §10 is the incident.

### Debugging exercise (`export_loop`)

`interval` defaults to `MissedTickBehavior::Burst`. When a push takes 10–30 s because the collector is slow, 10–30 ticks
are missed. When the push returns, `tick()` completes immediately once for every missed tick, so the loop takes 10–30
snapshots back to back, milliseconds apart (the collector sees near-identical "duplicate" samples) and pushes them all
(the CPU spike: 5,000 counters serialized 10–30 times, plus the pushes, possibly slow again, which causes the next
burst).

The one-line fix is `every_second.set_missed_tick_behavior(MissedTickBehavior::Skip)`: after a slow push, sample once and
realign. It's necessary but not the whole design question. When a push can take longer than the period, decide:

- **Don't overlap pushes** (the loop already doesn't). Overlapping would multiply load on a collector that's already
  slow.
- **Latest wins:** a sampler task writes each snapshot into a `watch` channel (or a bounded channel of 1, dropping the
  old value), and a pusher task pushes whatever is latest, with a timeout shorter than a few periods. Samples during a
  slow push are superseded, not queued.
- **Record the gap:** counters are cumulative, so the collector can compute rates across a gap. Export a
  `samples_skipped_total` so the gap is visible.

### Selected exercises

**Advanced (`disable_lifo_slot`).** Predicted, then check: with the LIFO slot disabled, experiment 3's heartbeat goes to
the local queue, an idle worker steals it, and it runs within microseconds (like the "another spawn" row). Experiment 1's
locality drops: children run on the parent's worker only when no other worker steals them first, so the "1,000 of 1,000"
becomes a smaller fraction. The trade: tail latency when a task misbehaves vs throughput and cache locality in the common
message-passing case.

**Architecture (runtime configuration standard).** Mandatory: explicit `worker_threads` from the deployment's CPU limit;
named threads; `max_blocking_threads` set to what the service's blocking dependencies can use; runtime metrics
(`num_alive_tasks`, `global_queue_depth`, per-worker busy duration) exported; a startup log line with the configuration
and Tokio version. Forbidden without a benchmark in the PR: `event_interval`, `global_queue_interval`,
`disable_lifo_slot`, `task::unconstrained`. Review rules: every `interval` states its missed-tick policy; no
`std::thread::sleep`, `std::fs`, or blocking client calls in async functions; a heartbeat-lateness probe in every load
test.

---

## Chapter 13.2 — Tasks, Spawning, and Blocking Code

### Interview & architecture questions

**1. `'static` and no scoped spawn.** A spawned task is owned by the runtime and can outlive the function that spawned it,
so it can't borrow that function's locals (E0373, listing `ch02-01`). A safe scoped spawn would need a guarantee that the
scope waits for its tasks before its borrowed data goes away. A blocking `std::thread::scope` can guarantee that. An async
scope is a future, and a future can be dropped or `mem::forget`-ed in safe code without being polled to completion, which
would leave tasks holding dangling borrows. The Leakpocalypse (Chapter 1.3) is the same argument. So Tokio offers
`'static` plus `Arc` and channels, and structured concurrency for task *lifetimes* (`JoinSet`), not borrows.

**2. `MutexGuard` across `.await`.** `std::sync::MutexGuard` is `!Send`, so a future holding one across an `.await` is
`!Send`, and `tokio::spawn` rejects it at compile time ("future cannot be sent between threads safely ... used across an
await"). That's the right outcome for two reasons. If the task moved to another worker, the mutex would be unlocked from
a different thread than locked it, which some platform mutexes forbid. And holding a blocking lock across a suspension
blocks every other task that tries to lock it, possibly on the same worker. Drop the guard before the `.await`, or use
`tokio::sync::Mutex` if the lock must span it.

**3. Dropping a handle; aborting blocking work.** Dropping a `JoinHandle` detaches the task: it keeps running, and its
output is dropped when it finishes (listing `ch02-04`, case 1). `abort()` on a `spawn_blocking` task whose closure is
already running does nothing to the closure: it runs to the end, and the handle reports its result (listing `ch02-06`:
"ok = true, closure ran to the end = true"). Aborting only prevents a *queued* closure from starting.

**4. Listing `ch02-05`.** 1 blocked worker of 4: 1.9 ms, because the heartbeat ran on another worker and work stealing
moved other runnable tasks off the stuck one. 4 of 4: 196 ms, because nothing polled the heartbeat, the timer wheel, or the
I/O driver for 200 ms. `spawn_blocking`: 1.1 ms, because the sleeps ran on blocking-pool threads. The first number is
why blocking bugs pass tests: the damage appears only when concurrent blockers reach the worker count (and it isn't zero:
the blocked worker's LIFO slot waits, 13.1).

**5. When `spawn_blocking` is wrong for CPU work.** When the CPU work can be concurrent in bursts: `spawn_blocking` has no
notion of cores, so 100 simultaneous CPU jobs become up to 100 runnable threads (up to 512), all competing for the
cores, with context-switch overhead and no queueing discipline. Use a pool sized to cores (rayon, or a dedicated thread
pool) behind a `Semaphore` so overload becomes a bounded wait or a fast rejection (§9's fraud-scoring design).
`spawn_blocking` is fine for occasional CPU work, and it's right for blocking *I/O*.

**6. `block_in_place` under `#[tokio::test]`.** It converts the current worker into a blocking thread and hands its
queue to a new worker, which requires the multi-thread runtime. `#[tokio::test]` defaults to the current-thread runtime,
so it panics ("can call blocking only when running on the multi-threaded runtime", listing `ch02-07`). Library code that
calls it breaks every caller on a current-thread runtime, including their tests. Libraries should use `spawn_blocking` or
document the requirement.

**7. Request IDs.** With `tracing` spans (`handler.instrument(info_span!("request", id = %id))`) or `task_local!` with
`.scope(id, fut)`. Both attach the value to the future, so it follows the task wherever it runs. `thread_local!` gives the
value of whichever request last ran on the current worker (listing `ch02-08`: wrong 1,489 times out of 1,600).

**8. The time budget.** Roughly 10–100 µs of work between `.await`s (Alice Ryhl, 2020). Measure with a heartbeat task
(listing `ch01-03`'s pattern) under load; with `tokio_unstable`'s poll-time histograms (`poll_time_histogram` metrics);
with `tokio-console` (task poll durations, not verified here); or with `perf` and flame graphs to find long polls.

**9. `ManagedBlocker` vs Tokio.** Java's `ForkJoinPool.managedBlock` lets a task declare "I'm about to block", and the
pool adds a compensating thread so parallelism stays constant. Tokio can't compensate automatically: it can't know a
function blocks. The closest tools are `block_in_place` (explicitly hand the worker's queue to a new worker) and
`spawn_blocking` (move the call to another thread). What Tokio can't do is make an accidental blocking call harmless.

**10. Placement.** The sync Postgres driver: `spawn_blocking` behind a semaphore equal to the connection pool size (or
better, an async driver). The 20 ms PDF render: a CPU pool (rayon) behind a semaphore, with a `oneshot` for the result;
20 ms is 200× the time budget between awaits. The HTTP client call: an ordinary `.await` on the workers (async I/O), with
a timeout derived from the request's deadline.

### Debugging exercise (`hourly_rollover`)

All of it runs on a worker at :00:

- (a) `std::fs::File::create` is a blocking syscall. It's usually sub-millisecond, but on a slow or network disk it can
  take much longer, and it blocks a worker for that long.
- (b) A `std::sync::Mutex` lock is fine if `rotate` is quick. If `rotate` flushes or closes the old file inside the lock,
  that's blocking I/O while holding a lock that every logging call needs. That stalls every task that logs, on every
  worker.
- (c) `std::fs::read` of ~300 MB is blocking I/O: from ~0.1 s from the page cache to several seconds from disk, plus a
  300 MB allocation.
- (d) `zstd` level 19 on 300 MB is **CPU-bound** and slow. Level 19 compresses at single-digit MB/s on one core
  (predicted from mechanism; measure it), so this is **tens of seconds to minutes** of CPU on a worker thread.
- (e) The upload is async (fine), but it holds 300 MB of input plus the compressed output in memory while it waits.

"CPU is low" fits: (d) keeps **one core** busy, which is a few percent on a 16-core dashboard. The latency spike is
every task queued on the stalled worker(s).

Rewrite: create the new file and rotate with `tokio::fs` (which uses the blocking pool) or one `spawn_blocking` call that
doesn't flush under the lock (swap the writer under the lock, flush the old one outside it). Run (c)+(d) together as a
**streaming** job in `spawn_blocking` behind a `Semaphore(1)`: read the file in chunks through `zstd::stream::Encoder` at
a moderate level (3–6), writing to a temporary file. Then upload the compressed file with an async streaming body. No
worker blocks, memory stays at buffer size, and the CPU cost drops by an order of magnitude at the lower level.

### Selected exercises

**Beginner.** `async move` moves `merchant` into the task (the task owns it). An `Arc<str>` lets the caller keep a
handle: `let m: Arc<str> = "m-1042".into(); let m2 = Arc::clone(&m); spawn(async move { println!("{m2}") })`. For
`ch02-02`: `let len = { let cache = Rc::new(...); cache.len() as u32 };` before `fetch_rate().await`. The `Rc` is dropped
before the `.await`, so it's not part of the future's saved state, and the future is `Send`.

**Advanced (`spawn_cpu`).** One solution:

```rust,ignore
// Unverified sketch.
async fn spawn_cpu<F, T>(pool: &rayon::ThreadPool, permits: Arc<Semaphore>, f: F) -> Result<T, Busy>
where F: FnOnce() -> T + Send + 'static, T: Send + 'static {
    let permit = timeout(Duration::from_millis(20), permits.acquire_owned()).await
        .map_err(|_| Busy)?.map_err(|_| Busy)?;
    let (tx, rx) = oneshot::channel();
    pool.spawn(move || { let _permit = permit; let _ = tx.send(f()); });
    rx.await.map_err(|_| Busy) // the job panicked: its sender was dropped
}
```

The permit moves into the rayon job, so it's released when the CPU work ends, not when the async caller gives up.
That's the right accounting: the core is busy until then.

---

## Chapter 13.3 — Async Channels and Synchronization

### Interview & architecture questions

**1. Mapping.** A request's reply: `oneshot` (one value, one receiver, and closure tells the caller the responder died).
A stream of audit records: `mpsc::channel(n)` into one writer task, with `n` sized by Little's law and a policy for full
(wait, or spill). Live configuration: `watch` (latest value; readers notified of change). "At most 20 calls to the
processor": `Semaphore::new(20)`. Invalidation events to 50 cache instances: `broadcast` if a lagging instance can resync
(on `Lagged`, flush the whole cache), otherwise 50 `mpsc` queues with an explicit overflow policy.

**2. Cancelled `send`.** The value was moved into the `send` future. Dropping the future drops the value: the message is
lost, as Tokio's docs say (listing `ch04-07`). `reserve().await` waits for a slot *before* the value is involved. If it's
cancelled, only the place in the queue is lost, and once it returns, `permit.send(v)` is synchronous and can't fail.

**3. `Lagged(6)`.** The broadcast buffer holds 4 messages. The slow receiver didn't read while 10 were sent, so messages
0–5 were overwritten before it read them. On its next `recv`, it's told it missed 6, then continues from the oldest still
buffered (6). Exactly right for fan-out where the producer must never wait for consumers and a consumer can recover by
resyncing (dashboards, cache invalidation, market data snapshots): the producer stays fast, and a slow consumer finds out
precisely.

**4. `watch` sees only the latest value.** It stores one value and a version. Receivers are notified that the version
changed and read the current value, so intermediate values are skipped. A bug when each value matters: a sequence of
commands, deltas ("+5, +3"), audit events. If a `watch` carries increments, skipped values are lost updates. Use `mpsc`
for events and `watch` for state.

**5. The lost wakeup.** The waiter reads the flag (false). Before it registers as a waiter, the notifier sets the flag and
calls `notify_waiters()`, which wakes only registered waiters: none. The waiter then registers and sleeps, having missed
the event (listing `ch03-05`, case 3). `enable()` registers the `Notified` future on the waiter list *before* the check,
so a notification between the check and the `.await` wakes it (case 4). Alternatively, `notify_one()` stores a permit,
which also closes the gap for a single waiter.

**6. Tokio mutex vs std mutex.** Use `tokio::sync::Mutex` only when the guard must be held across an `.await`, for
example to serialize access to a connection object whose methods are async. It costs ~22 ns uncontended against ~4 ns for
`std::sync::Mutex` (listing `ch03-04`, one run), and its failure mode is the convoy. For data protected briefly (maps,
counters, caches), `std::sync::Mutex` or `parking_lot` is faster and, because its guard can't cross an `.await` in a
spawned task, safer.

**7. The convoy.** Holding the lock across a 10 ms network call means only one task at a time can be in the call. 50
tasks take 500 ms instead of 10 ms (listing `ch03-04`, A vs B). No thread blocks, and CPU is idle, because every task is
parked waiting for the lock. Throughput is capped at 1 / (call latency), and it gets worse exactly when the dependency
slows down.

**8. Capacity one million.** Empty: 800 bytes (listing `ch03-07`); `mpsc` allocates blocks as messages arrive. Full: a
million messages' worth (~9 bytes of overhead per `u64` message at scale, plus the messages). In latency: a full queue
means the last message waits behind a million others, at the consumer's rate. At 1,000 msg/s that's 1,000 s. Capacity
should be chosen as "the delay I'm willing to hide × the arrival rate".

**9. Actor vs sharded lock for a counter at 500K updates/s.** A single actor at ~1 µs per command handles ~1M/s on one
core, but each update pays two channel hops (~0.3–1 µs of latency) and the actor is a single point of contention. A
sharded lock (or per-worker counters summed on read, Chapter 11.7) scales with cores and has no hop. For a counter,
sharded atomics win. Use an actor when the invariant spans the state (limits with a no-breach rule), and shard the
actors when the state is per-key.

**10. Shutting down a pipeline.** Stop the source: drop the first stage's senders (or close the source). Each stage runs
`while let Some(x) = rx.recv().await`, and `recv` returns `None` only when all senders are dropped *and* the queue is
drained. So each stage finishes its backlog, then drops its own sender, which ends the next stage. Wait for the last
stage's task (a `JoinSet` or `TaskTracker`). No message is lost, because nothing is cancelled: the shutdown propagates as
end-of-stream. Put a deadline around the whole drain.

### Debugging exercise (`Reloadable`)

The race: `wait_for_reload` reads `dirty == false`. Before it creates its `Notified` future, `reload` runs completely: it
sets the config, sets `dirty = true`, and calls `notify_waiters()`, which wakes nobody (no one is registered yet).
`wait_for_reload` then calls `notified().await` and sleeps until the *next* reload, up to 30 s later. The handler uses
the old config for that long. A second defect: `dirty` is one flag shared by all waiters, so the first waiter to clear
it hides the reload from the others.

Fix with `Notify`: create and `enable()` the `Notified` future before reading `dirty` (listing `ch03-05`, case 4), and give
each waiter its own "last seen version" instead of a shared flag.

Better fix: replace the whole structure with `watch::channel(Arc<Config>)`. The reloader calls `send_replace`. Each
handler holds a `Receiver` and uses `borrow()` for the current config. A task that needs to react to reloads does
`changed().await` then `borrow_and_update()`. Versions are per receiver, notifications can't be lost, and there's no flag
to clear.

### Selected exercises

**Intermediate (`reserve` in `select!`).** Inside the loop: `tokio::select! { permit = tx.reserve() => permit?.send(item),
_ = sleep_until(deadline) => { requeue(item); } }`. Nothing is lost when the deadline fires, because `item` never entered a
future: `reserve` holds no value, and the item is still owned by the loop.

**Advanced (single-flight).** Keep `Mutex<HashMap<Key, watch::Receiver<Option<Value>>>>` (a std mutex, never held across
an `.await`). On a miss, insert a new `watch` channel for the key, release the lock, run the fetch, `send` the value,
then remove the entry. Other tasks that miss the same key find the receiver and `wait_for(Option::is_some)`. With 50 tasks
and 5 keys on a paused clock, the fetch counter reads 5, and the total virtual time is one fetch latency.

---

## Chapter 13.4 — Cancellation and Structured Concurrency

### Interview & architecture questions

**1. Timeout at the second `.await`.** `timeout`'s own future completes with `Err(Elapsed)` and drops the inner future.
Dropping it runs the destructors of every value the future holds in its current state (locals alive across the second
`.await`, its argument values, guards, sub-futures) in reverse declaration order. No code after the second `.await` runs,
and effects of code before it (the first step) have already happened. The caller sees `Err(Elapsed)` without knowing how
far the operation got (listing `ch04-01`).

**2. Cancel safety.** An operation is cancel safe if dropping its future before completion loses nothing and leaves no
half-done effect, so it can be retried or raced freely. `recv`: safe (no message taken). `read`: safe (no bytes taken).
`read_exact`: not safe (bytes already copied into your buffer are lost). `write_all`: not safe (part of the buffer may
have been written). `send`: not safe for the message (it's dropped). `reserve`: safe apart from losing the queue position
(listing `ch04-07` prints Tokio's wording for each).

**3. The glued frame.** The `read_exact` for frame 1 had copied bytes `10, 11, 12` into its buffer when the tick fired.
`select!` dropped that future, and the next iteration's `read_exact` started with a new buffer at byte `13`. It read
`13..17` (the rest of frame 1) and `20, 21, 22` (the start of frame 2): misaligned from then on. Fixes: a framing reader
that owns its buffer (`FramedRead` with a codec, cancel safe); one `read_exact` future pinned outside the loop and polled
by reference, so a losing iteration doesn't drop it; or a dedicated reader task that sends whole frames over a channel.

**4. `abort()` vs `Future.cancel(true)`.** `abort()` makes the runtime drop the task's future at its next suspension: the
task can't refuse, but a task that never reaches an `.await` (a CPU loop) or is a running `spawn_blocking` closure can't
be stopped. `cancel(true)` sets the thread's interrupt flag. Blocking calls that honor interruption throw
`InterruptedException`, but code can catch it and continue, and CPU loops that don't check the flag aren't stopped.
Neither stops non-cooperative CPU work. Rust's version also can't be ignored by cooperative code, and it gives the code
no chance to run after the `.await`.

**5. `Drop` can't flush.** A flush to a socket may have to wait for the socket to become writable, and `Drop` is a
synchronous function with no way to `.await`. It could only block the thread (wrong on a worker) or discard the data
(listing `ch04-05`: 3,520 bytes lost). The API should offer explicit async cleanup (`close(self).await -> Result`,
`shutdown().await`) and document that dropping without it discards buffered data. `Drop` can act as a backstop that logs,
counts, or spawns cleanup if a runtime is available (`Handle::try_current`).

**6. The timed-out charge.** The handler knows that it stopped waiting. It doesn't know whether the processor received the
request, authorized it, or will authorize it later. This is Chapter 8.4's ambiguous class. It must not report "declined".
It records the attempt as outcome-unknown with its idempotency key, returns an ambiguous error (PAY_PROCESSOR_TIMEOUT /
504) so the client retries with the same key, and relies on the processor-side idempotency plus reconciliation to settle
the truth. If the processor call was cancelled by a client disconnect, better still: the call should have run in a
spawned, tracked task that the disconnect can't cancel (§14's design exercise).

**7. `JoinSet`, `TaskTracker`, `FuturesUnordered`.** `JoinSet` owns spawned tasks, yields results in completion order, and
aborts every remaining task when dropped: children can't outlive the scope, which is structured concurrency.
`TaskTracker` only tracks tasks so you can wait for all of them. It doesn't abort them, so it's for graceful shutdown,
not scoping. `FuturesUnordered` runs futures inside the current task (no spawning, no parallelism across workers) and drops
them when dropped, so it's structured too, but for concurrency within one task.

**8. Long-poll shutdown.** Cooperative: stop accepting; cancel a root token; each long-poll handler watches its token and
replies early ("no events yet; reconnect") instead of waiting out its 30 s. Normal requests finish. Forced: after a drain
deadline (well inside the orchestrator's grace period), abort the remaining tasks (`JoinSet::shutdown`). Deadline:
long-poll max wait plus margin, or shorter if clients handle an early empty reply (then a few seconds suffices). Measure
aborted requests per deploy.

**9. A slow `main` return.** Returning from `#[tokio::main]` drops the runtime, and a plain drop **waits for running
blocking tasks** (listing `ch04-06`: 280 ms for a 300 ms `spawn_blocking` closure). A `spawn_blocking` doing a long file
copy or a stuck blocking call holds the process open. Use `rt.shutdown_timeout(d)` (build the runtime yourself), or make
blocking work cooperative.

**10. Kotlin vs Rust.** A Kotlin coroutine is cancelled by a `CancellationException` thrown at its next suspension point,
so it can run `finally` blocks, catch the exception, and even run more suspending code in `withContext(NonCancellable)`.
A Rust future gets no control flow when dropped: only destructors run, and destructors can't `.await`. So "finish the
critical step asynchronously after cancellation" is expressible in Kotlin and not in Rust. Rust code must be designed so
that stopping between steps is safe, or it must move the critical step somewhere cancellation can't reach (a spawned,
tracked task).

### Debugging exercise (`copy_loop`)

The window: the `select!` races the whole unit of work (receive, ack, insert) against the stop signal. If shutdown
arrives after `queue.ack(&batch)` completed and while `db.insert_many(&batch)` is pending, `select!` drops the inner
future: the messages are deleted from the queue and never inserted. Lost. A crash (SIGKILL) at the same point loses them
too.

Fix 1, reordering: receive, insert, then ack. A crash or cancellation between insert and ack leaves the messages in the
queue. They become visible again after the 30 s visibility timeout and are inserted twice, so the insert must be
idempotent: an upsert keyed on the message ID, or a unique constraint and ignored duplicates. That's at-least-once plus
idempotence, effectively exactly-once in the database.

Fix 2, don't race the unit of work: `select!` only on receiving the next batch (`stop.cancelled()` vs
`queue.receive(100)`). Once a batch is received, process it to completion without racing, as in listing `ch04-04`. Put a
drain deadline around the whole loop.

Only Fix 1 survives SIGKILL at any point, because no cooperative code runs on SIGKILL, whatever the `select!` does.
Use both: Fix 2 makes graceful shutdown clean, and Fix 1 makes crashes safe.

### Selected exercises

**Beginner (45 ms).** Predicted: the source is debited, the first `sleep(40)` completes at 40 ms, and the destination is
credited. The timeout fires during the second sleep at 45 ms. The guard is dropped, and the result is
`Err("Elapsed") after 45ms, steps that ran: ["debited source", "credited destination"]`: the same steps as the 60 ms case,
a different time.

**Intermediate (pinned `read_exact`).** Hold the reader and one frame buffer outside the loop. Each frame:
`let fut = r.read_exact(&mut buf); tokio::pin!(fut); loop { select! { res = &mut fut => { ...; break } _ = tick.tick() =>
{ housekeeping } } }`. A tick no longer drops the read. The frame's partial progress stays inside `fut`, which survives
the iteration. All three frames arrive intact.

---

## Chapter 13.5 — Backpressure, Timeouts, and Load Shedding

### Interview & architecture questions

**1. Backpressure down to the task.** The consumer stops reading, so the receiver's socket buffer fills, and TCP
advertises a zero window. The sender's kernel stops transmitting, and its send buffer fills. `write(2)` returns `EAGAIN`,
so Tokio's write future returns `Pending` after registering interest in write readiness. The task parks (no thread
blocked), holding its buffers. When the consumer reads, the window opens, the send buffer drains, epoll reports writable,
and the task is woken. Kernel: buffers, window, `EAGAIN`, readiness. Tokio: `Pending`, waker, rescheduling (listing
`ch05-01`).

**2. UDP.** UDP has no connection state, no window, no acknowledgments: a send hands a datagram to the kernel, which
transmits it and forgets it. A full receive buffer means the kernel drops arriving datagrams, telling nobody. Listing
`ch05-05`: 20,000 datagrams sent without waiting, 92 received, and a 9,000-byte datagram truncated to the 2,048-byte
buffer. QUIC builds flow control and congestion control on top (per-stream and per-connection windows, acks). A metrics
pipeline (StatsD-style) accepts loss by design, aggregates to make loss tolerable, and monitors drops (`/proc/net/snmp`
`RcvbufErrors`).

**3. The unbounded row.** 120 req/s arrive for a 100 req/s server, so the queue grows by 20/s, and after ~5 s every
request waits more than the client's 500 ms. From then on the server spends 10 ms on each request whose client has
already gone: 906 of 1,200 units of work were wasted, and goodput was 294. After the load stops at 10 s, the queue still
holds ~200 requests (2 s of work), all already expired, so the server stays busy until 12 s producing nothing. In a real
system, clients retry the timed-out requests, which refills the queue: the overload sustains itself (metastability).

**4. Wait vs shed.** Wait when the producer can slow down and waiting doesn't cost anyone a deadline: internal pipeline
stages, batch jobs, a producer that is itself a consumer of backpressure-aware input (TCP). Shed when requests have
deadlines and clients can retry elsewhere or later: request/response services. Waiting there turns overload into latency
for everyone, and then into timeouts.

**5. Tower layer order.** `ConcurrencyLimit` waits for a permit in `poll_ready`, and `Timeout` only times `call`. `oneshot`
(and every server) calls `poll_ready` before `call`, so the wait for a permit is untimed whether `Timeout` wraps the limit
or the limit wraps `Timeout`. Request 4 waited 30 ms and then had its full 50 ms. `Buffer` answers `poll_ready`
immediately (while it has queue space) and moves the permit wait into the future returned by `call`, so the outer
`Timeout` covers queueing, and requests 3–5 time out at 50 ms (listing `ch05-03`).

**6. Timeout vs deadline.** A timeout is a duration each caller chooses for its own wait. A deadline is an absolute
instant after which the result is useless, carried with the request. With per-hop timeouts, each callee starts with its
full local timeout regardless of how much of the client's budget is left, and keeps working after upstream callers have
given up (listing `ch05-04`: 110 ms of processor work after the client left). With a deadline, a callee can see that it
can't finish and refuse at once, giving a fast definite answer and wasting nothing.

**7. Queue size.** Little's law: items in the queue = arrival rate × time in queue. If a consumer handles 1,000 msg/s and
you're willing to hide 200 ms of hiccup, the queue holds 200 messages. Anything larger means messages can wait longer than
200 ms, which only makes sense if their consumers still care. Size in time first, then convert to items.

**8. Hidden queues.** The kernel's accept backlog (128 by default in Tokio's `bind`, listing `ch05-06`); socket receive
and send buffers; the runtime's run queues (tasks runnable but not yet polled); tasks waiting on semaphores and mutexes
(each a queued request with its memory); the blocking pool's unbounded queue; HTTP/2 stream limits and the client library's
connection pools; and a downstream service's queues, which your requests join.

**9. Tomcat vs Tokio.** In the Java service, `maxThreads` (say 200) was an accidental concurrency limit: request 201 waited
in `acceptCount` and then was refused. Downstream systems (a 50-connection database) never saw more than 200 concurrent
requests, and excess load was shed at the door. The Tokio rewrite has no thread limit, so 100,000 tasks can all be
waiting on the database pool, and the overload appears as latency and timeouts everywhere. The fix is an explicit limit
(`Semaphore`, tower `ConcurrencyLimit` + `LoadShed`) sized from the downstream capacity.

**10. A helpful "busy".** A status that clients classify as retryable (503, `-ERR BUSY`, `RESOURCE_EXHAUSTED`); a
`Retry-After` (or equivalent) with jitter so retries spread out; an indication of whether retrying elsewhere is useful;
and enough promptness that the client's budget remains to retry. Plus client-side retry budgets, so shedding isn't
multiplied by retries.

### Debugging exercise (the 95% handler)

The queue that grows is the set of tasks waiting at (a) for one of 50 permits. Each request holds its permit across
**both** the database call and the ~40 ms pricing call (c), so the 50 database "slots" are occupied mostly by requests
that are waiting on pricing. At 80% load there are usually free permits. At 95%, permits run out, and every new request
waits in the semaphore's queue (unbounded: every waiting task is an admitted request). CPU stays low because everything is
waiting: on the semaphore, on pricing, on the database.

The timeout at (b) bounds only `load_user`. Time spent waiting for the permit at (a) and in the pricing call at (c) is
unbounded, so a client can wait for seconds.

Rewrite:

```rust,ignore
// Unverified sketch.
async fn handler(req: Request) -> Response {
    let deadline = req.deadline(); // from the client's budget header, capped at the service's default
    let user = async {
        let permit = timeout_at(deadline, DB_PERMITS.acquire()).await.map_err(|_| Overloaded)??;
        let user = timeout_at(deadline, db.load_user(req.user_id)).await.map_err(|_| Timeout)??;
        drop(permit); // the permit covers the database call only
        Ok::<_, Error>(user)
    };
    let prices = timeout_at(deadline, pricing_client.quote(&req.items));
    match tokio::join!(user, prices) {
        (Ok(user), Ok(Ok(prices))) => render(user, prices),
        (Err(Overloaded), _) => Response::busy_retry_after(),
        _ => Response::timeout(),
    }
}
```

The permit is held only around the database call. The permit wait is bounded by the deadline, and a request that can't
get one in time is shed with a retryable "busy". The database and pricing calls run concurrently. For stricter
shedding, `try_acquire` refuses at once when the database is saturated.

### Selected exercises

**Beginner (queue sizes).** Predicted from Little's law: capacity 5 bounds queueing at ~50 ms, so latency is low and
shedding high. Capacity 100 bounds it at ~1 s, which exceeds the client's 500 ms, so requests at the back of a full
queue time out and waste work again (goodput drops below the capacity-20 case). Capacity 20 (≈200 ms) sits inside the
deadline. Run it: the sweet spot is the largest queue whose drain time stays under the client deadline.

**Systems (`SO_SNDBUF` 64 KiB).** Predicted: the kernel accepts roughly the send buffer plus the receiver's receive
buffer and window, on the order of a few hundred KiB instead of 2.5 MiB. Linux doubles the requested `SO_SNDBUF`
internally for bookkeeping, and the listing prints what it reports. Per-connection kernel memory for 100,000 such
connections: on the order of 100,000 × (128 KiB + receive buffer), still gigabytes in the worst case, which is why an
application-level cap and a write timeout matter too.

---

## Project L5 — Architecture review (Ferrite v2)

**1. The store in async code.** Every `ShardedStore` method takes a `std::sync::RwLock` guard, does one `HashMap`
operation (for a GET, an `Arc<[u8]>` clone: a reference-count increment), and releases the guard before returning. Value
bytes are copied only outside the lock (`put` builds the `Arc` first; the reply encoder copies after). The guard never
crosses an `.await`, and the critical section is a hash lookup, so calling the store on a worker is ordinary CPU work.
It would become wrong if a store method did I/O or waited (a disk read, a network call), with or without the lock: that
blocks a worker (13.2). It would also be wrong if someone reintroduced copies of large values under the lock (Chapter
20.7 measured the copy as the dominant cost, 1.9–2.8× at 4 KiB). Review check: the `KvStore` impl contains no I/O and no
unbounded work under a guard, `handle_line` is the only call site, and clippy's `await_holding_lock` is enabled.

**2. Refuse vs queue.** Queueing connections smooths short bursts and returns fewer errors in the moment. It loses
because queued connections hold memory and kernel buffers, their clients' timeouts keep running (the queue fills with
connections whose clients give up, Chapter 13.5's unbounded row), and it hides overload from clients who could have gone
elsewhere. A client receiving `BUSY` should back off with jitter (exponential, capped), try another replica if there is
one, and count the refusals in its own metrics.

**3. The flush rule.** A client sends `GET a\nGE` in one write and then waits for the reply to `GET a` before sending `T b\n`
(a client that pipelines by accident, or a proxy that splits writes). After decoding `GET a`, the read buffer holds `GE`,
not empty. "Flush only when the read buffer is empty" would not flush, and the loop would wait for the rest of the next
request, which the client won't send until it gets the reply. Both sides wait until the idle timeout. Checking for a
complete request (a `\n`) flushes here, because the next iteration *will* wait for the network.

**4. Every `.await` in `serve`.** (i) The `select!` on `stop.cancelled()` / `timeout(idle, requests.next())`: interruptible
by shutdown and idleness. No request is in flight, and a partial request in the buffer is discarded (the client never got
a reply for it, so it knows it wasn't executed). (ii) `send_now` (shutdown and too-long replies): not interruptible by
shutdown, bounded by `write_timeout`. (iii) `replies.close()` and `linger`: bounded by timeouts. (iv) `feed`/`flush` inside
`timeout(write_timeout, ...)`: not raced with shutdown. The request has already been applied to the store, and the reply
is attempted until the write timeout. Only the drain deadline's `JoinSet::shutdown()` can abort it there, which leaves
an applied request without a reply (the client sees an ambiguous close). The design chose "finish what you started"
over prompt shutdown, and bounded it with the drain deadline.

**5. `write_timeout`.** It bounds each reply's `feed` + `flush`: the time to hand one reply (and, at the boundary, the
buffered replies) to the kernel. It doesn't bound the connection's lifetime or total throughput. A client reading 1 KB/s
steadily with small replies is never disconnected: each flush completes. It can keep a connection and a task busy
indefinitely, a slow-read attack if it pipelines heavily. Whether it should be disconnected is a policy choice: add a
minimum throughput over a window, or cap outstanding pipelined requests per connection.

**6. Memory at scale.** Per idle connection with v2's defaults: task ~88 B plus the `serve` future (a few hundred bytes to
~1 KiB, predicted: measure with `size_of_val`), plus 16,384 B of `FramedRead` + `FramedWrite` buffers (listing
`ch05-08`), plus kernel socket state (a few KiB idle, far more under load). That's ~17 KiB × 100,000 ≈ 1.7 GB of user
memory before traffic. Changes: `FramedRead::with_capacity` of ~512 B and a small `FramedWrite` initial buffer (cost:
reallocations when requests or replies are large); cap `SO_SNDBUF`/`SO_RCVBUF` (cost: lower throughput for large values
per connection); derive `max_connections` from the memory budget (cost: refusals at the limit).

**7. `-ERR NOT_FOUND`.** A missing key isn't an error: `GET` on an absent key is a normal, successful answer, and `_` says
so (Chapter 8.1's distinction between absence and malformed input). Turning it into an error would break every v1
client that handles `_`, and it would push "not found" into error metrics and alerts. The codes are for failures. If a
client team needs richer replies, that's a protocol version (a `HELLO 2` handshake in Project L6), not a change to v1
semantics.

**8. Shutdown under pipelining.** Each connection finishes the request it has decoded (execution is synchronous) and
writes its reply. At the next loop iteration, `select!` sees the token first (`biased`) and sends `SHUTTING_DOWN`.
Requests already in the read buffer but not yet decoded are discarded, never executed, and never answered: the client
must treat unanswered pipelined requests as not executed. No `SET` can be read but not executed, because a decoded
request is executed before any `.await`. The drain takes about one request's time per connection plus the final writes,
well under `drain_timeout` unless a client stopped reading. `Report` says `drained: true, aborted: 0`, unless slow
readers were aborted at the deadline.

---

## Part XIII Review — Capstone: the payout-relay PR

**Prompt 1 (m1's 4,040 ms).** `let mut log = log.lock().await;` held across `endpoint(&ev).await`. Every delivery holds
the global audit-log lock for its full network call, so deliveries are serialized, and m1's 20 ms deliveries wait behind
m3's 2 s calls. Chapter 13.3's listing `ch03-04` is the same convoy (500 ms vs 10 ms).

**Prompt 2 (parallelism).** One delivery at a time: the lock serialized them. The other 292 tasks were parked on the
lock's waiter queue, each holding its event. Parallelism should be bounded **per merchant** (a semaphore of a few
permits each), so a slow merchant can't consume all delivery capacity, and bounded in total by what the relay's
resources (connections, memory) allow.

**Prompt 3 (unbounded queues).** The `unbounded_channel` from intake; the spawned tasks themselves (every event becomes a
task immediately, an unbounded queue in disguise); the tokio mutex's waiter list (292 waiters); and the audit buffer
between flushes.

**Prompt 4 (the deploy).** Returning from `main` drops the runtime. Each of the 292 tasks is dropped at its current
`.await` (the lock wait or the endpoint call), and its `Event` is dropped with it. No record of which events were lost
exists anywhere, so they can't be retried or reconciled. That's worse than a crash with a durable queue, because the
system believes the events were handled (they were accepted).

**Prompt 5 (audit).** Chapter 13.4 §5: `Drop` can't do async I/O, and durable writes must be flushed explicitly at
shutdown. `AuditLog` only moves lines to durable storage every 64 lines and never on shutdown, so the 8 lines were in the
buffer when the process ended. A `Drop`-based flush would run only if the log's last `Arc` were dropped before exit
(it's held by 292 tasks being dropped by the runtime, in no particular order), and it couldn't await an async sink.

**Prompt 6 (no timeout).** A merchant endpoint that never answers holds its delivery's task (and here, the global lock)
forever: everything stops. A timeout on a POST makes the outcome ambiguous: the merchant may have processed it. Retrying
is safe only if the merchant deduplicates on a stable key (the event ID as an idempotency key), as Chapter 8.4 requires.
The payload must carry it and the contract must say so.

**Prompt 7 (m3).** Give each merchant its own bounded queue and concurrency limit (bulkheads). A slow merchant's queue
fills; overflow goes to a durable retry table (spilled), not memory. Each delivery gets a per-attempt timeout and a few
attempts with backoff on the timer. Failures go to a dead-letter table for a slower retry job and for merchant-visible
status. Nobody else is allowed to be affected by m3's slowness: m1 and m2's latency must not change (the redesign's test
asserts ≤ 40 ms).

**Prompt 8 (ordering).** A global lock gives a global order of *audit lines*, which nobody needs. What merchants may need
is **per-merchant order** of events (payout sent before payout failed). The design should state that requirement, then
meet it per merchant (sequence numbers in the payload, so the receiver can order and detect gaps), not by serializing the
whole relay. With parallel deliveries per merchant (4 in flight), delivery order isn't guaranteed. Sequence numbers make
that safe.

**Prompt 9 (statics).** Process-global counters are shared by every relay instance and every test in the process, so
tests interfere and metrics can't be attributed. They also hide the missing piece: an *accounting* invariant (accepted =
delivered + dead-lettered + spilled + requeued) that the service should check and export.

**Defects list (model review, 16 items):**

1. Lock held across the network call: a global convoy (13.3).
2. `tokio::sync::Mutex` where a std mutex around a push would do (13.3 §6).
3. Unbounded intake channel (13.3, 13.5).
4. One task per event, spawned without a limit: tasks as an unbounded queue (13.5 §8).
5. No per-merchant isolation: one slow merchant stalls all (bulkheads).
6. No per-merchant concurrency limit: without the lock, it would send 100 concurrent POSTs to a slow merchant.
7. No timeout on the endpoint call (13.5).
8. No retries or backoff, and no dead-letter path: failures and non-200 statuses are recorded and forgotten.
9. No idempotency key in the payload, so no retry can be safe (8.4).
10. Detached tasks (dropped `JoinHandle`s): orphans, silently dropped at the deploy (13.4).
11. No graceful shutdown: no signal handling, no cancellation token, no drain, no requeue (13.4 §9).
12. No outcome accounting: events can vanish without a record, and there's no invariant to check.
13. The audit log is never flushed at shutdown, and `Drop` couldn't do it (13.4 §5).
14. The ordering rationale is wrong: a global lock gives audit-line order, not the per-merchant event order merchants
    need.
15. Process-global statics for metrics.
16. No backpressure to the event source: intake accepts everything into memory instead of spilling to a durable table
    when a merchant's queue is full.

The redesign (listing `review-02`) addresses 1–13 and 15–16 and states the ordering decision (14) in its design notes.
Its numbers: m1 and m2 100 of 100 each at 20 ms, m3 bounded to 4 in flight, 300 of 300 accounted for, 200 of 200 audit
lines durable.

---

## Part XIII Review — Interview mode

**1. `Send + 'static`.** `'static`: the task can outlive its spawner, so it can't borrow the spawner's locals. `Send`: on
the multi-thread runtime a task may resume on any worker after any `.await`. A safe scoped spawn is impossible because
the scope's future could be dropped or leaked without being polled to completion, leaving tasks with dangling borrows.
For `!Send` state, use a current-thread runtime or a `LocalSet` with `spawn_local`, or keep `!Send` values out of the
state saved across `.await`s.

**2. Cancelled at the second `.await`.** The future is dropped, so the destructors of everything it holds run, and the
code after that `.await` never runs. Effects before it have happened. In Java, an interrupted thread blocked at the same
point gets an `InterruptedException`: the function continues executing in a `catch` or `finally`, and can even
complete its work. The Rust function gets no further control flow.

**3. Cancel safety.** An operation is cancel safe if dropping it midway loses nothing, so it can be raced or retried.
`recv`: safe (Tokio guarantees no message was taken). `read_exact`: not safe (bytes may already be in your buffer).
`send`: loses the message if cancelled. `reserve` takes a slot without committing the value, so only the queue position
is lost.

**4. The worker loop.** A task woken by a task on the same worker goes to that worker's LIFO slot (displacing any
previous occupant into the local queue); a task woken from outside goes to the global queue. The worker runs the LIFO
slot (at most 3 times in a row), then its local queue (256 slots, overflowing half to the global queue), checks the
global queue on an interval (31 ticks for current-thread, self-tuned on multi-thread), steals half of another worker's
queue when idle, and polls the I/O and timer drivers every 61 ticks or when idle. The LIFO slot trades locality and
latency for message passing against starvation when the slot's owner blocks (it can't be stolen).

**5. The cooperative budget.** Each poll of a task gets 128 units; each operation on a Tokio resource (channel, socket,
timer) spends one; at zero those operations return `Pending`, forcing a yield. It doesn't cover CPU work between awaits,
non-Tokio futures, or third-party code that never touches Tokio resources. Find a starving task with a heartbeat probe,
`tokio_unstable` poll-time histograms or `tokio-console`, or a profiler showing one long poll.

**6. Idle and waking at the OS level.** An idle worker parks: one worker blocks in `epoll_wait` with a timeout set to the
next timer deadline, the others on a condition variable (futex). Waking from another thread pushes the task to the global
queue and unparks a worker: a futex wake, or an eventfd write if the worker to wake is the one in `epoll_wait`.

**7. Costs.** A task: one allocation of 88 bytes plus its future (two allocations above 16 KiB in release), spawn+join
~300 ns on a current-thread runtime and ~800 ns on 4 workers, a round trip ~180–400 ns. An OS thread: 2 MiB of reserved
stack, tens of microseconds to create, a ~9 µs round trip through two threads. A framed TCP connection: 16,384 bytes of
tokio-util buffers before its first byte, plus kernel socket buffers.

**8. Four requests break p99.** Hypothesis: those requests block a worker (a blocking call or a long CPU stretch without
`.await`s), and with 4 workers, four at once stall everything. Confirm with a program like listing `ch02-05`: a heartbeat
task plus N concurrent copies of the suspect handler, and watch the lateness jump when N reaches the worker count.

**9. `tokio::sync::Mutex`.** Only when the guard must be held across an `.await` (serializing an async operation on a
shared resource). Cost: ~22 ns uncontended vs ~4 ns for the std mutex. Failure mode: a convoy, where every task waits
behind the slowest holder's `.await` (500 ms vs 10 ms in listing `ch03-04`).

**10. Placement.** 20 MB upload parsing: stream the body (async) into a CPU pool job, or parse incrementally in chunks
small enough for the time budget; for whole-file parsing, a rayon job behind a semaphore. Three HTTP APIs: concurrent
`.await`s (`join!`) with a shared deadline. Sync database driver: `spawn_blocking` behind a semaphore sized to its pool,
or an async driver.

**11. Graceful shutdown.** SIGTERM → mark unready → keep accepting for the load balancer's lag → stop accepting → cancel
the root token (connections finish their current request, then close with a protocol-level goodbye) → drain with a
deadline inside the grace period → abort stragglers (`JoinSet::shutdown`) → flush logs, metrics, and tracing explicitly
with timeouts → return. Measured per deploy: aborted requests, drain duration, flush failures.

**12. Queues and bounds.** Accept backlog (explicit `listen(n)`, refuse beyond); connection count (a semaphore, `BUSY`
beyond); per-connection read buffer (max line or frame size); per-connection unflushed replies (a cap plus a write
timeout); concurrent requests (a semaphore, shed with 503); the database pool (a semaphore sized to the pool, wait
bounded by the deadline); retries (a budget). Each has a metric.

**13. Timeouts vs deadlines.** With per-hop timeouts, each hop starts fresh, so callees keep working after callers have
given up, which wastes capacity exactly during a slowdown and produces ambiguous failures. A propagated deadline lets
each hop compute its remaining budget and refuse work it can't finish, which gives fast, definite errors and no wasted
work. Across processes it's carried in the request: gRPC's `grpc-timeout` header, or an `x-deadline` header with an
absolute time (clock skew matters) or a remaining budget.

**14. The unbounded queue.** Arrivals above capacity make the queue grow. Once queueing time exceeds client timeouts, the
server works on requests whose clients are gone (listing `ch05-02`: 906 of 1,200 units wasted, goodput 294). After the
dependency recovers, the server still has to drain the dead backlog while retries add new work, so the outage lasts far
longer than the slowdown.

**15. Ambiguous delivery.** The protocol must let the receiver deduplicate: a stable event ID (idempotency key) in every
delivery, and receivers that process each ID once. Then the relay can retry after a timeout. The relay records every
accepted event durably with its state (pending, delivered, dead-lettered), so a deploy or crash requeues what's
unfinished. Nothing lives only in memory, and every event ends in exactly one accounted-for state.
