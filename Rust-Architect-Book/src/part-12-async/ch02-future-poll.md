# Chapter 12.2 — The Future Trait and poll

> **Where this sits:** Part XII · Async Rust · chapter 2 of 6
> **Prerequisites:** Chapter 12.1. Traits and associated types (Part VI), `Arc`/`Mutex` (Part XI).
> **After this chapter you can:** state the `Future` contract precisely, including the part no compiler checks; write a
> hand-rolled future that waits for an external event; implement `join` and `select`; explain why cancellation in Rust is
> "drop the future"; and contrast Rust's pull-based futures with Java's `CompletableFuture`.

---

## Pass 1 · User level — *A value that isn't ready yet*

### 1. Problem

Chapter 12.1 ended with an event loop and a missing piece: something that remembers where each connection is, and can be
resumed when its socket becomes ready. That something needs an interface that works for a socket read, a timer, a
database query, a message from another task, and any combination of them. It must also work with no operating system
at all, with no heap allocation per operation, and without a garbage collector to keep callback chains alive.

Rust's answer is one trait with one method. This chapter is about that method, `poll`, and the one-sentence obligation
that comes with returning "not yet." Nearly every async bug that isn't a plain logic error violates that sentence.

### 2. Mental model

A **future** is a value that may be able to produce a result later. You don't wait on it; somebody (an **executor**)
**polls** it: "can you make progress?" The future does as much work as it can without blocking, then answers in one of
two ways:

```text
 executor                                   future
    │  poll(cx) ─────────────────────────────►│  tries to make progress, never blocks
    │                                          │
    │◄─────────────── Ready(value) ────────────│  done: here's the result (never poll again)
    │                         or               │
    │◄─────────────── Pending ─────────────────│  not yet, AND "I have arranged for cx.waker()
    │                                          │  to be called when polling me again is useful"
    │   (executor stops polling it; does       │
    │    other work, or sleeps)                │
    │                                          │
    │◄─────── waker.wake() ── from a reactor, a timer, another task, another thread
    │  poll(cx) again ────────────────────────►│
```

Three properties to hold on to:

1. **Futures are lazy.** Creating one runs nothing. Only `poll` makes progress.
2. **Futures are pulled, not pushed.** Nothing calls you back with a result. The executor asks, and the future answers.
3. **`Pending` is a promise.** It means "I've registered the waker with whatever will cause progress." A future that
   returns `Pending` without doing that will never be polled again. No error, no timeout: the task just stops.

### 3. Rust code

**Lazy, verified.** Listing `ch02-01-lazy.rs` polls a future by hand, using `Waker::noop()` (stable since 1.85), a waker
that does nothing when woken:

```rust
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Waker};

async fn charge(amount: u64) -> u64 {
    println!("   (charge body runs: {amount})");
    amount
}

fn main() {
    println!("1. calling charge(500)");
    let fut = charge(500); // no body runs: this only stores `amount` in a state machine
    println!("2. got a {}-byte future; the body hasn't run", std::mem::size_of_val(&fut));

    let mut fut = pin!(fut);
    let mut cx = Context::from_waker(Waker::noop());
    println!("3. first poll");
    let result = fut.as_mut().poll(&mut cx);
    println!("4. poll returned {result:?}");

    let never_polled = charge(700);
    drop(never_polled);
    println!("5. dropped a second future without polling it: its body never ran");
}
```

```text
1. calling charge(500)
2. got a 16-byte future; the body hasn't run
3. first poll
   (charge body runs: 500)
4. poll returned Ready(500)
5. dropped a second future without polling it: its body never ran
```

Calling `charge(500)` built a 16-byte value (the argument plus a state tag, Chapter 12.3) and printed nothing. The body
ran inside the first `poll`. The second future was dropped unpolled, and its body never ran at all. That has a practical
consequence: **forgetting `.await` means the work doesn't happen.** rustc warns about it by default; listing
`ch02-02-must-use.rs` turns the warning into an error, as many teams do:

```text
error: unused implementer of `futures::Future` that must be used
  --> src/main.rs:11:5
   |
11 |     charge(500); // BUG: builds a future and drops it; nothing is charged
   |     ^^^^^^^^^^^
   |
   = note: futures do nothing unless you `.await` or poll them
```

**Two hand-written futures.** Listing `ch02-03-hand-futures.rs` implements the trait directly. `Countdown` completes on
its fourth poll and meets the `Pending` obligation by waking itself (a cooperative "yield"). `Delay` waits for a timer
thread and meets it by storing the waker where the timer will find it:

