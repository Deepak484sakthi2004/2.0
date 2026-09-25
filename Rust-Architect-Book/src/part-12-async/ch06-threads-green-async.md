# Chapter 12.6 — OS Threads vs Green Threads vs Async Tasks

> **Where this sits:** Part XII · Async Rust · chapter 6 of 6
> **Prerequisites:** Chapters 12.1–12.5. Chapter 11.1 (threads and the OS) and Chapter 1.2's runtime comparison.
> **After this chapter you can:** say, with numbers, what one unit of concurrency costs in each model (memory, switch,
> spawn); explain how Go, Java virtual threads, and Erlang suspend a computation and what that buys and costs them;
> recognize and fix blocking and CPU hogging in async code; explain function coloring and every way across the boundary;
> compare cancellation across the models; and choose a concurrency model for a system with a decision matrix.

---

## Pass 1 · User level — *Three ways to wait for many things at once*

### 1. Problem

Every server spends most of its life waiting: for a socket, a database, a timer, another service. There are three
established ways to let one program wait for a hundred thousand things at once, and each language has made a bet:

- **OS threads.** The kernel schedules them; each has its own stack. C, C++, Java's platform threads, and Rust's
  `std::thread`.
- **Green threads** (stackful coroutines, M:N threading). A language runtime schedules its own lightweight threads,
  each with its own small stack, onto a few OS threads. Go's goroutines (since 2009), Erlang's processes, and Java's
  virtual threads (JDK 21, 2023).
- **Async tasks** (stackless coroutines). The compiler turns functions into state machines, and a library executor
  polls them. Rust, C#, JavaScript, Python, Kotlin.

Rust had green threads before 1.0 and removed them (Chapter 12.1, RFC 230). Java did the reverse in 2023: it added
green threads to a language that had been thread-per-request for 25 years. Both choices were deliberate, and both teams
had studied the other option carefully. This chapter puts the three models side by side with measurements for the
parts that can be measured here, and turns the result into a decision matrix. Chapters 12.1–12.5 each promised a piece
of it: memory per unit, switch cost, what blocking does to an executor, and Java's opposite bet.

### 2. Mental model

The question that decides everything else is **where a suspended computation keeps its state**:

```text
 OS THREAD                           GREEN THREAD (stackful)             ASYNC TASK (stackless)
 ─────────                           ───────────────────────             ──────────────────────
 state = its stack (2 MiB reserved)  state = its own stack, small and    state = the future: a value
   + registers saved by the kernel     growable, owned by the runtime      sized at compile time (12.3)
                                       + registers saved by the runtime
 switched by: the kernel             switched by: the language runtime   switched by: the executor
   preemptively, at any instruction    at blocking calls (and by signal     only where the code says
                                       or reduction count in Go, Erlang)    `.await` (cooperative)
 a switch: syscall, kernel           a switch: save registers, swap      a switch: `return Pending`,
   scheduler, cache and TLB effects    the stack pointer (user space)      pop the next task, call poll
 any code may block                  any code may block, IF the runtime  code must not block: that
                                       intercepts the blocking call        stalls the executor thread
```

Three consequences follow directly, and the rest of the chapter measures them:

1. **Memory.** A stack must be big enough for the deepest call chain the unit will ever make, so it's either large
   (OS threads) or growable (green threads). A future holds only what's live across an `.await`, so it's exactly sized,
   and it can't grow.
2. **Blocking.** With a stack, a function can stop anywhere, so ordinary blocking code works. Without one, a function
   can stop only where the compiler built a state for it: at `.await`. Everything between two `.await`s runs to
   completion on the executor's thread.
3. **Coloring.** Because an async function stops only at `.await`, its callers must be async too (or must block on it).
   Stackful models don't split functions into two kinds.

### 3. Rust code

