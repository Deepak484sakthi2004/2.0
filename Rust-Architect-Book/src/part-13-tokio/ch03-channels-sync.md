# Chapter 13.3 — Async Channels and Synchronization

> **Where this sits:** Part XIII · Tokio and Production Async · chapter 3 of 5
> **Prerequisites:** Chapters 13.1–13.2. Chapter 11.3 (`Mutex`, `Condvar`), 11.5 (std and crossbeam channels, bounded
> vs unbounded), 12.2 (wakers and the lost-wakeup problem), 12.4 (waiter lists).
> **After this chapter you can:** pick the right Tokio primitive for each communication shape (oneshot, mpsc,
> broadcast, watch, Semaphore, Notify, Mutex); use a bounded channel as a backpressure device and choose between
> `send().await`, `try_send`, and `reserve`; build an actor that owns state instead of locking it; decide between
> `std::sync::Mutex` and `tokio::sync::Mutex` with numbers; and recognize the convoy and lost-wakeup failures before they
> ship.

---

## Pass 1 · User level — *Tasks that talk*

### 1. Problem

Chapter 11 gave you thread-level tools: `Mutex`, `Condvar`, `std::sync::mpsc`, crossbeam channels. They block the
calling **thread** while they wait. In async code that's the blocking mistake of Chapter 13.2: a task waiting on a
`Condvar` holds a worker hostage. Tokio's `sync` module offers the same shapes with one difference: waiting **parks the
task** (returns `Pending` and registers a waker) instead of the thread.

Having async versions isn't the hard part. The hard parts are choosing among nine primitives, sizing the bounded ones,
and knowing which classic concurrency bugs survive the move to async. Two of them survive intact: holding a lock across
a slow operation (now "across an `.await`"), and checking a condition before waiting for a notification that already
happened.

### 2. Mental model

Choose by the **shape of the communication**, not by habit:

```text
 SHAPE                                   PRIMITIVE                     WHAT WAITS               BOUNDED BY
 ─────                                   ─────────                     ──────────               ──────────
 one value, once, one receiver           oneshot                       the receiver             1
 a stream of messages, many senders →    mpsc::channel(n)              senders when full;       n messages
   one receiver                                                        the receiver when empty
 every message to every receiver         broadcast::channel(n)         receivers when empty;    n (slow receivers
                                                                       senders never wait         lose messages)
 "the latest value" to many readers      watch::channel(v)             readers until it changes 1 value
 at most n things at once                Semaphore::new(n)             acquirers                n permits
 "something happened", no data           Notify                        waiters                  1 stored permit
 exclusive access across an .await       tokio::sync::Mutex / RwLock   lockers                  1 holder
 N tasks reach a point together          Barrier                       arrivers                 N
```

Two rules frame the chapter:

1. **A bounded channel is a backpressure device, not just a pipe.** Its capacity decides who waits when the consumer is
   slow: the producer (good: the pressure propagates upstream) or nobody (unbounded: the queue absorbs it until memory
   runs out, Chapter 11.5 §10).
2. **Prefer ownership to locking.** If one task owns the state and others send it requests, there's no lock to hold
   across an `.await`, and no lock at all. That's the actor pattern (§3), and it's the async version of Chapter 11.5's
   owner thread.

### 3. Rust code

**A bounded channel in action.** Listing `ch03-01-mpsc-backpressure.rs` pairs a producer that could send instantly with a
consumer that needs 10 ms per item, through a channel of capacity 4. The clock is paused (Tokio's `test-util`
feature: timers advance virtually when every task is waiting), so the timestamps are exact:

```text
send() of items 0..10 completed at [0, 0, 0, 0, 0, 10, 20, 30, 40, 50] ms
consumer received [0, 1, 2, 3, 4, 5, 6, 7, 8, 9], finished at 100 ms
try_send("a"): queued
try_send("b"): queued
try_send("c"): Full, value handed back
after reserve + send: queue holds 2 of 2
send after the receiver is gone: Err(SendError("e"))
```

