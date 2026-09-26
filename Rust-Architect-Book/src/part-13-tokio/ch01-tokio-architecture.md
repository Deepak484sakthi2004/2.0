# Chapter 13.1 — Tokio's Architecture: Scheduler, I/O Driver, Timers

> **Where this sits:** Part XIII · Tokio and Production Async · chapter 1 of 5
> **Prerequisites:** Part XII (futures, `poll`, wakers, the executor you built in 12.5, and 12.6's cost model).
> Chapter 11.1 (threads and the OS) and Chapter 12.1 (epoll and readiness).
> **After this chapter you can:** say what a Tokio runtime is made of and what each part asks of the operating system;
> predict where a spawned task will run and why the multi-thread scheduler sometimes lets a ready task wait; explain
> Tokio's cooperative budget and when it protects you (and when it doesn't); read Tokio's timer semantics (millisecond
> ticks, deadlines rounded up, missed-tick policies); choose between the current-thread and multi-thread runtimes with
> measured costs; and point at the exact line of Tokio's source behind each default.

---

## Pass 1 · User level — *What is a runtime made of?*

### 1. Problem

In Chapter 12.5 you built an executor: a queue of tasks, a `Waker` that pushes a task back onto the queue, a loop that
polls whatever is ready, and a timer thread. It ran futures correctly, and it would fall over in production on the first
busy afternoon. It had one thread. It learned about sockets by polling them. Its timer thread woke tasks one at a time
through a lock. It had no answer to a task that computes for 300 ms without yielding, and no way to run blocking code.

**Tokio** is the production answer, and it's the runtime Meridian chose for the `merchant-notify` pilot (Chapter 12.6
§9): up to 100,000 mostly idle connections per pod on a multi-thread runtime with 8 workers. Before building services on
it, an architect needs a precise model of four things:

1. **Who runs a task, and when?** Tokio has several queues and a work-stealing scheduler. Its choices decide latency.
2. **How does I/O readiness reach a task?** Through an I/O driver that owns an `epoll` instance. What does the kernel
   actually see?
3. **How do timers work?** `sleep(1µs)` doesn't sleep for a microsecond. What does it do, and what does an `interval` do
   after a stall?
4. **What does all this cost?** Per task, per spawn, per wakeup, compared with OS threads.

And the question the brief insists on: **where does Tokio end and the operating system begin?**

### 2. Mental model

A Tokio runtime is a small operating system for futures, built out of a few OS resources:

```text
                        ┌──────────────────────── Tokio runtime (library code, in your process) ─────────────────────────┐
                        │                                                                                                 │
  tokio::spawn(fut) ──► │  SCHEDULER                                                                                      │
                        │   global (inject) queue ◄── spawns/wakeups from non-worker threads                              │
                        │   worker 0: [LIFO slot] [local queue, 256 slots]   worker 1: [LIFO slot] [local queue] ...      │
                        │      loop: pick a task → poll it once → repeat; steal half of a busy worker's queue when idle   │
                        │                                                                                                 │
                        │  DRIVERS (polled by a worker every 61 ticks, or whenever it runs out of tasks)                   │
                        │   I/O driver:    one epoll instance + an eventfd to wake it; readiness → wake the waiting task   │
                        │   time driver:   a hierarchical timer wheel, 6 levels × 64 slots, 1 ms ticks                     │
                        │   signal driver: a process-wide socketpair the signal handler writes to                          │
                        │                                                                                                 │
                        │  BLOCKING POOL: separate threads for spawn_blocking (up to 512, idle ones exit after 10 s)       │
                        └─────────────────────────────────────────────────────────────────────────────────────────────────┘
  what the KERNEL sees:  N worker threads ("tokio-rt-worker"), one epoll fd, one eventfd, your sockets, a socketpair.
                         It never sees a task.
```

Hold on to the last line. A **task** is a heap allocation containing your future, owned by the runtime. The kernel
schedules the **worker threads**. Tokio schedules **tasks onto workers**. Chapter 12.6's comparison applies directly:
switching tasks is a function return and a queue pop, and switching threads is the kernel's job. Everything below is the
detail of those two layers and the line between them.

Three rules follow from the diagram, and the rest of the chapter measures each:

- **A task runs until it returns `Pending`.** Tokio can't preempt a task (Chapter 12.6 §4). It *can* make Tokio's own
  resources return `Pending` early, and that's the cooperative budget (§4).
- **Wakeups are queue pushes.** The I/O driver, the timer wheel, a channel, and a `Notify` all wake a task the same way:
  they call its `Waker`, which puts the task on a run queue. Which queue depends on who woke it (§4).
