# Chapter 13.2 — Tasks, Spawning, and Blocking Code

> **Where this sits:** Part XIII · Tokio and Production Async · chapter 2 of 5
> **Prerequisites:** Chapter 13.1 (workers, queues, the LIFO slot). Chapters 11.2 (`Send`/`Sync`), 11.6 (scoped
> threads and why the old scoped API was unsound), 12.3 (what makes a future `!Send`), 12.6 (blocking an executor).
> **After this chapter you can:** explain why `tokio::spawn` demands `Send + 'static` and what to do when your future
> isn't; state exactly what dropping, aborting, and awaiting a `JoinHandle` do; measure what a blocking call costs an
> async service; choose between `spawn`, `spawn_blocking`, `block_in_place`, a dedicated thread pool, and `LocalSet` for
> a given piece of work; and keep per-request context attached to a task instead of a thread.

---

## Pass 1 · User level — *Starting work, and keeping blocking work away from the workers*

### 1. Problem

`tokio::spawn` looks like `std::thread::spawn` with cheaper threads, and most of its surprises come from that
resemblance. Chapter 13.1 showed that a task is 88 bytes plus its future, and that four worker threads can run a hundred
thousand of them. The same numbers explain the dangers: a hundred thousand tasks **share** four threads. A task that
calls a blocking function, or computes for 150 ms, doesn't only slow itself down. It takes one of the four threads away
from every other task queued there.

Meridian has now paid for this lesson twice. In May 2026 a ~150 ms password hash inside the async reconnect handshake
drained a `merchant-notify` pod (Chapter 12.6 §10). In June a synchronous DNS lookup in an async function stalled a pod
for 2.3 s (Chapter 13.1 §10). This chapter is the systematic version: what a task is allowed to capture, what its handle
means, and where each kind of work should run.

### 2. Mental model

Sort the work in an async service into three kinds, because each belongs in a different place:

```text
 KIND OF WORK            EXAMPLE                              WHERE IT RUNS                    WHY
 ─────────────           ───────                              ─────────────                    ───
 waits on I/O            socket reads, timers, channel recv  a task on the async workers      waiting costs nothing: the task
                                                                                                returns Pending and frees the thread
 blocking I/O / syscalls std::fs, a sync DB driver, DNS via   spawn_blocking (the blocking     the THREAD waits: give it a
                         getaddrinfo, a mutex held for long   pool, up to 512 threads)          thread that has nothing else to do
 CPU-bound computation   hashing, compression, parsing a     a bounded CPU pool (rayon, or     the thread is BUSY: there are only
                         20 MB file, scoring                   spawn_blocking behind a limit)   as many useful threads as cores
```

And one rule about the handle: **a `JoinHandle` is a receipt, not an owner.** The runtime owns the task. The handle lets
you wait for the result or ask for cancellation. Dropping it does neither: the task keeps running, detached.

### 3. Rust code

**What `spawn` requires.** Its signature (tokio 1.53.1, `task/spawn.rs`):

```rust,ignore
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
```

`'static` because the task may outlive the function that spawned it. Listing `ch02-01-spawn-borrow.rs`:

```rust,compile_fail
//! tokio::spawn requires a 'static future: the task may outlive the function that spawned it.
#[tokio::main]
async fn main() {
    let merchant = String::from("m-1042");
    let task = tokio::spawn(async {
        println!("notifying {merchant}"); // borrows `merchant`, a local of main
    });
    task.await.unwrap();
}
```

```text
error[E0373]: async block may outlive the current function, but it borrows `merchant`, which is owned by the current function
 --> src/main.rs:6:29
  |
6 |     let task = tokio::spawn(async {
  |                             ^^^^^ may outlive borrowed value `merchant`
7 |         println!("notifying {merchant}"); // borrows `merchant`, a local of main
  |                              -------- `merchant` is borrowed here
  |
  = note: async blocks are not executed immediately and must either take a reference or ownership of outside variables they use
help: to force the async block to take ownership of `merchant` (and any other referenced variables), use the `move` keyword
```

