# Chapter 11.7 — The Concurrency Decision Matrix

> **Where this sits:** Part XI · Concurrency · chapter 7 of 7 (the two projects follow)
> **Prerequisites:** Chapters 11.1–11.6, Chapter 1.3 (the first data race and its three fixes), Chapter 9.5 (caches).
> **After this chapter you can:** rank the ways to share mutable state by cost and explain the ranking in terms of
> cache lines; choose a concurrent-map design from measurements rather than habit; spot the two most expensive
> production mistakes (hot shared writes and slow work under a lock); and map every Java concurrency tool you know to
> its Rust counterpart, including where the analogy breaks.

---

## Pass 1 · User level — *One question, three answers, measured*

### 1. Problem

Part XI has handed you a lot of tools: threads and pools, `Send` and `Sync`, `Mutex`, `RwLock`, `Condvar`, `Cell`,
`RefCell`, `OnceLock`, `ArcSwap`, atomics, channels, scoped threads, and rayon. Each chapter showed what one tool does.
An architect needs the reverse view: *given a piece of state that several threads touch, which tool, and what will it
cost?*

Chapter 1.3 answered that with a prediction. It fixed its first data race three ways (an atomic, a mutex, and no
sharing at all) and predicted, from mechanism, the ranking "no sharing < atomic < mutex". That prediction was
explicitly left unmeasured and promised to this chapter. Part IX (9.3) likewise promised measurements of the
concurrent-map options. This chapter delivers both, and turns them into a decision matrix.

### 2. Mental model

Every design for shared mutable state is one of three strategies, and they're ordered by **how often two cores have to
fight over the same cache line**:

```text
 1. DON'T SHARE                     2. SHARE, BUT IMMUTABLY             3. SHARE MUTABLE STATE
 ───────────────                    ──────────────────────              ──────────────────────
 ownership moves (channels)         Arc<T> with T frozen                atomics        (one line, RMW each time)
 partition the data (chunks_mut)    ArcSwap<T> snapshots (11.4)         locks          (a line for the lock + data)
 per-thread state, merge later      OnceLock / LazyLock                 owner thread   (all ops serialized, round trips)

 no line is written by two cores    lines are READ by many cores:       every write takes the line away from
                                    they stay in every cache (Shared)   every other core (MESI: Modified)
 cost: ~free                        cost: ~free on the read path        cost: grows with writers and frequency
```

The cost ladder inside strategy 3 comes from the hardware [CPU]. Writing a cache line requires owning it exclusively.
If another core has just written the same line, ownership has to move across the chip: tens of nanoseconds, and every
writer queues for it. A lock adds its own line, two atomic operations per critical section, and, once threads start to
wait, kernel sleeps and wakeups (microseconds). So the question is never "atomic or mutex?" on its own. It's **how many
cores write to the same lines, and how often.**

### 3. Rust code

**Chapter 1.3's ranking, measured, plus the two cases in between** (listing `ch07-01-counter-ranking.rs`, excerpt:
4 threads × 5M increments, release):

```rust,ignore
// 1. No sharing: a local counter; black_box keeps every increment (otherwise LLVM folds the loop away).
run("no sharing (local counter, merged at the end)", |id| {
    let mut local = 0u64;
    for _ in 0..PER_THREAD {
        local = black_box(local) + 1;
    }
    totals[id].store(local, Ordering::Relaxed);
});

// 2. One atomic per thread, padded to separate cache lines: atomic RMW, but no line is shared.
let padded: Vec<CachePadded<AtomicU64>> = (0..THREADS).map(|_| CachePadded::new(AtomicU64::new(0))).collect();
run("per-thread atomics, padded apart (CachePadded)", |id| {
    for _ in 0..PER_THREAD {
        padded[id].fetch_add(1, Ordering::Relaxed);
    }
});

// 3. One atomic per thread, adjacent in memory: logically unshared, physically one cache line.
let adjacent: [AtomicU64; THREADS] = std::array::from_fn(|_| AtomicU64::new(0));
// ... 4. one shared AtomicU64, 5. one std::sync::Mutex<u64>, 6. one parking_lot::Mutex<u64>
```

One run's output, verbatim:

```text
no sharing (local counter, merged at the end)           0.59 ns per increment
per-thread atomics, padded apart (CachePadded)          2.90 ns per increment
per-thread atomics, adjacent (false sharing)            5.05 ns per increment
one shared AtomicU64 (fetch_add, Relaxed)              19.00 ns per increment
one shared std::sync::Mutex<u64>                      111.79 ns per increment
one shared parking_lot::Mutex<u64>                     27.17 ns per increment
(4 threads, wall time / increments per thread; all six totals = 20000000: true)
size_of CachePadded<AtomicU64> = 128 bytes
```

A single run of a contention benchmark on a shared machine is an anecdote, so the same listing was run five times
(ns per increment, release; the Playground assigns whatever cores it has free each time):

| Strategy | Run A | Run B | Run C | Run D | Run E | Range |
|---|---|---|---|---|---|---|
| No sharing (local + merge) | 0.60 | 0.59 | 0.60 | 0.59 | 0.32 | **0.3–0.6** |
| Per-thread atomics, padded | 3.60 | 2.90 | 6.65 | 2.90 | 3.25 | **2.9–6.7** |
| Per-thread atomics, adjacent (false sharing) | 18.81 | 5.05 | 22.90 | 5.05 | 17.88 | **5–23** |
| One shared `AtomicU64` | 16.15 | 19.00 | 8.89 | 19.04 | 17.67 | **9–19** |
| One shared `parking_lot::Mutex` | 35.13 | 27.17 | 25.62 | 34.72 | 28.98 | **26–35** |
| One shared `std::sync::Mutex` | 180.73 | 111.79 | 38.17 | 181.49 | 163.24 | **38–181** |

The absolute numbers move by up to 5× between runs. **The ranking doesn't**, apart from the two middle rows, which trade
places. That's the result to keep. Chapter 1.3's prediction holds, with two refinements:

1. **"Atomic" isn't one cost.** An atomic that only one thread writes costs about 3 ns: the price of a locked
   instruction, with the line staying in that core's cache. An atomic that four threads write costs 9–19 ns, because
   the line is always somewhere else.
2. **False sharing costs like true sharing.** Four counters that are *logically* private but share a cache line cost
   5–23 ns: in some runs as bad as, or worse than, one genuinely shared counter. The hardware tracks ownership per line,
   not per variable, so it can't tell the difference. `CachePadded` puts each counter on its own 128-byte region, and
   the cost drops to that of an unshared atomic.

**The concurrent-map options** (the Part IX promise). Here are 4 threads, 10,000 keys, 95% reads and 5% writes, the
same operation streams for every design (listing `ch07-02-map-options.rs`, release; ns per operation per thread, five
runs):

| Design | Run A | Run B | Run C | Run D | Run E | Range |
|---|---|---|---|---|---|---|
| `Mutex<HashMap>` | 272.1 | 159.8 | 163.4 | 219.6 | 230.6 | **160–272** |
| `RwLock<HashMap>` | 154.1 | 132.7 | 92.3 | 129.2 | 131.2 | **92–154** |
| 16 shards × `Mutex<HashMap>` | 75.7 | 74.4 | 74.1 | 75.1 | 75.2 | **74–76** |
| 16 shards × `RwLock<HashMap>` | 78.1 | 80.4 | 76.2 | 80.0 | 80.5 | **76–81** |
| Owner thread + channels (reads round-trip) | 6,497 | 14,124 | 19,435 | 8,875 | 9,182 | **6,500–19,400** |

Three findings, stable across runs:

- **Sharding wins, and it's the most stable row** (74–81 ns). With 16 shards and 4 threads, two threads rarely want
  the same shard at the same moment, so most lock operations are uncontended.
- **Inside a shard, `Mutex` and `RwLock` tie.** When critical sections are this short, the read lock's advantage
  (parallel readers) is eaten by its cost: a read lock still *writes* the reader count (Chapter 11.3).