```rust,ignore
impl Future for Delay {
    type Output = u32; // how many times we were polled
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u32> {
        let mut state = self.shared.lock().unwrap();
        state.polls += 1;
        if state.done {
            return Poll::Ready(state.polls);
        }
        state.waker = Some(cx.waker().clone()); // register BEFORE returning Pending
        drop(state);
        if !self.started {
            self.started = true; // lazy: the timer starts on the first poll, not at construction
            let (shared, duration) = (self.shared.clone(), self.duration);
            std::thread::spawn(move || {
                std::thread::sleep(duration);
                let mut state = shared.lock().unwrap();
                state.done = true;
                if let Some(waker) = state.waker.take() {
                    waker.wake(); // the executor learns it's worth polling again
                }
            });
        }
        Poll::Pending
    }
}
```

Run under `futures::executor::block_on`:

```text
Countdown(3) finished on poll 4
Delay(30 ms) finished after ~30 ms, polled 2 times
```

Two polls for a 30 ms wait: one to start, one after the wake. The executor spent the 30 ms asleep, not spinning. That's
the whole economy of async: **waiting costs nothing but the memory of the future.**

**Composition without threads.** Listing `ch02-05-join-select.rs` writes `join` and `select` by hand; listing
`ch02-06-combinators.rs` uses the `futures` crate's versions. Their outputs:

```text
join(30 ms, 20 ms) -> (30, 20) after ~30 ms
   slow dropped after 1 poll(s), unfinished: cancelled
select(10 ms, 50 ms) -> Left(10) after ~10 ms
```

```text
join!:       ("A", 3000) ("B", 2000) after ~30 ms
join_all:    [("A", 3000), ("B", 1000), ("C", 2000)] (input order)
unordered:   ["B", "C", "A"] (completion order)
select:      deadline first; SLOW is dropped with the tuple
```

`join` of a 30 ms and a 20 ms operation takes 30 ms, not 50: both are waited on at once, on one thread. And `select`
returns the first result and **drops the loser**, which is how every timeout in Rust works.

---

## Pass 2 · Systems level — *The contract, the waker, and what cancellation really is*

### 4. Under the hood

**The trait** [LIB: `core::future::Future`, stable since 1.36]:

```rust,ignore
pub trait Future {
    type Output;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>;
}

pub enum Poll<T> {
    Ready(T),
    Pending,
}
```

Three design decisions are packed into that signature:

- **`self: Pin<&mut Self>`**: the future is polled in place, and once polled it may not move. Futures generated from
  `async fn` can hold references into themselves; Chapter 12.4 is about exactly this.
- **`cx: &mut Context<'_>`**: the context carries the current task's `Waker` (and, unstably, extension data [VERSION]).
  The waker is passed *at poll time*, not stored at construction. A future may be polled by different tasks over its
  life (moved into a spawned task, polled inside a `select` and then outside it), and the waker to use is always the one
  from the most recent poll.
- **No error type.** Failure is just an `Output` type (`Result<T, E>`), as in Part VIII.

**The contract, in full.** The trait's documentation states it; rustc checks none of it [LIB]:

| Rule | If violated |
|---|---|
| On `Pending`, the future must arrange for the **current** `cx.waker()` to be woken when progress is possible | The task hangs forever, silently |
| `poll` must not block | Every other task on that executor thread stalls (Chapter 12.6 measures it) |
| After `Ready`, don't poll again | Unspecified: may panic, hang, or return garbage. `async fn` futures panic (Chapter 12.3) |
| Spurious polls are allowed | Futures must tolerate being polled when nothing changed |
| `poll` should be cheap | Executors assume it returns quickly; long computations need `spawn_blocking` or chunking |

**The lost wakeup, measured.** Listing `ch02-04-lost-wakeup.rs` implements the same "wait for a signal" future three
ways. The first poll happens under a different waker (as it would inside a `select!` in another task); then a real
executor takes over on another thread, and 20 ms later the signal fires:

```rust,ignore
match self.strategy {
    // BUG: Pending, and nobody will ever call wake.
    Strategy::NeverRegisters => {}
    // BUG: keeps the waker from the first poll, even if a different task polls us later.
    Strategy::RegistersOnce => {
        if s.waker.is_none() {
            s.waker = Some(cx.waker().clone());
        }
    }
    // Correct: every poll leaves the CURRENT waker behind (skipping the clone if it's the same one).
    Strategy::RefreshesEveryPoll => match &mut s.waker {
        Some(w) if w.will_wake(cx.waker()) => {}
        slot => *slot = Some(cx.waker().clone()),
    },
}
```

```text
NeverRegisters: HUNG (done = true, but the executor was never woken)
RegistersOnce: HUNG (done = true, but the executor was never woken)
RefreshesEveryPoll: completed after ~20 ms
```