The same E0373 as `std::thread::spawn` in Chapter 4.3, for the same reason: the compiler can't know that `task.await`
runs before `merchant` is dropped. The fixes are also the same: `async move`, or share with `Arc`.

`Send` because on the multi-thread runtime a task can resume on a different worker after any `.await`. Listing
`ch02-02-spawn-not-send.rs`:

```rust,compile_fail
//! On the multi-thread runtime a task may resume on another worker thread, so tokio::spawn requires
//! the future to be Send. An Rc held across an .await makes it !Send.
use std::rc::Rc;

async fn fetch_rate() -> u32 {
    tokio::task::yield_now().await;
    42
}

#[tokio::main]
async fn main() {
    let task = tokio::spawn(async {
        let cache = Rc::new(vec![1u32, 2, 3]);
        let rate = fetch_rate().await; // `cache` is alive across this .await
        cache.len() as u32 + rate
    });
    println!("{}", task.await.unwrap());
}
```

```text
error: future cannot be sent between threads safely
   --> src/main.rs:13:16
    |
 13 |       let task = tokio::spawn(async {
    |                ^^^^^^^^^^^^ future created by async block is not `Send`
    |
    = help: within `{async block@src/main.rs:13:29: 13:34}`, the trait `Send` is not implemented for `Rc<Vec<u32>>`
note: future is not `Send` as this value is used across an await
   --> src/main.rs:15:33
    |
 14 |         let cache = Rc::new(vec![1u32, 2, 3]);
    |             ----- has type `Rc<Vec<u32>>` which is not `Send`
 15 |         let rate = fetch_rate().await; // `cache` is alive across this .await
    |                                 ^^^^^ await occurs here, with `cache` maybe used later
note: required by a bound in `tokio::spawn`
```

(Output trimmed.) The note is the important part: **"used across an await"**. An `Rc` that's created and dropped
between two `.await`s never becomes part of the future's state, so it doesn't affect `Send` (Chapter 12.3). The same goes
for a `std::sync::MutexGuard`: holding one across an `.await` makes the task `!Send`, and the compiler catches it at the
`spawn` call.

If the state really must be `!Send`, keep the tasks on one thread. Listing `ch02-03-localset.rs` runs `spawn_local`
tasks sharing an `Rc<RefCell<..>>` on a `LocalSet`:

```rust,ignore
let counts: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
let local = LocalSet::new();
for i in 0..3 {
    let counts = Rc::clone(&counts);
    local.spawn_local(async move {
        tokio::task::yield_now().await; // Rc alive across .await: fine, this task never changes threads
        counts.borrow_mut().push(i);
    });
}
local.await; // runs every spawn_local task to completion
```

```text
counts = [2, 1, 0], Rc strong count = 1
```

`spawn_local` requires `'static` but not `Send`. The price is that those tasks never use another core.

**What the handle does.** Listing `ch02-04-joinhandle.rs` checks four behaviors:

```text
1. handle dropped, task finished anyway: done = 1
2. aborted: is_cancelled = true, is_panic = false, done = 1
3. task panicked: "refund amount overflow"; the runtime and this task are fine
4. is_finished before abort: true
4. result after abort: Ok(7)
```

1. Dropping the `JoinHandle` **detaches** the task. It still ran to completion.
2. `abort()` asks the runtime to drop the task's future at its next suspension point. `await`ing the handle then gives
   `Err(JoinError)` with `is_cancelled() == true`. The `done` counter shows the code after the task's `sleep` never ran.
3. A panic inside a task is caught at the task boundary (the harness wraps each poll in `catch_unwind`), stored, and
   returned through the handle as a `JoinError` with `is_panic() == true`. `into_panic()` gives you the payload. The
   worker thread and every other task are unaffected. That's the "per-request containment" Chapter 8.3 asked for,
   provided by the runtime.
4. `abort()` after the task finished does nothing: the result is still there.

---

## Pass 2 · Systems level — *Threads under tasks*

### 4. Under the hood