- **The owner thread is two orders of magnitude slower per read**, because every read is a round trip: two channel
  operations and at least one thread wakeup (Chapter 11.5's microseconds). Owner threads are for *commands* that are
  mostly fire-and-forget, or for state where serialization is the point, not for hot read paths.

`ArcSwap` isn't in this table because it answers a different workload. For read-mostly data replaced in bulk, Chapter
11.4 measured 7.8 ns per read, against 219–259 ns through a lock. With 5% *per-key* writes, though, every write would
have to copy the whole map and publish a new version, which is prohibitive at 10,000 keys.

**The most expensive production mistake: slow work inside a critical section.** Four threads, each handling 100
requests. Each request does a tiny state update, plus an audit write that takes about 1 ms (listing
`ch07-03-io-under-lock.rs`):

```rust
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

const THREADS: u32 = 4;
const REQUESTS: u32 = 100;

fn write_audit_record(_line: &str) {
    thread::sleep(Duration::from_millis(1)); // stands in for a network call or an fsync
}

fn run(label: &str, handle: impl Fn(u32) + Sync) {
    let t = Instant::now();
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| (0..REQUESTS).for_each(&handle));
        }
    });
    let secs = t.elapsed().as_secs_f64();
    println!("{label:<34} {:>6.0} requests/s  ({:.2} s)", (THREADS * REQUESTS) as f64 / secs, secs);
}

fn main() {
    let balances = Mutex::new(vec![0i64; 64]);

    run("audit write INSIDE the lock", |i| {
        let mut b = balances.lock().unwrap();
        b[(i % 64) as usize] += 1;
        write_audit_record(&format!("credit {i}")); // every other thread waits for this sleep
    });

    run("audit write AFTER the lock", |i| {
        let line = {
            let mut b = balances.lock().unwrap();
            b[(i % 64) as usize] += 1;
            format!("credit {i}")
        }; // guard dropped: the lock is held for nanoseconds
        write_audit_record(&line);
    });

    println!("total credits: {}", balances.lock().unwrap().iter().sum::<i64>());
}
```

```text
audit write INSIDE the lock           943 requests/s  (0.42 s)
audit write AFTER the lock           3687 requests/s  (0.11 s)
total credits: 800
```

(An earlier run: 941 and 3,754.) Same work, same threads, **3.9× the throughput**, and the only change is where one
closing brace sits. With the write inside, the lock is held for about 1 ms per request, so the whole system can never
exceed about 1,000 requests per second, whatever the thread count. A lock converts parallel work into serial work for
exactly as long as it's held (Chapter 11.3). That makes **hold time**, not lock type, the first thing to review.

---

## Pass 2 · Systems level — *Cache lines, locked instructions, and futexes*

### 4. Under the hood

What each row of the counter table does, mechanically (x86-64 [CPU]; Chapter 11.3 showed the `Mutex` assembly):

| Row | Instructions per increment | What the cache does |
|---|---|---|
| No sharing | `add` on a register (the `black_box` forces a round trip through memory, still L1) | Nothing leaves the core |
| Padded atomic | `lock xadd` on a line only this core writes | The line stays Modified in this core's L1. The cost is the locked instruction draining the store buffer (Part XIV) |
| Adjacent atomics | `lock xadd` on a line **other cores also write** | Each write must pull the line from whichever core wrote last: a coherence transfer |
| Shared atomic | The same, with every thread on one variable | Same transfers, plus writes are fully serialized on one line |
| `parking_lot::Mutex` | CAS to lock, data update, release store; spin with backoff, then park | Two contended lines (lock word + data), usually in one line; parking is rare in a 4-thread microbenchmark |
| `std::sync::Mutex` | CAS to lock (Chapter 11.3's `lock cmpxchg`), data, `xchg` to unlock, `futex` wake if the state was "contended" | Same lines, plus **system calls** once the lock is marked contended |

The spread between the two mutexes (26–35 ns vs 38–181 ns) is an implementation difference, not a law [LIB]. Under
this workload (four threads hammering one lock with an empty critical section) std's futex mutex frequently ends up in
its "locked with waiters" state, and each unlock then makes a `futex(FUTEX_WAKE)` system call. `parking_lot`'s
adaptive spinning keeps most acquisitions in user space. Real services rarely hammer a lock like this, and the right
fix is to *stop hammering it* (sharding, per-thread state), not to swap mutex crates. Part XX shows how to measure
lock-wait time in a real service.

**Why 128 bytes of padding, not 64?** x86-64 cache lines are 64 bytes, but Intel's spatial prefetcher fetches lines in
**adjacent pairs**, so two counters in neighbouring lines can still interfere. crossbeam's `CachePadded` therefore uses
128-byte alignment on x86-64 and aarch64 (listing output: `size_of CachePadded<AtomicU64> = 128 bytes`) [LIB] [CPU].
Chapter 5.2 introduced the type; this is the measurement it promised.

**Why the false-sharing row swings from 5 to 23 ns.** The hypothesis (not verified here: the Playground doesn't expose
its CPU topology or `perf`) is **thread placement**. If the four threads happen to run on SMT siblings or cores that share
a cache level, moving the line is cheap. If they run on cores further apart, it's expensive. The shared-atomic row
swings for the same reason. This is why Part XX benchmarks pin threads to cores, and why "it was fast in my benchmark"
means little for contention-sensitive code until the placement is known.

### 5. Memory

The cheaper strategies usually cost memory:

| Strategy | Memory cost | Example |
|---|---|---|
| Per-thread state + merge | N copies of the state | logstat: an 80 KB histogram per chunk (11.6) |
| Padding | Up to 128 B per hot variable | 64 padded counters = 8 KB instead of 512 B |
| Sharding | N locks + N maps (per-map overhead and growth slack × N) | 16 shards of a 10K-key map: negligible; of a 10M-entry map: 16 separately growing tables |
| Immutable snapshots (`ArcSwap`) | Old versions live until the last reader drops them | Chapter 11.4's route table: 2 × 1M entries during a swap |
| Owner thread | The channel's buffer + one-shot reply channels per request | Chapter 11.5 |
| Global lock | Nothing extra | The cheapest in memory, the most expensive in contention |

That's the trade in one sentence: **you buy away contention with memory.** It's usually cheap, because hot shared state
is usually small (counters, a routing table, a cache index). When the state is big, the design question changes from
"how to share it" to "how to partition it". That's Part XXIV's territory.

### 6. CPU / OS

Two kernel-level effects show up only under contention, and neither shows in single-threaded profiling:

- **Futex system calls** [OS]. A contended std lock that has waiters makes a `futex` wake on unlock, and each waiter
  makes a `futex` wait. Each system call costs order of a microsecond, including the mode switch and scheduler work.
  In a profile they look like time "in the kernel", attributed to `syscall` or `futex_wait`, not to your lock.
- **Convoys** [OS]. When a lock holder is descheduled (its time slice ended, or it took a page fault), every waiter
  queues behind it for a whole scheduling quantum, milliseconds. With threads ≤ cores and short critical sections this
  is rare. With oversubscription (Chapter 11.1) and I/O under a lock (above), it becomes the tail latency.

The rule for reading a concurrent profile: **a flat CPU profile with low throughput means waiting.** Look at lock-wait
time, context-switch counts, and queue depths, not at hot functions. Part XX covers the tools (`perf sched`, off-CPU
flame graphs).

---

## Pass 3 · Architect level — *The matrix*

### 7. Trade-offs

**The decision procedure.** Walk it top to bottom and stop at the first "yes":

```text
 Is the state needed by more than one thread at all?
   no ──► keep it local. (Most state. Move it with channels if it changes owner.)
 Can it be partitioned, so each thread owns a disjoint part?
   yes ─► chunks_mut / scoped threads / rayon; per-thread state merged at the end (11.6)
 Is it read-mostly and replaced in bulk (config, routing tables, catalogs, models)?
   yes ─► Arc<T> if it never changes; ArcSwap<T> if it's republished (11.4)
 Is it initialized once, lazily?
   yes ─► OnceLock / LazyLock (11.4), and decide what a failed initialization means
 Is it a counter, a flag, or a small value updated independently?
   yes ─► atomics. If it's HOT (many writers, high rate): per-thread / padded counters, merged on read
 Is it a map or a structure with invariants across fields?
   yes ─► a lock around the smallest consistent unit. If it's hot: SHARD it. RwLock only after measuring.
 Must every operation be ordered, or does the state have a complex lifecycle (sessions, connections, a device)?
   yes ─► owner thread + commands (11.5); accept the round-trip cost, or make commands fire-and-forget
 Is the work CPU-bound over data you already have?        ──► rayon (11.6)
 Is the "concurrency" really waiting on I/O?              ──► a bounded pool (Project L3) or async (Part XII)
```

**The matrix** (the SPEC's twelve criteria, for the seven common designs; numbers are this Part's measurements):

| Criterion | Local / partitioned | Immutable `Arc` / `ArcSwap` | Atomics | `Mutex` | `RwLock` | Sharded locks | Owner thread |
|---|---|---|---|---|---|---|---|
| **Memory** | N copies | Versions in flight | 8 B (128 B padded) | Lock + T | Lock + T | N × (lock + part) | Channel + buffers |
| **CPU / op** | < 1 ns | ~8 ns read; copy on publish | 3 ns private, 9–19 ns shared | 26–181 ns contended | Reads still write | ~75 ns (4 thr, 16 shards) | µs per round trip |
| **Latency** | None | None for readers | Stable if not hot | Convoys possible | Writers wait on readers | Rarely contended | Queueing delay |
| **Throughput** | Scales with cores | Reads scale | Hot line serializes | Serializes | Reads scale if sections are long | Scales with shards | One core's worth |
| **Contention** | None | None on reads | On the line | On lock + data | On the reader count | Per shard | On the channel |
| **Cache behavior** | Private lines | Shared read lines | Ping-pong if shared | Ping-pong | Ping-pong on count | Mostly private | Data moves per message |
| **Allocation** | Per thread, up front | Per publish | None | None | None | None | Per command (replies) |
| **Complexity** | Merge design | Publication + reclamation | Memory orderings (Part XIV) | Lock scope | + upgrade/recursion traps | Routing, cross-shard ops | Protocol, shutdown |
| **Safety** | Borrow-checked | Borrow-checked | Wrong ordering = logic bug | Poisoning policy | Poisoning, recursion deadlock | Cross-shard atomicity is gone | Deadlock on reply cycles |
| **Maintainability** | Clear data flow | Clear versioning | Subtle | Familiar | Subtle | Moderate | Clear ownership |
| **Failure modes** | Wrong merge | Stale reads by design | Lost updates if you do read-modify-write by hand | Deadlock, I/O under lock | Writer starvation / nested read deadlock (11.3) | Hot shard | Unbounded mailbox, stuck owner |
| **Operational** | Merge cost at read time | Version metrics | Hard to observe | Lock-wait time | Same | Per-shard metrics | Queue depth |

The pattern in the matrix: **the leftmost columns are cheapest and scale best, but constrain the design most.** Moving
right buys flexibility (arbitrary updates, invariants across keys) with contention. An architect's job is to push each
piece of state as far left as its semantics allow, and to measure before moving right.

> **Why not a lock-free map everywhere?** Concurrent maps built on atomics (Java's `ConcurrentHashMap`, Rust crates such
> as `dashmap` (not on the Playground) or `flurry`) are internally the sharded-lock or striped design with extra
> engineering. They're good defaults when you need a general map. They still pay for the same cache lines under writes
> to the same keys, their iteration is weakly consistent, and they can't give you an atomic multi-key update.
> Chapter 14.4 shows what "lock-free" costs to build correctly.

### 8. Java comparison

| Java | Rust | Analogy limit |
|---|---|---|
| `synchronized` block / method | `Mutex<T>` + guard (11.3) | Java's monitor guards *code*; Rust's lock *owns the data*. Java monitors are reentrant; Rust's aren't |
| `ReentrantLock` (+ `Condition`) | `Mutex` + `Condvar` (11.3); `parking_lot` for timeouts and fairness | No reentrancy; waiting consumes and returns the guard |
| `ReentrantReadWriteLock` | `RwLock` | Nested reads while a writer waits deadlock in Rust (11.3 §10) |
| `volatile` field | `Atomic*` with `SeqCst` or `Acquire`/`Release` (Part XIV) | Java's `volatile` is roughly SeqCst for that field. Rust makes you pick the ordering per operation |
| `AtomicInteger` / `AtomicLong` | `AtomicU32` / `AtomicU64` | Same instructions. Rust has no boxing and no object header per atomic |
| `LongAdder` | Per-thread or padded counters, summed on read | `LongAdder` *is* the padded-row design, packaged (striped cells with contention-driven growth) |
| `ConcurrentHashMap` | Sharded `Mutex<HashMap>` / `RwLock` (Ferrite's `ShardedStore`), crates | Java's is a finer-grained, lock-striped, partly lock-free design. The *idea* is sharding |
| `Collections.synchronizedMap` | `Mutex<HashMap>` | The slow row, in both languages |
| `CopyOnWriteArrayList` / immutable snapshots | `ArcSwap<Vec<T>>` (11.4) | Same idea. Rust frees old versions deterministically, at the last reader |
| `ThreadLocal` | `thread_local!` | Rust's can't be reached from another thread at all |
| `ExecutorService` / `newFixedThreadPool` | A pool with an explicit bounded queue (Project L3), rayon for CPU work | Java's fixed pool has an **unbounded** queue by default (11.5) |
| `CompletableFuture` chains | Async/await (Parts XII–XIII) | Different model: futures are inert until polled |
| Parallel streams / `ForkJoinPool` | rayon (11.6) | Rust checks the closures' thread-safety at compile time |
| `BlockingQueue` | `sync_channel`, crossbeam-channel (11.5) | Values move; Java queues pass references |
| Virtual threads (JDK 21) | Async tasks (Part XII) | Stackful vs stackless (Chapter 12.6) |
| The JMM (happens-before, JLS 17) | The C++20-style model Rust adopts (Part XIV) | In Java a data race is a bug with bounded outcomes. In Rust (safe code can't write one), it's undefined behavior |

> **Analogy limit (the whole table).** Every Java row is a *runtime* tool: you can use `ConcurrentHashMap` correctly or
> put a plain `HashMap` in a static field, and both compile. Every Rust row is also a *type*, and the compiler checks
> that shared state goes through one of them (`Send`/`Sync`, Chapter 11.2). The design decisions are the same. The
> difference is that in Rust you can't forget to make one.

### 9. Production scenario

**Meridian's gateway metrics, three iterations.** The Rust gateway (400K req/s at peak across 55 pods, one worker thread
per core) exports per-route counters for about 5,000 routes (the Chapter 9.4 design exercise): requests, errors, bytes,
and a latency histogram.

1. **`Mutex<HashMap<RouteId, RouteStats>>`.** The obvious first version. Under load, lock-wait time showed up as the
   gateway's largest single cost in off-CPU profiles, because every request, on every worker, took the same lock twice.
2. **`HashMap<RouteId, RouteStats>` built at startup with atomic fields**, and no lock, since the route set changes only
   on a config reload (published with `ArcSwap`). That was much better, but the three hottest routes (health checks and
   the two largest partners' APIs) put most workers on the same few cache lines: exactly the "shared atomic" row.
   Worse, `requests` and `errors` sat in one 64-byte line, so even routes with few errors paid false sharing.
3. **Per-worker counters, merged on scrape.** Each worker owns a cache-line-aligned `Vec<RouteStats>` indexed by
   route, with no hashing (like logstat's per-thread `Summary`). The fields are still atomics, because the scraper reads
   them from another thread, but each has exactly **one writer**. So an increment can be
   `store(load(Relaxed) + 1, Relaxed)` instead of `fetch_add`, and in release that compiles to a plain
   `inc qword ptr [rdi]` without the `lock` prefix, where `fetch_add` gives `lock inc qword ptr [rdi]` (listing
   `ch07-04-single-writer-asm.rs`, x86-64 release asm). It's correct *only* because there's one writer, and the code
   says so in a comment. The metrics endpoint, scraped every 10 seconds, sums one vector per worker. That's a
   `LongAdder` built to fit the problem: writes are free of contention, and reads are rare and cheap enough.

The final design follows the matrix: the counters moved from the "`Mutex`" column to "atomics" to "local /
partitioned". Each step kept the same semantics, since metrics only need to be exact at scrape time, and each step
removed a shared cache line from the request path.

### 10. Failure scenario

**Risk limits and the audit write.** Meridian's risk-limits service (Chapter 1.2: 100K ops/s, p99 < 1 ms) keeps per-
merchant exposure behind sharded mutexes. A compliance feature required that every *limit change* be written to an
audit log. The developer put the audit write where the change happened, inside the shard's critical section. It was
correct, and it was tested on a laptop where the audit log was a local file that took microseconds.

In production, the audit log was a network-attached volume with occasional 20–50 ms `fsync` stalls. During a stall,
the shard lock stayed held for the whole stall, and every check for every merchant on that shard waited behind it.
With 16 shards, one stall blocked about 6% of all traffic for 50 ms. With several limit changes in flight, several
shards stalled at once. p99 went from 0.4 ms to 40+ ms, and the upstream payment path started timing out: the
`ch07-03` measurement, at production scale.

The fix and the rules that came out of it:

- **Compute under the lock, perform I/O after it.** The critical section returns the audit record, and the write
  happens after the guard is dropped (the listing's second closure). If the audit write fails, the limit change is
  rolled back through a second, short critical section (a compensating action).
- **Review rule: no I/O, no logging to a slow sink, no callbacks, and no channel `send` that can block, while holding
  a lock.** Chapter 11.3 listed "slow work under the lock" as the fourth most common lock bug. This incident promoted it
  to the first.
- **Lock hold time is a metric.** Holding a shard lock for more than 1 ms now logs the call site. It's a few lines in the
  wrapper that returns the guard, and it would have found this bug in the first staging run.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XI).*

1. Rank "no sharing", "one shared atomic", and "one shared mutex" by cost per operation, and explain the ranking in
   terms of cache lines and system calls.
2. Why did four *logically unshared* adjacent atomic counters sometimes cost as much as one shared counter? How do
   you fix it, and why is `CachePadded` 128 bytes on x86-64?
3. The benchmark's absolute numbers varied up to 5× between runs while the ranking held. What does that tell you about
   using microbenchmarks for concurrency decisions?
4. Why did a 16-shard `Mutex<HashMap>` beat a single `RwLock<HashMap>` on a 95%-read workload?
5. When is an owner thread the right design despite costing microseconds per read?
6. Why did moving one audit write out of a critical section multiply throughput by about 4? Derive the ~1,000
   requests/s ceiling.
7. Walk through the decision procedure for: a feature-flag table read on every request and changed a few times a day;
   per-merchant rate-limit counters; a connection's protocol state machine.
8. What does Java's `LongAdder` do, and how would you build the same thing in Rust?
9. What can `ConcurrentHashMap` not give you that a single `Mutex<HashMap>` can?
10. You see a service with flat CPU profiles, low CPU usage, and low throughput under load. What do you measure next?

### 12. Exercises

- **Beginner.** Add a seventh row to `ch07-01-counter-ranking.rs`: per-thread counters in a `thread_local!` `Cell<u64>`,
  merged at the end. Where does it land in the ranking?
- **Intermediate.** Add an `ArcSwap<HashMap>` row to `ch07-02-map-options.rs` using copy-on-write for the 5% writes
  (`rcu`-style: clone the map, update, swap). Measure it at 10,000 keys and at 100 keys. Where's the crossover?
- **Advanced.** Make the sharded map's shard count a parameter and measure 1, 2, 4, 8, 16, 64, and 256 shards. Explain
  the curve's shape (contention vs. per-shard overhead vs. cache footprint).
- **Systems.** Locally, pin the threads of `ch07-01` to chosen cores (e.g. with `taskset` or the `core_affinity` crate,
  not on the Playground) and rerun the false-sharing row on SMT siblings vs. separate physical cores. Does the placement
  hypothesis in §4 hold?
- **Architecture.** Write a one-page "shared state review" checklist for your team, based on §7's procedure and §10's
  rules. Apply it to one real service you know.

### 13. Debugging exercise

A service's CPU usage rose 30% after a harmless-looking refactor, with no change in traffic. The diff moved three
counters into one struct:

```rust,ignore
pub struct PipelineStats {
    pub decoded: AtomicU64,    // written by the decoder thread
    pub validated: AtomicU64,  // written by the validator thread
    pub published: AtomicU64,  // written by the publisher thread
}
pub static STATS: PipelineStats = PipelineStats { /* zeros */ };
```

Before the refactor, each counter was a separate `static` defined in its stage's module.

1. Each counter has exactly one writer. Why could this still make each stage slower? What evidence would you look
   for in a profile (Chapter 20.3's tools), and what does `size_of::<PipelineStats>()` tell you?
2. Why might the three separate `static`s *also* have been adjacent in memory, just by luck of the linker? Is "it used to
   work" evidence of a correct layout?
3. Fix it two ways: with `CachePadded`, and by making each stage own its counter and publish it. Which is better for a
   metrics scraper that reads all three?

### 14. Design exercise

**The Rust session cache.** Meridian's session cache (Chapter 1.3's design exercise, Chapter 9.3's Rust port: 2M
sessions, 16-byte IDs, 200-byte sessions) serves 100K lookups/s and 5K updates/s (logins, logouts, and "touch" updates
that extend expiry) from 32 threads, plus an expiry sweep once a minute.

- Walk the §7 procedure for each piece of state: the session map, the expiry order, the hit/miss counters, and the
  configuration (TTL, max sessions).
- Choose a design and fill in the twelve-row matrix against one alternative you rejected.
- The "touch" update happens on 60% of lookups. Does that change your choice? What would you do to avoid writing on
  every read?
- How does the expiry sweep avoid holding any lock for long (compare §10)?
