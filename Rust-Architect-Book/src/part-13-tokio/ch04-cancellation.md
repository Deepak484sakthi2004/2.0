# Chapter 13.4 — Cancellation and Structured Concurrency

> **Where this sits:** Part XIII · Tokio and Production Async · chapter 4 of 5
> **Prerequisites:** Chapters 13.1–13.3. Chapter 3.5 (`Drop`, and why `Drop` can't fail or await), 8.3 (panics and
> unwinding), 12.2 (cancellation = drop), 12.3 (the payouts reservation incident).
> **After this chapter you can:** predict exactly which lines of an async function run when it's cancelled at each
> `.await`; tell cancel-safe operations from unsafe ones using Tokio's documentation and fix `select!` loops that lose
> data; own spawned tasks with `JoinSet` and `TaskTracker` so none outlive their parent; implement graceful shutdown
> with a `CancellationToken` and a drain deadline; and handle cleanup that needs to `.await`.

---

## Pass 1 · User level — *Stopping work that's in the middle of something*

### 1. Problem

Chapter 1.3 promised it and Chapter 12.2 stated it: **in Rust, cancelling an async operation means dropping its
future.** There's no exception, no interrupt flag, no `CancellationException`. The future is dropped wherever it's
suspended, its destructors run, and the code after that `.await` never executes.

That design is cheap (no runtime machinery, no checks in every loop) and composable (`timeout`, `select!`, and
`abort()` all work on any future). It's also the source of a family of bugs that thread-based Java code rarely meets,
because in a thread-based world a function runs to the end unless it throws. Meridian already had one: the marketplace
payouts incident (Chapter 12.3 §10), where a timeout dropped a payout future between "reserve funds" and "pay out", and
212 sellers' payouts sat in "reserved" until the next morning's reconciliation.

A future is dropped in more situations than it looks:

| Situation | Who drops the future |
|---|---|
| `tokio::time::timeout(d, fut)` elapses | `timeout`, when it returns `Err(Elapsed)` |
| another branch of `tokio::select!` completes first | `select!`, for every losing branch |
| `JoinHandle::abort()`, `JoinSet` dropped, `JoinSet::shutdown()` | the runtime, at the task's next poll |
| the runtime is dropped (end of `main`, shutdown) | the runtime, for every task still alive |
| an HTTP client disconnects | the server framework drops the handler's future (hyper does) |
| the future that awaits this one is itself dropped | its owner, recursively |

This chapter makes cancellation a design property: what runs, what's lost, what must be made safe, and how to stop a
whole tree of tasks on purpose.

### 2. Mental model

**Every `.await` is a possible exit, and destructors are the only code guaranteed to run on the way out.**

```text
async fn transfer(log) {
    let _debit_guard = Reporter("debit guard");     ─┐ owned by the future
    log.push("debited source");                      │
    sleep(40 ms).await;        ◄── cancelled here? ──┼──► drop: _debit_guard's Drop runs; nothing below runs
    log.push("credited destination");                │
    sleep(40 ms).await;        ◄── or here? ─────────┼──► drop: same; "credited" happened, "notified" didn't
    log.push("notified");                            │
    "done"                                          ─┘
}
```

Two different mechanisms share the word "cancellation", and you need both:

1. **Drop-cancellation** (passive): the owner drops the future. Fast and universal, and the code gets no say in where
   it stops. Timeouts and `select!` work this way.
2. **Cooperative cancellation** (active): the code watches a signal (a `CancellationToken`) and stops at points it
   chooses, finishing what it considers atomic. Graceful shutdown works this way.

**Structured concurrency** is the discipline that ties them together: every spawned task has an owner that outlives it,
cancelling the owner cancels its children, and nothing is left running that nobody is waiting for. Tokio doesn't enforce
it (a `tokio::spawn` with a dropped handle is an orphan), but `JoinSet`, `TaskTracker`, and `CancellationToken` let you
build it.

### 3. Rust code