**Why there's no scoped `spawn`.** `std::thread::scope` (Chapter 11.6) lets threads borrow from the enclosing stack
frame because `scope` doesn't return until every thread has been joined, and a blocking function can guarantee that.
An async equivalent would be an `async fn scope(...)` whose future joins the tasks before completing. But a future can be
**dropped or leaked instead of completed**: a `select!` can drop it, `mem::forget` can leak it (safe code, Chapter
1.3's Leakpocalypse box). If the scope future disappears while the spawned tasks still hold borrows into its frame,
those borrows dangle. There's no way to make that sound without either blocking the thread in `Drop` (which defeats the
point of async) or `unsafe` contracts. So Tokio's API is `'static` plus explicit sharing (`Arc`, channels), and the
structured-concurrency tools of Chapter 13.4 (`JoinSet`, `TaskTracker`) manage *lifetimes of tasks*, not borrows.

**Why `Send`: tasks change threads.** Listing `ch02-08-thread-hop.rs` runs 8 tasks on 4 workers. Each yields 200 times
and records every thread it ran on. It also stores its request ID in a `thread_local!` once at the start (the
Java `ThreadLocal`/MDC habit) and in a `tokio::task_local!`, and checks both after every `.await`:

```text
most worker threads one task ran on: 4
reads of the thread_local that saw ANOTHER request's id: 1489 of 1,600
reads of the task_local that saw another request's id:  0 of 1,600
```

A single task ran on all four workers. So the `thread_local` read after an `.await` returned **another request's ID**
93% of the time: the value belongs to whichever request last ran on that thread. `task_local!` values live in the task's
future (set with `.scope(value, future)`), so they follow the task wherever it runs, and were right 1,600 times out of
1,600. The `Send` bound is what makes the migration safe for your *data*. Nothing makes it safe for your *assumptions*
about thread identity, and `thread_local!` state quietly breaks them.

**The task harness.** Each task cell (13.1 §5) holds a state word with bits for RUNNING, COMPLETE, NOTIFIED, CANCELLED,
and JOIN_INTEREST, updated with atomic compare-and-swap [LIB]. `abort()` sets CANCELLED and schedules the task. The next
time a worker polls it, the harness sees the bit and drops the future instead of polling it, which runs the destructors
of everything the future holds (Chapter 13.4 shows the destructor order). Dropping the handle clears JOIN_INTEREST, so
the output is dropped instead of stored. A panic is caught by `catch_unwind` around `poll`, and the payload is stored as
the output.

**The blocking pool.** `spawn_blocking(f)` sends the closure to a separate pool of OS threads: created on demand, up to
`max_blocking_threads` (default 512), exiting after `thread_keep_alive` (default 10 s) idle. Listing
`ch02-06-blocking-pool.rs` configures 2 workers, at most 8 blocking threads, and a 300 ms keep-alive, then runs 32 jobs of
100 ms:

```text
threads at start: 3 (main + 2 workers)
threads while 32 jobs run: 11
32 x 100 ms on max_blocking_threads(8) took 402ms
threads after 600 ms idle (keep-alive 300 ms): 3
aborted spawn_blocking: join result ok = true, closure ran to the end = true
```

- The pool grew to 8 threads (3 → 11) and **queued the rest**: 32 jobs took four waves of 100 ms, 402 ms total. The
  blocking pool's queue is unbounded [LIB]. `max_blocking_threads` bounds threads, not work.
- Idle threads exited after the keep-alive, back to 3.
- **A running blocking task can't be aborted.** `abort()` returned, the handle then reported success (`ok = true`,
  because the closure completed), and the closure ran to its end. There's no safe way to stop a thread in the middle of a
  function, so aborting only prevents a *queued* closure from starting.

`block_in_place(f)` is the other door. It turns the *current worker* into a blocking thread for the duration of `f`,
after handing its queue to a new worker, so no async task is stuck behind it. It needs other workers to exist. Listing
`ch02-07-block-in-place.rs` calls it on a current-thread runtime:

```rust
//! block_in_place turns the current worker into a blocking thread (and hands its queue to a new worker).
//! That needs other workers, so it panics on the current-thread runtime.
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let digest = tokio::task::block_in_place(|| {
        std::thread::sleep(std::time::Duration::from_millis(10)); // pretend: hash a statement file
        0xdeadbeef_u32
    });
    println!("{digest:x}");
}
```

```text
thread 'main' (14) panicked at src/main.rs:6:18:
can call blocking only when running on the multi-threaded runtime
```

A library that calls `block_in_place` internally therefore breaks every user on a current-thread runtime, including
`#[tokio::test]`, which defaults to current-thread. Prefer `spawn_blocking` in library code.

