# Part XII Review — The merchant-notify PR & Interview Mode

> Consolidate Part XII, then use it: review an async pull request that compiles, passes its happy-path test, and
> contains more than a dozen defects, including a hang, a use-after-free waiting to happen, and a timeout that never
> fires. Then answer senior-level questions without notes. Answers are in **Appendix A, Part XII**.

---

## Part XII on one page

```text
 WHY           thread per connection: 2 MiB virtual + ~10 KiB resident + a kernel task each; limits bite first
               readiness (epoll): register once, get back only what's ready; edge-triggered = drain to WouldBlock
               Rust: no runtime in std (RFC 230); Future in core; executors, reactors, timers are libraries
        │
 CONTRACT      poll(self: Pin<&mut Self>, cx) -> Ready(T) | Pending
               Pending = "I arranged for THE CURRENT cx.waker() to be woken": no compiler checks it
               lazy, pulled, cancellation = drop (nothing after the current .await runs; destructors do)
        │
 COMPILER      async fn = constructor of a coroutine; one state per .await; saved = what's live across it
               size = largest state (+ prefix, tag): 1 B for nothing, 1,026 B for a 1 KiB array across .await
               Send iff everything stored across .await is Send (Rc, MutexGuard, Box<dyn Error> are not)
               recursion needs Box::pin; async fn in traits: static only, or -> impl Future + Send, or boxed
        │
 PIN           moving = memcpy; self-references dangle; Pin withholds &mut T from safe code (no runtime cost)
               drop guarantee: pinned memory isn't reused before drop → intrusive waiter lists, zero allocation
               Unpin is a SAFE trait: one wrong `impl Unpin` breaks an unsafe projection elsewhere
        │
 RUNTIME       waker = 16 B (data + vtable); executor = run queue + dedup flag; reactor = epoll; timer = heap/wheel
               a task: ~2 allocations, ~160 ns to spawn; a thread: ~34 µs; round trip 0.12 µs vs 7.7–9.6 µs
               blocking or computing between .awaits stalls every task on that worker (194 ms / 143 ms measured)
        │
 CHOOSING      state lives in: a stack (threads, goroutines, virtual threads) or a future (Rust async)
               coloring, cooperative scheduling, cancellation at .await: the price of no runtime
               pick by where the waiting is and who owns the code that waits
```

## Ten ideas to carry forward

1. **Async is for waiting, not for speed.** It pays off when there are many more waiting units than cores. For
   CPU-bound work, or a few hundred connections, threads are simpler and just as fast.
2. **`async fn` is a constructor.** Calling it runs nothing; the body runs inside `poll`. Forgetting `.await` means the
   work never happens.
3. **`Pending` is a promise with no enforcement.** Wake the waker from the most recent poll, publish it atomically with
   the condition check, and wake after releasing locks.
4. **A future's size is a budget.** Everything live across an `.await` is stored in it, and every instance pays for its
   largest state. Put buffers on the heap and test the size in CI.
5. **`Send` is decided by what crosses an `.await`.** Keep guards, `Rc`, and non-`Send` errors inside a scope that ends
   before the suspension.
6. **Every `.await` is a place your function may stop forever.** Anything that must be undone if it stops is owned by a
   guard; anything that can't be undone synchronously needs idempotency and reconciliation.
7. **`Pin` is a promise made by whoever pins and relied on by the type's own code.** Use `pin!` and `Box::pin`; generate
   projections with `pin-project-lite`; treat a hand-written `impl Unpin` next to `unsafe` as a bug until proven
   otherwise.
8. **An executor is policy.** Run queue, wake deduplication, where the thread sleeps, timers, and fairness are choices
   with measurable costs; that's why a deterministic simulation executor can replace the real one in tests.
9. **Nobody preempts an async task.** Blocking calls and long computations belong on `spawn_blocking`, a rayon pool, or
   in chunks separated by `yield_now`.
10. **Choose the model per workload.** Threads, Rust async, Go, Java virtual threads, and Erlang each put the suspended
    state somewhere different, and that one choice decides memory, switch cost, blocking, coloring, and cancellation.

---

## Capstone: review the merchant-notify PR