**Where cancellation lands.** Listing `ch04-01-cancel-points.rs` runs the `transfer` above under three timeouts (paused
clock, exact times), then aborts a spawned task:

```text
timeout(100 ms):
    drop(debit guard)
    result Ok("done") after 80ms, steps that ran: ["debited source", "credited destination", "notified"]
timeout(60 ms):
    drop(debit guard)
    result Err("Elapsed") after 60ms, steps that ran: ["debited source", "credited destination"]
timeout(20 ms):
    drop(debit guard)
    result Err("Elapsed") after 20ms, steps that ran: ["debited source"]
    drop(spawned task's guard)
abort(): join result is_cancelled = true
```

The destructor ran every time. What differs is **which side effects happened**: with 60 ms the source was debited and
the destination credited but nobody was notified, and with 20 ms the source was debited and nothing else happened. The
caller sees the same `Err(Elapsed)` in both cases. That's the core problem: **a timeout tells you that you stopped
waiting, not what was done.** It's Chapter 8.4's "ambiguous" error class, produced locally.

**A scope for tasks.** Listing `ch04-03-joinset.rs` asks five quote providers at once, uses the first two good quotes,
and drops the rest:

```rust,ignore
let mut set = JoinSet::new();
for (provider, ms) in [(1, 120), (2, 40), (3, 60), (4, 500), (5, 80)] {
    set.spawn(quote(provider, ms));
}
// Take the first two good quotes, then stop caring about the rest.
let mut good = Vec::new();
while let Some(res) = set.join_next().await {
    match res {
        Ok(q) => good.push(q),
        Err(e) if e.is_panic() => println!("  at {:>3?}: a quote task panicked", start.elapsed()),
        Err(e) => println!("  at {:>3?}: {e}", start.elapsed()),
    }
    if good.len() == 2 {
        break;
    }
}
```

```text
    task 2 dropped
    task 3 dropped
  at 60ms: a quote task panicked
    task 5 dropped
  at 80ms: using [(2, 40), (5, 80)]; 2 tasks still running; dropping the JoinSet
    task 1 dropped
    task 4 dropped
  at 80ms: done
```