### 5. Memory

**Blocking threads are OS threads.** Each has a 2 MiB default stack (configurable with `thread_stack_size`), so the
default cap of 512 is up to **1 GiB of reserved virtual memory** and 512 kernel threads (Chapter 11.1 measured what that
costs to create). A service that sends a slow dependency's calls through `spawn_blocking` can reach that cap in one
incident, and the unbounded queue behind it then grows without limit. Bound blocking work *before* it reaches the pool
(a `Semaphore`, Chapter 13.3), and set `max_blocking_threads` to what the dependency can actually serve.

**Captured state lives in the task.** Everything an `async move` block captures is moved into the future, and the future
into the task cell. A 64 KiB `Vec` captured by 10,000 tasks is 640 MB, whether or not the tasks are running. Capture
`Arc`s of shared data, not copies.

**`task_local!` values** live inside the future returned by `.scope()`, so they cost the size of the value per task and
nothing per thread. `thread_local!` values cost one per thread, and they're the wrong tool for per-request data (§4).

**A detached task keeps its captures alive.** If nobody holds its `JoinHandle` and nothing cancels it, a task that waits
on a channel whose sender is never dropped lives forever, along with everything it captured. The stable
`num_alive_tasks()` metric (13.1 §9) is the way to see this leak from outside.

### 6. CPU / OS

**Blocking a worker, measured.** Listing `ch02-05-blocking-stall.rs` runs a heartbeat task (sleeps 10 ms in a loop,
records its worst lateness) on 4 workers while other tasks call `std::thread::sleep(200 ms)`, a stand-in for any
blocking call:

```text
4 workers, 1 task calls thread::sleep(200 ms)           heartbeat worst lateness    1.9ms
4 workers, 4 tasks call thread::sleep(200 ms)           heartbeat worst lateness  196.2ms
4 workers, 4 x spawn_blocking(thread::sleep(200 ms))    heartbeat worst lateness    1.1ms
```