merchant-notify pushes payment events to merchant terminals and dashboards (Chapter 12.1), and the protocol needs an
ACK for every event: at-least-once delivery, with the terminal de-duplicating by `event_id`. A teammate submits PR #412,
the delivery path (listing `review-01-notify-pr.rs`; verified to compile as a library crate). On the happy path, with
one task and a fast merchant, it works.

```rust,ignore
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

pub struct Delivery {
    pub merchant: u64,
    pub event_id: u64,
    pub payload: Vec<u8>,
}

#[derive(Default)]
pub struct AckState {
    acked: bool,
    waker: Option<Waker>,
}

#[derive(Default)]
pub struct AckTracker {
    pending: Mutex<HashMap<u64, Arc<Mutex<AckState>>>>,
}

pub struct AckWait(Arc<Mutex<AckState>>);

impl Future for AckWait {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut st = self.0.lock().unwrap();
        if st.acked {
            return Poll::Ready(());
        }
        if st.waker.is_none() {
            st.waker = Some(cx.waker().clone());
        }
        Poll::Pending
    }
}

impl AckTracker {
    /// Called by the connection reader when a merchant's ACK frame arrives.
    pub fn ack(&self, event_id: u64) {
        let pending = self.pending.lock().unwrap();
        if let Some(st) = pending.get(&event_id) {
            let mut st = st.lock().unwrap();
            st.acked = true;
            if let Some(w) = &st.waker {
                w.wake_by_ref();
            }
        }
    }

    pub fn wait(&self, event_id: u64) -> AckWait {
        let st = Arc::new(Mutex::new(AckState::default()));
        self.pending.lock().unwrap().insert(event_id, st.clone());
        AckWait(st)
    }
}

pub trait Connection {
    fn send(&mut self, frame: &[u8]) -> impl Future<Output = std::io::Result<()>>;
}

pub struct Notifier {
    pub tracker: Arc<AckTracker>,
    pub delivered: Mutex<Vec<u64>>,
}

impl Notifier {
    pub async fn deliver<C: Connection>(&self, conn: &mut C, d: Delivery) -> bool {
        let mut frame = [0u8; 65536];
        let n = encode(&d, &mut frame);
        let mut delivered = self.delivered.lock().unwrap();
        delivered.push(d.event_id);
        for attempt in 0..3 {
            if conn.send(&frame[..n]).await.is_ok() {
                let ack = self.tracker.wait(d.event_id);
                return with_timeout(ack, Duration::from_secs(5)).await;
            }
            std::thread::sleep(Duration::from_millis(100 << attempt));
        }
        false
    }
}

pub struct Timeout<F> {
    inner: F,
    deadline: Instant,
}

impl<F> Unpin for Timeout<F> {}

impl<F: Future> Future for Timeout<F> {
    type Output = Option<F::Output>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<F::Output>> {
        if Instant::now() >= self.deadline {
            return Poll::Ready(None);
        }
        // SAFETY: `inner` is never moved.
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        inner.poll(cx).map(Some)
    }
}

async fn with_timeout<F: Future>(f: F, d: Duration) -> bool {
    Timeout { inner: f, deadline: Instant::now() + d }.await.is_some()
}

fn encode(d: &Delivery, out: &mut [u8]) -> usize {
    let header = format!("EVT {} {} {}\n", d.merchant, d.event_id, d.payload.len());
    out[..header.len()].copy_from_slice(header.as_bytes());
    out[header.len()..header.len() + d.payload.len()].copy_from_slice(&d.payload);
    header.len() + d.payload.len()
}

/// For the admin API (called from its async handlers).
pub fn deliver_blocking<C: Connection>(n: &Notifier, conn: &mut C, d: Delivery) -> bool {
    futures::executor::block_on(n.deliver(conn, d))
}
```

**Your review.**

1. Find **at least twelve** defects. For each, name the Part XII chapter whose rule it breaks, and say what happens in
   production: which merchant notices, which dashboard shows it, or which on-call engineer gets paged.
2. Classify them: which ones **hang** a delivery, which **corrupt the record** of what was delivered, which **stall
   other tasks**, which are **memory** problems, and which are **undefined behavior waiting for a refactor**?