Results arrive **in completion order** (provider 2 at 40 ms, provider 3's panic at 60 ms, provider 5 at 80 ms). A
panicked task is just an `Err` in the stream. And when the `JoinSet` goes out of scope, it **aborts every task still in
it**: providers 1 and 4 are dropped at 80 ms instead of running to 120 ms and 500 ms with nobody waiting. That's the
structured-concurrency guarantee: the tasks don't outlive the block that owns the set.

---

## Pass 2 · Systems level — *Cancel safety, select!, and shutdown*

### 4. Under the hood

**Cancel safety is a property of each operation.** An operation is *cancel safe* if dropping its future before it
completes loses nothing: no data consumed, no side effect half done. Tokio documents it per method, and listing
`ch04-07-cancel-safety-docs.rs` prints those sections from Tokio 1.53.1's source. Condensed:

| Method | Tokio's documentation says |
|---|---|
| `mpsc::Receiver::recv` | cancel safe: "it is guaranteed that no messages were received on this channel" |
| `broadcast::Receiver::recv` | cancel safe, same guarantee |
| `AsyncReadExt::read` | cancel safe: "it is guaranteed that no data was read" |
| `AsyncReadExt::read_exact` | **not** cancel safe: "some data may already have been read into `buf`" |
| `AsyncBufReadExt::read_line` | **not** cancel safe: "some data may have been partially read, and this data is lost" |
| `AsyncWriteExt::write_all` | **not** cancel safe: "the provided buffer may have been partially written" |
| `mpsc::Sender::send` | "the message was not sent. **However, in that case, the message is dropped and will be lost.**" |
| `mpsc::Sender::reserve` | cancelling "makes you lose your place in the queue" (nothing else is lost) |
| `Mutex::lock` | cancelling "makes you lose your place in the queue" |

The pattern: an operation is cancel safe when all of its progress lives **outside** the future (in the channel, in the
socket's kernel buffer, in the reader's own buffer), and unsafe when the future itself holds partial progress (bytes
already copied into your buffer, a message moved into the future).

**`select!` drops the losers.** `tokio::select!` polls its branches (in a random order by default, to avoid starving
the later ones; `biased;` makes the order the textual one), and when one completes, it drops every other branch's
future. In a loop, each iteration creates fresh futures. Listing `ch04-02-select-cancel-safety.rs` shows what that does
to a non-cancel-safe read. A peer sends three 8-byte frames, each in two pieces (3 bytes, then 5 bytes 15 ms later), and
the reader races `read_exact` against a 25 ms housekeeping tick:

```rust,ignore
// BROKEN: a fresh read_exact future each loop turn, raced against a 25 ms housekeeping tick.
loop {
    let mut buf = [0u8; 8];
    tokio::select! {
        res = r.read_exact(&mut buf) => match res {
            Ok(_) => frames.push(buf.to_vec()),
            Err(e) => { println!("read_exact in select!: stream ended: {e}"); break; }
        },
        _ = sleep(Duration::from_millis(25)) => { ticks += 1; } // housekeeping: flush metrics, check deadlines
    }
}
```

```text
read_exact in select!: stream ended: early eof
read_exact in select!: [[0, 1, 2, 3, 4, 5, 6, 7], [13, 14, 15, 16, 17, 20, 21, 22]]
  2 ticks; 24 bytes sent, 16 bytes delivered in frames, 8 lost
FramedRead in select!: [[0, 1, 2, 3, 4, 5, 6, 7], [10, 11, 12, 13, 14, 15, 16, 17], [20, 21, 22, 23, 24, 25, 26, 27]]
  2 ticks; every frame intact
```

The tick fired while `read_exact` had already copied the first 3 bytes of frame 1 (`10, 11, 12`) into `buf`. The
`select!` dropped that `read_exact` future, and the next loop iteration started a new one with a new `buf`. Those 3 bytes
were gone, the stream was **misaligned**, and the second "frame" is the tail of frame 1 glued to the head of frame 2:
`[13, 14, 15, 16, 17, 20, 21, 22]`. With a real protocol that's a corrupted message, not a missing one. The stream then
ended early because the reader was 8 bytes behind the writer.

The fixed version uses `tokio_util::codec::FramedRead`, which keeps partial frames in **its own buffer**, a field of the
reader that survives across polls. Its `next()` future holds no progress, so dropping it loses nothing. Two other fixes
work: create the `read_exact` future once *outside* the loop and poll it by `&mut` reference (`tokio::pin!` it), so a
losing iteration doesn't drop it; or move reading into its own task that sends whole frames over a channel.

**How `abort()` reaches a task.** It sets the CANCELLED bit in the task's state word and schedules the task (Chapter 13.2
§4). The next time a worker picks it up, the harness drops the future instead of polling it. So an aborted task stops
at the `.await` where it was **already suspended**. A task that's running at the moment of `abort()` finishes its
current poll first. A task stuck in a CPU loop without `.await`s can't be aborted at all.

**Graceful shutdown: signal, drain, deadline.** Listing `ch04-04-graceful-shutdown.rs` combines the cooperative tools:
a `CancellationToken` to tell connection tasks to stop, a `TaskTracker` to wait for them, and a timeout to bound the
wait. Each connection handler races the *arrival of the next request* against the token, but deliberately **not the
serving of a request**:

```rust,ignore
loop {
    tokio::select! {
        _ = stop.cancelled() => {
            return format!("conn {id}: stopped cleanly at {:?} after {served} requests", start.elapsed());
        }
        _ = sleep(Duration::from_millis(30)) => {} // the next request arrives
    }
    sleep(Duration::from_millis(request_ms)).await; // serve it: deliberately NOT raced with `stop`
    served += 1;
}
```

```text
at 100ms: shutdown requested
conn 2: stopped cleanly at 100ms after 1 requests
conn 1: stopped cleanly at 105ms after 3 requests
at 1.1s: drain deadline passed, aborting 1 straggler(s)
at 1.1s: done
```