- **Drivers only run when a worker runs them.** A worker checks for I/O and timer events between tasks: every 61 tasks,
  or when it has nothing else to do. A worker stuck inside one task checks nothing.

### 3. Rust code

`#[tokio::main]` hides the runtime. Here's the whole program (listing `ch01-07-tokio-main.rs`):

```rust
#[tokio::main]
async fn main() {
    println!("hello from a task");
}
```

and here's what the attribute macro turns it into, fetched with `tools/emit.ps1 -Target expand` (nightly
`-Zunpretty=expanded`, trimmed):

```rust,ignore
fn main() {
    let body = async {
        { ::std::io::_print(format_args!("hello from a task\n")); };
    };
    // ... a type check that `body` is a Future<Output = ()> ...
    {
        use tokio::runtime::Builder;
        return Builder::new_multi_thread().enable_all().build().expect("Failed building the Runtime").block_on(body);
    }
}
```

Three facts are visible in the expansion. `main` is an ordinary synchronous function. The runtime is a value, built by
a `Builder`, and dropped when `main` returns (§6 and Chapter 13.4 show what that drop does to running tasks). And the
body of `main` is not a spawned task: `block_on` runs it **on the main thread**, while spawned tasks run on the workers.
So the default is a multi-thread runtime with one worker per CPU and every driver enabled.

What does the kernel see of that? Listing `ch01-01-os-view.rs` builds a 4-worker runtime, binds one listening socket, and
reads `/proc/self/task` (threads), `/proc/self/fd` (descriptors), and `/proc/self/fdinfo/<fd>` (what an epoll instance
watches). Real output from the Playground (Linux x86-64, tokio 1.53.1):

```text
before any runtime
  threads: ["playground"]
  fds:     []
inside a 4-worker runtime with one listening socket
  threads: ["playground", "tokio-rt-worker", "tokio-rt-worker", "tokio-rt-worker", "tokio-rt-worker"]
  fd  3 anon_inode:[eventpoll]     watching fds [8, 4, 9]
  fd  4 anon_inode:[eventfd]
  fd  5 anon_inode:[eventpoll]     watching fds [8, 4, 9]
  fd  6 socket:[203009224]
  fd  7 socket:[203009225]
  fd  8 socket:[203009224]
  fd  9 socket:[203009226]
  runtime metrics: 4 workers, 1 alive tasks
after dropping the runtime
  threads: ["playground"]
  fds:     [(6, "socket:[203009224]"), (7, "socket:[203009225]")]
```

Read it as an inventory of the runtime's OS resources:

- **Four `tokio-rt-worker` threads**, created when the runtime was built (the builder names them) and joined when it
  was dropped.
- **One epoll instance** (fds 3 and 5 list the same targets because one is a `dup` of the other: mio clones its
  registry handle, and a duplicated descriptor refers to the same kernel object [LIB][OS]). It watches three descriptors.
- **fd 4, an eventfd**: the doorbell. When a task is woken from outside the I/O driver while a worker is parked in
  `epoll_wait`, Tokio writes to the eventfd and the kernel wakes the worker.
- **fd 9, the listening socket** (inode ...226), registered because the `accept` task is waiting on it.
- **fds 6, 7, 8: the signal driver.** 6 and 7 are a socketpair (inodes ...224 and ...225) that Tokio's signal handler
  writes into. 8 is a duplicate of 6 (same inode), registered with epoll. After the runtime is dropped, 6 and 7 are
  still open: the signal pipe is **process-global** and lives as long as the process [LIB].

That's the whole footprint. No thread per connection, no thread per timer, and no descriptor per task. When the
`merchant-notify` pilot holds 100,000 connections, the kernel sees 8 workers, one epoll instance with 100,000 registered
sockets, and 100,000 socket descriptors.

---

## Pass 2 · Systems level — *Queues, budgets, wheels, and syscalls*

### 4. Under the hood

**Where the numbers come from.** Tokio documents much of its scheduling behavior under a heading that says "at the time
of writing": it's an implementation, not a contract [LIB]. So rather than quote a blog post, listing
`ch01-06-tokio-source.rs` reads the defaults out of Tokio's own source on the machine that compiled it (the Playground
keeps the registry sources next to the build):