**Memory per unit, measured.** Listing `ch06-01-memory-per-unit.rs` spawns 400 idle OS threads (each blocked in
`recv`) and reads the process's own memory counters, then spawns 100,000 idle async tasks on `futures`' `LocalPool`
(each awaiting a oneshot channel that's still open, like a connection waiting for its next message), measuring the heap
with the Part III counting allocator and the resident set from `/proc/self/status`:

```text
idle OS thread:    2064 KiB virtual,   9.6 KiB resident each
idle async task:    247 B heap (task + future + oneshot),  0.28 KiB resident each
after completion: 3145760 B still allocated
after a second 100,000 tasks: 3145760 B still allocated
```

(Release build, one run.) The thread numbers are Chapter 12.1's again: 2 MiB of reserved stack plus a little more, and
under 10 KiB actually resident. An idle task is **247 bytes** of heap. Section 5 explains the last two lines.

**Switch cost, measured.** Listing `ch06-02-ping-pong.rs` bounces a counter 100,000 times between two OS threads (each
blocked in a `std::sync::mpsc` receive) and between two async tasks on one thread (each awaiting a `futures` channel):

```text
OS threads, std mpsc:              9608 ns per round trip
async tasks, one thread, mpsc:      118 ns per round trip
```

Two more runs gave 7,799 / 115 ns and 7,718 / 115 ns, so the fair summary is **7.7–9.6 µs versus about 0.12 µs**, a
factor of 65–80 (release builds, three runs on a shared machine). Chapter 11.1's thread-pool hand-off, the same two-wake
pattern, measured 5.7 µs.

**Blocking in async, measured.** Listing `ch06-03-blocking-stall.rs` runs a heartbeat task that wants to wake every
10 ms, next to a task that makes a 200 ms blocking call (a stand-in for a password hash, a synchronous DNS lookup, or
a JDBC-style driver), on Tokio's single-threaded runtime:

```rust,ignore
let worst = rt.block_on(async {
    let hb = tokio::spawn(heartbeat(30));
    tokio::time::sleep(Duration::from_millis(25)).await;
    let _ = hash_password(); // BUG: blocks the only executor thread
    hb.await.unwrap()
});
```

```text
blocking call on the executor thread: worst heartbeat delay 194 ms
blocking call moved to spawn_blocking: worst heartbeat delay 1 ms
```

One blocking call delayed an unrelated task by 194 ms. The fix, `tokio::task::spawn_blocking(hash_password).await`,
runs the call on a separate thread pool and gives the executor back a future to await [LIB]. With threads, the kernel
would have preempted the hashing thread and run the heartbeat on time; with async tasks, nobody preempts anybody.

---

## Pass 2 · Systems level — *How each model suspends, and what that costs*

### 4. Under the hood

**OS threads** [OS]. A thread is a kernel task with its own kernel stack, saved registers, and scheduling state. A
blocking system call puts it on a wait queue; a timer interrupt can deschedule it at any instruction. Linux's scheduler
(CFS, replaced by EEVDF in Linux 6.6, 2023 [VERSION]) picks the next runnable task per core. A switch saves one task's
registers, restores another's, and returns to user space in the new task. The direct cost is on the order of a
microsecond; the indirect cost, cold caches and TLB entries and branch-predictor state, is often larger (order of
magnitude, not measured here). Everything the kernel does is the same whether the thread runs Rust, Java, or C: that's
why Rust's threads and Java's platform threads cost the same.

**Green threads** move scheduling into the language runtime. A switch is cheap: save the callee-saved registers, swap
the stack pointer, restore the other thread's registers, all in user space. The hard problems are the ones the kernel
used to solve, and each successful green-thread runtime solved them by **owning the whole program**:

- **Stacks.** A green thread needs a stack that starts small (so a million fit) and grows when needed. Rust (before
  1.0) and Go (before 1.3) both tried *segmented* stacks, which link a new segment on overflow, and both abandoned them
  because of the "hot split": a call at a segment boundary inside a hot loop allocates and frees a segment on every
  iteration (Chapter 1.2). Go 1.3 (2014) switched to **contiguous stacks that grow by copying**: a function prologue
  checks for room, and if there isn't any, the runtime allocates a stack twice the size, copies the old one, and
  **adjusts every pointer into it** (Go release notes). That last step is only possible because Go's compiler records
  where every pointer in every frame is. Native code, with raw pointers anywhere, can't be moved that way. Java 21's
  virtual threads solve the same problem differently: when a virtual thread blocks, its frames are copied into **stack
  chunk objects on the Java heap**, which the GC understands and may move like any other object, and copied back
  (lazily, a few frames at a time) when it resumes [RUNTIME: JEP 444].
- **Blocking.** A green thread that makes a real blocking system call blocks its OS thread, and every green thread
  scheduled on it. So the runtime must intercept blocking operations. Go's **netpoller** sets sockets non-blocking,
  parks the goroutine, and registers the descriptor with epoll/kqueue: blocking-style code on top of Chapter 12.1's
  event loop. For system calls that really block (file I/O, some DNS paths), Go hands the goroutine's logical processor
  (the P in G-M-P) to another OS thread so other goroutines keep running [RUNTIME: Go scheduler]. Java rewrote the
  JDK's blocking APIs (sockets, `java.util.concurrent` locks, `Thread.sleep`) to **unmount** the virtual thread from its
  carrier OS thread instead of blocking it. Operations it can't unmount from (many file-system calls) temporarily add a
  carrier thread to compensate (JEP 444).
- **Pinning.** Some frames can't be copied off the carrier. In JDK 21–23 a virtual thread blocking inside a
  `synchronized` block stayed **pinned** to its carrier, blocking the OS thread underneath; JEP 491 (JDK 24, 2025)
  removed that case [VERSION]. A native frame (JNI, FFM upcall) on the stack still pins, because the JVM can't move C
  frames. That's the same reason Go switches to a separate system stack for cgo calls, which makes a cgo call much more
  expensive than a Go call [RUNTIME]: foreign code can't live on a movable, growable stack.
- **Preemption.** Go preempts long-running goroutines asynchronously with signals since Go 1.14 (2020). Erlang's VM
  counts **reductions** (roughly, function calls) and switches a process out when its budget of a few thousand runs
  out, so no process can hog a scheduler [RUNTIME: BEAM]. Java's virtual-thread scheduler does *not* time-slice: "The
  scheduler does not currently implement time sharing for virtual threads" (JEP 444). A CPU-bound virtual thread holds
  its carrier until it blocks.

**Async tasks** (Chapters 12.3–12.5) have no stack to manage and nothing to intercept. The state is the future; a
switch is a function return and a function call; foreign code runs on the executor thread's ordinary stack. The price
moves from the runtime to the program: code between two `.await`s must not block and must not run for long, and
async functions have a color. Rust took this side because of **embeddability** (Chapter 12.1): no runtime to own the
program means async works inside a C library, a kernel, a browser, or on a microcontroller with no OS at all.

> **Why not green threads as a library in Rust?** It has been done (`may`, and older `libgreen`-style crates) with
> fixed-size `mmap`ed stacks and guard pages [LIB]. It works until the code calls anything that blocks the OS thread:
> std's `Mutex`, a C library's `read`, `std::fs`. Without owning every blocking call in the program (as Go does) or
> retrofitting the standard library (as Java did), a green-thread library can't stop one blocking call from stalling
> every green thread on that OS thread. Stacks also can't grow by copying, because Rust code may hold raw pointers into
> them. RFC 230 made this argument in 2014; it still holds.

**History worth knowing.** "Green threads" is the name of Java 1.1's user-level threads on Solaris (from Sun's Green
team); the JVM moved to native threads by about JDK 1.3. Solaris and Linux both had M:N threading libraries around
2000, and both converged on 1:1 (Linux's NPTL in 2003). M:N lost in C because it was a library bolted onto a world of
blocking calls it didn't control. It succeeded in Go, Erlang, and JDK 21 because each runtime controls its world.

### 5. Memory

| Unit | Idle memory | Source |
|---|---|---|
| Rust `std` thread (Linux, glibc) | **2,064 KiB virtual, 9.6 KiB resident** | measured, `ch06-01` |
| Rust async task (`LocalPool`, small future + oneshot) | **247 B heap, 0.28 KiB resident** | measured, `ch06-01` |
| Tokio task | one allocation: header + future + output slot [LIB] | not measured here; Part XIII |
| Go goroutine | 2 KiB initial stack (Go 1.4+; since Go 1.19 the start size adapts to the program's average), grows by copying, shrinks during GC | Go release notes (attributed) |
| Java platform thread | 1 MiB stack reservation (`-Xss` default) + a native thread | JDK docs (Chapter 11.1) |
| Java virtual thread | the `Thread` object plus heap stack chunks sized to the frames in use: from hundreds of bytes to KiBs, depending on call depth | JEP 444 (order of magnitude, not measured) |
| Erlang process | about 330 machine words (≈2.6 KiB) when spawned, including its own small heap | Erlang Efficiency Guide (varies by release) |

Three things in this table are easy to misread.

**Virtual vs resident.** The headline ratio between a thread and a task is 2,064 KiB to 247 B, about 8,500×. The
resident ratio is 9.6 KiB to 0.28 KiB, about 34×. An idle thread's untouched stack pages cost address space, not RAM
[OS]. That's why Chapter 12.1's threaded server hit kernel limits (task count, `vm.max_map_count`) long before it ran
out of memory. At 100,000 connections, threads cost about 1 GB resident, which is survivable; they cost 200 GB of
address space and 100,000 kernel tasks, which usually isn't.

**What the 247 bytes are.** Everything the heap holds for one idle task: `LocalPool`'s task node and the boxed future
(Chapter 12.5 measured about 191 bytes per task for that machinery with a 24-byte future), the async block's own state
(the channel receiver and a counter), the oneshot channel's shared state, and a share of the pool's spawn queue (below).
Resident memory per task is a little higher (≈287 bytes) because it also counts allocator overhead and the test's own
vector of 100,000 senders.

**The 3 MiB that stayed.** After all 100,000 tasks finished, 3,145,760 bytes were still allocated, and exactly the same
after a second batch of 100,000. A leak would have doubled; this is a high-water mark being reused. It's consistent
with `LocalPool`'s queue of newly spawned tasks: a `Vec` that grew to 131,072 entries of 24 bytes each (3,145,728
bytes) while 100,000 spawns arrived before the pool ran, and kept its capacity afterwards (an inference from the
`futures` source [LIB]). It's Chapter 9.1's capacity rule showing up in an executor: a burst leaves its peak behind.

**How each model fails on memory.** They fail differently, and that matters more than the per-unit averages:

- **Stackful** units grow with the *deepest call chain* each one makes. One goroutine that recurses deeply grows its
  own stack; the others stay small. Deep recursion is the stack problem it always was (the Part IX interlude), just
  per unit.
- **Stackless** units are sized by the *largest state* in their code, and every instance pays it, whether or not it
  ever reaches that state. A rarely taken branch that holds a 64 KiB buffer across an `.await` costs 64 KiB in every one
  of 100,000 tasks (Chapter 12.3's production scenario). The size is known at compile time, which makes it testable
  (`size_of_val` in CI) but not adaptive.

### 6. CPU / OS

**Switch cost.** The thread round trip in `ch06-02` includes, per direction, a `send` that finds the receiver blocked
and wakes it with a futex system call, the kernel scheduling the woken thread (often on another core, which adds an
inter-processor wakeup), and the sender then blocking in its own `recv` (another futex call and a context switch).
That's at least four system calls and two context switches per round trip [OS]. The task round trip is a buffer write,
a waker call that pushes the other task onto the pool's ready queue, a `Pending` return, and a poll: all function calls
in user space, no kernel entry. Go and Java virtual threads fall between the two: a user-space switch, plus, for Java,
copying frames between the heap and the carrier's stack. Reported figures for both are in the hundreds of nanoseconds
up to about a microsecond, depending on stack depth (order of magnitude, not measured here; the Systems exercise
measures the OS side locally).

**Spawn cost.** Chapter 12.5 measured 158 ns per spawned task (two allocations) against about 36 µs per OS thread
spawned and joined; Chapter 11.1 measured about 34 µs. A goroutine or a virtual thread is also a user-space object:
spawning one is an allocation and a queue push, not a `clone(2)`.

**Blocking stalls the executor.** In `ch06-03`, the heartbeat task starts as soon as the main task first awaits
(t ≈ 0), wakes at about 10 and 20 ms, and asks to wake again at about 30 ms. The main task's own 25 ms sleep ends
first, and it calls `hash_password`, which holds the only executor thread from t ≈ 25 ms to t ≈ 225 ms. The
heartbeat's 30 ms wakeup can only run after that, about 195 ms late: the measured 194 ms. The kernel did nothing wrong;
it ran the thread that was running. In a multi-threaded runtime, one blocked worker stalls only the tasks queued on it, but
enough concurrent blocking calls (one per worker) stall everything, which is §10's incident.

**Computation stalls it too.** Blocking isn't the only way to hold an executor thread. Listing
`ch06-06-cpu-starvation.rs` runs the same heartbeat next to a task that *computes* for about 150 ms without awaiting,
then the same computation with a `tokio::task::yield_now().await` every 50,000 iterations:

```text
CPU-bound task never yields                   : took 149 ms,    0 yields; worst heartbeat delay 143 ms
CPU-bound task yields every 50,000 iterations : took 141 ms, 1025 yields; worst heartbeat delay   1 ms
```

(Release build, one run: the 8 ms difference in total time is noise.) Yielding a thousand times cost nothing
measurable and brought the heartbeat's worst delay from 143 ms to 1 ms. Tokio's **cooperative budget** doesn't help
here: it makes a task yield after it has performed a number of operations on *Tokio's resources* (socket reads, channel
receives) in one poll [LIB], and a pure computation performs none. There's no preemption for CPU-bound async code:

| Model | Who preempts a long computation |
|---|---|
| OS threads | The kernel, at any instruction (timer interrupt) |
| Go goroutines | The runtime, by signal (Go 1.14+) |
| Erlang processes | The VM, when the reduction budget runs out |
| Java virtual threads | Nobody: it holds its carrier until it blocks (JEP 444) |
| Rust async tasks | Nobody: it runs until its next `.await` returns `Pending` |

**Where CPU work belongs in an async program.** Short bursts (microseconds) are fine inline. Longer work goes to
`spawn_blocking` (a separate, growable thread pool; Tokio's default cap is 512 threads [LIB]), to a rayon pool for data
parallelism (Chapter 11.6), or is chunked with `yield_now` when it must stay on the task. Tokio's `block_in_place` lets
a multi-threaded runtime's worker hand its other tasks to another thread before blocking [LIB] (Part XIII).

---

## Pass 3 · Architect level — *Choosing a model*

### 7. Trade-offs

**Function coloring.** Bob Nystrom's 2015 essay "What Color is Your Function?" named the problem: in a language with
async functions, there are two kinds of function, and the kinds don't mix freely. A synchronous function can't
`.await` (listing `ch06-04-coloring.rs`):

```rust,compile_fail
//! Function coloring: a synchronous function can't `.await`. To call async code it must either
//! become async itself (and so must its callers) or block on an executor.
async fn load_limits(merchant: u64) -> u64 {
    merchant * 1_000
}

fn check_limit(merchant: u64, amount: u64) -> bool {
    amount <= load_limits(merchant).await
}

fn main() {
    println!("{}", check_limit(7, 500));
}
```

```text
error[E0728]: `await` is only allowed inside `async` functions and blocks
 --> src/main.rs:9:37
  |
8 | fn check_limit(merchant: u64, amount: u64) -> bool {
  | -------------------------------------------------- this is not `async`
9 |     amount <= load_limits(merchant).await
  |                                     ^^^^^ only allowed inside `async` functions and blocks
```

The ways across the boundary, and when each one is wrong:

| From → to | Mechanism | Wrong when |
|---|---|---|
| sync → async | Make the caller `async` | The color spreads up every caller; public sync APIs can't change |
| sync → async | `block_on(fut)` (a runtime `Handle`, or `futures::executor::block_on`) | Called on a thread that is already running async tasks: Tokio panics, a single-threaded executor deadlocks |
| async → sync (blocking) | `spawn_blocking(f).await` | The work can't be cancelled once started; the pool is shared |
| async → sync (blocking) | `block_in_place(f)` (Tokio, multi-threaded runtime only) | On a current-thread runtime (it panics), or when it's used for everything |

The second row is the trap, because it works in unit tests. Listing `ch06-05-nested-runtime.rs` puts a synchronous
helper over an async function, the way a team might after hitting E0728, and calls it from inside a request handler:

```rust,ignore
/// A synchronous API over an async one, via a (global) runtime.
fn check_limit(merchant: u64, amount: u64) -> bool {
    let rt = tokio::runtime::Handle::current();
    amount <= rt.block_on(load_limits(merchant))
}
```

```text
thread 'main' (14) panicked at src/main.rs:11:18:
Cannot start a runtime from within a runtime. This happens because a function (like `block_on`) attempted to block the
current thread while the thread is being used to drive asynchronous tasks.
```

Tokio detects the mistake and panics [LIB]. `futures::executor::block_on` doesn't detect it: it parks the executor's
thread, and if the future it waits for needs that same executor to make progress, nothing ever wakes it. The Part XII
review's PR contains exactly that.

Color is also information. Every `.await` marks a point where the function may suspend, and therefore a point where it
may be cancelled (Chapter 12.3) and where a held lock stays held (the `MutexGuard` that isn't `Send`). Stackful models
hide those points: in Go or with virtual threads, any call might park the unit, and nothing in the source says which.

**Cancellation** differs more between the models than any other behavior:

| Model | How you stop a unit | When it takes effect |
|---|---|---|
| OS threads (Rust) | You can't force it; set a flag, close a channel | When the thread checks |
| OS / virtual threads (Java) | `interrupt()`: blocking JDK calls throw `InterruptedException`; `Thread.stop` throws `UnsupportedOperationException` since JDK 20 [VERSION] | At the next interruptible blocking call |
| Go goroutines | Cancel a `context.Context`; code must check `ctx.Done()` | When the goroutine checks |
| Erlang processes | Send an exit signal; the process dies at once; links and supervisors react | Immediately. Safe because processes share no memory |
| Rust async tasks | Drop the future (or `JoinHandle::abort`) | At its current `.await`: nothing after it runs, destructors run (Chapter 12.2) |

Rust's is the only one where the canceller needs no cooperation from the cancelled code, and the only one where every
`.await` is a potential exit. Erlang gets immediate cancellation safely by forbidding shared memory; Rust gets it by
making the exit points visible and running destructors at them.

**The decision matrix.**

| Criterion | OS threads | Rust async (Tokio) | Go goroutines | Java virtual threads | Erlang processes |
|---|---|---|---|---|---|
| Idle memory per unit | ~10 KiB resident, 2 MiB virtual, 1 kernel task | bytes to KiB, exact, fixed per type | KiB, growable | KiB, grows with depth | KiB |
| Switch | µs, kernel | ~0.1 µs, user space | sub-µs, user space | sub-µs to µs, frame copying | sub-µs |
| Preemption | yes | no (`.await` only) | yes (signals) | no (blocking points only) | yes (reductions) |
| Any function may block | yes | **no** | yes (runtime intercepts) | yes, except pinning cases | yes (runtime intercepts) |
| Function coloring | no | **yes** | no | no | no |
| Cancellation | cooperative | drop, at `.await` | cooperative (`context`) | cooperative (interrupt) | immediate (exit signals) |
| Calling C / being called from C | natural | natural | costly (cgo, stack switch) | pins the carrier | ports / NIFs |
| Needs a runtime | no | a library executor | built in | the JVM | the BEAM VM |
| Works with no OS | no | **yes** (e.g. Embassy) | no | no | no |
| Data-race freedom | compile time (Send/Sync) | compile time (Send futures) | runtime detector only | no (JMM) | no shared memory |

Rust offers two columns, and the right answer is often the first. Threads are simpler, preemptive, and debuggable with
an ordinary stack trace. Async earns its complexity when there are many more waiting units than cores (tens of
thousands of mostly idle connections, fan-out to many slow backends), or when there's no OS.

### 8. Java comparison

Java 21 and Rust made opposite bets on the same problem. Java changed the **runtime** so that existing blocking code
scales; Rust changed the **program** so that no runtime is needed.

How a virtual thread works [RUNTIME: JEP 444]: it's a `java.lang.Thread` whose continuation (its frames) runs mounted on
a **carrier** platform thread. The default scheduler is a work-stealing `ForkJoinPool` with one carrier per core, the
same structure as Tokio's multi-threaded scheduler (Chapter 12.5 §8). When the virtual thread blocks in a retrofitted
JDK call, it **unmounts**: its frames move to heap stack chunks, and the carrier runs another virtual thread. When the
socket is ready or the lock is free, it's scheduled again and mounts on whichever carrier is free.

| | Java virtual threads | Rust async tasks |
|---|---|---|
| Your code | Ordinary blocking code: JDBC, `HttpClient.send`, `synchronized` | `async fn` and `.await`; async versions of every I/O library |
| Suspension points | Any blocking JDK call, invisible in the source | Every `.await`, visible |
| Suspended state | Heap stack chunks, sized by current depth | The future, sized at compile time |
| What stalls a carrier/worker | Pinning: native frames; `synchronized` before JDK 24; CPU loops | Blocking calls; CPU loops |
| Cancellation | `interrupt()`, cooperative | Drop, at `.await` |
| Per-unit context | `ThreadLocal` (one copy per virtual thread: millions of copies); `ScopedValue` finalized in JDK 25 (JEP 506) [VERSION] | Explicit parameters, or task-local values in the runtime [LIB] |
| Structured concurrency | `StructuredTaskScope`, still a preview API in JDK 25 [VERSION] | `join!`/`select!`, `JoinSet`, scoped task crates [LIB] |
| Observability | Thread dumps include virtual threads (`jcmd <pid> Thread.dump_to_file`) | A task's "stack" is a chain of `poll` calls; `tokio-console` shows tasks [LIB] |
| Runtime requirement | The JVM | None; an executor library |

The failure modes mirror each other. A virtual thread pinned to its carrier by a native frame and a Tokio worker blocked
by a synchronous call are the same bug: a scheduler that relies on units giving the thread back, and a unit that
doesn't. In Java the list of offending operations is short and shrinking (JEP 491 removed the largest one). In Rust it's
anything that blocks or computes for long, and the compiler doesn't flag any of it.

> **Analogy limit.** "Virtual threads are Java's `async`/`await`" gets the goal right and the mechanism wrong. A
> virtual thread is a `Thread`: blocking calls, `synchronized`, thread-locals, and stack traces all work as before, and
> nothing in the source changes. Rust's async changes the source: functions acquire a color, futures need `Pin`, and a
> blocking call becomes a latent bug. What Java paid for that convenience is the runtime: stacks the GC can move, and a
> standard library rewritten to unmount. Rust couldn't pay it without giving up embeddability, which is RFC 230's
> argument (Chapter 12.1) in one sentence.

### 9. Production scenario

**One model per workload.** In 2026 Meridian's platform group reviewed the concurrency model of four systems, as part
of the merchant-notify pilot's architecture review. The rule it wrote down was short: *choose the model by where the
waiting happens and who owns the code that waits.*

| System | Workload shape | Decision | Reasoning |
|---|---|---|---|
| merchant-notify (Rust pilot) | ~600,000 mostly idle connections at peak, 100,000 per pod target (Chapter 12.1) | **Rust async (Tokio)** | Waiting dominates and memory per idle connection decides the pod count: ≈250 B per idle task plus buffers vs ≈10 KiB resident and a kernel task per thread. The team owns all the code that waits. |
| Ledger (Java, ~3K TPS, ~90% of time in the database; Part I) | Concurrency capped by its database connection pool | **Stays on a platform-thread pool** | Little's law: a few thousand requests per second times tens of milliseconds is on the order of a hundred requests in flight fleet-wide. The pool, not threads, is the limit. Virtual threads would move the queue from the executor to the connection pool and change nothing that matters. |
| Partner webhook dispatcher (Java) | Thousands of concurrent outbound HTTP calls to partner endpoints, many taking seconds | **Java virtual threads** | Waiting-heavy, and the code is existing blocking `HttpClient` code. Virtual threads remove the thread-pool ceiling without a rewrite. A per-partner `Semaphore` still caps concurrency: virtual threads make threads cheap, not partners' capacity. |
| Fraud feature extraction (Rust library via FFM; ~3 ms of CPU per score, 50K scores/s; Part I) | CPU-bound, called synchronously from Java | **No async at all** | There's nothing to wait for. An async API would force an executor into the JVM process and add a poll per call. It stays synchronous; the Java caller supplies the parallelism. |

Two of the four answers weren't async, and one wasn't Rust. The review's point was not that one model wins; it was that
each model is the right answer for a different shape of waiting, and that "we should use async" is only a meaningful
statement once you know where the waiting is.

### 10. Failure scenario

**The reconnect handshake that stalled a pod.** In May 2026, the merchant-notify pilot ran on four production pods at
up to 100,000 connections each, on Tokio's multi-threaded runtime with eight worker threads. When a terminal
reconnected, the connection task verified its credentials with a password-hashing function tuned to take about 150 ms
of CPU, called directly inside the `async` handshake function. At normal reconnect rates nobody noticed.

Then a regional ISP blip dropped about 20,000 terminals on one pod, and they reconnected within seconds. Each handshake
occupied a worker thread for 150 ms: eight workers could complete about 53 handshakes per second (8 / 0.15 s), so the
backlog alone would have kept every worker busy for over six minutes. While they hashed, the 80,000 still-connected
terminals and dashboards got no heartbeats. Dashboards treat three missed 5-second heartbeats as a dead connection, so
they disconnected and reconnected too, adding handshakes to the queue. It was Chapter 12.1's 2019 reconnect storm in a
new form: the recovery traffic was worse than the incident. Traffic was shifted off the pod, which recovered only after
it was drained. `ch06-03` is the reduction: one blocking call, and a heartbeat 194 ms late.

The investigation was short once someone looked at poll durations: `tokio-console` showed handshake tasks with single
polls of 150 ms [LIB; not verified here]. The fixes:

1. **Take the hash off the workers.** Verification runs through `spawn_blocking`, behind a semaphore of four permits, so
   hashing can use at most four cores and the rest of the pod keeps serving. Handshakes waiting for a permit are cheap
   suspended tasks.
2. **Make the common case cheap.** The expensive hash is now used only when a terminal first pairs. Reconnects present a
   signed resumption token that takes microseconds to verify.
3. **Admit handshakes deliberately.** Each pod accepts a bounded number of handshakes per second and answers the rest
   with a retry-after hint plus jitter, the admission policy from Chapter 12.1's design exercise.
4. **Guard the rule.** Code review flags any call in an async function that can block or take more than a millisecond of
   CPU, and a load test fails if any single poll exceeds 10 ms.

The lesson is the one this chapter's measurements keep repeating: in a thread-per-connection design, a slow function is
slow for its caller; in an async design, it's slow for every task on the same worker. The kernel used to enforce
fairness. Now the program does, one `.await` at a time.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XII).*

1. Where does a suspended computation keep its state in each of the three models? What follows from that for memory,
   switch cost, blocking, and function coloring?
2. `ch06-01` measured 2,064 KiB virtual vs 247 B per unit, but 9.6 KiB vs 0.28 KiB resident. Why is the resident ratio
   so much smaller, and which number limits a thread-per-connection server in practice?
3. Why is a thread-to-thread round trip about 8 µs and a task-to-task round trip about 0.12 µs on the same machine?
   What does each one include?
4. How does Go make blocking-style code cheap inside goroutines? What happens when a goroutine makes a system call that
   really blocks, or calls C?
5. How does a Java virtual thread suspend and resume? What is pinning, what did JEP 491 change, and what still pins?
6. Why could Go and JDK 21 make M:N threading work when Rust's pre-1.0 green threads and the C world's M:N libraries
   didn't?
7. What is function coloring? List the ways across the sync/async boundary in Rust, and when each one is wrong.
8. Compare cancellation in Rust async, Java (platform and virtual threads), Go, and Erlang.
9. A CPU-bound loop runs inside a Tokio task. What happens to other tasks? Why doesn't Tokio's cooperative budget help,
   and what are the fixes?

### 12. Exercises

- **Beginner.** Wrap `ch06-03`'s `spawn_blocking(hash_password)` call in `tokio::time::timeout` with a 50 ms limit.
  What does the caller see, and what happens to the hashing itself when the timeout fires? What does that mean for a
  `spawn_blocking` job that holds a database connection?
- **Intermediate.** In `ch06-06`, vary the yield interval (1,000; 50,000; 1,000,000; 10,000,000 iterations) and record
  total time and worst heartbeat delay for each. Where is the knee? Convert the interval to microseconds of CPU between
  yields and state a rule of thumb.
- **Advanced.** Extend `ch06-01` to compare a task that holds a 4 KiB buffer across its `.await` with a thread that has
  4 KiB of locals live while it blocks. Predict the heap and resident growth for 1,000 of each, then measure. Which
  model pays for the buffer while idle, and which pays only for pages it touched?
- **Systems.** On a Linux machine (not verified here: requires a local toolchain and `perf`), run `ch06-02`'s thread
  version under `perf stat -e context-switches,cpu-migrations`, then again with both threads confined to one core
  (`taskset -c 0`). Explain how the round-trip time changes, and why.
- **Architecture.** Write the ADR for merchant-notify's concurrency model, comparing Tokio's multi-threaded runtime,
  thread-per-core executors (Chapter 12.5's design exercise), and Java virtual threads on Netty's replacement. Use this
  Part's numbers and state what measurement would reverse your decision.

### 13. Debugging exercise

Once a night, merchant-notify's p99 delivery latency on one pod jumps to about a second for roughly a second, then
recovers. CPU is far from saturated (one of eight workers is busy). The nightly job:

```rust,ignore
async fn nightly_digest(store: &SessionStore) -> Digest {
    let mut digest = Digest::default();
    for s in store.snapshot().iter() {   // about 2 million sessions
        digest.add(score(s));            // about 0.5 µs of CPU each; no .await in the loop
    }
    digest
}

// at startup:
tokio::spawn(async move {
    let digest = nightly_digest(&store).await;
    publish(digest).await;
});
```

1. Estimate how long one poll of this task runs. Why are only *some* deliveries delayed, and by about how much?
2. Why doesn't Tokio's cooperative budget make this task yield?
3. Give three fixes (for example: `spawn_blocking` with an owned snapshot, chunking with `yield_now`, a rayon pool) and
   the trade-off of each. Which keeps the job's memory bounded, and which keeps the digest consistent?

### 14. Design exercise

**Choosing a model for the risk-limits service.** Chapter 1.2 introduced Meridian's risk-limits service: about 100,000
operations per second, p99 under 1 ms, and a hard invariant that no merchant's limit is ever breached. Each operation
reads and updates a merchant's counters in memory; the only I/O is the network request and an asynchronous
replication stream. Compare four designs:

- Tokio multi-threaded runtime with per-merchant state behind sharded `Mutex`es;
- thread-per-core: N single-threaded executors, merchants partitioned by core, requests forwarded to the owning core;
- a pool of OS threads with blocking I/O and the same sharded state;
- Java virtual threads with a `ConcurrentHashMap` of counters.

For each: where is the waiting, where is the CPU, how is the no-breach invariant enforced without a global lock, and
what happens at p99 when one merchant receives a burst? Pick one, and name the measurement (from this Part or Part XX)
that would change your mind.
