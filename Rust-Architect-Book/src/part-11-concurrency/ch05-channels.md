# Chapter 11.5 — Channels and Message Passing

> **Where this sits:** Part XI · Concurrency · chapter 5 of 7
> **Prerequisites:** Chapter 3.2 (moves), Chapter 11.1 (threads and hand-off cost), Chapter 11.2 (`Send`/`Sync`),
> Chapter 11.3 (the `Condvar` queue).
> **After this chapter you can:** move work and data between threads with a channel and explain what "moving" means
> there; end every consumer loop cleanly through disconnection; choose between unbounded, bounded, and rendezvous
> channels as an admission policy; build the owner-thread pattern; and predict a channel's cost per message and per
> byte of backlog from how it's built.

---

## Pass 1 · User level — *Moving values, not sharing them*

### 1. Problem

Chapters 11.3 and 11.4 made *shared* state safe: locks, atomics, published snapshots. The other way to structure a
concurrent program is to share nothing and **pass values from thread to thread**. Go's documentation turned this into a
slogan ("Do not communicate by sharing memory; instead, share memory by communicating", *Effective Go*). Java engineers
know the shape from `BlockingQueue`, from the queue inside every `ExecutorService`, and from actor frameworks.

The questions that matter in production are the same in every language, and most codebases answer them by accident:

- What happens to the **producer** when the consumer is slower: does it wait, fail, or keep going?
- Where does the **backlog** live, and how big may it get?
- How does a consumer know there will be **no more work**, and how does a producer learn the consumer is **gone**?
- Who **owns** a message once it's in the queue?

Rust's channels answer the last question in the type system, and they make the other three explicit choices. That's
what this chapter is about.

### 2. Mental model

A channel is **an ownership conveyor belt with two kinds of ends**:

```text
 producers (many)                     the channel                       consumer (one, for std)
 ────────────────                     ───────────                       ───────────────────────
 Sender<T> ─┐                                                           Receiver<T>
 Sender<T> ─┼── send(value) ──► [ slot │ slot │ slot │ ... ] ──► recv() ─► Ok(value)   value MOVES through
 Sender<T> ─┘   (clone to add          capacity: ∞ │ n │ 0               Err(RecvError) = "every Sender is gone
                 producers)                                                          and the queue is empty"

 drop(last Sender)    → the consumer drains what's queued, then its loop ends          (no poison pill needed)
 drop(the Receiver)   → send(v) returns Err(SendError(v)): you get your value back    (the consumer is gone)

 capacity ∞  (channel)          the producer never waits: overload becomes HEAP
 capacity n  (sync_channel(n))  the producer waits when n are queued: overload becomes BACKPRESSURE
 capacity 0  (sync_channel(0))  every send waits for a recv: a RENDEZVOUS (hand-to-hand)
```

Three rules follow:

1. **`send` takes the value by move** [LANG]. After `send`, the producer can't touch it: not by convention, but because
   the compiler rejects any later use. There's no shared mutable object to race on.
2. **Disconnection is the "end of stream" signal** [LIB]. The number of live `Sender`s and `Receiver`s is tracked, and
   the loop `for msg in rx` ends exactly when the last `Sender` is dropped and the queue is empty.
3. **Capacity is an admission policy, not a tuning knob.** Unbounded means "I've decided overload goes to memory".
   Bounded means "I've decided overload slows the producer, or is refused". Choosing between them is a design decision
   with a failure mode attached (§10).

### 3. Rust code

**Many producers, one consumer, and both kinds of disconnection** (listing `ch05-01-mpsc-basics.rs`, excerpt):

```rust,ignore
let (tx, rx) = mpsc::channel::<Event>();
for p in 0..3 {
    let tx = tx.clone(); // each producer owns a Sender
    thread::spawn(move || {
        for seq in 0..3 {
            tx.send(Event { producer: p, seq }).unwrap();
        }
    }); // this producer's Sender is dropped here
}
drop(tx); // main's own Sender: forget this and the loop below never ends

let mut per_producer = [0u32; 3];
for e in &rx {
    per_producer[e.producer as usize] += 1;
    let _ = e.seq;
}
```