- The first **five** sends complete at once (four fill the queue, and the consumer had already taken one), and from then
  on `send().await` completes every 10 ms: the producer now runs **at the consumer's pace**. That's backpressure,
  delivered by a task waiting, with no thread blocked.
- `try_send` never waits. When the queue is full it returns `Full(value)` and hands the value back, so the caller decides
  what to do: drop it and count it, spill it somewhere durable, or reply "busy".
- `reserve().await` waits for a slot **before** you commit the value. With `send(v).await`, if the task is cancelled while
  waiting, the value is dropped with the future (Chapter 13.4). With `reserve`, the value is only moved once the slot is
  already yours: `permit.send(v)` can't fail and doesn't wait.
- When the receiver is dropped, `send` fails and returns the value. When every sender is dropped, `recv()` returns
  `None` after the queue drains. Those two signals are how pipelines shut down cleanly.

**The actor: a task that owns state.** Listing `ch03-02-actor.rs` implements a merchant limits service. One task owns the
`HashMap` of remaining limits, and callers talk to it through a cloneable handle:

```rust,ignore
enum Command {
    Reserve { merchant: u32, cents: i64, reply: oneshot::Sender<Result<i64, String>> },
    Balance { merchant: u32, reply: oneshot::Sender<i64> },
}

/// The only owner of `limits`: runs until every handle is dropped.
async fn limits_actor(mut inbox: mpsc::Receiver<Command>, mut limits: HashMap<u32, i64>) {
    while let Some(cmd) = inbox.recv().await {
        match cmd {
            Command::Reserve { merchant, cents, reply } => {
                assert!(cents >= 0, "negative reservation reached the actor"); // a bug: kills the actor
                // ... check and subtract, then:
                let _ = reply.send(result); // the caller may have given up: that's fine
            }
            // ...
        }
    }
}

impl LimitsHandle {
    async fn reserve(&self, merchant: u32, cents: i64) -> Result<i64, String> {
        let (reply, rx) = oneshot::channel();
        self.0.send(Command::Reserve { merchant, cents, reply }).await.map_err(|_| "limits actor is gone".to_string())?;
        rx.await.map_err(|_| "limits actor died before replying".to_string())?
    }
}
```

(Excerpt; the full listing compiles and runs.) A hundred concurrent reservations of 150 cents against a 10,000-cent
limit:

```text
reservations granted: 66 of 100; balance now Ok(100)
negative reservation: Err("limits actor died before replying")
next call:            Err("limits actor is gone")
actor task: panicked = true
```

Exactly 66 succeed (66 × 150 = 9,900; a 67th would overdraw), with no lock anywhere: the actor processes one command at
a time, so the check and the subtraction can't interleave. And the failure behavior is **visible**: when a bug panics the
actor, the caller in flight sees "died before replying" (its `oneshot::Sender` was dropped), the next caller sees
"gone" (the `mpsc::Receiver` was dropped), and the actor's `JoinHandle` reports the panic. Nobody hangs.

---

## Pass 2 · Systems level — *How the primitives are built*

### 4. Under the hood

The descriptions below are Tokio 1.53.1 implementation details [LIB], useful for predicting costs, not guarantees.

**`mpsc` is a semaphore plus a linked list of blocks.** A bounded `mpsc::channel(n)` has a semaphore with `n` permits.
`send` acquires a permit (waiting in a FIFO queue of senders if none are free), then pushes the value into a lock-free
list of fixed-size blocks. `recv` pops a value and releases a permit, which wakes the first waiting sender. Two
consequences follow. Waiting senders are served **in order**, so a burst can't starve an early sender. And the
channel's memory tracks its **contents**, not its capacity (§5).

**`oneshot`** is one allocation holding a state word and a slot for the value. `send` writes the value and wakes the
receiver; dropping either side sets a "closed" bit the other side can observe. It's the reply channel of every
request/response actor.