Connection 2 was idle when shutdown arrived and stopped at once. Connection 1 was in the middle of a 5 ms request,
finished it, and stopped at 105 ms. Connection 3 was in a 5-second request. Graceful shutdown waited for it until the
1-second drain deadline and then aborted it: cooperation has a time limit, and after that, drop-cancellation takes over.
That's the shape Project L5 (Ferrite v2) uses.

### 5. Memory

**Cancellation frees memory; that's the point of it.** Dropping a future drops everything it owns: buffers, guards,
channel senders, permits. That's why a timeout around a request handler also releases the request's memory, its
semaphore permit, and its database connection (if the connection guard lives in the future). It's the RAII story of
Chapter 3.5, applied to suspended computations.

**What cancellation can't do: `Drop` can't `.await`.** A destructor is a synchronous function. Cleanup that needs I/O
(flush a buffer to a socket, send a "goodbye" frame, release a remote lock) can't happen inside `Drop`. Listing
`ch04-05-async-cleanup.rs` delivers 1,000 settlement lines through a `BufWriter` over an in-memory pipe:

```text
bytes written: 19890
received, dropped without shutdown(): 16370
received, with shutdown().await:      19890
  session 1: async cleanup ran in a spawned task
  session 2: no runtime in Drop (there is no reactor running, must be called from the context of a Tokio 1.x runtime); cleanup skipped
```

Without an explicit `shutdown().await`, the last 3,520 bytes were still in the `BufWriter`'s 8 KiB buffer when it was
dropped, and they were **discarded silently**: the async version of Chapter 3.5's `process::exit` export incident. The
rule is the same as there: **make cleanup an explicit async method (`close().await`, `shutdown().await`), and treat
`Drop` as a backstop.** A backstop can hand async work to the runtime with `Handle::try_current()` and `spawn` (session
1), but only if a runtime exists. In session 2, the value was dropped after the runtime had gone, and the cleanup was
skipped with the error message Tokio gives for "there is no reactor running". A `Drop` that calls
`tokio::spawn` directly would have **panicked** with that message instead.

**`JoinSet` keeps results until you collect them.** A task that finished stores its output in its task cell until the
`JoinSet` hands it to `join_next()`. A set that spawns for hours and never joins keeps every result. Reap in the loop
(`try_join_next()`, or a `join_next()` branch in the `select!`), as Project L5's accept loop does.

### 6. CPU / OS

**Runtime shutdown drops tasks where they wait.** Listing `ch04-06-runtime-drop.rs` spawns an upload task that sleeps
between two parts, and a blocking export, then returns from the `block_on`:

```text
  audit upload: part 1 sent
main's work is done; returning
  drop(audit-upload task state)
runtime shut down after 100.13364ms (didn't wait for the blocking task)
  blocking export finished
plain drop of a runtime with a 300 ms blocking task took 280.180762ms
```

- The upload task was dropped at its `sleep`: "part 2" never ran, and its guard's destructor did. A detached task
  gets no warning when `main` returns. Chapter 11.1's settlement-upload incident (a detached thread that never ran)
  has an exact async twin.
- A **blocking** task can't be dropped mid-closure (Chapter 13.2). A plain drop of the runtime, which is what
  `#[tokio::main]` does when `main` returns, **waits** for running blocking tasks: 280 ms here (the 300 ms job had
  already run for 20 ms). `shutdown_timeout(100 ms)` bounds that wait, and the blocking thread keeps running detached
  after the runtime is gone ("blocking export finished" printed later).

**What the kernel keeps.** Cancelling a future doesn't undo kernel work. If `write_all` had handed 5,000 bytes to the
kernel before being dropped, those bytes are in the socket's send buffer and **will be sent**. The peer sees a partial
message, then whatever you write next. When a Tokio I/O resource (`TcpStream`, `UdpSocket`) is dropped, its epoll
registration is removed and the descriptor closed. A socket closed with unread data in its receive buffer makes Linux
send an RST instead of a FIN, which can destroy data the peer hasn't read yet [OS] (Project L5 handles that with a
"lingering close"). Completion-based I/O (io_uring, Chapter 12.1) is different again: the kernel owns the buffer until the
operation completes, so dropping the future can't free it, which is why io_uring runtimes use owned buffers.