Look at what "hung" means: the event happened (`done = true`), the result is sitting in shared memory, and the task will
never look at it, because nothing told its executor to poll it. No thread is blocked; no CPU is used; no error is
logged. **Wake the waker from the latest poll** is the rule, and `Waker::will_wake` makes it cheap: it compares the data
and vtable pointers, so the common case (same task, polled again) costs a comparison instead of a clone.

**The ordering inside `poll`.** Notice where `Delay` registers its waker: inside the same lock that guards `done`,
*before* returning `Pending`. If it checked `done`, released the lock, and only then stored the waker, the timer thread
could set `done` and look for a waker in the gap. Nobody would be woken. It's the same lost-wakeup race that condition
variables solve by atomically releasing the lock and sleeping (Part XI); here, "check the condition and publish the
waker" must be one atomic step with respect to the completer.

**Cancellation is `drop`.** `select` returned as soon as one side finished and dropped the other. In `ch02-05` the
dropped future was wrapped in `Loud`, whose `Drop` reported it: `slow dropped after 1 poll(s), unfinished`. That's the
entire cancellation mechanism in Rust: **a future that is never polled again and is dropped has been cancelled.** There
is no cancellation token to check, no exception thrown into the future. The code after its current `.await` simply never
runs, and its destructors run. Two consequences follow, and both come back later:

- Cancellation is **immediate and cooperative only at `.await` points**: a future can't be interrupted in the middle of
  synchronous code, but it can be abandoned at any suspension.
- Anything a cancelled future started *outside itself* keeps going unless `Drop` stops it. In `ch02-05`, the timer
  thread behind the slow future still sleeps for 50 ms and then sends into a closed channel, harmlessly. If the future
  had started a database transaction, dropping it wouldn't roll anything back unless a guard's `Drop` did. Chapter 12.3's
  failure scenario is exactly that.

### 5. Memory

A future is an ordinary value. Its size is known at compile time, and it lives wherever you put it: on the stack under
`pin!`, in a `Box`, inside another future. Composition works by containment:

```text
 Join2<A, B>                          Select2<A, B>
 ┌─────────────────────────────┐      ┌──────────────────────┐
 │ a: A          (inline)      │      │ a: A     (inline)    │
 │ b: B          (inline)      │      │ b: B     (inline)    │
 │ a_out: Option<A::Output>    │      └──────────────────────┘
 │ b_out: Option<B::Output>    │      size ≈ size(A) + size(B)
 └─────────────────────────────┘
 size ≈ size(A) + size(B) + outputs
```

No allocation happens for `join` or `select` themselves. A task built from twenty nested combinators is one value, often
one heap allocation when spawned (Chapter 12.5). That's the "zero-cost" in the 2016 design: combinators compile to one
state machine, the same way iterator adapters compile to one loop (Part X).

The costs sit elsewhere. `Waker` is 16 bytes (a data pointer and a vtable pointer, Chapter 12.5 verifies it), and cloning
one usually bumps a reference count. Shared state between a future and whoever completes it (the `Arc<Mutex<...>>` in
`Delay`) is a heap allocation plus a lock. In `ch02-06`, `sleep_ms` is built from a `oneshot` channel and a thread: fine
for a demonstration, far too heavy for a real timer. Runtimes use a timer wheel or heap with no thread per timer.

### 6. CPU / OS

`poll` is a plain function call. Nothing in the `Future` trait touches the OS: no syscall, no thread, no scheduler. That's
why the same trait works on a microcontroller. The OS appears only at the edges:

- **Where waiting really happens:** in the executor, when it has nothing to poll. It parks the thread (`futex` on
  Linux), or blocks in `epoll_wait` (Chapter 12.5).
- **Where wakes cross threads:** when a timer thread or another worker calls `wake()`, the executor's thread must be
  unparked, which is a syscall if it's asleep. A wake from within the same thread (like `Countdown`'s self-wake) is just
  a push onto a run queue.
- **What a `Pending` costs:** a return from a function, plus whatever the future did to register interest (a mutex, a
  waker clone). Compare with a thread blocking on a condition variable: a syscall, a context switch out, and one back in
  later.

---

## Pass 3 · Architect level — *Pull vs push, and what it means for systems*

### 7. Trade-offs

