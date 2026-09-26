# Part XIII Review — The payout-relay PR & Interview Mode

> Consolidate Part XIII, then use it: review a Tokio service that compiles, passes its happy-path demo, and turns one
> slow merchant into a total outage, loses almost everything at the next deploy, and never notices. Then answer
> senior-level questions without notes. Answers are in **Appendix A, Part XIII**.

---

## Part XIII on one page

```text
 RUNTIME       workers (tokio-rt-worker threads) + one epoll + an eventfd + a timer wheel + a blocking pool
               task = one allocation: 88 B + the future (boxed first if > 16 KiB in release, 2 KiB in debug)
               worker loop: LIFO slot (≤ 3 in a row, can't be stolen) → local queue (256) → global → steal half
               drivers polled every 61 ticks or when idle; coop budget 128 ops per poll, only at Tokio resources
               timers: 1 ms ticks, deadlines rounded UP; interval after a stall: Burst (default) / Skip / Delay
        │
 PLACEMENT     spawn: Send + 'static (tasks migrate at any .await; thread_local! lies, task_local! doesn't)
               blocking I/O → spawn_blocking (512 threads max, unbounded queue, can't abort a running closure)
               CPU work → a pool sized to cores (rayon + oneshot) behind a semaphore; block_in_place panics on current_thread
        │
 SYNC          mpsc (bounded = backpressure; send loses the value if cancelled, reserve doesn't), oneshot,
               broadcast (slow receivers get Lagged), watch (latest value only), Semaphore (limit / shed / close),
               Notify (enable() before checking), tokio Mutex only across .await (22 ns vs 4 ns; convoys)
        │
 CANCELLATION  cancel = drop at the current .await; destructors run, code after the .await doesn't
               cancel safety is per method (recv, read: yes; read_exact, read_line, write_all: no)
               JoinSet (owns, aborts on drop) + CancellationToken (cooperative) + a drain deadline = graceful shutdown
               Drop can't .await: flush and close explicitly; runtime drop waits for blocking tasks
        │
 OVERLOAD      every queue needs a bound and a policy: wait, shed, drop-oldest, skip-if-expired, degrade
               unbounded queue under 120% load: 25% goodput; shed at the door: 85% and zero wasted work
               one deadline per request, propagated; tower layer order decides which waits are timed
               the kernel's queues count too: backlog (bind: 128), socket buffers (2.5 MiB per slow peer)
```

## Ten ideas to carry forward

1. **A runtime is a scheduler plus drivers, built from a few OS resources.** The kernel sees worker threads, one epoll
   instance, an eventfd, and sockets. It never sees a task.
2. **Read your dependency's source at the version you ship.** Every default in this Part (128, 256, 3, 61, 512, 10 s,
   16 KiB, 128 for the backlog) came from Tokio's and mio's own files, printed by a listing.
3. **A task runs until it returns `Pending`.** The cooperative budget protects you at Tokio's await points, not in your
   loops. Blocking one worker hides behind work stealing; blocking all of them stops the service.
4. **Put each kind of work where it belongs.** Waiting on the workers, blocking calls on the blocking pool behind a
   limit, CPU work on a core-sized pool.
5. **Tasks migrate.** `Send` keeps your data safe across the move. Nothing keeps `thread_local!` assumptions safe.
6. **Prefer ownership to locks.** An actor has no lock to hold across an `.await`. When you must lock, lock around data,
   never around a network call.
7. **Cancellation is a drop.** Design every async function so that stopping at any `.await` leaves the world
   consistent, and race only the waits between units of work.
8. **Own your tasks.** A `JoinSet` makes "no task outlives its parent" true by construction. A bare `spawn` with a dropped
   handle is an orphan, and a deploy kills orphans silently.
9. **Refuse early; queue briefly.** A queue must be bounded by time. Shedding at the door keeps goodput high and gives
   clients the fastest possible "no".
10. **One deadline per request.** Per-hop timeouts waste downstream work; a propagated deadline lets every hop refuse
    what it can't finish.

---

## Capstone: review the payout-relay PR