**`broadcast`** is a ring buffer of `n` slots, each with a count of receivers that haven't read it yet. A send
overwrites the oldest slot **whether or not every receiver has read it**: senders never wait. A receiver that falls more
than `n` messages behind finds its next message overwritten and gets `Lagged(k)`, then continues from the oldest message
still in the buffer. Listing `ch03-03-broadcast-watch.rs` (capacity 4, a slow receiver that doesn't read until 10
messages were sent):

```text
fast receiver: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
slow receiver: ["Lagged(6)", "6", "7", "8", "9"]
watch receiver after 4 updates sees: ("limits-v5", 500)
changed since then? false
```

The slow receiver lost messages 0–5 and was told how many. That's the right semantics for fan-out to consumers you
can't let slow down the producer (dashboards, cache invalidations), and the wrong one for anything that must be
delivered: use one `mpsc` per consumer then, and decide explicitly what a full queue means.

**`watch`** holds one value behind a lock plus a version number. Senders replace the value and bump the version.
Receivers remember the version they last saw: `changed().await` completes when the version moves, and
`borrow_and_update()` reads the latest value and marks it seen. Intermediate values are skipped **by design**: the
receiver above saw `limits-v5`, never v2–v4. That's what configuration and "current state" need, and it's why `watch`
costs one value however many updates happen.

**`Notify`** is a waiter list plus one stored permit. Listing `ch03-05-notify.rs` runs four deterministic interleavings:

```text
1. notify_one, then wait:     Ok("woken")
2. notify_waiters, then wait: Err(Elapsed(()))
3. check, event, wait:        Err(Elapsed(()))   <- lost wakeup
4. enable, check, event, wait: Ok("woken")
```

`notify_one()` with nobody waiting **stores a permit**, so a later `notified().await` completes at once (case 1).
`notify_waiters()` wakes only tasks already waiting and stores nothing (case 2). Case 3 is the classic lost wakeup,
exactly Chapter 12.2's bug in library form: the waiter checks a flag (not ready), the event happens, `notify_waiters()`
finds nobody, and then the waiter goes to sleep, forever (here, until the timeout). Case 4 is the fix: create the
`Notified` future and call `enable()` **before** checking the condition, so the task is on the waiter list from that
point on and a notification in between is not lost:

```rust,ignore
let notified = n.notified();
tokio::pin!(notified);
notified.as_mut().enable(); // now on the waiter list
let checked = ready.load(Ordering::SeqCst);
```

This is the async version of Java's rule "call `wait()` inside a `synchronized` block that also checks the condition":
registering and checking must not have a gap between them.