- **One blocked worker out of four: 1.9 ms.** The heartbeat was on another worker, and work stealing moves runnable
  tasks away from a stuck worker. That's why blocking bugs often pass tests: the damage is invisible until the number of
  concurrent blockers reaches the number of workers. (It isn't zero damage: 13.1's experiment 3 showed the task in the
  blocked worker's LIFO slot waiting the whole time.)
- **Four blocked workers: 196 ms.** Every worker was inside `thread::sleep`, so nothing polled the heartbeat, the timer
  wheel, or the I/O driver. For the duration, the service was dead to the network.
- **`spawn_blocking`: 1.1 ms.** The sleeps ran on blocking-pool threads, and the kernel scheduled them alongside the
  workers.

**CPU-bound work, measured.** Blocking isn't only waiting. Listing `ch02-09-cpu-bound.rs` runs eight ~100 ms SHA-256 jobs
on a **2-worker** runtime (the machine has 4 vCPUs), three ways, with a 5 ms heartbeat:

```text
inline on the async workers    8 jobs took   482ms   heartbeat worst lateness  444.5ms
spawn_blocking                 8 jobs took   354ms   heartbeat worst lateness    2.5ms
rayon pool + oneshot           8 jobs took   425ms   heartbeat worst lateness   17.4ms
```

- **Inline:** the heartbeat was 444 ms late. Both workers were hashing, so nothing else ran.
- **`spawn_blocking`:** heartbeat fine, and the jobs finished *faster* (354 ms), because the pool created 8 threads
  and the kernel spread them over all 4 vCPUs, not just the runtime's 2 workers. That's a hidden hazard as well as a
  win: `spawn_blocking` has no notion of cores, and a burst of CPU-heavy jobs becomes up to 512 threads competing for
  them.
- **rayon + oneshot:** heartbeat fine (17 ms, since rayon's pool also competes for the same cores), and the pool is sized
  to the core count by default, so CPU work is **bounded by cores**, not by a thread cap. The task sends the job to
  rayon and awaits a `oneshot` for the result:

```rust,ignore
let (tx, rx) = tokio::sync::oneshot::channel();
rayon::spawn(move || {
    let _ = tx.send(hash_statement(rounds));
});
rx.await.unwrap()
```

(Excerpt of listing `ch02-09-cpu-bound.rs`.) If the task is cancelled, `rx` is dropped, the send fails silently, and the
result is discarded. The computation itself still runs to completion, as with `spawn_blocking`.

**What "blocking" means, in time.** Tokio's guidance (and Alice Ryhl's widely cited 2020 post, "Async: What is
blocking?") puts the budget at roughly **10–100 µs between `.await`s**. That's not a hard rule, but it's the right order
of magnitude. At 400K requests/s (the Meridian gateway) spread over 8 workers, each worker has about 20 µs per request.
A 1 ms stretch without an `.await` delays 50 requests on that worker.

---

## Pass 3 · Architect level — *Placing work on purpose*

### 7. Trade-offs

| Tool | Runs on | Use it for | Bounded by | Cancellable | Pitfalls |
|---|---|---|---|---|---|
| `tokio::spawn` | async workers | I/O-bound work, short CPU bursts (< ~100 µs between awaits) | nothing (bound it yourself) | yes, at `.await` points | blocking inside stalls a worker; needs `Send + 'static` |
| `spawn_blocking` | blocking pool | blocking syscalls and libraries (files, sync drivers, DNS) | `max_blocking_threads` (512); queue unbounded | only before it starts | CPU bursts become hundreds of threads; can't abort running work |
| `block_in_place` | the current worker, converted | a blocking call where moving data to another thread is awkward | worker count | no | panics on current-thread runtimes (and `#[tokio::test]`) |
| rayon (or a dedicated CPU pool) + `oneshot` | a pool sized to cores | CPU-bound work: hashing, compression, parsing, scoring | cores; add a semaphore for queueing | result discarded; work runs | must not do blocking I/O inside; two pools on one machine compete |
| `LocalSet` + `spawn_local` | one thread | `!Send` state (`Rc`, `RefCell`, some FFI handles) | one core | yes | no parallelism; everything on it stalls together |
| A second runtime | its own threads | isolating a class of work (admin endpoints, a noisy dependency) | its worker count | yes | two schedulers competing for cores; don't `block_on` one from the other |

> **Why not just use `spawn_blocking` for everything that might block?** Because it's unbounded in the dimension that
> hurts. A dependency that slows from 5 ms to 5 s turns `spawn_blocking` calls into 512 threads, each holding its
> captured request, plus an unbounded queue of the rest. Put a `Semaphore` (Chapter 13.3) in front of any blocking call
> to a remote dependency, sized to what that dependency can serve.

### 8. Java comparison

| Java | Tokio | Notes |
|---|---|---|
| `CompletableFuture.supplyAsync(f)` (common `ForkJoinPool`) | `tokio::spawn` | blocking inside either starves the shared pool; `ForkJoinPool` can compensate with `ManagedBlocker`, Tokio can't |
| a dedicated `ExecutorService` for blocking calls | `spawn_blocking` (or a separate runtime) | same idea; Java's executor queue is also usually unbounded by default (`newFixedThreadPool`) |
| Netty: "never block the event loop"; `EventExecutorGroup` for blocking handlers | same rule; `spawn_blocking` | identical failure mode: a blocked loop stalls every channel it owns |
| Virtual threads (JDK 21+): blocking is fine, except pinning | no equivalent: blocking always stalls a worker | JDK 24 (JEP 491) removed pinning on `synchronized`; JNI calls still pin |
| `ThreadLocal`, SLF4J MDC | `task_local!`, `tracing` spans | Java code ported with `thread_local!` breaks on the multi-thread runtime (§4) |
| `ScopedValue` (JDK 25, JEP 506) | `task_local!` with `.scope()` | both bind a value for the dynamic extent of a computation |
| `Future.cancel(true)` interrupts a thread | `abort()` drops the future at its next `.await` | Rust has no thread interruption; blocking work can't be cancelled |

> **Analogy limit.** Java's thread pools and Tokio's workers fail the same way when blocked, but Java hides the failure
> better. A `ForkJoinPool` can grow compensation threads (`ManagedBlocker`), and a virtual thread that blocks just
> unmounts from its carrier. Tokio does neither: a worker that blocks is gone until the call returns. And Java's
> `ThreadLocal` works for request context under thread-per-request and virtual threads, because the request stays on
> "its" thread. It never works for a Tokio task, which has no thread of its own.

### 9. Production scenario

**Fraud scores inside `payments-core`.** `payments-core` (Chapter 8.4) calls the fraud feature library in-process
before authorizing a charge. The library is synchronous by design (Chapter 12.6 §9: "stays synchronous") and costs about
3 ms of CPU per score. At Black Friday's forecast peak of 2,000 charges/s, that's **6 cores of pure CPU work**, on pods
with 8 cores and 8 Tokio workers.

The first version called `fraud::score(&features)` directly in the request handler. Load testing at 1,500 charges/s
showed p99 latency for *unrelated* endpoints (health checks, refund lookups) rising from 3 ms to 90 ms: 4.5 cores of
scoring meant most workers were busy hashing at any moment, and every other request queued behind scores. It's listing
`ch02-09`'s "inline" row at production scale.

The design the team shipped:

- **A dedicated rayon pool of 6 threads** (`ThreadPoolBuilder::new().num_threads(6).thread_name(..)`), leaving 2 cores'
  worth of CPU for the 8 async workers, which mostly wait.
- **A `Semaphore` of 64 permits in front of it.** A request that can't get a permit within 20 ms fails fast with
  `PAY_OVERLOADED` (Transient, 503 with `Retry-After`). Without it, a traffic spike queues scoring work without limit,
  and every queued request times out anyway.
- **The async side awaits a `oneshot` with the request's remaining deadline** (Chapter 13.5), so a cancelled request
  stops waiting immediately. The score still completes, and the pool drops the result.
- **Metrics:** permits in use, time waiting for a permit, and pool queue depth. Scoring saturation shows up as permit
  wait, long before it shows up as request latency.

Why not `spawn_blocking`? Measured in the same load test: it bounded nothing. Under a spike it grew to 180 threads, all
runnable, on 8 cores, and context switching raised p99 for everything. The rayon pool keeps CPU work at 6 threads, and
the semaphore turns overload into fast, explicit rejections.

### 10. Failure scenario

**The wrong merchant in the log excerpt (2026, fictional).** `settlement-api`, a Rust service ported from Java in early
2026, kept Java's logging idiom: a request filter stored the merchant ID in a thread-local, and a custom log formatter
added it to every line (the MDC pattern). It passed every test. The tests used `#[tokio::test]`, which is a
current-thread runtime, where each request runs on the one thread from start to end.

In production (multi-thread runtime, 8 workers), log lines written after an `.await` carried the merchant ID of
whichever request had last set the thread-local on that worker. Listing `ch02-08` measures the rate: 1,489 of 1,600 reads
after an `.await` saw another request's ID. The failure surfaced when support exported "all log lines for merchant
m-2208" to answer a dispute, and the excerpt sent to that merchant contained payment IDs belonging to **three other
merchants**. It was a data-exposure incident, reported as one.

The analysis:

1. **The type system couldn't catch it.** A `thread_local!` read is `Send`-safe. Nothing is shared across threads
   unsafely; the value is simply the wrong one.
2. **The test runtime differed from production** in exactly the dimension that mattered: which thread a task resumes
   on.
3. **The Java habit was correct in Java**: under thread-per-request, the request owns its thread.

The fixes: `tracing` spans instead of the thread-local (`handler.instrument(info_span!("req", merchant = %id))`, which
attaches context to the future, like `task_local!`, and is what `tracing`'s formatters read); a lint in CI that forbids
`thread_local!` in service crates without an allowlist comment; and multi-thread tests
(`#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`) for request-context code.

---

## Practice

### 11. Interview & architecture questions

1. Why does `tokio::spawn` require `'static`? Why can't Tokio offer a safe `scope` like `std::thread::scope`?
2. A future holds a `std::sync::MutexGuard` across an `.await`. What happens at `tokio::spawn`, and why is that the
   right outcome?
3. What happens to a task when its `JoinHandle` is dropped? When `abort()` is called during a `spawn_blocking` closure?
4. Explain listing `ch02-05`'s three numbers. Why did one blocked worker out of four cost almost nothing?
5. When is `spawn_blocking` the wrong tool for CPU-bound work, and what would you use instead?
6. Why does `block_in_place` panic under `#[tokio::test]`, and what does that imply for library authors?
7. How would you carry a request ID through an async request handler? Why not `thread_local!`?
8. What is the rough time budget between `.await` points, and how would you measure whether a service respects it?
9. Compare Java's `ForkJoinPool.ManagedBlocker` with Tokio's options for blocking work. What can't Tokio do?
10. Design the placement of three kinds of work in a service: a Postgres driver with a sync API, a 20 ms PDF render, and
    an HTTP client call.

### 12. Exercises

- **Beginner.** Fix listing `ch02-01` two ways (`async move`, and an `Arc<str>`). Then make `ch02-02` compile by
  dropping the `Rc` before the `.await`, and explain why that works.
- **Intermediate.** Extend listing `ch02-04`: spawn a task that holds a guard object with a `Drop` impl, `abort()` it, and
  show when the guard is dropped relative to `h.await`.
- **Advanced.** Write `spawn_cpu<F, T>(pool: &rayon::ThreadPool, permits: &Semaphore, f: F) -> Result<T, Busy>` that
  waits at most 20 ms for a permit, runs `f` on the pool, and returns the result through a `oneshot`. Test it with a
  heartbeat like listing `ch02-09`.
- **Systems.** Measure the cost of `spawn_blocking` for a no-op closure versus `tokio::spawn` for a no-op future (reuse the harness of
  listing `ch01-05-flavors-cost.rs`). Explain the difference in terms of queues, threads, and wakeups.
- **Architecture.** Your service calls a C library (through FFI) that is not thread-safe and must be called from one
  thread. Design how async request handlers use it: which primitive runs it, how requests reach it, how many can wait,
  and what happens when it hangs.

### 13. Debugging exercise

A notification service's p99 latency spikes to 2 s for a few minutes every hour, always at :00. CPU is low during the
spikes. The code that runs at :00:

```rust,ignore
// Debugging exercise: find the defect (not a verified listing)
async fn hourly_rollover(state: Arc<AppState>) {
    let path = format!("/var/log/notify/{}.log", Utc::now().format("%Y%m%d%H"));
    let file = std::fs::File::create(&path).expect("create log file");      // (a)
    state.logger.lock().unwrap().rotate(file);                              // (b)
    let old = state.archive_dir.join(previous_hour_name());
    let bytes = std::fs::read(&old).unwrap_or_default();                    // (c) ~300 MB on a busy hour
    let compressed = zstd::encode_all(&bytes[..], 19).unwrap();             // (d)
    state.uploader.upload(compressed).await.ok();                           // (e)
}
```

Which lines block a worker, for how long (estimate), and which of them are CPU-bound rather than I/O-bound? Rewrite the
function so that no worker is blocked, and say which tool you used for each line and why. Model answer in Appendix A.

### 14. Design exercise

Meridian's **statement-export service** receives about 50 export requests per minute. Each export queries the ledger
(an async client), then builds a PDF (~200 ms of CPU for a large merchant), then compresses and uploads it (~50 ms CPU,
~500 ms network). A few merchants request 12 months at once (60× the work). The pods have 4 cores.

Design the service's work placement: which runtime(s), which pools, which limits, and what a merchant sees when the
service is saturated. Estimate CPU per minute at the average and in the 12-month case, and show that your limits keep
the health-check endpoint under 10 ms p99 while a 12-month export runs.