```text
task/coop/mod.rs:116: Budget(Some(128))
runtime/mod.rs:628: pub(crate) const BOX_FUTURE_THRESHOLD: usize = if cfg!(debug_assertions)  {
runtime/mod.rs:629: 2048
runtime/mod.rs:630: } else {
runtime/mod.rs:631: 16384
runtime/mod.rs:632: };
runtime/scheduler/multi_thread/queue.rs:63: const LOCAL_QUEUE_CAPACITY: usize = 256;
runtime/scheduler/multi_thread/worker.rs:270: const MAX_LIFO_POLLS_PER_TICK: usize = 3;
runtime/builder.rs:278: Builder::new(Kind::MultiThread, 61)
runtime/builder.rs:294: nevents: 1024,
runtime/builder.rs:306: max_blocking_threads: 512,
runtime/blocking/pool.rs:172: const KEEP_ALIVE: Duration = Duration::from_secs(10);
runtime/time/wheel/level.rs:39: const LEVEL_MULT: usize = 64;
runtime/time/wheel/mod.rs:45: const NUM_LEVELS: usize = 6;
runtime/time/wheel/mod.rs:48: pub(super) const MAX_DURATION: u64 = (1 << (6 * NUM_LEVELS)) - 1;
```

Every number in this chapter's diagrams is one of these lines. (The `61` carries a comment in `builder.rs`: "fairly
arbitrary. I believe this value was copied from golang.") The technique is worth keeping: **for any dependency whose
behavior matters to your design, read its source at the version you ship**, and pin the version.

**The worker loop.** Each worker thread runs roughly this loop [LIB, tokio 1.53.1]:

```text
loop {
    task = lifo_slot.take()                       // (a) the task this worker woke most recently, at most 3 times in a row
        or local_queue.pop()                      // (b) FIFO, 256 slots; overflow moves HALF to the global queue
        or (every global_queue_interval ticks) global_queue.pop()   // (c) fairness for tasks spawned from outside
        or steal_half_from(random other worker)   // (d) work stealing
        or global_queue.pop();
    if task: poll it once (with a fresh coop budget of 128), tick += 1
    every 61 ticks, or when there's nothing to run: poll the drivers
    nothing at all: park in epoll_wait(timeout = next timer deadline)
}
```

The Tokio docs spell out the parts that matter for latency (quoted from `runtime/mod.rs` of 1.53.1): when a task wakes
another task, "the other task is added to the worker thread's lifo slot instead of being added to a queue"; "if a worker
thread uses the lifo slot three times in a row, it is temporarily disabled"; and "the lifo slot is separate from the
local queue, so other worker threads cannot steal the task in the lifo slot." The global-queue check interval on the
multi-thread runtime isn't fixed: unless you set it, it's tuned dynamically to target roughly 10 ms between checks.

Listing `ch01-04-scheduler.rs` shows each rule on a 4-worker runtime (release, one run on a shared 4-vCPU machine):

```text
1. child ran on the parent's worker thread: 1000 of 1,000
2. 4,000 tasks spawned from one worker ran on 4 threads: [1449, 1110, 784, 657] (85ms; 200 ms of work)
3. heartbeat only                : heartbeat first ran after 300.0ms
3. heartbeat, then another spawn : heartbeat first ran after 28.2µs
```

- **Experiment 1: locality.** A task that spawns a child and immediately awaits it gets the child on its own worker,
  1,000 times out of 1,000. The child went into the LIFO slot and ran as soon as the parent returned `Pending`, while
  its data was still in that core's cache. That's the design goal: request/response message passing between tasks
  behaves like a function call.
- **Experiment 2: stealing.** One task spawned 4,000 CPU-bound tasks (50 µs each). They all landed in *one* worker's
  local queue (overflowing into the global queue), and idle workers stole them: the work ended up spread over 4
  threads, and 200 ms of work finished in 85 ms of wall time.
- **Experiment 3: the price of the LIFO slot.** A task spawns a heartbeat task and then spins for 300 ms without
  yielding, while three workers sit idle. The heartbeat is in the spinning worker's LIFO slot, which can't be stolen,
  so it waits the full **300 ms**. If the task spawns one more task after the heartbeat, the heartbeat is pushed from the
  LIFO slot into the local queue (the new task takes the slot), an idle worker steals it, and it runs after **28 µs**.

Experiment 3 is the scheduler's honest trade-off: a latency optimization for the common case (short tasks that
message each other) that becomes a starvation case when a task misbehaves. The fix isn't to tune the scheduler (you can
`disable_lifo_slot()`, but then experiment 1 loses its locality). The fix is to not block a worker (Chapter 13.2).