| Decision in Rust's design | Buys | Costs |
|---|---|---|
| Lazy futures | No work without a consumer; futures are cheap to build and drop | Forgetting `.await` silently skips work (hence the lint) |
| Pull (`poll`) instead of push (callbacks) | No callback allocation or captured environment per step; natural backpressure (nothing is produced until someone asks); cancellation is `drop` | Someone must drive the future: you need an executor |
| Waker passed per poll | A future can move between tasks and executors | Hand-written futures must refresh the waker: the rule no compiler checks |
| Cancellation = drop | Immediate, zero-cost, no tokens to thread through | Every `.await` is a place your function may stop forever ("cancellation safety") |
| No error channel | `Result` in `Output`, one error model (Part VIII) | — |

When you write a future by hand, you take on the whole contract. That's why production code writes `async` blocks and
reserves hand-written `Future` impls for the leaves: the socket read, the timer, the channel, the primitive that knows
where to register a waker. Everything above the leaves is `async fn` (Chapter 12.3), which gets the waker plumbing
right by construction.

### 8. Java comparison

`CompletableFuture` (JDK 8) solves the same problem with the opposite choices:

```java
// Java: eager and push-based. supplyAsync starts the work NOW, on a pool thread.
CompletableFuture<Long> charge = CompletableFuture.supplyAsync(() -> processor.charge(500), ioPool);
CompletableFuture<Receipt> receipt = charge
    .thenApply(id -> ledger.record(id))           // callback registered, runs when charge completes
    .orTimeout(2, TimeUnit.SECONDS);               // completes exceptionally after 2 s
// Nobody has to "run" it: pool threads push results through the chain.
```

| | `CompletableFuture` | Rust `Future` |
|---|---|---|
| Starts | When created (`supplyAsync` submits to a pool) | When first polled |
| Progress | Pushed: completing thread runs the callbacks | Pulled: the executor calls `poll` |
| Per stage | A new `CompletableFuture` object + a callback object | Nothing: combinators are fields of one struct |
| Where code runs | Whichever thread completes the previous stage, or the given executor | The task's executor thread |
| Cancellation | `cancel(true)` marks it cancelled; the running computation **isn't interrupted** | Drop it: nothing after the current `.await` runs |
| Timeout | `orTimeout` completes the future exceptionally; the underlying work continues | `select` with a timer drops the work's future |
| Blocking to get a result | `join()`/`get()` from any thread | `block_on`, never from inside async code (Chapter 12.6) |

The cancellation row is the one that bites Java teams: a `CompletableFuture` that timed out has still charged the card
if `processor.charge` finished later. In Rust, a timed-out future that is dropped before its charge call completes does
stop, at its current `.await`, but that's not automatically *better*: if the charge request was already on the wire,
you've abandoned the answer, not the charge. The ambiguity is the network's (Chapter 8.4), and both languages need
idempotency keys for it.

> **Analogy limit.** "A Rust future is a lazy `CompletableFuture`" misses the executor. A `CompletableFuture` is a
> *container* that a result is pushed into; it doesn't do any work itself. A Rust future *is* the work, as a state
> machine, and does nothing unless someone polls it. The closest Java analogue to `poll` is a hand-written NIO state
> machine's `handleRead()` method, which is what Rust futures replace.

### 9. Production scenario

**Coalescing duplicate charges in payments-core.** Chapter 8.4 established payments-core's idempotency keys: a retried
`POST /charges` with the same key returns the stored result, and a duplicate that arrives *while the first is still in
flight* got a `409 PAY_IDEMPOTENCY_CONFLICT` (8.4: "return 409, or wait"). Clients handled it by retrying with
backoff, which added seconds to checkout during processor slowdowns. In 2026 the team changed the in-flight case: a duplicate that reaches the same instance now
**waits for the first attempt's result** (bounded by its own request deadline) instead of answering 409.

The waiting is a hand-written leaf future, `WaitFor`, from listing `ch02-07-inflight-waiters.rs`. Each waiting request
leaves its current waker in the slot; the first request, when done, stores the result and wakes them all, after
releasing the lock:

```rust,ignore
impl Future for WaitFor {
    type Output = String;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<String> {
        let mut slot = self.0.lock().unwrap();
        if let Some(r) = &slot.result {
            return Poll::Ready(r.clone());
        }
        if !slot.waiters.iter().any(|w| w.will_wake(cx.waker())) {
            slot.waiters.push(cx.waker().clone());
        }
        Poll::Pending
    }
}
```

Four concurrent requests with one idempotency key:

```text
request 1: did the work, ch_1
request 2: duplicate, got ch_1
request 3: duplicate, got ch_1
request 4: duplicate, got ch_1
processor calls: 1
```