**`Semaphore`** is the primitive under most of the others (`mpsc` capacity and `tokio::sync::Mutex` are both built on
Tokio's internal batch semaphore). Listing `ch03-06-semaphore.rs`:

```text
1. 12 x 100 ms through Semaphore(3): peak in flight 3, took 400ms
2. third try_acquire: NoPermits -> reply 'busy' now, don't queue
2. after one permit is dropped: available = 1
3. waiter after close(): Err("semaphore closed")
```

Three uses in three lines: a **concurrency limit** (12 calls, never more than 3 in flight, 4 waves of 100 ms);
**shedding** with `try_acquire`, which fails fast instead of queueing; and **shutdown** with `close()`, which fails
every current and future waiter. The permit is an RAII guard: dropping it (including when a task is cancelled or panics)
returns it. `acquire_owned()` returns a permit that owns an `Arc<Semaphore>`, so it can move into a spawned task.

**`tokio::sync::Mutex`** is a semaphore with one permit plus an `UnsafeCell<T>`. Its guard can be held across an
`.await` (it's `Send` when `T` is), which is the entire reason it exists. That capability is also its main hazard (§6).

### 5. Memory

**Capacity is a limit, not a preallocation, for `mpsc`.** Listing `ch03-07-channel-memory.rs` (counting allocator):

```text
channel(1_000_000) created:              800 bytes live
after 100 messages queued:              2688 bytes live
after 100,100 messages queued:        902752 bytes live (9.0 bytes per message)
after draining the queue:               2752 bytes live
broadcast::channel(1_000) created:     41096 bytes live
```

- A channel with a capacity of one million costs 800 bytes when empty. Memory grows with the **queued** messages (9
  bytes per `u64` at scale: the 8-byte value plus block overhead) and is released as blocks empty (a couple of blocks
  are kept for reuse). So a generous capacity is cheap in the normal case and exactly as expensive as its contents
  during an incident. Size capacity by **how much delay you're willing to hide** (Little's law: queued = arrival rate ×
  time in queue), not by memory.
- `broadcast` is the opposite: 41,096 bytes at creation for capacity 1,000. The ring buffer is allocated up front (1,024
  slots of 40 bytes: the capacity was rounded up to a power of two). A large `broadcast` capacity costs memory
  immediately, and a message stays in memory until the slowest receiver reads it or it's overwritten.
- An **unbounded** `mpsc` has no permits and no limit: it grows until the process is killed. Use it only where the
  producer is bounded by construction (a fixed number of messages per request, say), and say so in a comment.

**Where a waiting task lives.** A task waiting on `send().await` or `lock().await` is on the primitive's waiter list:
an intrusive linked list threaded through the waiting futures themselves (Chapter 12.4's pinned waiter nodes). Waiting
costs no allocation, and a million waiters cost a million futures, which you already paid for when you spawned the
tasks.

### 6. CPU / OS

**Everything here is user space.** Sending, receiving, locking, and notifying are atomic operations plus, when someone
was waiting, a waker call that pushes a task onto a run queue (Chapter 13.1 §4). The kernel is involved only if a worker
thread has to be unparked, and that costs a futex wake or an eventfd write.

**std `Mutex` vs Tokio `Mutex`, measured.** Listing `ch03-04-mutex-choice.rs`, part 1 (release, one run):

```text
uncontended lock+unlock: std::sync::Mutex 4ns, tokio::sync::Mutex 22ns (one run)
```

The Tokio mutex is ~5× slower uncontended: an async acquire goes through the semaphore's state machine and a future,
where the std mutex is one compare-and-swap. And under contention, `std::sync::Mutex` blocks the thread (a futex wait),
which is fine for a critical section of a few hundred nanoseconds and a disaster for one that includes an `.await`.
Tokio's documentation gives the rule, and the listing confirms it: **use `std::sync::Mutex` (or `parking_lot`) unless
you must hold the guard across an `.await`**, and try hard not to need that.

**The convoy.** Part 2 of the same listing runs 50 tasks that each update a cache entry after a 10 ms upstream call, on
a paused clock:

```text
A: 50 tasks, lock held across a 10 ms await: 500ms (virtual time)
B: 50 tasks, await first, then lock:       10ms (virtual time)
```

```rust,ignore
// Version A: hold the lock across the upstream call, "so two tasks don't both refresh".
let mut guard = cache.lock().await;
let rate = fetch_rate_from_upstream().await; // the lock is held across this .await
guard.insert(merchant, rate);

// Version B: await first, lock only to publish. A std Mutex is enough, since no guard crosses an .await.
let rate = fetch_rate_from_upstream().await;
cache.lock().unwrap().insert(merchant, rate); // guard dropped at the end of the statement
```

Version A turns 50 independent 10 ms calls into a **500 ms serial chain**: every task waits for the lock while the holder
waits for the network. Nothing blocks a thread, the CPU is idle, and throughput collapses to one call at a time. Version
B takes 10 ms. The lock protects the map, not the call. When the goal really is "only one refresh at a time" (a
thundering-herd guard), the fix is a **single-flight** structure: the first task starts the fetch and the others await
its result (a `watch` channel or a shared future per key), still without a lock held across the call.

---

## Pass 3 · Architect level — *Choosing, sizing, and failing well*

### 7. Trade-offs

**Choosing the primitive for shared state:**

| Option | Hold across `.await`? | Cost | Invariants across fields | Failure mode |
|---|---|---|---|---|
| `std::sync::Mutex` / `parking_lot::Mutex` | no (the future becomes `!Send`, and it would block a worker) | ~4 ns uncontended | yes, within one critical section | long critical sections block workers |
| `tokio::sync::Mutex` | yes | ~22 ns uncontended | yes | convoy when held across slow awaits |
| `RwLock` (either) | as above | as above | yes | writer starvation depends on the implementation |
| Actor (task + `mpsc` + `oneshot`) | the actor can await freely | ~2 channel hops per call (~0.3–1 µs) | yes, trivially: one owner | actor becomes a serial bottleneck; mailbox full |
| `watch` / `ArcSwap` snapshot | readers never lock | one atomic load | yes, per snapshot | writers must rebuild the whole value |
| Atomics | n/a | ~1–20 ns | only single-word | anything multi-word |

**Choosing how to send into a bounded channel:**

| Call | Waits when full? | Loses the value when cancelled while waiting? | Use for |
|---|---|---|---|
| `send(v).await` | yes | **yes** (the value is inside the dropped future) | producers that should slow down |
| `reserve().await` then `permit.send(v)` | yes, before `v` is committed | no | producers in a `select!` or with a timeout |
| `try_send(v)` | no: `Full(v)` | no (you get `v` back) | shedding, spilling, "busy" replies |
| `send_timeout(v, d)` | up to `d` | no (returned on timeout) | bounded waiting |

**Sizing a bounded channel.** Capacity = the delay you're willing to hide × the arrival rate. A queue in front of a
consumer that handles 1,000 msg/s with a 200 ms tolerance needs 200 slots. A capacity of 100,000 means "hide up to 100 s
of consumer trouble", and during those 100 s every message waits behind the backlog. Big queues convert overload into
latency (Chapter 13.5 measures what that costs).

> **Why not make every shared thing an actor?** Because an actor serializes. A limits actor that spends 2 µs per
> command caps out near 500K commands/s on one core, and every call pays two channel hops. For read-mostly state
> (configuration, routing tables), a `watch` or `ArcSwap` snapshot lets readers go wide with no coordination. For
> independent keys, **shard** the actors (one per key range) the way Chapter 11.7 sharded locks. Use a single actor when
> the invariant really spans all the state.

### 8. Java comparison

| Java | Tokio | Difference that matters |
|---|---|---|
| `ArrayBlockingQueue(n)` | `mpsc::channel(n)` | Java's `put` blocks a thread; Tokio's `send().await` parks a task |
| `LinkedBlockingQueue()` (default capacity `Integer.MAX_VALUE`) | `mpsc::unbounded_channel()` | both are "bounded by the heap"; Java's default constructor makes this the easy mistake |
| `CompletableFuture<T>` completed once | `oneshot` | a `oneshot` has exactly one consumer and knows when the other side is gone |
| Reactive Streams / `Flow` `request(n)` | bounded channel capacity, `Semaphore` | Flow makes demand explicit per subscriber; a Tokio channel encodes it in capacity |
| Akka / virtual-thread "actor" | task + `mpsc` + `oneshot` | same pattern; Tokio actors are ~100 bytes plus their state |
| `ReentrantLock` | `tokio::sync::Mutex` | Tokio's mutex is not reentrant: locking twice in one task deadlocks |
| `synchronized` + `wait`/`notifyAll` | `Notify` (+ state) | same lost-wakeup trap; `enable()` plays the role of "check under the monitor" |
| `java.util.concurrent.Semaphore` | `tokio::sync::Semaphore` | Tokio's is fair (FIFO) by construction; Java's is unfair by default |
| `CountDownLatch`, `CyclicBarrier` | `Barrier` | similar |
| a `volatile` reference to an immutable config object | `watch` / `ArcSwap` | both publish snapshots; `watch` also tells readers *that* it changed |

> **Analogy limit.** A Java `BlockingQueue` producer that blocks holds a thread, so backpressure costs a thread per
> waiting producer, and it can't be cancelled without interruption. A Tokio producer waiting on `send().await` holds
> only its future, and dropping that future cancels the wait instantly. But it also **drops the message** unless you
> used `reserve`. Java code never had to think about "cancelled while waiting to enqueue". Async code must.

### 9. Production scenario

**The risk-limits service goes async.** Chapter 1.2 introduced Meridian's risk-limits service (100K ops/s, p99 < 1 ms, and
the invariant that no merchant ever exceeds its limit), and Chapter 11.7 §10 described the shard-lock incident in its
threaded version. In 2026 the team moved it onto Tokio, and the design review chose actors:

- **16 shard actors**, each owning the limits for a key range of merchants (merchant ID hash), each with a mailbox of
  1,024 commands. The no-breach invariant is enforced by construction: one owner per merchant, one command at a time.
  There's no lock to hold across the audit write that caused the 11.7 incident. The actor writes its audit records to
  its own `mpsc` feeding a single audit writer, and the check never waits for it.
- **Callers use `try_send`, not `send().await`.** A full mailbox (1,024 commands at ~1 µs each is about 1 ms of backlog,
  which is the whole latency budget) means the shard is overloaded, so the call fails at once with a "limits
  unavailable" error, and payments fail closed. Waiting would only turn overload into timeouts.
- **Configuration through `watch`.** Limit policies are published as `watch::Sender<Arc<Policies>>`. Each actor holds a
  receiver and calls `borrow_and_update()` at the top of its loop: a policy update reaches every shard within one
  command, and intermediate versions during a rapid rollout are skipped, as listing `ch03-03` shows.
- **A panic kills one shard, visibly.** The supervisor task holds the 16 `JoinHandle`s in a `JoinSet` (Chapter 13.4). A
  panicked shard is restarted from the last durable snapshot, and callers of that shard get errors, not hangs, for the
  restart window (listing `ch03-02`'s "gone" and "died before replying").

Load test result the team reported: 100K ops/s used about 1.6 cores across the 16 shards, with p99 at 0.4 ms, the same
as the threaded version's best case, and without the tail from lock contention.

### 10. Failure scenario

**The FX-rate convoy (payments-core, 2026, fictional).** `payments-core` converts amounts for cross-currency charges
using an FX rate cache: `tokio::sync::Mutex<HashMap<CurrencyPair, Rate>>`. On a cache miss, the code held the lock while
it fetched the rate from the FX provider, "so two requests don't fetch the same pair at once":

```rust,ignore
// Failure-scenario sketch (not a verified listing; listing ch03-04 reproduces the effect)
let mut rates = self.rates.lock().await;
if let Some(r) = rates.get(&pair).filter(|r| r.fresh()) { return Ok(r.rate); }
let fresh = self.fx_client.fetch(pair).await?;   // ~10 ms normally
rates.insert(pair, fresh);
```

Rates expire after 60 s, so misses happen every minute for each active pair. One morning the FX provider slowed from
10 ms to ~800 ms per call. Every cross-currency charge now waited behind whichever request held the lock, for
**any** pair: EUR→JPY waited for a USD→GBP refresh. Listing `ch03-04` is this incident at small scale (500 ms vs 10 ms).
p99 for cross-currency charges went from 12 ms to over 4 s, and the gateway's 5 s timeout started returning 504s. CPU was
idle, no thread was blocked, and the dashboard's "blocked threads" panel showed nothing. That sent the on-call engineer
looking in the wrong place for 40 minutes.

Fixes:

1. **Never hold a lock across a network call.** The map moved to `std::sync::Mutex`, locked only to read and to insert.
2. **Single-flight per pair**, not per map: a miss inserts a `watch::Receiver` for that pair's pending fetch, so
   concurrent misses on the same pair share one fetch and other pairs never wait.
3. **Serve stale on slow refresh.** A rate up to 5 minutes old is acceptable for authorization (settlement re-rates), so
   an expired entry is returned while one refresh runs in the background.
4. **A review rule**: a `tokio::sync::Mutex` guard held across an `.await` requires a comment naming the awaited
   operation and its worst-case latency. (Clippy's `await_holding_lock` catches the std-mutex version of this; for the
   Tokio mutex, holding across `.await` is legal by design, so the check is a review rule.)

---

## Practice

### 11. Interview & architecture questions

1. Map each of these to a Tokio primitive and justify it: a request's reply; a stream of audit records; live
   configuration; "at most 20 calls to the processor"; invalidation events to 50 cache instances.
2. What happens to a message when a task waiting in `send(msg).await` is cancelled? How does `reserve()` change that?
3. Explain `Lagged(6)` in listing `ch03-03`. When is this behavior exactly what you want?
4. Why does a `watch` receiver see only the latest value? Give a case where that's a bug.
5. Walk through the lost-wakeup interleaving with `Notify` and explain why `enable()` fixes it.
6. When is `tokio::sync::Mutex` the right choice over `std::sync::Mutex`? Quote the costs from listing `ch03-04`.
7. Why is holding a Tokio mutex across a network call a throughput bug even though no thread blocks?
8. An `mpsc` channel has a capacity of one million. What does that cost when empty, when full, and in latency?
9. Compare an actor with a sharded lock for a per-merchant counter at 500K updates/s.
10. How do you shut down a pipeline of three stages connected by bounded channels, without losing messages?

### 12. Exercises

- **Beginner.** Add a `Refund { merchant, cents, reply }` command to listing `ch03-02`'s actor, and a test that 100
  concurrent reservations plus 50 refunds leave the right balance.
- **Intermediate.** Rewrite listing `ch03-01`'s producer to use `reserve()` inside a `tokio::select!` with a 25 ms
  deadline per item. Show that no item is lost when the deadline fires.
- **Advanced.** Implement a single-flight cache: `get_or_fetch(key, fetch_fn)` where concurrent misses on the same key
  share one fetch (via `watch` or `Shared` futures) and misses on different keys run in parallel. Test it on a paused
  clock with 50 tasks and 5 keys: the total should be one fetch per key.
- **Systems.** Measure the throughput of listing `ch03-02`'s actor (commands/s) on the current-thread and multi-thread
  runtimes, and with 1, 4, and 16 sharded actors. Explain the curve.
- **Architecture.** Design the channel topology for `merchant-notify`'s fan-out: one payment event must reach every
  connected dashboard of that merchant (1–200 connections), and a slow dashboard must never slow the others. Choose
  primitives, capacities, and what happens to a connection that falls behind.

### 13. Debugging exercise

A config reloader and a request handler share a flag. Under load, some requests use the old config for up to 30 s
after a reload (the reloader runs every 30 s):

```rust,ignore
// Debugging exercise: find the defect (not a verified listing)
struct Reloadable { cfg: RwLock<Arc<Config>>, changed: Notify, dirty: AtomicBool }

async fn wait_for_reload(r: &Reloadable) {
    if !r.dirty.load(Ordering::Acquire) {
        r.changed.notified().await;
    }
    r.dirty.store(false, Ordering::Release);
}

fn reload(r: &Reloadable, new: Config) {
    *r.cfg.write().unwrap() = Arc::new(new);
    r.dirty.store(true, Ordering::Release);
    r.changed.notify_waiters();
}
```

Find the race, show the interleaving that loses a reload, and give two fixes: one with `Notify` and one that replaces the
whole structure with a single primitive from this chapter. Model answer in Appendix A.

### 14. Design exercise

Meridian's **ledger event bus** (inside the Java ledger's Rust sidecar) receives ~3K ledger postings per second (the
ledger's TPS, Chapter 1.2) and must deliver each posting to three consumers: the settlement batcher (must see every
posting, can pause for a 30 s deploy), the fraud feature store (may skip postings when it's slow, but must know it
skipped), and the balance cache (only needs each account's latest balance).

Choose a primitive for each consumer, size every bounded structure with Little's law, specify what happens when each
consumer is slow or restarting, and describe how you'd test "settlement never loses a posting" on a paused clock.
