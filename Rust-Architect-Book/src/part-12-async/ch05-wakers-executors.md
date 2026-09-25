# Chapter 12.5 — Wakers and Executors: Build One from Scratch

> **Where this sits:** Part XII · Async Rust · chapter 5 of 6
> **Prerequisites:** Chapters 12.1 (epoll, mio), 12.2 (the `Future` contract), 12.4 (`Pin`). `Arc`, `Mutex`, channels,
> `thread::park` (Part XI).
> **After this chapter you can:** build `block_on`, a `Waker` from a hand-written vtable, a multi-task executor with a
> timer and join handles, and an epoll reactor that serves 100 TCP clients on one thread; explain every design decision
> inside a production runtime as a trade-off you've now made yourself; and use a deterministic simulation executor to
> find and replay concurrency bugs.

---

## Pass 1 · User level — *Who calls `poll`?*

### 1. Problem

Chapters 12.2–12.4 described futures from the inside: a state machine that returns `Pending` after arranging for a
waker to be called. Nothing so far has said who does the polling, what "arranging for a waker" connects to, or where a
thread actually goes to sleep when every future is waiting. That's the **executor** and the **reactor**, and Rust's
standard library ships neither (Chapter 12.1 explained why).

Production code uses Tokio (Part XIII), and you shouldn't write your own runtime for a service. But every one of Tokio's
design decisions answers a question you can only appreciate after answering it yourself: what happens when a task is
woken twice before it runs, how a timer finds the right task, how the thread sleeps without missing a wake, and what a
spawned task costs. This chapter builds five executors, each about 100–150 lines, each verified.

### 2. Mental model

```text
                    ┌──────────────────────────── executor ────────────────────────────┐
  spawn(fut) ──────►│  run queue: [task 3, task 7, ...]                                  │
                    │     │ pop                                                          │
                    │     ▼                                                              │
                    │  task.poll(cx with a waker for THIS task) ──► Ready: drop the task │
                    │     │ Pending                                                      │
                    │     ▼                                                              │
                    │  (the future stored its waker somewhere: a reactor, a timer, a     │
                    │   channel, another task)                                           │
                    │  queue empty? ── sleep: park the thread, or block in epoll_wait    │
                    └──────▲─────────────────────────────────────────────────────▲───────┘
                           │ waker.wake(): push the task back on the run queue     │
          ┌────────────────┴─────────┐   ┌──────────────────────┐   ┌────────────┴──────────┐
          │ reactor: fd → waker      │   │ timer: deadline heap │   │ other tasks, threads  │
          │ epoll_wait says fd 9 is  │   │ → waker when due     │   │ (channels, JoinHandle)│
          │ readable → wake its task │   │                      │   │                       │
          └──────────────────────────┘   └──────────────────────┘   └───────────────────────┘
```

Three components, three jobs:

- The **executor** owns tasks and a run queue. Polling a task gives it a waker whose `wake()` means "put this task
  back on my queue."
- The **reactor** maps I/O registrations to the wakers waiting on them, and it's the place where the thread blocks in
  the kernel (`epoll_wait`) when nothing is runnable.
- The **timer** maps deadlines to wakers.

A `Waker` is the only link between them: 16 bytes, a data pointer and a vtable pointer (§4). The executor doesn't know
what a task is waiting for, and the reactor doesn't know what a task is. That separation is why the same `Future` trait
works with any runtime, and also why mixing runtimes goes wrong (Part XIII).

### 3. Rust code

**1. `block_on`: one future, one thread.** Listing `ch05-01-block-on.rs` is the smallest real executor. The waker
unparks the thread that polls; `Pending` parks it:

```rust,ignore
struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<F: Future>(fut: F) -> (F::Output, u32) {
    let mut fut = pin!(fut);
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut polls = 0;
    loop {
        polls += 1;
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return (v, polls),
            // Sleep until unpark(). If the wake already happened, park() returns at once: the
            // unpark token is what prevents the lost-wakeup race. Spurious returns just re-poll.
            Poll::Pending => {
                PARKS.fetch_add(1, Ordering::Relaxed);
                thread::park();
            }
        }
    }
}
```

```text
yielded: 4 polls, 3 parks
timer fired: 2 polls, 1 parks (at least 1)
```