**The cooperative budget.** Tokio can't interrupt a task, but it controls every Tokio resource the task touches. Each
time a task is polled, it gets a budget of **128** operations. Every Tokio resource operation (a channel `recv`, a socket
read, a timer check) spends one unit, and once the budget is spent those operations return `Pending` even if they're
ready, after scheduling a wakeup. The task yields, others run, and it continues on its next poll. Listing
`ch01-03-coop-budget.rs` runs a busy task next to a heartbeat (a task that sleeps 10 ms in a loop and records its worst
lateness) on a single-threaded runtime:

```text
A: mpsc recv loop (budgeted)             busy    83.6ms  heartbeat worst lateness     2.0ms  (sum 12499997500000)
B: same loop inside task::unconstrained  busy    48.3ms  heartbeat worst lateness    38.3ms  (sum 12499997500000)
C: loop over std::future::ready          busy   409.1ms  heartbeat worst lateness   399.1ms  (sum 19999999900000000)
D: C + yield_now every 10,000            busy   298.7ms  heartbeat worst lateness     1.4ms  (sum 19999999900000000)
```

- **A**: draining 5 million ready messages took 84 ms, and the heartbeat was never more than 2 ms late, because
  `recv()` returned `Pending` every 128 messages.
- **B**: `task::unconstrained` switches the budget off. The same loop ran faster (48 ms: no forced yields), and the
  heartbeat was 38 ms late: the whole drain ran as one poll.
- **C**: `std::future::ready` knows nothing about Tokio's budget. 200 million always-ready awaits ran as one 409 ms poll,
  and the heartbeat was 399 ms late. **The budget protects you only at Tokio's own await points.** Your own
  always-ready futures, CPU work between awaits, and third-party futures that don't opt in aren't covered.
- **D**: an explicit `yield_now()` every 10,000 iterations restores fairness (1.4 ms).

So the budget is a guard rail for a specific failure (a task whose Tokio resources are always ready, such as a hot
socket or a full channel), not preemption. The table under Trade-offs (§7) comes back to it.

**How a wakeup crosses the line.** When a socket becomes readable, the kernel marks it ready in the epoll instance. The
next time a worker polls the I/O driver (`epoll_wait` with a zero timeout between tasks, or a blocking one when parked),
it gets the event, finds the `ScheduledIo` record for that socket, and calls the `Waker` of every task waiting for that
readiness. Waking from the driver pushes to the waking worker's queues; waking from a thread that isn't a worker (a
blocking-pool thread, a plain `std::thread`) pushes to the global queue and, if workers are parked, unparks one. At most
one parked worker sits in `epoll_wait` (it owns the driver while parked); the others sleep on a condition variable. So
unparking is a futex wake, or an eventfd write when the worker to wake is the one blocked in `epoll_wait` [LIB][OS]. The
kernel's part ends at "this fd is readable". Everything after it (which task, which queue, which thread) is Tokio.

### 5. Memory

**A task is one allocation.** `tokio::spawn` allocates a single block (Tokio calls it a task *cell*) holding the task
header (state, vtable, queue links), the future, and space for its output. Listing `ch01-08-task-memory.rs` spawns 10,000
tasks of each kind from inside a worker and counts the allocator's traffic (release; the counting allocator from
Part III):

```text
tiny: await a oneshot                      future    24 B   per task: 1.00 allocations,   112 B live
with a 1 KiB buffer alive across the await future  1048 B   per task: 1.00 allocations,  1136 B live
with a 16 KiB buffer alive across the await future 16408 B   per task: 2.00 allocations, 16520 B live
```

- The fixed cost is **88 bytes** per task on this build (112 − 24 and 1,136 − 1,048). Everything else is the future,
  whose size you already know how to read (Chapter 12.3: whatever is alive across an `.await`).
- The 16 KiB future cost **two** allocations. That's `BOX_FUTURE_THRESHOLD` from §4: in release builds, a future bigger
  than 16,384 bytes is boxed first and the box is spawned (2,048 bytes in debug builds, where futures are bigger). The
  reason in Tokio's source is stack usage: moving a large future by value through `spawn`'s call chain can overflow a
  worker's stack.

Compare Chapter 12.6's numbers: an OS thread reserves 2 MiB of virtual stack (and commits at least a page or two), a
Tokio task costs 88 bytes plus its future. At the `merchant-notify` pilot's 100,000 connections, 1 KiB per connection
future is ~110 MB, which is why 12.3 put a `size_of_val` budget in CI.