```text
received per producer [3, 3, 3]; the loop ended because every Sender was dropped
try_recv now: Disconnected
send to a dropped receiver: Err(SendError("audit record #1")): the value comes back to you
recv_timeout on an idle channel: Timeout
try_recv on an idle channel: Empty
after the last Sender drops: Disconnected (not Timeout, not Empty)
```

Look at the three different "no message" answers: `Empty` (nothing *yet*), `Timeout` (nothing within the deadline), and
`Disconnected` (nothing *ever again*). A consumer that treats them the same either spins forever or quits early.

**Ownership moves with the message.** The producer can't keep a handle to what it sent (listing
`ch05-09-use-after-send.rs`):

```rust,compile_fail
use std::sync::mpsc;
use std::thread;

struct Batch {
    ids: Vec<u64>,
}

fn main() {
    let (tx, rx) = mpsc::channel::<Batch>();
    let consumer = thread::spawn(move || rx.recv().unwrap().ids.len());

    let mut batch = Batch { ids: vec![1, 2, 3] };
    tx.send(batch).unwrap();
    batch.ids.push(4); // the consumer owns the batch now
    println!("{}", consumer.join().unwrap());
}
```

```text
error[E0382]: borrow of moved value: `batch`
  --> src/main.rs:17:5
   |
15 |     let mut batch = Batch { ids: vec![1, 2, 3] };
   |         --------- move occurs because `batch` has type `Batch`, which does not implement the `Copy` trait
16 |     tx.send(batch).unwrap();
   |             ----- value moved here
17 |     batch.ids.push(4); // the consumer owns the batch now
   |     ^^^^^^^^^ value borrowed here after move
```

This is Chapter 3.2's move, nothing more. It's also the whole safety story of message passing: a Java producer that
keeps mutating an object after `queue.put(obj)` has a data race the compiler never sees.

**Bounded channels are backpressure.** With room for two items and a consumer that takes 20 ms per item, the producer
is forced to the consumer's pace (listing `ch05-02-sync-channel.rs`, one run):

```text
producer: send() returned at ms [0, 0, 0, 20, 40, 60]
try_send #1: Ok(())
try_send #2: Err(Full("b")): refused, and the value comes back
rendezvous: send() blocked until the receiver arrived (>= 30 ms: true); it got 42
```

The first three sends return at once: two fill the buffer, and the consumer has already taken one. From then on, each
`send` returns only when the consumer frees a slot, 20 ms apart. `try_send` is the non-blocking version: it refuses
instead of waiting, and it **hands the value back** in `Full(v)`, so the caller can decide what refusal means (drop,
count, spill to disk, fail the request). Capacity 0 is a rendezvous: `send` doesn't return until a receiver has the
value.

**std's receiver has exactly one owner.** Sharing it between workers doesn't compile (listing
`ch05-03-receiver-not-sync.rs`):

```rust,compile_fail
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

fn main() {
    let (tx, rx) = mpsc::channel::<u32>();
    let rx = Arc::new(rx);
    for _ in 0..3 {
        let rx = Arc::clone(&rx);
        thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                println!("job {job}");
            }
        });
    }
    tx.send(1).unwrap();
}
```

```text
error[E0277]: `std::sync::mpsc::Receiver<u32>` cannot be shared between threads safely
   --> src/main.rs:12:23
    |
 12 |           thread::spawn(move || {
    |  _________-------------_^
...
    |     = help: the trait `Sync` is not implemented for `std::sync::mpsc::Receiver<u32>`
    |     = note: required for `Arc<std::sync::mpsc::Receiver<u32>>` to implement `Send`
```