The design review's two rules were both about the contract. The registry's lock is **never held across an `.await`**
(it's a `std::sync::Mutex` held for microseconds, Chapter 12.3 explains why that matters), and wakers are **woken after
the lock is released**, because a waker may run arbitrary executor code. Duplicates that land on *another* instance still
see the stored in-progress row and get the 409: coalescing is an optimization, and the idempotency table remains the
source of truth.

### 10. Failure scenario

**The acknowledgement that never woke anyone.** merchant-notify's first Rust prototype (March 2026) waited for each
merchant's ACK frame with a hand-written `AckWait` future. The prototype awaited it inside a per-connection `select!`
alongside the heartbeat timer, and it worked. A refactor then moved each delivery into its own spawned task so that one
slow merchant couldn't delay another's heartbeats. Afterwards, about 3% of deliveries showed an ACK latency of exactly
30 seconds: the client-side resend timeout.

The cause was `RegistersOnce`, exactly as in `ch02-04`. `AckWait` stored a waker on its first poll and never replaced it.
After the refactor, the first poll still happened in the connection task (which created the future before spawning the
delivery task), so the ACK woke the *connection* task. The delivery task that owned the future was never polled again
until the 30 s timeout on the other side forced a resend. Nothing crashed, no error was logged, and the p50 looked fine.

The fixes, in the order the team applied them:

1. `AckWait` refreshes its waker on every poll (`will_wake`, as in `RefreshesEveryPoll`).
2. A test that polls the future under one waker and then completes it under another, which is listing `ch02-04` turned
   into a regression test. The Part XII review's fixed code carries it as `ack_reaches_the_task_that_polls_last`.
3. A review rule: hand-written `Future` impls are allowed only for leaf primitives, need a second reviewer, and must
   include that two-waker test.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XII).*

1. Why does a Rust future not execute immediately when created? What would change if futures were eager?
2. State the obligation that comes with returning `Poll::Pending`. What happens if it's violated, and why is there no
   error?
3. Why is the waker passed to every `poll` call instead of being given to the future once? Give a scenario where the
   waker changes.
4. What does `Waker::will_wake` compare, and why is it useful?
5. Explain "cancellation = drop." What runs when a future is cancelled, and what doesn't?
6. Why does `join` of a 30 ms and a 20 ms future take 30 ms on a single thread?
7. Compare `CompletableFuture.cancel(true)` with dropping a Rust future.
8. Why is `poll` required not to block? What does "block" include besides I/O?

### 12. Exercises

- **Beginner.** Remove `cx.waker().wake_by_ref()` from `Countdown` in `ch02-03`. What happens under `block_on`? Why?
- **Intermediate.** Write a `Yield` future that returns `Pending` exactly once (waking itself) and then `Ready`. Use it to
  make two long-running loops, joined in one task, take turns. What does it tell you about fairness in cooperative
  scheduling?
- **Advanced.** Implement `Select2` without the `Unpin` bounds, returning the unfinished future along with the result
  (like `futures::future::select`). You'll need Chapter 12.4's pin projection; check your version under Miri.
- **Systems.** Build `Delay` without a thread per timer: one timer thread that holds a `BinaryHeap` of deadlines and
  wakers. (Compare with Chapter 12.5's `ch05-03-executor.rs` afterward.) What's the cost of registering and cancelling a
  timer in your design?
- **Architecture.** List every hand-written `Future` impl in a codebase you know (or in a crate like `tokio::sync`).
  For each, identify where the waker is stored, who wakes it, and what happens on drop.

### 13. Debugging exercise

A team's cache-refresh future sometimes completes instantly and sometimes never:

```rust,ignore
impl Future for Refresh {
    type Output = Snapshot;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Snapshot> {
        if let Some(s) = self.shared.result.lock().unwrap().take() {
            return Poll::Ready(s);
        }
        // the loader thread sets `result`, then wakes whatever is in `waker`
        *self.shared.waker.lock().unwrap() = Some(cx.waker().clone());
        Poll::Pending
    }
}
```

1. There's a window in which the loader can finish and nobody is woken. Draw the interleaving.
2. Fix it without holding two locks at once. (Hint: after publishing the waker, what must you check again?)
3. The same future is used by several tasks at once through `Arc<Refresh>` clones. What else breaks?

### 14. Design exercise

**A per-merchant rate-limited sender.** merchant-notify must send at most 50 events per second to each merchant, and
thousands of tasks may want to send to the same merchant at once. Design a leaf future `Permit` (acquired with
`limiter.acquire(merchant).await`):

- Where do waiting tasks' wakers live, and who wakes them when a token becomes available?
- What happens when a waiting `Permit` future is dropped (the task was cancelled)? Does it hold a place in the queue?
  Does a token get lost?
- How do you avoid waking all waiters when one token frees up (a thundering herd)?

Compare your design with `tokio::sync::Semaphore` when you reach Part XIII.