**Timers cost no allocation in the wheel.** The timer wheel is 6 levels of 64 slots. Level 0 slots are 1 ms, level 1
slots are 64 ms, level 2 slots are 4.096 s, and so on (×64 each), up to `MAX_DURATION` = 2^36 − 1 ms, about 2.2 years.
A `Sleep` future contains its own timer entry, and the wheel links entries intrusively, so registering a timer is a
pointer insertion, not an allocation [LIB]. That's why listing `ch01-02-timers.rs` could do this on one thread:

```text
100,000 sleeping tasks: spawned in 33.9ms, all done after 76.7ms, worst lateness 24.2ms
```

100,000 tasks, each with a 50 ms deadline, all registered at once. They were spawned in 34 ms (the spawns, not the
timers, are the cost), and every one fired within 24 ms of its deadline, the lateness coming from one thread working
through 100,000 wakeups. A per-timer heap (`BinaryHeap`, `O(log n)` per insert) would work too. The wheel makes insert
and expiry `O(1)` at the price of 1 ms resolution, which is the next section.

**The local queue is preallocated.** Each worker's queue is a fixed 256-slot ring buffer allocated when the runtime
starts (the `Box<[...; LOCAL_QUEUE_CAPACITY]>` in `queue.rs`), so scheduling a task never allocates. The global queue
is an intrusive linked list threaded through task headers. The only allocation on the spawn path is the task cell.

### 6. CPU / OS

**Timers round up to the next millisecond tick.** `Sleep` "operates at millisecond granularity and should not be used for
tasks that require high-resolution timers" (Tokio's docs, `time/sleep.rs`). Listing `ch01-02-timers.rs`, release, one
run:

```text
sleep(     1µs) x100:    1.061ms each
sleep(   500µs) x100:    1.593ms each
sleep(   1.5ms) x100:    2.565ms each
sleep(     5ms) x100:    6.063ms each
```

A 1 µs sleep lasts about 1 ms. Every sleep lasts at least one tick past its deadline, because a deadline is rounded
**up** to a tick boundary (never fire early) and then the worker has to notice it: it parks in `epoll_wait` with a
millisecond timeout [OS]. Two consequences: don't use `sleep` for sub-millisecond pacing (spin or batch instead), and
don't build a retry loop whose correctness depends on a 1 ms sleep being 1 ms.

**Intervals after a stall.** `tokio::time::interval` ticks at a fixed period. What happens when the consumer falls
behind is a policy, `MissedTickBehavior`. The same listing stalls the consumer once for 35 ms (3.5 periods of 10 ms):

```text
Burst: ticks at [1, 11, 46, 46, 46, 50] ms
Skip: ticks at [1, 11, 46, 51, 61, 71] ms
```

With the default, **Burst**, the interval fires the missed ticks immediately, back to back (three ticks at 46 ms), to
catch up. With **Skip**, it fires once and then realigns to the original schedule (51, 61, 71). A third policy, `Delay`,
restarts the period from the late tick. Burst is right for "exactly N samples per second on average" and wrong for
"heartbeat every 5 s", as §10 shows.

**The syscalls.** A worker with nothing to do calls `epoll_wait(epfd, events, 1024, timeout)`: 1,024 events per call
(`nevents`) and a timeout equal to the time until the next timer deadline [OS][LIB]. When it has work, it still checks
for events every 61 ticks with a zero timeout, so a busy runtime doesn't ignore I/O. Waking a parked worker costs a
futex wake or an eventfd `write` (§4). None of this is visible per task: 100,000 tasks waiting on 100,000 sockets cost
one `epoll_wait` that returns the ready ones.

**What a task switch costs, measured.** Listing `ch01-05-flavors-cost.rs` measures spawn-and-join and a ping-pong round
trip between two tasks (a message there and back through two bounded channels), three times on each runtime flavor,
release:

```text
current_thread                 spawn+join per task [   312ns,    304ns,    301ns]   round trip [   179ns,    177ns,    182ns]
multi_thread, 4 workers        spawn+join per task [   798ns,    815ns,    769ns]   round trip [   275ns,    398ns,    248ns]
multi_thread, 1 worker         spawn+join per task [   379ns,    382ns,    377ns]   round trip [   178ns,    178ns,    179ns]
OS threads (std sync_channel)                                           round trip      9µs
```

- A task round trip costs **~180 ns** on one thread, against **~9 µs** for two OS threads handing a message back and
  forth: the 50× gap Chapter 12.6 predicted, measured here through Tokio.