3. There's an **ordering** bug that no type fixes: the code starts waiting for the ACK only after the send completes.
   Walk through what happens when a fast terminal's ACK arrives first. What's the correct order, and what makes it
   correct?
4. Two defects would become **compile errors** the day someone spawns `deliver` on Tokio's multi-threaded runtime
   (`tokio::spawn` requires a `Send` future). Which two, and why doesn't the PR's own test notice them?
5. The timeout "works" in the PR's test. Explain why it may never fire in production, citing the `Future` contract.
6. Write the new signatures: `deliver`'s parameters and return type, the `Connection` trait, and whatever replaces
   `Timeout` and `thread::sleep` so the code can be tested without real time.

A redesign (listing `review-02-notify-fixed.rs`, verified) injects the clock, registers for the ACK before sending,
reuses a per-connection frame buffer, and records a delivery only after its ACK. Its demo delivers one event whose ACK
beats the send's own completion:

```text
deliver future: 136 bytes
Acked; delivered [41]; pending acks 0
```

and its five tests pass: `ack_reaches_the_task_that_polls_last` (the Chapter 12.2 regression test),
`no_ack_is_not_delivered_and_leaves_nothing_behind`, `retries_back_off_asynchronously` (asserting the exact backoff
sleeps on a fake clock), `cancelled_delivery_leaves_nothing_behind` (dropping the future mid-flight), and
`deliver_future_is_small_and_send` (a size budget and a `Send` check). Write your review first, then compare with the
redesign and the model answers. The redesign is one defensible answer, not the only one.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. What does `async fn f(x: u64) -> u64` desugar to? What does calling it do, what does `.await` expand to, and what
   decides the size of the value it returns?
2. State the `Future` contract, including the parts no compiler checks. Which violation hangs a task silently?
3. What does `Pin<&mut T>` prevent, how, and at what run-time cost? Why is every `async` future `!Unpin`?

### Compiler and runtime

4. Walk through a coroutine's MIR layout: states, saved locals, storage conflicts. Why can two sequential awaits share
   bytes?
5. What's in a `Waker`, and what must each of its four vtable functions do? Why must a wake of an already-queued task
   be a no-op?
6. How does an executor sleep when nothing is runnable, and how does another thread wake it?

### Performance

7. What does an idle async task cost in memory compared with an idle OS thread, virtual and resident? What does a spawn
   cost, and a switch?
8. A service's RSS is 8 GB at 100,000 connections. Where do you look first, and what's the usual cause?
9. One task hashes a password on the executor thread. What happens to the p99 of every other task, and how do you find
   the culprit?

### Architecture

10. When is async the wrong choice for a Rust service? Give two workloads where threads win.
11. Design cancellation safety into an `async fn` that reserves funds, calls a fraud service, and pays out.
12. Rust async vs Java virtual threads vs Go goroutines: compare where suspended state lives, preemption, blocking,
    coloring, and cancellation, and say which you'd choose for a million mostly idle WebSocket connections.

### Distributed systems

13. An ACK-based at-least-once delivery protocol: where must the sender register interest, when may it record
    "delivered," and what must the receiver do about duplicates?
14. How does deterministic simulation (seeded scheduling, virtual time) find races that unit tests and thread sanitizers
    miss? What must be abstracted to make a Tokio service simulatable?
15. A regional network blip reconnects 20,000 clients to one pod at once. Walk through what an async server must do in
    the first ten seconds to avoid a metastable failure.

---

## Looking ahead: Part XIII

Part XII built every piece of an async runtime by hand: futures, wakers, an executor, a timer, and a reactor, each
about 150 lines. Part XIII studies the production version. Tokio's multi-threaded scheduler (work stealing, the LIFO
slot, the cooperative budget), its I/O driver and timing wheel, spawning and `spawn_blocking`, async channels and
synchronization (`tokio::sync::Mutex`, `Notify`, `Semaphore`, the designs this Part's exercises sketched), structured
concurrency and cancellation, and backpressure. Its project is Ferrite v2: the key-value store from Project L4, served
over Tokio with a connection limit, per-connection backpressure, timeouts, and graceful shutdown. Most of the
merchant-notify lessons in this Part become its requirements.