---

## Pass 3 · Architect level — *Designing for being stopped*

### 7. Trade-offs

**Choosing a cancellation mechanism:**

| Mechanism | Who decides where it stops | Latency to stop | Good for | Risk |
|---|---|---|---|---|
| `timeout(d, fut)` | the runtime: at whatever `.await` is pending | immediate | bounding waits on idempotent or read-only work | ambiguous side effects ("did it happen?") |
| `select!` losing branch | same | immediate | racing, housekeeping ticks, shutdown signals | non-cancel-safe operations lose data |
| `abort()` / dropping a `JoinSet` | the runtime, at the next suspension | next poll | stopping tasks you own | same as timeout; can't stop CPU loops |
| `CancellationToken` | the code: at points it checks | when the code next checks | graceful shutdown, "finish the current request" | code that never checks never stops (pair it with a deadline) |
| Runtime drop | the runtime | immediate for async, waits for blocking | end of process | detached work silently lost |

**Making an operation safe to cancel.** Four techniques, in order of preference:

1. **Keep progress outside the future.** A codec that buffers in the reader (`FramedRead`), a queue position held by a
   permit (`reserve`), state in the database rather than in local variables.
2. **Make each step idempotent and resumable.** If the transfer above records "debited, id 42" durably before the
   credit, a restart can resume. That's the saga or outbox pattern (Part XXIV develops it).
3. **Don't race the critical section.** Race only the waits *between* units of work (listing `ch04-04`'s connection
   loop), never the unit itself.
4. **Guards with a compensating `Drop`.** A reservation guard whose `Drop` releases the reservation unless `commit()` was
   called (Chapter 12.3's payouts fix). It works only if the compensation is synchronous and local. Otherwise, `Drop`
   records the need for compensation (a durable "orphaned reservation" row) and a background job does it.

**Structured-concurrency tools:**

| Tool | Owns tasks? | Cancels on drop? | Collects results? | Use for |
|---|---|---|---|---|
| `tokio::join!` / `try_join!` | futures, not tasks | yes (the futures are dropped) | yes, all | a fixed set of concurrent calls within one task |
| `FuturesUnordered` | futures | yes | as they complete | many concurrent futures in one task, no spawning |
| `JoinSet` | spawned tasks | **yes: aborts all** | as they complete | a dynamic set of tasks that must not outlive a scope |
| `TaskTracker` | tracks, doesn't own | no | no (wait for all) | "wait until everything I started has finished" at shutdown |
| `CancellationToken` (+ `child_token()`) | nothing | n/a | n/a | a stop signal that fans out down a tree |
| bare `tokio::spawn` | nobody | no | only via the handle | fire-and-forget, and document why it's safe to lose |

### 8. Java comparison

| Java | Tokio / Rust |
|---|---|
| `Thread.interrupt()` + checking `isInterrupted()` / catching `InterruptedException` | `CancellationToken` + `cancelled()` (cooperative) |
| `Future.cancel(true)`: interrupts the thread, which may ignore it | `abort()`: drops the future at its next `.await`; the task can't ignore it |
| `CompletableFuture.cancel()`: completes the future exceptionally; the computation keeps running | dropping a `JoinHandle`: the task keeps running (detached) |
| `ExecutorService.shutdown()` → `awaitTermination(t)` → `shutdownNow()` | `CancellationToken::cancel()` → `timeout(t, tracker.wait())` → `abort()` / `JoinSet::shutdown()` |
| `StructuredTaskScope` (preview since JDK 21, still preview in JDK 25) | `JoinSet` (owning scope, cancels on drop) |
| `try-with-resources` / `finally` run on cancellation (as an exception) | destructors run on drop; no `finally` block runs *code after the `.await`* |
| Kotlin coroutines: cooperative cancellation at suspension points, `CancellationException` | drop at suspension points, no exception, no catch |

> **Analogy limit.** In Java (and in Kotlin), cancellation arrives as an *exception at a suspension or blocking point*,
> so the code can catch it, run `finally` blocks, and even decide to finish. In Rust, a dropped future gets **no
> control flow at all**: there's no point at which "the rest of the function" could run. Only destructors run. So
> Java's idiom "catch the interruption and finish the critical step" has no Rust equivalent. Its replacement is
> designing the steps so that stopping between any two of them is safe, and using a `CancellationToken` where you
> need to choose the stopping point.

### 9. Production scenario

**Gateway deploys without dropped requests.** Meridian's Rust gateway pods (Chapter 2.1's workspace; 55 pods at 400K
req/s in the Java baseline) are replaced on every deploy. Kubernetes sends SIGTERM, waits `terminationGracePeriodSeconds`
(30 s), then SIGKILL. The shutdown sequence the team specified:

1. **SIGTERM → mark unready.** `tokio::signal::unix::signal(SignalKind::terminate())` resolves (the signal driver's
   socketpair from Chapter 13.1 §3), the readiness endpoint starts failing, and the load balancer stops routing new
   connections within its probe interval (5 s).
2. **Keep accepting for 5 s**, because the load balancer's view lags (connections still arrive during the probe
   interval). Then stop accepting.
3. **Cancel the root `CancellationToken`.** Connection tasks hold `child_token()`s. An idle keep-alive connection closes
   at once. A connection that's serving a request finishes it and replies with `Connection: close` (HTTP/1.1) or a
   GOAWAY (HTTP/2) instead of taking another request.
4. **Drain for up to 20 s** (`timeout(20 s, tracker.wait())`), then abort the stragglers (long-poll and streaming
   responses) and log their count.
5. **Flush explicitly**: access-log writer, metrics push, tracing exporter, each with `shutdown().await` and a
   2-second timeout. None of it relies on `Drop`.
6. Return from `main` well inside the 30 s grace period, so SIGKILL never cuts a flush short.

The deploy dashboards now track "requests aborted by shutdown" per deploy. It stays at zero except for streaming
endpoints, where the clients reconnect by design.

### 10. Failure scenario

**The housekeeping tick that corrupted a feed (market-data, 2026, fictional).** The market-data ingest service (the
Rust fan-out rewrite of Chapter 8.2 §10) reads the exchange feed's frames: the 0xCAFE header from Chapters 2.4–2.5,
then a body. A refactor merged the frame reader and the stale-quote check into one loop:

```rust,ignore
// Failure-scenario sketch (not a verified listing; listing ch04-02 reproduces the effect)
loop {
    tokio::select! {
        r = read_frame(&mut sock) => handle(r?)?,        // read_exact(header) then read_exact(body)
        _ = stale_check.tick() => mark_stale_quotes(),  // every 100 ms
    }
}
```