- The multi-thread runtime with 4 workers is **2.5× slower per spawn** than the current-thread runtime (800 ns vs
  300 ns) and noisier per round trip: spawned tasks can land on other workers, which means cross-core cache traffic,
  atomic queue operations, and sometimes waking a parked worker. With 1 worker the multi-thread runtime costs about the
  same as current-thread. The cost is **parallelism overhead**, and it buys the ability to use every core.

---

## Pass 3 · Architect level — *Configuring a runtime on purpose*

### 7. Trade-offs

**Current-thread vs multi-thread.**

| | `new_current_thread()` | `new_multi_thread()` (the `#[tokio::main]` default) |
|---|---|---|
| Threads | the one calling `block_on` | `worker_threads` (default: CPU count) + blocking pool |
| Spawn + join (measured §6) | ~300 ns | ~800 ns with 4 workers |
| Task round trip (measured) | ~180 ns | 250–400 ns with 4 workers |
| `tokio::spawn` requires | `Send + 'static` (the API is shared) | `Send + 'static` |
| `!Send` tasks | `LocalSet` / `spawn_local` (13.2) | `LocalSet` on one thread |
| Uses all cores | no | yes |
| One task blocking | stalls everything | stalls one worker's queue (and its LIFO slot) |
| LIFO slot, work stealing | no | yes |
| Good for | tests, CLI tools, a sidecar thread inside a sync program, thread-per-core designs | servers |

**Knobs worth knowing, and when to touch them:**