`YieldTimes(3)` wakes itself *before* returning `Pending`, three times: each `park()` returns immediately because the
unpark token is already set. The `Delay` future is woken by another thread 20 ms later: one real sleep. The token is
what makes `block_on` correct: if the wake happens between `poll` returning `Pending` and the call to `park`, the token
remembers it [LIB: `std::thread::park` documents this]. Both runs pass Miri.

**2. A `Waker` by hand.** `Waker::from(Arc<impl Wake>)` is the convenient constructor. Underneath, a waker is a
`RawWaker`: a data pointer and a table of four functions. Listing `ch05-02-waker-vtable.rs` builds one, with the data
pointer being an `Arc` turned into a raw pointer, and counts every call:

```rust,ignore
static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop_waker);

unsafe fn clone(data: *const ()) -> RawWaker {
    // SAFETY: `data` came from Arc::into_raw in `counting_waker` (or here) and its count is live;
    // the new RawWaker owns one more strong count.
    unsafe { Arc::increment_strong_count(data as *const Counters) };
    // SAFETY: the Arc is alive (we hold at least one count), so the reference is valid.
    unsafe { &*(data as *const Counters) }.clones.fetch_add(1, Relaxed);
    RawWaker::new(data, &VTABLE)
}

unsafe fn wake(data: *const ()) {
    // SAFETY: `wake` consumes the waker: take back its strong count and release it at the end.
    let counters = unsafe { Arc::from_raw(data as *const Counters) };
    counters.wakes.fetch_add(1, Relaxed);
}
```

The program clones, wakes by reference, wakes by value (which consumes the waker, so its `drop` function is not
called), drops one, and wakes from another thread:

```text
size_of::<Waker>() = 16 bytes (data pointer + vtable pointer)
clones 2, wakes 3, drops 1, strong count now 1
```