`read_frame` is two `read_exact` calls, and it's not cancel safe. In testing, frames arrived whole and fast, so the tick
never fired in the middle of one. In production, the exchange's gateway split large snapshot frames across TCP segments
(Chapter 2.4's design exercise said it would), and when a tick landed between two segments the partial frame was dropped
with the future. The next `read_frame` started in the middle of a body, found no 0xCAFE magic, and returned
`HeaderError::BadMagic`. The error handler (correctly, per Chapter 8.2) closed the connection and reconnected. Each
reconnect triggered a full snapshot (large, split frames), which made the next tick-in-the-middle more likely. The feed
flapped for 25 minutes during the market open, and quotes went stale.

Listing `ch04-02` is the same failure in 24 bytes: "8 lost", and the frame after the cut glued to the next one.

Fixes: the reader became a `FramedRead` with a codec that decodes the 0xCAFE header and length (partial frames live in
the reader's buffer, cancel safe); the stale check moved into its own task (fewer `select!` branches means fewer
cancellation points); and the team added a CI test that feeds every recorded frame **split at every byte offset** with a
tick firing between the pieces. The review checklist gained a line: "every branch of a `select!` in a loop: is it cancel
safe? Quote the docs."

---

## Practice

### 11. Interview & architecture questions

1. What exactly happens, line by line, when `timeout` elapses around an async function suspended at its second
   `.await`?
2. What does "cancel safe" mean? Classify `recv`, `read`, `read_exact`, `write_all`, `send`, and `reserve`.
3. Why did listing `ch04-02` produce the frame `[13, 14, 15, 16, 17, 20, 21, 22]`? Give three different fixes.
4. How is `abort()` different from `Future.cancel(true)` in Java? What can each of them *not* stop?
5. Why can't `Drop` flush a `BufWriter` over a socket? What should the API look like instead?
6. A handler times out after 2 s while calling a payment processor. What can the handler know about the charge, and
   what should it do next? (Connect to Chapter 8.4.)
7. Compare `JoinSet`, `TaskTracker`, and `FuturesUnordered`. Which one gives structured concurrency, and why?
8. Design a graceful shutdown for a server with long-poll endpoints. What's cooperative, what's forced, and what's the
   deadline?
9. Why does `#[tokio::main]` returning sometimes hang the process for seconds?
10. Kotlin cancels coroutines cooperatively at suspension points, too. What can a Kotlin coroutine do when cancelled
    that a Rust future can't?

### 12. Exercises

- **Beginner.** Add a fourth timeout (45 ms) to listing `ch04-01` and predict the output before running it.
- **Intermediate.** Fix listing `ch04-02`'s broken loop by pinning one `read_exact` future outside the loop and polling
  it with `&mut`. Show that all three frames arrive intact.
- **Advanced.** Write a `ReservationGuard` for the payouts flow: `reserve(amount) -> Guard`, `guard.commit().await`, and a
  `Drop` that records an "orphaned reservation" in an in-memory outbox if `commit` never ran. Test it with `timeout` at
  each `.await` of the flow (paused clock).
- **Systems.** Measure how long `JoinSet::shutdown()` takes for 10,000 tasks that are all suspended on a `sleep`, and for
  10,000 tasks where 100 are in a 50 ms CPU loop. Explain the difference.
- **Architecture.** Write the shutdown specification for `merchant-notify` (100,000 WebSocket-style connections per pod):
  what the clients are told, how long the drain lasts, how reconnects are spread so the remaining pods aren't hit by
  100,000 simultaneous reconnects, and what's measured per deploy.

### 13. Debugging exercise

A batch job copies records from a queue to a database. Sometimes, after a deploy, the database is missing records that
the queue marked as consumed:

```rust,ignore
// Debugging exercise: find the defect (not a verified listing)
async fn copy_loop(queue: Queue, db: Db, stop: CancellationToken) {
    loop {
        tokio::select! {
            _ = stop.cancelled() => return,
            batch = async {
                let batch = queue.receive(100).await?;   // marks messages in-flight (visibility timeout 30 s)
                queue.ack(&batch).await?;                  // deletes them from the queue
                db.insert_many(&batch).await?;             // 50–200 ms
                Ok::<_, Error>(batch.len())
            } => { batch?; }
        }
    }
}
```

Find the window in which a shutdown loses records, explain it with this chapter's model, and fix it two ways: by
reordering, and by changing what the `select!` races. Which fix also survives a crash (SIGKILL) at any point? Model
answer in Appendix A.

### 14. Design exercise

Design the cancellation behavior of `payments-core`'s charge endpoint (Chapter 8.4: idempotency keys, processor call,
ledger write, ambiguous outcomes). For each `.await` in the flow (fraud score, processor authorization, ledger write,
outbox publish, reply), state what happens if the client disconnects (hyper drops the handler future), if the request's
deadline fires, and if the pod shuts down. Where do you use a `CancellationToken` instead of letting the future be
dropped? Which steps do you move into a spawned, tracked task so that a client disconnect can't interrupt them, and how
does the idempotency key make the retry safe?