| Builder method | Default [LIB 1.53.1] | Touch it when |
|---|---|---|
| `worker_threads(n)` | `TOKIO_WORKER_THREADS` if set, else `std::thread::available_parallelism()` | the pod's CPU limit isn't what that function reports, or you reserve cores for something else |
| `max_blocking_threads(n)` | 512 | blocking work must be capped (a DB driver with 20 connections doesn't need 512 threads) |
| `thread_keep_alive(d)` | 10 s | bursty blocking work creates and destroys threads |
| `event_interval(n)` | 61 ticks | I/O latency under CPU-heavy load matters more than throughput (lower) |
| `global_queue_interval(n)` | self-tuned (~10 ms) | tasks spawned from outside the runtime wait too long |
| `disable_lifo_slot()` | enabled | measurements show LIFO starvation you can't fix at the source |
| `thread_name`, `on_thread_start` | "tokio-rt-worker" | always name them: it's what `top -H` and `perf` show |

The last row is the only one to change by default. The others change only after measuring, and each move trades
throughput against fairness.

**The cooperative budget, as a decision.**

| Code shape | Protected by the budget? | What to do |
|---|---|---|
| Loop over Tokio resources that may be always ready (`recv`, socket reads) | yes | nothing; don't wrap it in `unconstrained` without a reason |
| CPU work between awaits (parsing, hashing) | no | keep each stretch short, or move it off the workers (13.2) |
| Loop over always-ready non-Tokio futures | no | `tokio::task::coop::consume_budget().await` (listing `ch01-09`) or `yield_now()` every N iterations |
| A third-party stream that's always ready | only if it uses Tokio resources inside | measure with a heartbeat (listing `ch01-03`) |

> **Why not one runtime per core?** Thread-per-core runtimes (Glommio, Monoio, and `tokio-uring` on Linux) pin one
> single-threaded executor to each core, never move a task, and often use io_uring instead of epoll. No work stealing
> means no cross-core traffic and perfect locality, and no stealing also means that one hot connection overloads its
> core while the others idle. They suit systems that can partition work evenly (a database sharded by core, as in
> ScyllaDB's Seastar, the C++ design these follow). A general-purpose service with uneven requests usually does better
> with stealing. None of these runtimes is on the Playground; this box is a pointer, not a measurement.

### 8. Java comparison

The closest Java analog to a Tokio worker is a **Netty `EventLoop`**: one thread, one selector (`epoll` on Linux), a task
queue, and timers.

| | Netty (`EventLoopGroup`) | Tokio multi-thread runtime |
|---|---|---|
| Unit of work | a `Channel`'s handlers, run on its event loop | a task (any future) |
| Assignment | a channel is **pinned** to one event loop for life | a task can run on any worker; it migrates at any `.await` |
| Load balancing | round-robin assignment at connect time; no stealing | work stealing between workers |
| Readiness | one selector per event loop | one epoll instance for the whole runtime |
| Timers | `HashedWheelTimer` (a wheel, typically 100 ms ticks) or the loop's scheduled tasks | a 6-level wheel, 1 ms ticks |
| Blocking | forbidden on the loop; offload to another executor | forbidden on workers; `spawn_blocking` |
| Preemption | none | none; a cooperative budget on Tokio resources |

Java virtual threads (Chapter 12.6 §8) are the other comparison: their carrier threads are a `ForkJoinPool`, which also
work-steals. The difference is what gets scheduled: a virtual thread is a stack that the JVM parks and resumes; a Tokio
task is a future that returns `Pending`.

> **Analogy limit.** Netty's pinning gives it something Tokio doesn't: state touched only by one channel's handlers is
> confined to one thread, with no synchronization needed. A Tokio task can wake up on another worker, so everything it
> holds across an `.await` must be `Send` (Chapter 13.2 shows the compile error), and `thread_local!` state is shared
> between tasks that happen to run on the same worker. Code ported from Netty that assumes "my handler always runs on
> my thread" is wrong on Tokio's multi-thread runtime.

### 9. Production scenario

`merchant-notify`'s pilot pods (Chapter 12.6 §9–§10) run up to 100,000 dashboard and POS-terminal connections each. After
the May 2026 handshake incident, the team wrote down the runtime configuration and why:

- **`worker_threads(8)`, set explicitly.** The pods have an 8-CPU limit. Tokio's default asks
  `std::thread::available_parallelism()`, which on Linux reads the affinity mask and cgroup quota; the team pinned the
  number anyway so that a node-level or platform change can't silently change the worker count.
- **Named threads and a startup log line** with the runtime's configuration (workers, blocking cap, and the Tokio version
  from `Cargo.lock`), so an incident review starts from facts.
- **`max_blocking_threads(16)`.** The only blocking work is the password hash in reconnect handshakes (already behind a
  4-permit semaphore after May) and log-file rotation. 512 idle-able threads is a limit nobody should hit, and a cap
  that low turns a regression into a queue, not a thread explosion.
- **`RLIMIT_NOFILE` raised to 1,048,576 in the container spec.** Each connection is a descriptor, and the default soft
  limit (often 1,024) fails `accept` with `EMFILE` at the 1,000th connection. The accept loop backs off on that error
  (Project L5 shows the code).
- **Runtime metrics exported.** On stable Tokio, `RuntimeMetrics` offers `num_workers()`, `num_alive_tasks()`,
  `global_queue_depth()`, and per-worker `worker_total_busy_duration()` and park counts (listing
  `ch01-09-runtime-metrics.rs` calls each). The team exports alive tasks as a gauge and alerts when it diverges from the
  connection count (a task leak), and turns busy duration into a per-worker utilization graph. Steal counts, poll-time
  histograms, and queue depths per worker exist only when Tokio is compiled with `--cfg tokio_unstable` (on stable,
  listing `ch01-10-unstable-metric.rs` gets `error[E0599]: no method named worker_steal_count`), so the team enabled
  that flag in a canary build first.
- **A heartbeat probe in load tests**, exactly listing `ch01-03`'s pattern: a task that sleeps 10 ms and reports its worst
  lateness. It turned May's incident from "dashboards flap under load" into "worker stalls of 150 ms per handshake".

### 10. Failure scenario

**The heartbeat burst (June 2026, `merchant-notify`, fictional).** Two weeks after the handshake fix, one pilot pod
stalled for 2.3 s: a new feature called a synchronous DNS resolver inside an async function, and the resolver timed out
(Chapter 13.2's exact failure). The stall itself was expected to cost one missed heartbeat. What happened next wasn't.

Each connection task sent a heartbeat every 5 s from a `tokio::time::interval`, and each terminal kept a small receive
window. The stall lasted less than one period, so no ticks were missed there. But the same pod had a second interval
per connection, a 100 ms "flush pending events" timer, created with the default `MissedTickBehavior::Burst`. After the
2.3 s stall, each of the ~90,000 connections fired **23 catch-up flushes back to back**: about 2 million flush polls
within a few milliseconds, each doing a (usually empty) queue check and some a socket write. Workers saturated for
~400 ms. Heartbeat sends were delayed past the terminals' 3-missed-heartbeats rule for a subset of connections, and about
6,000 terminals reconnected, which reran the (now semaphore-limited) handshakes. The pod recovered in about a minute,
from a 2.3 s stall.

The analysis, in this chapter's terms:

1. **`Burst` turns a stall into a synchronized storm.** Listing `ch01-02` shows it at small scale: three ticks at 46 ms.
   At 90,000 intervals, the catch-up is 90,000 × 23 polls, all due "now", all runnable at once.
2. **Nothing needed the missed flushes.** A flush is idempotent: one flush after the stall does the work of 23.
3. **The root cause was the blocking call** (fixed separately with `spawn_blocking` and an async resolver), but the
   amplification was a configuration default nobody had chosen.

Fixes: `set_missed_tick_behavior(MissedTickBehavior::Skip)` for the flush timer and the heartbeat (a heartbeat must not
be sent twice to catch up); a review rule that **every `interval` states its missed-tick policy**; and a load test that
injects a 2 s stall with `std::thread::sleep` on one worker and checks that the reconnect count stays at zero.

---

## Practice

### 11. Interview & architecture questions

1. What does the kernel see of a Tokio runtime with 10,000 tasks waiting on 10,000 sockets? List every OS resource.
2. Walk through what happens between "a socket becomes readable" and "the task waiting for it runs". Which parts are
   the kernel, and which are Tokio?
3. Why does the multi-thread scheduler have a LIFO slot? Describe the workload it helps and the failure it creates, with
   listing `ch01-04`'s numbers.
4. A task loops over `rx.recv().await` on a channel that's always full. Another loops over a `Vec` of `Future`s that are
   all `ready`. Which one starves its neighbors, and why?
5. `sleep(Duration::from_micros(200))` in a pacing loop produces 1,000 iterations per second at most, not 5,000.
   Explain, and propose two alternatives.
6. When would you choose `new_current_thread()` for a production service?
7. Why is spawning a task on a 4-worker runtime slower than on a current-thread runtime, and why is that usually a good
   trade?
8. What does `MissedTickBehavior::Burst` do, and when is it the right choice?
9. Why does Tokio box futures larger than 16 KiB before spawning them? What does it tell you about the size of your
   futures?
10. You're porting a Netty service. Which assumption about threads is most likely to be silently wrong on Tokio?

### 12. Exercises

- **Beginner.** Build a runtime with `Builder`, name its threads `"notify-worker"`, and print `/proc/self/task/*/comm`
  from inside a task. Then build a current-thread runtime and show which thread a spawned task runs on.
- **Intermediate.** Modify listing `ch01-02` to measure `sleep(Duration::from_millis(1))` 1,000 times on a runtime whose
  workers are busy (spawn 8 CPU-bound tasks first). How does the distribution change, and why?
- **Advanced.** Reproduce experiment 3 of listing `ch01-04` with `disable_lifo_slot()`. Then repeat experiment 1. Report
  both numbers and explain the trade-off in two sentences.
- **Systems.** On a Linux machine, run a Tokio echo server under `strace -f -e trace=epoll_wait,epoll_ctl,write,futex`
  (not verified here: needs strace) with one client sending a message per second. Identify the park, the wakeup, and the
  eventfd write. Then add 4 busy tasks and look again.
- **Architecture.** Write a one-page runtime configuration standard for Meridian's Rust services: which builder
  settings are mandatory, which are forbidden without a benchmark, which metrics must be exported, and which review
  rules (like "every `interval` states its missed-tick policy") apply.

### 13. Debugging exercise

A metrics exporter samples 5,000 per-route counters every second and pushes them to a collector. After a deploy, the
collector sees bursts of duplicate samples every few minutes, and the exporter's CPU spikes at the same moments. The
code:

```rust,ignore
// Debugging exercise: find the defect (not a verified listing)
async fn export_loop(routes: Arc<Routes>, client: Collector) {
    let mut every_second = tokio::time::interval(Duration::from_secs(1));
    loop {
        every_second.tick().await;
        let snapshot = routes.snapshot(); // ~5,000 counters
        if let Err(e) = client.push(snapshot).await { // sometimes takes 10-30 s when the collector is slow
            tracing::warn!(error = %e, "push failed");
        }
    }
}
```

Explain the bursts and the CPU spikes using §6. What is the fix, and is it one line? What should the exporter do when
a push takes longer than the period: queue, skip, or overlap pushes? Model answer in Appendix A.

### 14. Design exercise

Meridian's ledger team wants a **sidecar inside the Java ledger's pod**: a Rust process that terminates 2,000 client TLS
connections, batches ledger writes, and forwards them over 16 persistent connections to the JVM. Its CPU limit is 2
cores. The JVM takes 1–5 ms per batch, and a TLS handshake costs ~1 ms of CPU.

Design the runtime topology: flavor and worker count; whether handshakes run on the workers; how many blocking threads
it needs, and for what; what the `event_interval` and missed-tick policies should be; and what three runtime metrics you
would alert on. Justify each choice with a measurement from this chapter or an experiment you'd run.