Every strong count taken by `clone` is released by exactly one `wake` or `drop`, so the `Arc` ends at 1 (the
program's own handle). Miri checks the accounting: an extra count would be reported as a leak, a missing one as a
use-after-free. The vtable's order (`clone`, `wake`, `wake_by_ref`, `drop`) is also why Chapter 12.3's assembly called
`wake_by_ref` through `[rcx + 16]`: the third slot.

**3. A multi-task executor with a timer and join handles.** Listing `ch05-03-executor.rs` (about 150 lines of logic) has
a run queue (an `mpsc` channel of `Arc<Task>`), wakers that re-queue their task, a timer thread holding a min-heap of
deadlines, and a `JoinHandle<T>` that is itself a future. The task and its waker:

```rust,ignore
struct Task {
    future: Mutex<Option<BoxFuture>>, // None once finished: dropping the future frees its state
    queue: Sender<Arc<Task>>,
    queued: AtomicBool, // at most one queue entry per task, however many times it's woken
}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if !self.queued.swap(true, Ordering::AcqRel) {
            let _ = self.queue.send(self.clone()); // fails only if the executor is gone
        }
    }
}
```

and the run loop:

```rust,ignore
fn run(self) {
    while let Ok(task) = self.0.recv() {
        task.queued.store(false, Ordering::Release); // a wake during the poll re-queues it
        let waker = Waker::from(task.clone());
        let mut cx = Context::from_waker(&waker);
        let mut slot = task.future.lock().unwrap();
        if let Some(fut) = slot.as_mut() {
            POLLS.fetch_add(1, Ordering::Relaxed);
            if fut.as_mut().poll(&mut cx).is_ready() {
                *slot = None; // drop the finished future now, not when the last waker dies
            }
        }
    }
}
```

A parent task spawns three refresh jobs that sleep 30, 10, and 20 ms, then awaits their handles in spawn order:

```text
log: ["fx-rates done", "fraud-model done", "settlement done", "settlement refreshed", "fx-rates refreshed", "fraud-model refreshed"]
4 tasks, 8 polls, ~30 ms total (sleeps of 30 + 10 + 20 ms)
```

The jobs finished in deadline order (10, 20, 30 ms), the whole run took about 30 ms (not 60) on one thread, and the
parent received results in the order it awaited them. Eight polls for four tasks: each child is polled once to start
its sleep and once when the timer wakes it (six). The parent is polled once to spawn the children and start waiting on
the first handle (settlement, the slowest job), and once more when settlement's job wakes it (two); by then the other
two results are already stored, so that poll runs to completion. The two faster jobs finished while nobody was
awaiting their handles, so their completions woke no one.

**4. A reactor: I/O on the same thread.** Listing `ch05-04-reactor.rs` replaces "park the thread" with "block in
`epoll_wait`" (through mio), and builds async `TcpListener`/`TcpStream` types on top. Every I/O method follows Chapter
12.1's rule: *try the operation; only if it says `WouldBlock`, register interest and return `Pending`*:

```rust,ignore
async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
    poll_fn(|cx| match self.inner.read(buf) {
        Err(e) if e.kind() == ErrorKind::WouldBlock => {
            self.rt.reactor.wait_for(self.token, Dir::Read, cx);
            Poll::Pending
        }
        other => Poll::Ready(other),
    })
    .await
}
```

The executor's loop polls everything runnable, and only when the queue is empty does it call the reactor, which blocks
in `epoll_wait` and wakes the tasks whose tokens came back ready. An echo server (an acceptor task that spawns one task
per connection) and 100 client tasks run together:

```text
100 of 100 clients echoed
threads: 1; tasks spawned: 201; polls: 302; epoll_wait calls: 2
```

One thread in the whole process, 201 tasks, and **two** trips into the kernel's wait. The counts can be derived from
the code: the first pass polls all 101 initial tasks (the acceptor finds no connection yet; each client connects on
loopback, writes, and waits to read); the first `epoll_wait` reports the listener readable; the acceptor accepts all 100
connections in one poll and spawns 100 server tasks, each of which finds its request already buffered, echoes it, and
finishes on its first poll; the second `epoll_wait` reports 100 readable client sockets, and each client's second poll
reads its echo. That's 101 + 1 + 100 + 100 = 302 polls. Readiness batching at work: under load, one `epoll_wait`
returns many events, and the cost per event falls (Chapter 12.1 §6).

Two details matter. The wakers carry only a task **id** (the tasks live in a `Slab` owned by the runtime), so the
futures themselves can hold `Rc` and be `!Send`: the waker must be `Send + Sync` [LIB: `Waker` is], but the task doesn't
have to be. And `AsyncStream`'s `Drop` removes its wakers from the reactor, so a cancelled read leaves nothing behind.

---

## Pass 2 · Systems level — *The contracts inside a runtime*

### 4. Under the hood

**The `Waker` contract** [LIB: `core::task`]. A `RawWakerVTable` has four functions, and a runtime's correctness
depends on implementing each one's contract:

| Function | Called when | Must |
|---|---|---|
| `clone(data) -> RawWaker` | a future stores the waker (`cx.waker().clone()`) | produce an independent waker for the same task (usually: bump a refcount) |
| `wake(data)` | `waker.wake()`, consuming it | schedule the task, **and** release this waker's resources |
| `wake_by_ref(data)` | `waker.wake_by_ref()` | schedule the task; release nothing |
| `drop(data)` | a waker is dropped without being woken | release this waker's resources |

`Waker` is `Send + Sync`, so all four may be called from any thread, concurrently, any number of times, and after the
task has finished. That last case is common: a timer fires for a task that was cancelled. `wake` on a finished task
must be harmless. In `ch05-03` it is: the task is re-queued, the executor finds `future == None`, and skips it.

**Waking twice must not mean polling twice.** The `queued` flag in `ch05-03` is the most important line in the
executor. Without it, a task woken by ten I/O events before it runs gets ten queue entries and ten polls, nine of them
useless. With it, the first wake queues the task and later ones see `queued == true` and do nothing. The ordering is
subtle in the other direction too: the executor clears the flag **before** polling, so a wake that arrives *during* the
poll (a child completing on another thread, or a future waking itself) re-queues the task, and no wake is lost. Tokio's
task header keeps the same information as state bits (`NOTIFIED`, `RUNNING`, ...) updated with atomic operations
[LIB]. §10 measures what happens when an executor gets this wrong.

**Where the thread sleeps.** There are two choices, and they can't be mixed freely:

- **Park the thread** (`ch05-01`, `ch05-03`): `park()` is a futex wait on Linux [OS]; `unpark()` from another thread is
  a futex wake. Wakes from I/O would need a separate thread watching sockets.
- **Block in the reactor** (`ch05-04`): `epoll_wait` with a timeout. A wake from *another thread* (a timer thread, a
  `spawn_blocking` job) must interrupt it, so real runtimes register an extra eventfd (mio's `Waker`) and write to it
  to break the wait [LIB]. `ch05-04` doesn't need one because everything happens on its single thread.

**The timer.** `ch05-03`'s timer is a thread with a `BinaryHeap` of `(deadline, seq)` entries and a `Condvar`: a new
`Sleep` pushes an entry and notifies the timer thread (the new deadline might be the earliest), and the thread sleeps
until the earliest deadline, then wakes every expired entry's waker. It's correct, and its costs are visible: O(log n)
per insert, a cross-thread wake per expiry, and cancelled sleeps stay in the heap until they expire. Tokio drives a
**hierarchical timing wheel** from its own worker threads instead (slots per millisecond at the lowest level, O(1)
insert and cancel) [LIB].

**`JoinHandle`.** A spawned task's output has to reach whoever awaits the handle. `ch05-03` wraps the user's future in
an `async move` block that stores the output in shared state and wakes the handle's waker, a second allocation per
spawn. Tokio stores the output in the task's own allocation and has the handle read it from there [LIB].

### 5. Memory

**What a task costs.** Listing `ch05-05-spawn-cost.rs` spawns 100,000 tasks running a small handler (a 24-byte future
that suspends once) on this chapter's executor and on `futures::executor::LocalPool`, and 1,000 OS threads that are
spawned and joined, with the Part III counting allocator:

```text
handler future: 24 bytes
our executor, 100,000 tasks:        2.06 allocs,  136.5 bytes,       158 ns per unit
futures LocalPool, 100,000 tasks:   2.00 allocs,  190.9 bytes,       162 ns per unit
OS threads, 1,000 spawn + join:     3.00 allocs,  120.0 bytes,     35929 ns per unit
```

(Release build, one run on a shared machine: the times are noisy; the allocation counts are exact.)

- **Two allocations per task** in both executors: the boxed future and the task structure (`Arc<Task>` here, a
  `FuturesUnordered` node in `LocalPool` [LIB]). The extra 0.06 in ours is the run queue's own storage. std's `mpsc`
  stores messages in blocks of 31 slots [LIB], and each task is sent twice (spawn, then one wake): 200,000 / 31 ≈ 6,450
  blocks, or 0.065 per task. That derivation is an inference from the implementation, and the measured 0.06 agrees.
- **An OS thread's three small allocations** (the thread handle, the result packet, the boxed closure [LIB]) are not
  its real cost. Its stack is a 2 MiB `mmap` that the global allocator never sees, plus kernel structures (Chapter 12.1
  §5). Chapter 12.6 measures that side.
- **Tokio allocates once per spawned task**: header, future, and output slot in one block [LIB]. That's one fewer
  allocation per task than either executor here.

**Task memory is the future's size.** Beyond those two allocations, a task is as large as its future, which is why
Chapter 12.3's size budget matters: 100,000 tasks of a 24-byte future plus about 110 bytes of overhead is about 13 MB;
100,000 tasks holding 64 KiB buffers across an `.await` is 6.4 GB.

**The intrusive alternative.** The wakers stored by `ch05-03`'s `Sleep` and `JoinHandle` are separate `Arc`-counted
objects in shared state. Chapter 12.4's intrusive lists put a waiter's node *inside* its pinned future instead, which
is how production runtimes avoid an allocation per wait [LIB].

### 6. CPU / OS

**Spawn: 158 ns vs 36 µs.** A task spawn in `ch05-05` cost about 160 ns (two allocations, a queue push, the polls); an
OS thread spawn and join about 36 µs: a `clone(2)` system call, a stack `mmap`, the kernel scheduling a new task, and a
futex wait to join. About 230 times more, in one noisy run. Chapter 12.6 compares the switch cost, which matters more
than the spawn cost for long-lived connections.

**Where the syscalls are.** In `ch05-04`, the whole 100-client exchange made exactly two `epoll_wait` calls, plus the
socket operations themselves. Polling a task, waking it, and re-queuing it touched no kernel code at all: a wake on the
same thread is a push onto a `VecDeque` under an uncontended mutex. Wakes that cross threads are different: waking a
parked executor is a futex wake (`ch05-01`'s `Delay`: 1 park), and waking an executor blocked in `epoll_wait` costs a
write to an eventfd. Batching those is one reason Tokio's multi-threaded scheduler prefers to keep a woken task on the
worker that woke it [LIB] (Part XIII).

**Fairness is cooperative.** None of this chapter's executors can take the CPU away from a task. A task whose futures
are always ready (a socket that always has data, a channel that's never empty) could run forever inside one `poll`,
starving everything else on the thread. Tokio defends against this with a per-task **cooperative budget**: after a
fixed number of operations on Tokio resources in one poll, those resources start returning `Pending` so the task yields
[LIB]. A task that simply computes for a long time without awaiting still can't be interrupted (Chapter 12.6).

---

## Pass 3 · Architect level — *Every runtime is a set of trade-offs*

### 7. Trade-offs

You've now made each of these decisions once. Here's how this chapter's executors and Tokio (Part XIII) answer them:

| Decision | This chapter | Tokio (multi-thread) [LIB] | Trade-off |
|---|---|---|---|
| Run queue | one `mpsc` channel or `VecDeque` | per-worker local queues + a global injection queue, with work stealing | Locality and no contention vs one shared, simple queue |
| Duplicate wakes | `queued` flag (`AtomicBool`) | `NOTIFIED` bit in the task state word | Correctness is the same; Tokio folds it into one atomic state machine |
| Task storage | `Arc<Task>` + boxed future (2 allocations), or slab + ids | one allocation: header + future + output | Fewer allocations vs simpler code |
| `Send` requirement | `Send` futures (`ch05-03`) or `!Send` on one thread (`ch05-04`) | `Send` for `spawn`; `LocalSet` / current-thread runtime for `!Send` | Moving tasks between threads requires `Send` futures |
| Sleeping | `park()` or `epoll_wait` | a worker parks on the I/O driver; others park on a condvar; eventfd to interrupt | Where cross-thread wakes pay a syscall |
| Timers | thread + binary heap | hierarchical timing wheel on the workers | O(log n) + a thread vs O(1) and no extra thread |
| Fairness | none | cooperative budget per task poll; LIFO slot with limits | Throughput for ping-pong tasks vs starvation risk |
| Blocking work | not supported | `spawn_blocking` thread pool | Chapter 12.6 |

The executor is policy, and the `Future` trait isn't. That's why the same `async fn` runs on Tokio, on a
microcontroller executor, and in the deterministic simulator of §9, and why tests can swap executors to control time and
scheduling.

### 8. Java comparison

The closest Java analogue to `ch05-04` is Netty's `NioEventLoop`: one thread that alternates between `Selector.select()`
(epoll underneath) and running queued tasks, with an `ioRatio` setting that divides time between I/O and tasks
[LIB: Netty 4.1]. The structural difference is what gets run:

| | Netty event loop | Rust executor + reactor |
|---|---|---|
| On readiness | calls the channel's handler pipeline (your callbacks) | wakes a task; the executor later calls `poll` |
| Continuation state | fields of your handler objects, on the GC heap | the task's future, one value of known size |
| Scheduling unit | a `Runnable` or a channel event | a task (a whole call tree of futures) |
| Multi-core | N event loops, each channel bound to one | one runtime; tasks may migrate (work stealing) |
| Timers | `HashedWheelTimer` / scheduled tasks on the loop | runtime timer wheel (Tokio) |

Java's other scheduler is `ForkJoinPool`: work-stealing deques per worker, the same structure Tokio's multi-threaded
scheduler uses, and the default scheduler of Java virtual threads (Chapter 12.6).

> **Analogy limit.** "A waker is a callback" is wrong in a way that matters. A Netty callback *carries the
> continuation*: calling it runs your code, on the completing thread. A Rust waker carries nothing but a task identity:
> calling it only puts the task back in a queue. The task's own `poll` then decides what to do, on the executor's
> thread. That's why a waker can be called twice, late, or from a signal-handler-like context without running user code,
> and why the pull model gives natural backpressure: work is only done when the executor asks for it.

### 9. Production scenario

**A deterministic simulator for payments-core.** Chapter 8.4 gave payments-core its idempotency design: a request stores
an in-progress record under its key *before* calling the card processor, and a duplicate sees that record. In 2026 the
team added a fast path, an in-process cache in front of the idempotency table, and the first version of the handler
checked the cache, then awaited a fraud-score call, then recorded the key. Unit tests passed, because a test's
single request never races with its own retry.

The team's response was a **simulation executor**: listing `ch05-07-sim-executor.rs` is a reduced version. It has two
properties no production runtime offers:

- **Virtual time.** `sleep` doesn't wait; when no task is runnable, the executor jumps the clock to the next timer
  deadline. A test that simulates hours of timeouts runs in milliseconds.
- **Seeded scheduling.** When several tasks are ready, the executor picks one with a seeded random generator. The same
  seed gives the same interleaving, every time, on every machine.

The test runs two requests with the same idempotency key (a client retry arriving 0–19 ms after the original), under
1,000 seeds, against both versions of the handler:

```text
v1 (check, await, act): 183 of 1000 seeds double-charge
  first failing seed 4: 2 charges; replay identical: true
    t=0ms req 1: key not found, scoring
    t=1ms req 2: key not found, scoring
    t=5ms req 1: charged ch_1
    t=6ms req 2: charged ch_2
v2 (reserve first): 0 of 1000 seeds double-charge
```

18% of schedules double-charge in v1, and the failing schedule replays identically from its seed, with a readable
trace: both requests passed the check before either reached the `.await`, and both charged. v2 makes "check and
reserve" one synchronous step with no `.await` between them, which is Chapter 8.4's in-progress record, and no schedule
fails. This bug is invisible to a thread sanitizer (there's no data race: everything is on one thread), and it would
appear in production only under a retry storm.

The idea is attributed to FoundationDB, which built its whole database on deterministic simulation (Will Wilson,
"Testing Distributed Systems w/ Deterministic Simulation," Strange Loop 2014). In Rust, `turmoil` (from the Tokio
project) and `madsim` offer simulated time and networks for Tokio-style code [LIB]. They aren't on the Playground, so
`ch05-07` builds the core idea from scratch. Part XXIV uses the same technique for Ferrite's replication tests.

### 10. Failure scenario

**The simulator that got slower with every merchant.** The first version of payments-core's simulator (§9) queued a
task on *every* wake: no `queued` flag. Most tests ran fine. Then someone simulated the settlement fan-in, one task
awaiting responses from 1,000 merchant ledgers, and the test took minutes instead of milliseconds.

Listing `ch05-06-wake-dedup.rs` reproduces it. One task awaits N child futures with a simple `join_all` that polls
every unfinished child on each poll; the responses arrive in two batches (as they would from two `epoll_wait` calls),
so the task is woken N/2 times before it runs:

```text
n =    16, dedup true : task polls    3, child polls      40, entries after completion   0
n =    16, dedup false: task polls   10, child polls      96, entries after completion   7
n =  1000, dedup true : task polls    3, child polls    2500, entries after completion   0
n =  1000, dedup false: task polls  502, child polls  252000, entries after completion 499
```

With deduplication, the task is polled three times whatever N is (start, after batch one, after batch two). Without it,
every wake becomes a queue entry and a poll, and each poll re-polls every pending child: 252,000 child polls for 1,000
responses, a hundredfold more work, growing as N². And 499 entries sit in the queue for a task that has already
finished.

Two separate defects multiply here, and the fix addressed both:

1. **The executor:** a wake of an already-queued task must be a no-op (the `queued` flag, or Tokio's `NOTIFIED` bit).
2. **The combinator:** a `join_all` over many children shouldn't poll all of them on every wake. `futures`' own
   `join_all` switches strategy with size [LIB]. Listing `ch05-08-join-all-threshold.rs` runs it under the same
   two-batch workload, polled by hand (so the executor side is correct):

   ```text
   join_all n=  16: 3 polls, child polls 40
   join_all n=  30: 3 polls, child polls 75
   join_all n=  31: 3 polls, child polls 62
   join_all n=1000: 3 polls, child polls 2000
   ```

   Up to 30 children it re-polls every pending child on each poll (2.5 child polls per child here). From 31 on it
   uses a `FuturesOrdered`, which gives each child its own waker, so only the children that were actually woken are
   polled: exactly 2 per child, whatever N is. That's the N² from Chapter 12.3 §6, measured and then removed. The
   threshold is an implementation detail of the `futures` version on the Playground, not a guarantee.

The team's regression test asserts on poll counts, not only on results. A test that checks only that the answer is
right can't catch an executor that computes it a hundred times.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XII).*

1. Describe the executor, the reactor, and the timer, and how a `Waker` connects them without any of them knowing about
   the others.
2. What are the four functions in a `RawWakerVTable`, and what must each do with the waker's resources?
3. Why must `wake()` on a finished task be harmless? Give a realistic way it happens.
4. In `ch05-03`, why is `queued` cleared *before* the task is polled rather than after? What bug appears if you swap the
   order?
5. How does `block_on` avoid missing a wake that happens between `poll` returning `Pending` and `park()`?
6. Why can the futures in `ch05-04` hold `Rc` while `Waker` must be `Send + Sync`?
7. What does a spawned task cost, in allocations and time, compared with an OS thread? Where does each allocation come
   from?
8. What is deterministic simulation testing, and what class of bugs does it find that a thread sanitizer can't?
9. A `join_all` over 1,000 futures is woken 500 times before it runs. Walk through the cost with and without wake
   deduplication, and with and without per-child wakers.

### 12. Exercises

- **Beginner.** Remove the `queued` check from `ch05-03`'s `wake_by_ref`. Predict the poll count for the demo, then
  run it.
- **Intermediate.** Add a `yield_now()` future to `ch05-03` and use it to show that two CPU-heavy tasks can share one
  thread fairly only if they yield. Measure how the time between yields affects a third task's latency.
- **Advanced.** Replace `ch05-03`'s timer thread with timers driven by the executor itself: when the run queue is empty,
  compute the next deadline and wait on the channel with `recv_timeout` until then. What happens to cross-thread wakes?
  What happens to a sleep that's cancelled?
- **Systems.** Extend `ch05-04` with an eventfd-based waker (mio's `Waker`) so that another thread can wake a task on the
  reactor thread. Count `epoll_wait` calls and measure the cross-thread wake latency against `ch05-01`'s park/unpark.
- **Architecture.** Your team wants deterministic simulation for merchant-notify, which uses Tokio's timers, TCP, and
  `spawn`. List what must be abstracted (time, network, randomness, task scheduling), compare building on `turmoil`
  with building your own, and describe how a failing seed from CI becomes a reproducible bug report.

### 13. Debugging exercise

A teammate's executor loses tasks under load: occasionally a task that was woken is never polled again, and the process
stays idle with work pending. The relevant code:

```rust,ignore
fn run(&self) {
    while let Ok(task) = self.queue.recv() {
        let waker = Waker::from(task.clone());
        let mut slot = task.future.lock().unwrap();
        if let Some(fut) = slot.as_mut() {
            if fut.as_mut().poll(&mut Context::from_waker(&waker)).is_ready() {
                *slot = None;
            }
        }
        task.queued.store(false, Ordering::Release);
    }
}
```

1. Describe the interleaving that loses a wake. (Where is the task's future when another thread calls `wake()`?)
2. Why does moving one line fix it? Which line, and where?
3. With the fix, a task can now be queued while it's being polled. Why is that harmless in this executor? Would it
   still be harmless in a multi-threaded executor where two workers pop from the same queue?

### 14. Design exercise

**A thread-per-core executor for merchant-notify.** Instead of a work-stealing runtime, the team considers N
independent single-threaded executors, one per core, each with its own reactor, with connections assigned to cores at
accept time (the `glommio`/`monoio` model, Chapter 12.1 §7).

- What does each connection task gain (no `Send` requirement, no cross-thread wakes, cache locality)?
- What do you lose? Consider a merchant whose 500 terminals landed on one core, a burst of events for one hot merchant,
  and a slow client on a busy core.
- How would a notification for a merchant reach connections on other cores?
- Which measurement from this chapter or the next would decide the question, and what result would make you choose each
  design?