The marketplace payouts team (whose reservation incident opened Chapter 12.3 §10) is building `payout-relay`, a new Rust
service that pushes payout status events to merchants' webhook endpoints: "payout 7781 sent", "payout 7782 failed".
Merchants reconcile against these events, so each one must be delivered or explicitly accounted for. A teammate submits
the first PR. Its core, from listing `review-01-webhook-relay-pr.rs` (which compiles and runs; the listing wraps it in a
simulation):

```rust,ignore
async fn run_relay(mut events: mpsc::UnboundedReceiver<Event>, log: Arc<tokio::sync::Mutex<AuditLog>>) {
    while let Some(ev) = events.recv().await {
        let log = Arc::clone(&log);
        tokio::spawn(async move {
            let n = ALIVE.fetch_add(1, Relaxed) + 1;
            PEAK_ALIVE.fetch_max(n, Relaxed);
            let mut log = log.lock().await; // hold the log while delivering, "so audit lines stay in order"
            let status = endpoint(&ev).await;
            log.record(&ev, status);
            DELIVERED[ev.merchant as usize].fetch_add(1, Relaxed);
            if ev.merchant == 1 {
                M1_WORST_MS.fetch_max(ev.created.elapsed().as_millis() as u32, Relaxed);
            }
            ALIVE.fetch_sub(1, Relaxed);
        });
    }
}
```

`AuditLog::record` appends a line to a buffer and moves the buffer to durable storage every 64 lines. `endpoint` stands
in for the HTTPS POST to the merchant. The PR description: "Spawns a task per event so deliveries run in parallel; the
audit log is locked during delivery so audit lines stay in order. Tested locally against three mock merchants: all
events delivered."

The listing replays an incident window on a paused clock: 300 events at 100 per second for three merchants, where m1
and m2 answer in 20 ms and **m3 takes 2 s per call**. At t = 5 s a deploy sends SIGTERM and `main` returns. Its output:

```text
at the deploy (t = 5 s): 300 events accepted, delivered m1 3 / m2 3 / m3 2 (of 100 each)
  tasks alive 292 (peak 295), m1's worst delivery latency so far 4040 ms (its endpoint answers in 20 ms)
after main returned: 292 events never delivered, and nothing records which
  audit lines on durable storage 0 of 8 deliveries (the rest were in the buffer)
```

Eight deliveries in five seconds, a 20 ms merchant waiting four seconds, 292 events lost at the deploy with no record,
and not one audit line durable. Write your review before reading further. Aim for at least **twelve distinct defects**,
and for each: what goes wrong, which chapter's mechanism explains it, and the fix. Some prompts:

1. Explain **m1's 4,040 ms**. Which single line causes it, and which Chapter 13.3 measurement is the same failure in
   miniature?
2. The PR "delivers in parallel". How many deliveries were actually in flight at any moment, and what were the other 292
   tasks doing? What *should* bound parallelism here, and per what?
3. Name every **unbounded** queue in the design, including the ones that aren't written as queues (Chapter 13.5's
   hidden queues).
4. What happens to each of the 292 tasks when `main` returns (Chapter 13.4 §6)? Why is "nothing records which" the worst
   part of the output?
5. The audit log says "0 of 8". Which Chapter 13.4 rule was broken, and what would have happened with a `Drop`-based
   flush?
6. `endpoint` has no timeout. What does a merchant endpoint that never answers do to this service, and what does a
   timeout on a POST do to delivery semantics (Chapter 8.4)? What makes retrying safe?
7. m3 is slow. Should its events be queued, retried, spilled, or dead-lettered, and by what rule? Who else is allowed
   to be affected by m3's slowness?
8. Is "audit lines stay in order" a real requirement? If it is, is a lock held across the delivery the way to get it?
9. The PR's metrics are process-global statics. What's wrong with them beyond style?

A redesign (listing `review-02-webhook-relay-fixed.rs`, verified) gives each merchant its own bounded queue (32) and
dispatcher task, at most 4 deliveries in flight per merchant (a `Semaphore`), 500 ms per attempt with 3 attempts and
exponential backoff on the timer, a `std::sync::Mutex` held only for pushes, an explicit audit flush, and a shutdown that
requeues everything unfinished. Every event ends in exactly one of four lists. Same simulation, same deploy at t = 5 s:

