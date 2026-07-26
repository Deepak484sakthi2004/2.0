# Week 2 — Low-Level Design / Machine Coding (Days 8–14)

This is the signature Atlassian round: 45–75 minutes, a working component from scratch, judged on OO structure, thread safety, error handling, extensibility, and how you narrate. This chapter contains complete reference solutions for the four canonical problems. **Attempt each under the timer before reading its solution.**

## The 6-step LLD method (use it every single time)

1. **Clarify requirements** (5 min): functional ops, scale ("in-memory, single process?"), concurrency ("called from many threads?"), what happens at the limit (block, reject, evict?).
2. **Define the public API** first — interfaces before classes. The interface *is* your contract with the interviewer.
3. **Identify entities and state** — what must be stored, what invariants must hold.
4. **Choose the concurrency unit** — what does one lock protect? Global lock, per-key lock, or lock-free?
5. **Code the happy path**, narrating trade-offs as you type.
6. **Harden**: error states, edge cases, a couple of tests (or test descriptions if time is short).

**SOLID in one breath each** (you'll be asked to justify structure with these): **S**ingle responsibility — one reason to change per class. **O**pen/closed — extend via new implementations, don't edit stable code (today's Strategy interface is the demo). **L**iskov — subtypes must honor the base contract. **I**nterface segregation — small, focused interfaces. **D**ependency inversion — depend on abstractions; inject the clock, don't call `System.nanoTime()` inline (it also makes tests deterministic).

Patterns that actually appear in these four problems: **Strategy** (pluggable rate-limit algorithms), **Factory** (constructing strategies from config), **Observer** (pub-sub itself), **Builder** (fluent config objects), **Template method** (retry wrappers).

---

## Day 8 — Rate Limiter I: Token Bucket

### Requirements to state out loud

Per-client limits; `tryAcquire(clientId)` returns boolean (non-blocking — callers decide to reject/queue); allow controlled bursts; in-memory, single node; thread-safe under many concurrent callers; O(1) memory per client.

### The design

**Token bucket:** a bucket holds up to `capacity` tokens; tokens drip in at `refillRate` per second; a request costs one token; an empty bucket means reject. Capacity = burst allowance, rate = sustained throughput.

The two decisions that make this solution senior-level:

1. **Lazy refill.** No background thread. On each call, compute tokens accrued since the last call: `tokens = min(capacity, tokens + elapsed × rate)`. Zero threads, zero drift, tokens as `double` so fractional accrual isn't lost.
2. **`System.nanoTime()`, never `currentTimeMillis()`.** Wall clocks jump (NTP, DST); `nanoTime` is monotonic. Inject a `Clock`/`LongSupplier` for testability.

**Concurrency unit:** one lock **per bucket**, buckets in a `ConcurrentHashMap` via `computeIfAbsent`. Contention only exists between calls for the *same* client.

### Reference solution

```java
public interface RateLimiter {
    boolean tryAcquire(String clientId);
}

public final class TokenBucketRateLimiter implements RateLimiter {

    private final long capacity;             // max burst
    private final double refillPerNano;      // sustained rate
    private final LongSupplier nanoClock;    // injected for tests
    private final ConcurrentHashMap<String, Bucket> buckets = new ConcurrentHashMap<>();

    public TokenBucketRateLimiter(long capacity, double refillPerSecond, LongSupplier nanoClock) {
        if (capacity <= 0 || refillPerSecond <= 0) throw new IllegalArgumentException("positive values required");
        this.capacity = capacity;
        this.refillPerNano = refillPerSecond / 1_000_000_000.0;
        this.nanoClock = nanoClock;
    }

    public TokenBucketRateLimiter(long capacity, double refillPerSecond) {
        this(capacity, refillPerSecond, System::nanoTime);
    }

    @Override
    public boolean tryAcquire(String clientId) {
        Objects.requireNonNull(clientId, "clientId");
        return buckets.computeIfAbsent(clientId, id -> new Bucket(capacity, nanoClock.getAsLong()))
                      .tryConsume(1);
    }

    private final class Bucket {
        private double tokens;
        private long lastRefillNanos;

        Bucket(long initialTokens, long now) { tokens = initialTokens; lastRefillNanos = now; }

        synchronized boolean tryConsume(int permits) {
            refill();
            if (tokens >= permits) { tokens -= permits; return true; }
            return false;
        }

        private void refill() {
            long now = nanoClock.getAsLong();
            double accrued = (now - lastRefillNanos) * refillPerNano;
            if (accrued > 0) {
                tokens = Math.min(capacity, tokens + accrued);
                lastRefillNanos = now;
            }
        }
    }
}
```

### Narration script (the part that earns the hire)

"I chose lazy refill so there's no scheduler thread and no drift — the bucket is only ever touched by its own callers. The lock is per-bucket, so client A never contends with client B; the map itself is a `ConcurrentHashMap` and `computeIfAbsent` gives me atomic bucket creation. I inject the clock so tests can advance time deterministically. If contention on one hot client became a problem, I'd replace the per-bucket monitor with a CAS loop over a packed `AtomicLong` (tokens and timestamp in one word) — same semantics, lock-free."

**Follow-ups to be ready for:** distributed version (→ Redis + Lua, solved fully on Day 17), `acquire()` that blocks with a deadline (compute wait time = deficit/rate, `Thread.sleep`, re-check), and per-client differing configs (Day 9).

### DSA Drills — heaps

**Kth Largest in a Stream:** keep a min-heap of size k; the root is the answer. **Merge k Sorted Lists:** heap of heads (done Day 4 — redo from memory). **Task Scheduler (cooldown n):** greedy with a max-heap of counts + a cooldown queue of (count, availableTime); or the arithmetic formula `max(tasks, (maxCount−1)(n+1)+numMax)`. Know both.

### Review

Grade the timed attempt against the rubric (week 4 appendix). Write down every follow-up you couldn't answer — Day 9 exists to close them.

---

## Day 9 — Rate Limiter II: Algorithms & the Strategy Refactor

### Deep Work: the algorithm zoo

| Algorithm | How | Accuracy | Memory/client | Bursts |
|---|---|---|---|---|
| Fixed window | counter per window (e.g., per minute), reset at boundary | poor at edges | O(1) | **2× limit** possible straddling a boundary |
| Sliding window **log** | store every request timestamp, count those in window | exact | O(requests) | none beyond limit |
| Sliding window **counter** | current window count + previous window count weighted by overlap | ~exact | O(1) | tiny approximation error |
| Token bucket | refillable token pool | exact for its model | O(1) | **allowed**, capped at capacity (a feature) |
| Leaky bucket | FIFO queue drained at constant rate | exact | O(queue) | **smoothed away** — constant outflow |

The fixed-window edge burst is the standard "why not the simple thing" answer: limit 100/min lets 100 requests at 11:59:59 and 100 more at 12:00:01 — 200 in two seconds. Token bucket *permits* bursts by design (good for user-facing APIs); leaky bucket *shapes* traffic to a constant rate (good for protecting a fragile downstream). Choosing between them **is** the interview.

### Build Lab: pluggable strategies + per-client config

Open/closed principle made concrete — new algorithms plug in without touching the limiter:

```java
public interface RateLimitStrategy {
    boolean allow(ClientState state, long nowNanos);
    ClientState newState(long nowNanos);
}

public final class ConfigurableRateLimiter implements RateLimiter {
    private final ConcurrentHashMap<String, ClientState> states = new ConcurrentHashMap<>();
    private final Function<String, RateLimitStrategy> strategyForClient;   // per-client config
    private final LongSupplier nanoClock;

    public ConfigurableRateLimiter(Function<String, RateLimitStrategy> strategyForClient,
                                   LongSupplier nanoClock) {
        this.strategyForClient = strategyForClient;
        this.nanoClock = nanoClock;
    }

    @Override public boolean tryAcquire(String clientId) {
        long now = nanoClock.getAsLong();
        RateLimitStrategy s = strategyForClient.apply(clientId);
        ClientState st = states.computeIfAbsent(clientId, id -> s.newState(now));
        synchronized (st) { return s.allow(st, now); }
    }
}
```

Implement `TokenBucketStrategy` (port yesterday's math into `allow`) and `SlidingWindowLogStrategy` (an `ArrayDeque<Long>` of timestamps; evict `< now − window`, admit if `size < limit`). Stress test: 16 threads, one shared client, assert admitted count over 10 s ≈ rate × 10 ± capacity.

### DSA Drills — intervals

**Merge Intervals:** sort by start; extend or emit. **Insert Interval:** three phases — copy the before-part, merge overlaps into the new interval, copy the after-part. **Non-overlapping Intervals (min removals):** sort by **end**, greedily keep the earliest-ending; removals = total − kept. The sort-by-end greedy is the transferable idea.

### Review

Write the README for the limiter repo: the comparison table above, why token bucket is the default, and the one-paragraph extensibility story. Explaining it in writing is rehearsal for explaining it aloud.

---

## Day 10 — Key-Value Store with TTL

### Deep Work: expiry design space

`put(k, v, ttl)`, `get(k)`, `delete(k)`. The entire problem is *when do expired entries actually leave memory?*

- **Lazy (on-read) expiry:** `get` checks the deadline and deletes if past. O(0) background cost, but keys nobody reads again **leak forever**. Necessary, insufficient.
- **Active sweeping:** a background pass removes expired entries. Variants: full scan (O(n) periodically — fine for interviews, say so), **random sampling** (Redis's approach: sample 20 keys with TTLs, delete the expired, repeat if >25% were expired — cost proportional to expired fraction, not to n), **min-heap of (deadline, key)** (exact and ordered, O(log n) per op, but a re-`put` leaves a stale heap entry you must detect on pop), **hierarchical timer wheel** (O(1) insert/expire buckets by time slot — what Kafka and Netty use; name it, don't build it).

The senior answer, and Redis's actual answer: **lazy + sampling sweeper together.** Correctness never depends on the sweeper (lazy check guards every read); the sweeper only bounds memory.

The subtle race to narrate: sweeper decides key K is expired → user calls `put(K, fresh)` → sweeper deletes the *fresh* entry. Fix: delete with the two-arg `remove(key, expectedEntry)`, which only removes if the map still holds the exact entry object the sweeper examined.

### Reference solution

```java
public final class TtlKeyValueStore<K, V> implements AutoCloseable {

    private record Entry<V>(V value, long expiresAtNanos) {
        boolean isExpired(long now) { return now >= expiresAtNanos; }
    }

    private final ConcurrentHashMap<K, Entry<V>> map = new ConcurrentHashMap<>();
    private final LongSupplier nanoClock;
    private final ScheduledExecutorService sweeper;

    public TtlKeyValueStore(Duration sweepInterval, LongSupplier nanoClock) {
        this.nanoClock = nanoClock;
        this.sweeper = Executors.newSingleThreadScheduledExecutor(r -> {
            Thread t = new Thread(r, "ttl-sweeper");
            t.setDaemon(true);
            return t;
        });
        sweeper.scheduleAtFixedRate(this::sweep,
                sweepInterval.toNanos(), sweepInterval.toNanos(), TimeUnit.NANOSECONDS);
    }

    public void put(K key, V value, Duration ttl) {
        Objects.requireNonNull(key); Objects.requireNonNull(value);
        if (ttl.isNegative() || ttl.isZero()) throw new IllegalArgumentException("ttl must be positive");
        map.put(key, new Entry<>(value, nanoClock.getAsLong() + ttl.toNanos()));
    }

    /** Lazy expiry: correctness never depends on the sweeper. */
    public V get(K key) {
        Entry<V> e = map.get(key);
        if (e == null) return null;
        if (e.isExpired(nanoClock.getAsLong())) {
            map.remove(key, e);              // two-arg: don't kill a fresh re-put
            return null;
        }
        return e.value();
    }

    public void delete(K key) { map.remove(key); }
    public int size() { return map.size(); }   // may include not-yet-swept expired entries — document it

    private void sweep() {
        long now = nanoClock.getAsLong();
        for (Map.Entry<K, Entry<V>> me : map.entrySet())
            if (me.getValue().isExpired(now))
                map.remove(me.getKey(), me.getValue());   // same race-safe removal
    }

    @Override public void close() { sweeper.shutdownNow(); }
}
```

**Adding LRU eviction (the standard extension):** bound entries with `maxSize`; on insert past the bound, evict least-recently-used. Combining TTL + LRU cleanly needs the Day-4 linked-list structure under one lock, with expiry checks at the same points — build it as an exercise by merging the two classes; state in the interview that at production scale you'd reach for Caffeine, which does TTL + size + TinyLFU properly.

### DSA Drills — tries

**Implement Trie:** node = `children[26]` (or map) + `isWord`. Insert/search/startsWith are straight walks. **Word Search II:** build a trie of the dictionary, DFS the board *through the trie* (prune branches with no trie child); mark found words and clip found leaves to avoid duplicates. This trie-guided-DFS pattern is the takeaway.

### Review

Three differences vs Redis to write down: Redis sweeps by *sampling* (cost independent of keyspace size); Redis expiry resolution is ms and replicated as explicit `DEL` to replicas (replicas never expire independently — consistency!); Redis also bounds memory with eviction policies (`allkeys-lru`, `volatile-ttl`) — TTL and eviction are separate axes.

---

## Day 11 — Thread-Safe Pub-Sub Broker

### Deep Work: the design space

`createTopic(name)`, `publish(topic, msg)`, `subscribe(topic, handler)` — then the real questions: **Push or pull?** Push (broker calls handlers) is simpler but a slow handler can stall the broker unless each subscriber gets its own delivery thread. Pull (subscribers poll offsets) is Kafka's model and makes **replay** natural. **Delivery semantics?** In-process: effectively at-least-once if a handler can fail and be retried; exactly-once is a lie you should never claim casually. **Slow consumers?** Isolate them: per-subscriber thread + per-subscriber cursor means one slow consumer never blocks the topic or its peers — this is the bulkhead pattern in miniature, and saying "bulkhead" here scores.

Design chosen: **append-only in-memory log per topic + per-subscription cursor + per-subscription dispatch thread.** It gives ordering per topic, replay-from-offset, and slow-consumer isolation in ~120 lines.

### Reference solution

```java
public final class Message {
    public final long offset; public final Object payload; public final long timestampNanos;
    Message(long offset, Object payload, long ts) { this.offset = offset; this.payload = payload; this.timestampNanos = ts; }
}

public interface MessageHandler { void onMessage(Message m) throws Exception; }

final class Topic {
    private final List<Message> log = new ArrayList<>();     // append-only, guarded by 'this'

    synchronized long append(Object payload, long now) {
        long offset = log.size();
        log.add(new Message(offset, payload, now));
        notifyAll();                                          // wake blocked readers
        return offset;
    }
    /** Blocks until a message exists at 'offset'. */
    synchronized Message read(long offset) throws InterruptedException {
        while (offset >= log.size()) wait();
        return log.get((int) offset);
    }
    synchronized long endOffset() { return log.size(); }
}

public final class Subscription {
    private final Topic topic;
    private final MessageHandler handler;
    private final Thread worker;
    private volatile long cursor;
    private volatile boolean active = true;

    Subscription(String name, Topic topic, MessageHandler handler, long startOffset) {
        this.topic = topic; this.handler = handler; this.cursor = startOffset;
        this.worker = new Thread(this::runLoop, "sub-" + name);
        worker.start();
    }

    private void runLoop() {
        while (active) {
            try {
                Message m = topic.read(cursor);
                try {
                    handler.onMessage(m);
                    cursor++;                                  // advance only after success:
                } catch (Exception handlerFailure) {           //   at-least-once, retry same msg
                    Thread.sleep(100);                         //   naive backoff; Day 12 does it right
                }
            } catch (InterruptedException e) {
                if (!active) return;
            }
        }
    }

    public void replayFrom(long offset) { cursor = offset; worker.interrupt(); }
    public void cancel() { active = false; worker.interrupt(); }
}

public final class Broker {
    private final ConcurrentHashMap<String, Topic> topics = new ConcurrentHashMap<>();
    private final LongSupplier nanoClock;
    public Broker(LongSupplier nanoClock) { this.nanoClock = nanoClock; }

    public long publish(String topicName, Object payload) {
        return topics.computeIfAbsent(topicName, t -> new Topic())
                     .append(payload, nanoClock.getAsLong());
    }
    public Subscription subscribe(String subName, String topicName, MessageHandler handler,
                                  boolean fromBeginning) {
        Topic t = topics.computeIfAbsent(topicName, x -> new Topic());
        long start = fromBeginning ? 0 : t.endOffset();
        return new Subscription(subName, t, handler, start);
    }
}
```

**Narrate the trade-offs:** the log grows without bound (fix: retention — trim below the min cursor across subscriptions); cursor-advance-after-success gives at-least-once, so handlers must be idempotent; "consumer group" = several members sharing one cursor with a lock around read-and-advance — describe it, add it if time remains. **Failure-mode table for the evening block:** slow consumer (isolated by design — only its own lag grows), handler crash (message redelivered — duplicates possible), broker crash (in-memory ⇒ everything lost; durability = write-ahead log, which is Kafka's whole thesis and your week-3 segue).

### DSA Drills — graphs

**Number of Islands:** DFS/BFS flood-fill, mark visited in place. **Course Schedule (cycle detection):** Kahn's BFS — compute indegrees, repeatedly remove zero-indegree nodes; if processed < n there's a cycle. Also know the DFS-with-colors (white/gray/black) version — a gray→gray edge is a cycle.

### Review

Complete the failure-mode table above with one mitigation each, in your own words.

---

## Day 12 — Job Scheduler

### Deep Work: the design

`schedule(task, delay)`, `scheduleRecurring(task, interval)`, `cancel(id)` — plus retries with backoff. Core machinery: a **`DelayQueue<ScheduledJob>`** (a thread-safe min-heap keyed by time; `take()` blocks until the head is due) feeding a **dispatcher thread** that hands due jobs to a **worker pool** (execution must never block dispatch — a long job must not delay other due jobs).

Two semantics you must distinguish unprompted: **fixed-rate** (next run = *scheduled* time + period; runs can bunch up after a stall) vs **fixed-delay** (next run = *completion* time + delay; drifts but never overlaps itself). And **cancellation** is a flag checked at dequeue and before execution — lazily skipping cancelled jobs beats O(n) queue removal.

**Retries:** on failure, re-enqueue with `backoff = base × 2^attempt + jitter`, up to `maxAttempts`, then park the job in a dead-letter list. Jitter prevents synchronized retry storms — the same argument reappears at system scale on Day 23.

### Reference solution

```java
public final class JobScheduler implements AutoCloseable {

    public interface Job { void run() throws Exception; }

    private static final AtomicLong IDS = new AtomicLong();

    private final class ScheduledJob implements Delayed {
        final long id; final Job job;
        final long periodNanos;            // >0 ⇒ recurring (fixed-rate)
        final int attempt;                 // retry count for this execution
        volatile long runAtNanos;
        volatile boolean cancelled;

        ScheduledJob(long id, Job job, long runAt, long period, int attempt) {
            this.id = id; this.job = job; this.runAtNanos = runAt;
            this.periodNanos = period; this.attempt = attempt;
        }
        @Override public long getDelay(TimeUnit unit) {
            return unit.convert(runAtNanos - nanoClock.getAsLong(), TimeUnit.NANOSECONDS);
        }
        @Override public int compareTo(Delayed o) {
            return Long.compare(runAtNanos, ((ScheduledJob) o).runAtNanos);
        }
    }

    private final DelayQueue<ScheduledJob> queue = new DelayQueue<>();
    private final ConcurrentHashMap<Long, ScheduledJob> live = new ConcurrentHashMap<>();
    private final ExecutorService workers;
    private final Thread dispatcher;
    private final LongSupplier nanoClock;
    private final int maxAttempts;
    private final long baseBackoffNanos;
    private final List<ScheduledJob> deadLetters = new CopyOnWriteArrayList<>();
    private volatile boolean running = true;

    public JobScheduler(int workerThreads, int maxAttempts, Duration baseBackoff, LongSupplier clock) {
        this.workers = Executors.newFixedThreadPool(workerThreads);
        this.nanoClock = clock;
        this.maxAttempts = maxAttempts;
        this.baseBackoffNanos = baseBackoff.toNanos();
        this.dispatcher = new Thread(this::dispatchLoop, "scheduler-dispatcher");
        this.dispatcher.start();
    }

    public long schedule(Job job, Duration delay) {
        return enqueue(job, nanoClock.getAsLong() + delay.toNanos(), 0, 0);
    }
    public long scheduleRecurring(Job job, Duration every) {
        return enqueue(job, nanoClock.getAsLong() + every.toNanos(), every.toNanos(), 0);
    }
    public boolean cancel(long id) {
        ScheduledJob j = live.remove(id);
        if (j == null) return false;
        j.cancelled = true;                          // lazy removal: skipped at dequeue
        return true;
    }

    private long enqueue(Job job, long runAt, long period, int attempt) {
        long id = IDS.incrementAndGet();
        ScheduledJob sj = new ScheduledJob(id, job, runAt, period, attempt);
        live.put(id, sj);
        queue.put(sj);
        return id;
    }

    private void dispatchLoop() {
        while (running) {
            try {
                ScheduledJob j = queue.take();       // blocks until head is due
                if (j.cancelled) { live.remove(j.id); continue; }
                workers.submit(() -> execute(j));    // never run jobs on dispatcher
            } catch (InterruptedException e) {
                if (!running) return;
            }
        }
    }

    private void execute(ScheduledJob j) {
        try {
            j.job.run();
            if (j.periodNanos > 0 && !j.cancelled) {          // fixed-rate reschedule
                j.runAtNanos += j.periodNanos;
                queue.put(j);
            } else live.remove(j.id);
        } catch (Exception failure) {
            if (j.attempt + 1 >= maxAttempts) {
                live.remove(j.id);
                deadLetters.add(j);                            // give operators something to inspect
            } else {
                long jitter = ThreadLocalRandom.current().nextLong(baseBackoffNanos / 2 + 1);
                long backoff = (baseBackoffNanos << j.attempt) + jitter;   // base·2^attempt + jitter
                enqueue(j.job, nanoClock.getAsLong() + backoff, j.periodNanos, j.attempt + 1);
                live.remove(j.id);
            }
        }
    }

    @Override public void close() {
        running = false;
        dispatcher.interrupt();
        workers.shutdown();
    }
}
```

Narration: DelayQueue = heap + blocking-take, so dispatch is O(log n) and idle-cheap; dispatcher/worker separation keeps slow jobs from delaying due jobs; recurring here is fixed-rate — show where fixed-delay would differ (re-enqueue in `execute` *after* `run()` with `now + period`); tasks must be **idempotent** because retry means re-run. Cron-expression parsing is "a parser producing the next `runAt`; out of scope but it plugs into `enqueue` unchanged" — perfect extensibility answer.

### DSA Drills — topological order & union-find

**Course Schedule II:** Kahn's algorithm; the pop order *is* the topological order. **Accounts Merge:** union emails sharing an account via union-find (path compression + union by rank), then group by root. Write union-find from memory — it recurs everywhere:

```java
int find(int x){ while (p[x]!=x){ p[x]=p[p[x]]; x=p[x]; } return x; }
void union(int a,int b){ int ra=find(a), rb=find(b); if (ra!=rb) p[ra]=rb; }
```

### Review

Idempotency cheat sheet, in your own words: natural idempotency (setting a value) vs idempotency keys (dedup store of processed ids) vs conditional writes (version checks). At-least-once delivery + idempotent processing = the only honest "exactly-once."

---

## Day 13 — Hardening: Tests, Races, and the Cold Redo

**Cold redo (60 min):** pick Rate Limiter or Pub-Sub — whichever felt shakier — blank file, full timer, using the 6-step timeboxes: 5 clarify / 10 interfaces / 35 code / 10 harden-narrate. Compare against your Day 8/11 version: you should be faster *and* cleaner. If not, diagnose which step ate the time.

**The concurrency-test playbook** (apply to all four projects today):

1. **Start-gun** (`CountDownLatch` ready/go/done — Day 7 template) to maximize interleaving.
2. **Assert invariants, not schedules:** limiter admits ≤ rate×t+capacity; KV store never returns an expired value (inject a controllable fake clock and advance it!); pub-sub delivers every offset to every subscriber exactly-in-order (collect per-subscriber lists, compare); scheduler never runs a cancelled job, always dead-letters after maxAttempts.
3. **Repeat** — run racy tests 100–1000× (`@RepeatedTest`); a race that shows up 1-in-50 runs is still a bug.
4. **Timeout every await** so deadlocks fail instead of hanging.
5. The injected `LongSupplier` clock is what makes TTL and scheduler tests deterministic — this is why Day 8 injected it.

Close the day making the repo presentable: one top-level README linking all four components, each with its own short design doc (requirements → API → concurrency unit → trade-offs → failure modes). This repo is now an interview artifact.

---

## Day 14 — Full Mock + the Prompt Bank

**Run the mock first (75 min, recorded), choosing a prompt you have NOT read the outline for.** Then compare.

**Prompt A — Logger Rate Limiter** (warm-up tier): `shouldPrint(message, timestamp)` — each unique message at most once per 10 s. Solution: map message → last-printed time; print iff absent or `now − last ≥ 10`; bound memory by evicting old entries (sweep or LRU) — the memory question is the real test.

```java
private final ConcurrentHashMap<String, Long> lastPrinted = new ConcurrentHashMap<>();
public boolean shouldPrint(String msg, long now) {
    Long prev = lastPrinted.putIfAbsent(msg, now);
    if (prev == null) return true;
    if (now - prev >= 10) return lastPrinted.replace(msg, prev, now);   // CAS-style: exactly one wins
    return false;
}
```

**Prompt B — In-Memory File System** (structure tier): `mkdir(path)`, `addFile(path, content)`, `ls(path)`, `readFile(path)`. Composite pattern: `Node { name; Map<String,Node> children; StringBuilder content; boolean isFile; }`; path parsing = split on `/`, walk, create-as-needed for mkdir; `ls` on a file returns just its name (classic edge case); sort children (use `TreeMap`). Concurrency: a single RW-lock on the tree is the honest v1; per-directory locks invite deadlock — say why you didn't.

**Prompt C — Parking Lot with concurrency** (modeling tier): spot types, vehicle types, ticketing. Model spots-per-type as `Semaphore` counts for admission plus a `ConcurrentLinkedQueue` of free spot ids per type for assignment; `Ticket` records (vehicle, spot, entryTime); fee strategy = Strategy pattern. The semaphore-guards-inventory idea is the insight worth remembering.

**Grade yourself** on the seven axes (5 = strong hire): requirements clarified · interfaces before classes · thread-safety unit chosen and justified · error/edge handling · extensibility story · tests (written or precisely described) · narration quality. Anything ≤ 3 becomes a week-4 mock focus.

**Evening retro:** update the gap list; pre-read nothing of week 3 — rest matters more.

**[→ Week 3 — System Design & the Data Layer](week3-system-design-and-data.md)**