This is Chapter 11.2's reasoning applied to a library type: `Receiver` is `Send` (it can move to one thread) but not
`Sync` (it can't be *used* from several). The "mpsc" in the module name means multi-producer, **single**-consumer.
Wrapping it in `Arc<Mutex<Receiver>>` compiles and works, but every worker then serializes on the mutex just to wait.
The better answers are a channel designed for multiple consumers, or the `Condvar` queue from Chapter 11.3.

std *has* a multi-consumer channel, `std::sync::mpmc`, but on Rust 1.98 it's still unstable (listing
`ch05-08-mpmc-unstable.rs`: ``error[E0658]: use of unstable library feature `mpmc_channel` ``, tracking issue #126840)
[VERSION]. On stable Rust, multi-consumer means **crossbeam-channel**: `Sender` *and* `Receiver` both clone, and
`select!` waits on several channels at once (listing `ch05-04-crossbeam-select.rs`, excerpt):

```rust,ignore
fn worker(id: usize, jobs: Receiver<u32>, shutdown: Receiver<()>) -> (usize, u32, u32) {
    let heartbeat = tick(Duration::from_millis(5));
    let (mut done, mut beats) = (0, 0);
    loop {
        select! {
            recv(jobs) -> job => match job {
                Ok(j) => { std::hint::black_box(j); done += 1; }
                Err(_) => break, // every Sender dropped: no more work will ever come
            },
            recv(shutdown) -> _ => break, // a message OR disconnection: both mean stop
            recv(heartbeat) -> _ => beats += 1,
        }
    }
    (id, done, beats)
}
```

```text
worker 0: some jobs = true, heartbeats seen = true
worker 1: some jobs = true, heartbeats seen = true
worker 2: some jobs = true, heartbeats seen = true
jobs processed in total: 3000
```

The shutdown channel carries no messages at all. `main` simply drops its only `Sender`, and **disconnection wakes every
`select!` at once**. That's the broadcast cancellation signal that Chapter 11.1 said Rust has instead of
`Thread.interrupt()`. It's cooperative, typed, and it can't be lost.

**The owner-thread pattern ("actor").** Instead of guarding a `HashMap` with a lock, give it to one thread and send that
thread commands. Requests that need an answer carry their own one-shot reply channel (listing
`ch05-05-owner-thread.rs`, excerpt):

```rust,ignore
enum Command {
    Touch { id: u64, user: String },
    Get { id: u64, reply: SyncSender<Option<Session>> },
    Expire { max_hits: u32, reply: SyncSender<usize> },
}

pub fn get(&self, id: u64) -> Option<Session> {
    let (reply, answer) = mpsc::sync_channel(1); // a one-shot reply channel per request
    self.tx.send(Command::Get { id, reply }).expect("owner thread gone");
    answer.recv().expect("owner dropped the reply")
}

/// The owner: plain `&mut HashMap`, no synchronization inside. Returns the number of commands handled.
fn run(rx: Receiver<Command>) -> usize {
    let mut sessions: HashMap<u64, Session> = HashMap::new();
    let mut handled = 0;
    for cmd in rx {
        handled += 1;
        match cmd {
            Command::Touch { id, user } => sessions.entry(id).or_insert(Session { user, hits: 0 }).hits += 1,
            // ...
        }
    }
    handled // every SessionStore handle was dropped: the loop ends, the map is dropped here
}
```

```text
session 3: Some(Session { user: "user3", hits: 10 })
expired sessions with <= 1 hit: 1
session 999 after expiry: None
owner thread handled 404 commands, then exited
```

Four threads sent 400 `Touch` commands, and the main thread sent 4 more. Every operation on the map ran on one thread,
in one order, with plain `&mut` access, so "expire sessions with ≤ 1 hit" is trivially atomic with respect to every
touch. The map's lifetime is the channel's lifetime: when the last `SessionStore` handle drops, the owner's loop ends
and the map is freed on the owner thread. Chapter 11.7 measures what this costs when every read needs a round trip.

---

## Pass 2 · Systems level — *Slots, stamps, and parked threads*

### 4. Under the hood

**Since Rust 1.67, `std::sync::mpsc` is a port of crossbeam-channel** [VERSION] [LIB]. The public API stayed the same
(it dates from 1.0), and the implementation underneath was replaced. That's also why `Sender` became `Sync` in 1.72:
the new implementation could support it. Internally there are three "flavors", chosen by the constructor [LIB]:

```text
 mpsc::channel()          → list flavor:  a linked list of fixed-size blocks, allocated as it grows (unbounded)
 mpsc::sync_channel(n>0)  → array flavor: one ring buffer of n slots, allocated up front (bounded)
 mpsc::sync_channel(0)    → zero flavor:  no buffer; a sender and a receiver meet and hand over directly
```

The **array flavor** (the one to understand, since it's what a bounded queue should be) works like this [LIB]:

```text
 buffer: [ slot 0 | slot 1 | ... | slot n-1 ]     slot = { stamp: AtomicUsize, msg: MaybeUninit<T> }
 head: AtomicUsize (next to read)   ┐ each on its own cache line (padded), so producers and the
 tail: AtomicUsize (next to write)  ┘ consumer don't slow each other down when neither is waiting

 send(v): read tail → is slot[tail]'s stamp "empty for this lap"?
            yes → compare_exchange tail → tail+1, write v into the slot, store stamp = "full" (Release)
            no  → full: spin briefly, then register in the senders' waiter list and PARK the thread
 recv():  mirror image on head; after taking the message, store stamp = "empty for the next lap",
          then wake one parked sender if there is one
```

Three things in that sketch explain everything the listings showed:

- **A message is written once and read once.** `send` moves `v` into the slot (a `size_of::<T>()`-byte copy), and
  `recv` moves it out. There's no clone, no serialization, and a `Box<LargeThing>` travels as one pointer.
- **The stamp is the publication.** The consumer's Acquire load of the stamp synchronizes with the producer's Release
  store, so everything the producer wrote into the message **happens-before** the consumer reads it [LANG]. Channels
  are synchronization primitives. That's why the message's fields need no locks of their own (Part XIV explains
  Acquire/Release).
- **Blocking is parking.** A full `send` or an empty `recv` spins a few times and then parks the thread: on Linux, a
  `futex` wait [OS]. The other side's next operation unparks it: a `futex` wake. That's the ~5 µs hand-off from Chapter
  11.1, and it's why a channel whose ends constantly wait for each other is expensive (§6).

**Disconnection is reference counting** [LIB]. The channel keeps a count of `Sender`s and a count of `Receiver`s. Cloning
a `Sender` increments one, and dropping decrements it. When the sender count reaches zero, the channel is marked
disconnected and **every parked receiver is woken**. A receiver drains what's left and then gets `RecvError`. When the
receiver side goes, `send` fails immediately and returns the value in `SendError(v)`. The last handle to go (sender or
receiver) frees the channel itself. It's the same "last owner frees" rule as `Arc` (Chapter 3.6), applied to a queue.

**`select!` is registration on several waiter lists** [LIB]. crossbeam's `select!` first tries every operation without
blocking. If none is ready, it registers the thread with each channel's waiter list, parks once, and on wakeup
unregisters from all of them and completes exactly one operation. A timer (`tick`, `after`) is a channel whose sender is
a clock.

### 5. Memory

**An unbounded channel moves overload from the producer's CPU to your heap.** With a stalled consumer, 100,000
64-byte audit records sent into `mpsc::channel()` (listing `ch05-07-unbounded-growth.rs`, the Part III counting
allocator):

```text
unbounded:  25000 queued, live heap +1766 KiB
unbounded:  50000 queued, live heap +3530 KiB
unbounded:  75000 queued, live heap +5295 KiB
unbounded: 100000 queued, live heap +7058 KiB
after dropping the channel: live heap +1 KiB
bounded(1000): accepted 1000, refused 99000, live heap +72 KiB
```

The growth is perfectly linear, about 72 bytes per 64-byte record, and it's the list flavor's layout showing through
[LIB] (as of the 1.98 source; not a contract). Blocks hold 31 slots, and each slot is the message plus an 8-byte state
word, so a block is 8 + 31 × 72 = 2,240 bytes. 100,000 messages need ⌈100,000 / 31⌉ = 3,226 blocks = 7,226,240 bytes
= **7,057 KiB**, against 7,058 KiB measured. The bounded channel allocated its 1,000 slots up front (1,000 × 72 bytes =
70 KiB, plus the channel's own header) and then **refused** 99,000 sends instead of growing.

Three memory rules follow:

1. **A backlog is freed only when it's consumed or the channel is dropped.** Draining a burst doesn't give memory back
   to the OS either (Chapter 3.1's freed-vs-RSS point), so a service that once queued 5 GB keeps a large RSS.
2. **The per-message cost is `size_of::<T>()` plus a word, before counting heap data that `T` owns.** A
   `String` message is 24 bytes in the slot plus its heap buffer. A 4 KiB inline array (Chapter 2.6's
   `Message::Data([u8; 4096])`) is 4 KiB **per slot**. Box large variants, as Chapter 2.6 did.
3. **Bounded memory is a design property.** `sync_channel(n)` gives a hard upper bound, `n × (size_of::<T>() + 8)`
   plus the messages' own heap data, which you can put in a capacity plan.

### 6. CPU / OS

What a message costs depends on whether the two threads ever have to wait for each other. Here are 1,000,000 `u64`s from
one thread to another (listing `ch05-06-channel-cost.rs`, release, one run on the Playground's 4 cores; noisy):

```text
std mpsc::channel (unbounded)                  18.3 ns per item
std mpsc::sync_channel(1024)                   19.3 ns per item
std mpsc::sync_channel(1): near-lockstep     7037.9 ns per item
crossbeam bounded(1024)                        18.4 ns per item
std sync_channel(16) of Vec<u64> batches of 1024    5.2 ns per item
no channel: sum on the same thread              0.4 ns per item
```

(An earlier run of the same listing gave 24.3, 21.1, 5,738.7, 18.6, 5.4, and 0.4 ns.) How to read it:

- **~18–24 ns when neither side waits.** With room in the buffer, a send is a compare-and-swap on `tail` plus a write
  and a Release store, and a receive is the mirror image. The real cost is the **cache line holding the message moving
  from the producer's core to the consumer's** (coherence traffic, order of tens of ns [CPU]). std's port and crossbeam
  cost the same, as you'd expect from the same design.
- **~6–7 µs when they wait for each other on every item.** With `sync_channel(1)`, the buffer is almost always full or
  empty, so nearly every operation parks one thread and wakes the other: futex system calls and context switches for
  each item [OS]. It's the same hand-off cost as Chapter 11.1, paid a million times: **about 350× slower** than the
  buffered case for the same work.
- **~5 ns in batches.** Sending `Vec<u64>` batches of 1,024 pays the channel cost once per batch. What's left is
  mostly the batch's cache lines moving between cores.
- **0.4 ns without a channel.** Moving work to another thread has to buy back at least 18 ns per item before it helps.
  Per-item channel sends are for items that take microseconds of work, not nanoseconds.

The operational rule: **size buffers so the producer and consumer rarely wait for each other in steady state**, and
batch when items are tiny. A channel that's always full or always empty in steady state measures its scheduler, not its
queue.

---

## Pass 3 · Architect level — *Choosing the queue, and its failure mode*

### 7. Trade-offs

**Channels (message passing) vs shared state (locks)** (the SPEC's twelve criteria):

| Criterion | Channels / owner thread | Shared state behind a lock |
|---|---|---|
| **Memory** | Buffer per channel; unbounded = unbounded | The data once |
| **CPU** | ~20 ns per message without waiting; µs with a wakeup | ~10–40 ns uncontended lock pair; much more contended (11.7) |
| **Latency** | Adds queueing delay; request/reply adds a round trip (µs) | A short critical section: ns, until contention |
| **Throughput** | One owner thread is a serial bottleneck; pipelines scale by stage | Scales with sharding (11.7) |
| **Contention** | Only on the channel's head/tail | On the lock and the data's cache lines |
| **Cache behavior** | Data moves between cores once per message | Data bounces between cores on every access |
| **Allocation** | List-flavor blocks; boxed messages; one-shot reply channels | None per operation |
| **Complexity** | Protocol design (commands, replies, shutdown) | Lock scope, ordering, poisoning |
| **Safety** | Ownership moves: no shared mutation at all | Type-checked (`Mutex<T>`), but invariants are yours |
| **Maintainability** | Clear ownership and stage boundaries | Lock-ordering rules live in comments |
| **Failure modes** | Unbounded growth, stuck producers, forgotten `Sender` = no shutdown | Deadlock, convoys, poisoning |
| **Operational implications** | Queue depth is the saturation metric | Lock wait time is the (hidden) metric |

**Which channel:**

| Need | Choose | Why |
|---|---|---|
| Stage-to-stage pipeline, one consumer | `mpsc::sync_channel(n)` | Bounded, in std, backpressure by default |
| Work distribution to N workers | `crossbeam_channel::bounded(n)` | Clonable `Receiver` (MPMC); std's `mpmc` is unstable |
| Waiting on several sources, timers, shutdown | crossbeam `select!` + `tick`/`after` | std's channels have no select |
| Fire-and-forget with a known small volume | `mpsc::channel()` | Unbounded is fine when the producer is bounded by something else |
| Hand-off where the producer must know it was taken | `sync_channel(0)` | Rendezvous |
| Async tasks | `tokio::sync::mpsc` and friends | Chapter 13.3: an async `send().await` must not block a thread |

> **Why not always unbounded?** Because "unbounded" is not a capacity: it's a promise that the consumer always keeps up.
> When that promise breaks, the failure shows up far from its cause (a heap-growth OOM kill on a service whose
> *downstream* was slow) and with the worst possible timing (§10). A bounded channel makes the same situation visible
> where it happens: the producer waits or refuses, and a queue-depth metric flattens at its limit.

### 8. Java comparison

| Java | Rust | Note |
|---|---|---|
| `ArrayBlockingQueue(n)` | `mpsc::sync_channel(n)`, `crossbeam_channel::bounded(n)` | Both are preallocated rings |
| `LinkedBlockingQueue()` (capacity `Integer.MAX_VALUE`) | `mpsc::channel()` | Both are effectively unbounded |
| `SynchronousQueue` | `sync_channel(0)` | Rendezvous |
| `put` / `take` | `send` / `recv` | Blocking |
| `offer(e)` → `false` | `try_send(v)` → `Err(Full(v))` | Rust hands the value back |
| `poll(timeout)` | `recv_timeout(d)` | |
| A poison-pill object to stop consumers | Drop the last `Sender` | Disconnection can't be forgotten in one branch, or read twice |
| `InterruptedException` from `take()` | No interruption: `select!` on a shutdown channel | Cooperative, and explicit in the type |
| `Executors.newFixedThreadPool(n)` | A pool plus a queue you choose (Project L3) | Java's uses an **unbounded** `LinkedBlockingQueue` |
| Go channels (`make(chan T, n)`, `close`, `select`) | crossbeam-channel | Close ≈ dropping Senders; Go panics on send-after-close, Rust returns `Err` |
| Akka actors / mailboxes | Owner thread + command enum | Rust's "actor" is a thread and an enum; no framework needed |

The `newFixedThreadPool` row is the one promised in Chapter 11.1. Per the JDK documentation, the fixed pool's work
queue is an unbounded `LinkedBlockingQueue`. A Java service built on it has §10's failure mode by default: under
overload, `submit` never blocks and never fails, and the queue grows until the heap is gone. Rust's std gives you no
pool, so the queue's bound is a decision you have to write down (Project L3's pool refuses with `Err(item)`).

> **Analogy limit.** "`sync_channel` is an `ArrayBlockingQueue`" is right about capacity and blocking, and wrong about
> what's in the queue. A Java queue holds **references**. After `put(order)`, the producer still has `order` and can
> mutate it while the consumer reads it: a data race that `final` fields and immutability conventions try to prevent.
> A Rust channel holds **values**. After `send(order)`, the producer has nothing (E0382 above). The analogy breaks back
> the other way if you send `Arc<Mutex<Order>>`: then the channel carries a *shared handle*, and you're back to
> Chapter 11.3's rules.

### 9. Production scenario

**Meridian's payments-core audit trail.** Every charge in payments-core (the Rust service from Chapter 8.4) must leave
an audit record before the charge is acknowledged. Compliance says a charge without an audit record must not happen.
The audit sink is a durable append log that is usually fast, and occasionally stalls for a second or two during
compaction.

The first design wrote the record synchronously in the request handler. Request latency then included the sink's p99,
and during stalls every request thread waited on the sink. The redesign used a channel and made each part of the
policy explicit:

- **One writer thread owns the sink** (the owner-thread pattern). It batches: up to 500 records or 20 ms, whichever
  comes first. It appends the batch, `fsync`s once, then acknowledges every record in the batch through its one-shot
  reply channel. One `fsync` covers hundreds of charges.
- **A bounded crossbeam channel of 10,000 records** between request threads and the writer. That's several seconds of
  peak traffic, enough to ride out a compaction stall without anyone waiting.
- **`send_timeout(record, 50 ms)` in the request path.** If the channel stays full for 50 ms, the sink is really down,
  and the charge fails with a `Transient` error (Chapter 8.4's taxonomy: `503`, retryable with the same idempotency
  key). "No audit, no charge" is enforced by the code path, not by hope.
- **Metrics come from the channel**: queue depth (`len()`), time spent in `send_timeout`, batch sizes, and the writer's
  last-flush timestamp. A depth pinned at 10,000 is the page.

Metrics events took a *different* policy on purpose. They go through `try_send` into their own bounded channel, and a
full channel means **drop and count** (`metrics_dropped_total`). Losing a metric is acceptable. Blocking a charge for a
metric is not. The two channels encode two business decisions, and each one is one line of code.

### 10. Failure scenario

**The access-log shipper that ate the heap.** The Rust gateway formats one access-log line per request (Chapter 9.2's
per-worker buffer), and a shipper thread forwards the lines to the log collector over the network. The worker-to-
shipper link was an `mpsc::channel()`: unbounded, "because logging must never slow a request".

At 400K req/s across 55 pods, each pod produces about 7,300 lines per second. One afternoon the collector cluster had a
20-minute outage. The shipper's network writes blocked, so it stopped receiving, and nothing else noticed. Every worker's
`send` still succeeded in nanoseconds. Each queued line cost its `String` (about 250 bytes of heap) plus the slot, about
2 MB of heap per second per pod. After roughly a quarter of an hour, pods began hitting their memory limit. The OOM
killer restarted them one after another, and each restart **dropped the queued lines** plus the in-flight requests.
The policy meant to protect requests had killed them, and it lost the logs it was meant to keep.

The postmortem's points map onto this chapter:

1. **Unbounded was a hidden capacity decision.** "Never slow a request" was implemented as "use unlimited memory",
   which no pod has.
2. **The right policy for logs is drop-and-count.** The shipper link became `crossbeam_channel::bounded(200_000)`
   (about 30 s of lines, roughly 60 MB of worst-case memory, which is in the pod's budget). Workers use `try_send`, and
   `Full` increments `access_log_dropped_total`. An alert fires on the drop rate, not on the pod's memory.
3. **Queue depth is exported**, so a collector outage now looks like a flat line at 200,000 and a rising drop counter,
   while every request still succeeds.

Compare the audit trail in §9. Same shape, opposite policy, because the business answer to "can we lose this?" is
opposite. The channel's capacity and full-policy are where that answer lives in the code.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XI).*

1. What happens to a value's ownership when you `send` it? Why does that make Rust channels safer than a Java
   `BlockingQueue` of mutable objects?
2. How does a `for msg in rx` loop know to end? What is Rust's replacement for a poison pill, and why is it more robust?
3. Why is `mpsc::Receiver` `Send` but not `Sync`? What do you use when several workers must consume from one queue?
4. Unbounded, bounded, rendezvous: what does each do when the consumer is slower than the producer? Which do you
   default to in a service, and why?
5. Why did `sync_channel(1)` cost about 7 µs per item while `sync_channel(1024)` cost about 19 ns?
6. Where does an unbounded channel's backlog live, how much memory does each message take, and when is it freed?
7. What is the owner-thread pattern? When is it a better choice than `Mutex<HashMap>`, and what does it cost per read?
8. `try_send` and `SendError` both hand the value back. Why does that matter for load shedding and for correctness?
9. How do you stop a worker that is blocked in `recv`, without `Thread.interrupt()`?
10. Compare Go channels, Java `BlockingQueue`, and Rust channels on ownership, closing semantics, and `select`.

### 12. Exercises

- **Beginner.** Build a three-stage pipeline (`parse → validate → print`) with `sync_channel(16)` between stages.
  Make sure the program ends by itself when the input ends, without any special "end" message.
- **Intermediate.** Extend `ch05-05-owner-thread.rs` with a `Snapshot` command that returns a `Vec<(u64, Session)>`.
  Then add a `Shutdown { reply }` command and compare it with "drop every handle". Which one can a client forget?
- **Advanced.** Implement a bounded MPMC queue with `Mutex<VecDeque<T>>` and two `Condvar`s (start from Chapter 11.3's
  listing), with `close()`, `try_send`, and `recv_timeout`. Benchmark it against `crossbeam_channel::bounded` using
  `ch05-06-channel-cost.rs`'s harness. Explain the difference with §4.
- **Systems.** Locally, run `ch05-06-channel-cost.rs` under `perf stat -e context-switches,cpu-migrations` (not
  available on the Playground). How many context switches does the `sync_channel(1)` row cause per item? Does it match
  §6's explanation?
- **Architecture.** A service fans each incoming event out to three consumers: a ledger mirror (must not lose events),
  a fraud-feature updater (latency-sensitive; stale events are useless after 2 s), and an analytics exporter
  (best-effort, bursty). Choose the channel type, capacity, and full-policy for each, and what happens when one
  consumer thread dies.

### 13. Debugging exercise

A worker pool's shutdown hangs forever in `join`. The code, simplified:

```rust,ignore
struct Dispatcher {
    jobs: crossbeam_channel::Sender<Job>,
    retry: crossbeam_channel::Sender<Job>, // re-queue failed jobs on the same channel
}

fn start(n: usize) -> (Dispatcher, Vec<JoinHandle<()>>) {
    let (tx, rx) = crossbeam_channel::bounded::<Job>(1_000);
    let workers = (0..n)
        .map(|_| {
            let rx = rx.clone();
            thread::spawn(move || {
                for job in rx {
                    job.run();
                }
            })
        })
        .collect();
    (Dispatcher { jobs: tx.clone(), retry: tx }, workers)
}

fn stop(d: Dispatcher, workers: Vec<JoinHandle<()>>) {
    drop(d.jobs); // "close the queue"
    for w in workers {
        w.join().unwrap(); // hangs
    }
}
```

1. Using §4's disconnection rule, explain exactly why the workers never leave their `for` loop.
2. A thread dump shows every worker parked. In which function, and how would you have found this without reading the
   code?
3. Fix `stop` in two ways: one that keeps the retry path, and one that redesigns it so this bug can't recur.

### 14. Design exercise

**Meridian's merchant notification service.** Events such as "payout sent" and "dispute opened" must reach merchants by
e-mail, webhook, and in-app inbox. There are about 2,000 events/s at peak. E-mail goes through a provider limited to 500
calls/s. Webhooks go to 40,000 merchant endpoints of very uneven quality. The inbox is a database write (~2 ms).

- Design the thread and channel topology: which stages, which channels, which capacities, and which policy when each is
  full (block, refuse, spill, drop).
- One merchant's webhook endpoint is down for a day. Show that your design keeps it from affecting the others (compare
  Chapter 11.1's webhook dispatcher).
- How does the service shut down during a deploy without losing an accepted event? Which events may it drop, if any?
- Which metrics tell on-call *which* stage is the bottleneck, using only the channels?