```text
after the deploy: 300 events accepted
  delivered m1 100 / m2 100 / m3 0; dead-lettered 8; spilled at intake 59; requeued at shutdown 33
  accounted for: 300 of 300
  worst delivery latency: m1 20 ms, m2 20 ms; peak in flight per merchant [1, 1, 4]
  audit lines on durable storage 200 of 200 deliveries
```

Its four tests pass: `every_event_is_accounted_for_exactly_once` (the ids of the four outcome lists, sorted, are exactly
0..300), `a_slow_merchant_does_not_delay_the_others` (m1 and m2 ≤ 40 ms), `in_flight_deliveries_per_merchant_are_bounded`,
and `the_audit_trail_matches_the_deliveries`. Two things in it deserve a second look. m3's 100 events became 8
dead-lettered (all attempts timed out), 59 **spilled** (its queue was full at intake: they went to the durable retry
table instead of memory), and 33 **requeued** at shutdown, a number that includes deliveries still in flight at the drain
deadline, whose outcome is unknown. They're requeued because the receiver deduplicates on the event id: without that
idempotency key, requeueing an in-flight POST would be a double delivery waiting to happen. Compare your review with the
redesign and the model answers. The redesign is one defensible answer, not the only one.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. Why does `tokio::spawn` require `Send + 'static`, and why can't Tokio offer a safe scoped spawn? What do you use when
   your state is `!Send`?
2. What exactly runs when a future is cancelled at its second `.await`? Contrast with a Java thread interrupted in the
   same place.
3. What makes an operation cancel safe? Classify `mpsc::Receiver::recv`, `AsyncReadExt::read_exact`, and
   `mpsc::Sender::send`, and say what `reserve` changes.

### Runtime

4. Walk through Tokio's worker loop: which queue a woken task goes to, when the global queue is checked, when drivers are
   polled, and what the LIFO slot trades.
5. What does the cooperative budget do, what doesn't it cover, and how would you find a task that starves its
   neighbors?
6. What happens, at the OS level, when a Tokio worker has nothing to do? And when a task on another thread wakes it?

### Performance

7. What does a Tokio task cost in memory and in spawn time, compared with an OS thread? What does a framed TCP connection
   cost before it receives a byte?
8. A service's p99 is fine until four specific requests arrive at once. CPU is low. What's your first hypothesis, and
   how do you confirm it with one small program?
9. When is `tokio::sync::Mutex` the right tool? Quote the costs and the failure mode.

### Architecture

10. Design the work placement for a service that parses 20 MB uploads, calls three HTTP APIs, and writes to a sync
    database driver.
11. Design graceful shutdown for a server with long-lived connections: signals, what's cooperative, what's forced, and
    what's measured per deploy.
12. Every queue needs a bound and a policy: list the queues between a client's socket and a downstream database call in
    an async Rust service, and give each a bound and a policy.

### Distributed systems

13. Per-hop timeouts vs a propagated deadline: what does each do to downstream work during a slowdown? How does a
    deadline cross a process boundary?
14. Why does an unbounded queue in front of an overloaded server turn a short dependency slowdown into a long outage?
    Use the numbers from listing `ch05-02`.
15. A delivery times out after the merchant may or may not have processed it. What must be true of the protocol for the
    relay to retry, and what does the relay record so that nothing is lost at a deploy?

---

## Looking ahead: Part XIV

This Part used synchronization as a set of well-behaved tools: channels that release a permit when a message is
received, a `Semaphore` whose permits are atomic counters, a `watch` whose version number tells readers that something
changed, a task state word updated with compare-and-swap. Part XIV (Memory Model and Atomics) opens those tools. It covers
what the CPU and the compiler may reorder, happens-before and the Release/Acquire pairs inside every channel and lock
used here, `Relaxed` counters like the ones in Ferrite's `Stats`, compare-and-swap loops, fences, and false sharing,
with the same measure-it-on-real-hardware discipline.
