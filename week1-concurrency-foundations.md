# Week 1 — Concurrency & Java Foundations (Days 1–7)

The whole week answers one question: **how do you write code that is correct when many threads run it at once?** Every Atlassian LLD round in week 2 assumes you already can.

---

## Day 1 — The Java Memory Model

### Deep Work: threads, `synchronized`, `volatile`, happens-before

**Thread basics.** A thread is created with `new Thread(runnable).start()` (never call `run()` directly — that executes on the *current* thread). Lifecycle states: `NEW → RUNNABLE → (BLOCKED | WAITING | TIMED_WAITING) → TERMINATED`. `BLOCKED` means waiting for a monitor lock; `WAITING` means parked in `wait()`/`join()`/`park()`.

**The three problems concurrency creates:**

1. **Atomicity.** `count++` is three operations (read, add, write). Two threads interleaving them lose updates.
2. **Visibility.** Without synchronization, a write by thread A may *never* become visible to thread B — the value can live in a CPU cache or register indefinitely. The classic bug: a `boolean running` flag that another thread sets to `false`, and the loop never stops.
3. **Ordering.** The compiler and CPU reorder instructions freely as long as *single-threaded* semantics hold. Other threads can observe the reordering.

**The Java Memory Model (JMM)** fixes this with the **happens-before (HB)** relation: if action A happens-before action B, then B sees all of A's writes. The rules you must know cold:

- *Program order:* within one thread, each action HB the next.
- *Monitor lock:* unlocking a monitor HB every subsequent lock of that same monitor. (This is why `synchronized` gives visibility, not just mutual exclusion.)
- *Volatile:* a write to a `volatile` field HB every subsequent read of it.
- *Thread start:* `t.start()` HB every action in `t`.
- *Thread join:* every action in `t` HB `t.join()` returning.
- *Transitivity:* HB composes.

**`synchronized`** takes the intrinsic monitor of an object (`synchronized(obj)` or on `this` for instance methods, on the `Class` object for static methods). It provides mutual exclusion *and* visibility. It is reentrant: a thread holding a monitor may re-acquire it.

**`volatile`** provides visibility and ordering but **not atomicity**. `volatile int x; x++` is still a race. Legit uses: status flags, and *safe publication* — publishing a reference to an immutable object so readers see a fully-constructed object. The famous consequence: double-checked locking is broken *unless* the field is `volatile`.

Interview one-liner: *"`synchronized` = exclusion + visibility. `volatile` = visibility + ordering, no exclusion. `Atomic*` = lock-free atomicity via CAS."*

### Build Lab: producer–consumer, twice

**Version 1 — `wait`/`notify` from scratch.** The rules: you may only call `wait`/`notify`/`notifyAll` while holding the object's monitor (else `IllegalMonitorStateException`); always `wait()` in a `while` loop, never an `if`, because of **spurious wakeups** and because another thread may consume the condition between your wakeup and your re-acquiring the lock; prefer `notifyAll()` when waiters wait on different conditions sharing one monitor.

```java
public final class HandRolledQueue<T> {
    private final Deque<T> items = new ArrayDeque<>();
    private final int capacity;
    public HandRolledQueue(int capacity) { this.capacity = capacity; }

    public synchronized void put(T item) throws InterruptedException {
        while (items.size() == capacity) wait();      // while, not if
        items.addLast(item);
        notifyAll();                                   // wake consumers
    }
    public synchronized T take() throws InterruptedException {
        while (items.isEmpty()) wait();
        T item = items.removeFirst();
        notifyAll();                                   // wake producers
        return item;
    }
}
```

**Version 2 — `BlockingQueue`.** In production you never hand-roll this:

```java
BlockingQueue<Task> q = new ArrayBlockingQueue<>(1024);
// producer:  q.put(task);      blocks when full  (backpressure!)
// consumer:  Task t = q.take(); blocks when empty
```

Narration point for interviews: a *bounded* queue is a backpressure mechanism — an unbounded queue converts overload into an OutOfMemoryError later instead of slowness now.

### DSA Drills — arrays & hashing

**Group Anagrams.** Key idea: canonical key per word — either the sorted word (O(k log k) per word) or a 26-count signature (O(k)).

```java
Map<String, List<String>> groups = new HashMap<>();
for (String s : strs) {
    char[] c = s.toCharArray(); Arrays.sort(c);
    groups.computeIfAbsent(new String(c), k -> new ArrayList<>()).add(s);
}
return new ArrayList<>(groups.values());
```

**Top-K Frequent Elements.** Count with a map, then either a size-k min-heap (O(n log k)) or bucket sort by frequency (O(n)): index `i` of an array of lists holds all numbers appearing `i` times; walk buckets from the top.

**Subarray Sum Equals K.** Prefix sums: at index `i`, the number of subarrays ending at `i` with sum `k` equals how many earlier prefixes had value `prefix − k`.

```java
Map<Integer,Integer> seen = new HashMap<>(); seen.put(0, 1);
int prefix = 0, count = 0;
for (int x : nums) {
    prefix += x;
    count += seen.getOrDefault(prefix - k, 0);
    seen.merge(prefix, 1, Integer::sum);
}
```

### Review — 10 flashcards to write tonight

Why must `wait()` be in a `while` loop? · What two guarantees does `synchronized` give? · Does `volatile` make `x++` safe? · State the volatile HB rule. · State the monitor HB rule. · What does `t.join()` guarantee about visibility? · Why is double-checked locking broken without `volatile`? · Difference between BLOCKED and WAITING? · What is safe publication? · Why is a bounded queue a form of backpressure?

---

## Day 2 — Executors and Thread Pools

### Deep Work: `ThreadPoolExecutor` anatomy

Never spawn raw threads per task; pools amortize thread cost and bound concurrency. `ThreadPoolExecutor(corePoolSize, maximumPoolSize, keepAliveTime, unit, workQueue, threadFactory, rejectionHandler)` — and the admission algorithm interviewers love:

1. Fewer than `corePoolSize` threads running → **create a new thread** for the task.
2. Core full → **offer to the queue**.
3. Queue full and threads < `maximumPoolSize` → **create a non-core thread**.
4. Queue full and at max → **reject** via the handler.

Consequence: with an unbounded `LinkedBlockingQueue`, `maximumPoolSize` is meaningless — step 3 never triggers. `Executors.newFixedThreadPool` does exactly that, which is why serious codebases construct `ThreadPoolExecutor` explicitly with a bounded queue.

**Rejection policies:** `AbortPolicy` (throw — the default), `CallerRunsPolicy` (the submitting thread runs the task itself, a beautiful built-in backpressure valve), `DiscardPolicy`, `DiscardOldestPolicy`.

**Sizing heuristics:** CPU-bound → threads ≈ cores. IO-bound → threads ≈ cores × (1 + wait/compute). Always justify with the workload, never a magic number.

**`ForkJoinPool`** uses per-thread deques with **work stealing** — idle workers steal from the tail of busy workers' deques. It shines for recursive divide-and-conquer and backs parallel streams and `CompletableFuture`'s default async pool (`commonPool`). Don't run blocking IO on the common pool; pass your own executor.

**Shutdown:** `shutdown()` stops accepting and lets queued work finish; `shutdownNow()` interrupts workers and returns the undrained queue; then `awaitTermination(timeout)`. Know all three.

### Build Lab: a thread pool from scratch

```java
public final class MiniThreadPool {
    private final BlockingQueue<Runnable> queue;
    private final List<Worker> workers = new ArrayList<>();
    private volatile boolean shutdown = false;

    public MiniThreadPool(int nThreads, int queueCapacity) {
        queue = new ArrayBlockingQueue<>(queueCapacity);
        for (int i = 0; i < nThreads; i++) {
            Worker w = new Worker("mini-pool-" + i);
            workers.add(w);
            w.start();
        }
    }

    public void submit(Runnable task) {
        if (shutdown) throw new RejectedExecutionException("pool is shut down");
        boolean offered = queue.offer(task);
        if (!offered) throw new RejectedExecutionException("queue full");
    }

    public void shutdown() {                 // graceful: drain queue, then stop
        shutdown = true;
        workers.forEach(Thread::interrupt);  // wake any worker blocked in take()
    }

    private final class Worker extends Thread {
        Worker(String name) { super(name); }
        @Override public void run() {
            while (true) {
                try {
                    Runnable task = queue.take();
                    try { task.run(); } catch (RuntimeException e) { /* log, keep worker alive */ }
                } catch (InterruptedException e) {
                    if (shutdown && queue.isEmpty()) return;   // drain-then-exit
                    // else: interrupted spuriously or mid-drain; loop again
                }
            }
        }
    }
}
```

Narration points: worker threads must survive task exceptions; `volatile shutdown` gives visibility; interrupt is how you wake a blocked `take()`; draining-before-exit is a *policy decision* you should state out loud.

### DSA Drills — two pointers & sliding window

**Longest Substring Without Repeating Characters.** Window `[l, r]`; a map of last-seen indices; when `s[r]` was seen at `i ≥ l`, jump `l = i + 1`. O(n).

```java
int[] last = new int[128]; Arrays.fill(last, -1);
int best = 0, l = 0;
for (int r = 0; r < s.length(); r++) {
    l = Math.max(l, last[s.charAt(r)] + 1);
    best = Math.max(best, r - l + 1);
    last[s.charAt(r)] = r;
}
```

**Container With Most Water.** Two pointers at the ends; area is limited by the shorter wall, so move the shorter pointer inward — moving the taller one can never help.

**Minimum Window Substring.** `need` counts for `t`; expand `r`, and once every needed char is satisfied (`have == needKinds`), shrink `l` while still valid, recording the best window. Each pointer moves at most n times → O(n).

### Review

Read (or summarize from this chapter) the executor-lifecycle material and write five takeaways in your log. Suggested: (1) unbounded queue nullifies max pool size, (2) CallerRuns is backpressure, (3) pool sizing follows the wait/compute ratio, (4) workers must outlive task failures, (5) shutdown is a two-verb protocol.

---

## Day 3 — Locks, Conditions, Deadlock

### Deep Work

**`ReentrantLock` vs `synchronized`.** Same mutual exclusion and memory semantics, plus superpowers: `tryLock()` (non-blocking), `tryLock(timeout)`, `lockInterruptibly()`, optional **fairness** (FIFO handoff — lower throughput, prevents starvation), and **multiple `Condition` objects per lock**. The idiom is non-negotiable:

```java
lock.lock();
try { /* critical section */ } finally { lock.unlock(); }
```

**`Condition`** is `wait`/`notify` for explicit locks: `await()` atomically releases the lock and parks; `signal()`/`signalAll()` wake waiters. Same *while-loop* rule. Two conditions on one lock let producers and consumers wake only the side they mean to — that's today's build.

**`ReadWriteLock`** (`ReentrantReadWriteLock`): many concurrent readers OR one writer. Great for read-heavy structures; beware writer starvation under constant reads (fair mode mitigates). **`StampedLock`** adds *optimistic reads*: read without locking, then `validate(stamp)`; if a write intervened, fall back to a real read lock. Faster, but non-reentrant and easy to misuse — mention it, don't reach for it first.

**Deadlock** requires all four Coffman conditions: mutual exclusion, hold-and-wait, no preemption, circular wait. You prevent it by breaking one — in practice, by breaking *circular wait* with a **global lock ordering** (e.g., always lock the account with the smaller id first), or breaking *hold-and-wait* with `tryLock` + backoff. Detection: `jstack` prints "Found one Java-level deadlock"; programmatically, `ThreadMXBean.findDeadlockedThreads()`.

Write up the classic transfer deadlock and its fix in your notes:

```java
// BROKEN: T1 transfer(a,b) and T2 transfer(b,a) deadlock
void transfer(Account x, Account y, long amt) {
    synchronized (x) { synchronized (y) { /* move money */ } }
}
// FIX: impose a total order
Account first = x.id < y.id ? x : y, second = x.id < y.id ? y : x;
synchronized (first) { synchronized (second) { /* move money */ } }
```

### Build Lab: `BoundedBlockingQueue` with two Conditions

```java
public final class BoundedBlockingQueue<T> {
    private final Object[] items;
    private int head, tail, count;
    private final ReentrantLock lock = new ReentrantLock();
    private final Condition notFull  = lock.newCondition();
    private final Condition notEmpty = lock.newCondition();

    public BoundedBlockingQueue(int capacity) { items = new Object[capacity]; }

    public void put(T t) throws InterruptedException {
        lock.lockInterruptibly();
        try {
            while (count == items.length) notFull.await();
            items[tail] = t;
            tail = (tail + 1) % items.length;
            count++;
            notEmpty.signal();               // exactly one consumer can proceed
        } finally { lock.unlock(); }
    }

    @SuppressWarnings("unchecked")
    public T take() throws InterruptedException {
        lock.lockInterruptibly();
        try {
            while (count == 0) notEmpty.await();
            T t = (T) items[head];
            items[head] = null;              // help GC
            head = (head + 1) % items.length;
            count--;
            notFull.signal();
            return t;
        } finally { lock.unlock(); }
    }

    public int size() { lock.lock(); try { return count; } finally { lock.unlock(); } }
}
```

Why two conditions beat `notifyAll` on one monitor: signals are *targeted*, so a `put` never wakes other producers pointlessly. Say that in the interview.

### DSA Drills — monotonic stack

**Daily Temperatures.** Stack of indices with strictly decreasing temperatures. When a warmer day arrives, pop and answer everything cooler.

```java
Deque<Integer> st = new ArrayDeque<>();
int[] ans = new int[t.length];
for (int i = 0; i < t.length; i++) {
    while (!st.isEmpty() && t[i] > t[st.peek()]) {
        int j = st.pop(); ans[j] = i - j;
    }
    st.push(i);
}
```

**Largest Rectangle in Histogram.** Increasing-height stack of indices; when `h[i]` breaks the increase, pop each bar — its rectangle's width runs from the element *below it on the stack* (exclusive) to `i` (exclusive). Append a sentinel height 0 to flush. O(n). Practice until you can re-derive the width formula `i - stack.peek() - 1` without memorizing it.

### Review — 5 cards

Fair vs unfair lock trade-off · why `tryLock` breaks hold-and-wait · reentrancy definition · the four deadlock conditions · why `unlock` lives in `finally`.

---

## Day 4 — Atomics, CAS, ConcurrentHashMap; the LRU build

### Deep Work

**CAS (compare-and-swap)** is the hardware primitive under all lock-free code: `compareAndSet(expected, next)` atomically writes only if the current value equals `expected`; otherwise you loop ("CAS retry loop"). Lock-free wins under low-to-moderate contention (no context switches, no priority inversion); under fierce contention the retry storm can lose to a lock.

**ABA problem:** value goes A→B→A; CAS sees A and succeeds although the world changed. Fix with a version stamp — `AtomicStampedReference`. In interviews it's enough to name it and the fix.

**`AtomicLong` vs `LongAdder`:** a hot `AtomicLong` counter serializes all threads on one cache line. `LongAdder` stripes across cells and sums on read — use it for high-write, occasional-read metrics.

**`ConcurrentHashMap` (JDK 8+):** no segment locks anymore. Reads are lock-free (volatile reads of a `Node[]` table). Writes: an empty bin is claimed by **CAS**; a populated bin is locked by **synchronizing on its head node** — so contention is per-bin, not global. Long collision chains convert to red-black trees. `size()` is a striped estimate (`LongAdder`-style cells). Two crucial API facts: `computeIfAbsent` is atomic *per key* (your mapping function may run under the bin lock — keep it cheap and never touch the same map inside it), and null keys/values are banned so that `get(k) == null` unambiguously means absent.

**`CopyOnWriteArrayList`:** every mutation copies the backing array; readers iterate a snapshot with zero locking. Perfect for listener lists — tiny write rate, huge read rate.

### Build Lab: thread-safe LRU cache

Interviewers want the raw version — hash map for O(1) lookup, doubly-linked list for O(1) recency ordering — not `LinkedHashMap` (but *mention* that `LinkedHashMap(cap, 0.75f, true)` + `removeEldestEntry` solves it in five lines; knowing the shortcut and building it anyway is the senior move).

```java
public final class ThreadSafeLRUCache<K, V> {
    private static final class Node<K, V> {
        K key; V value; Node<K, V> prev, next;
        Node(K k, V v) { key = k; value = v; }
    }
    private final int capacity;
    private final Map<K, Node<K, V>> map = new HashMap<>();
    private final Node<K, V> head = new Node<>(null, null);  // sentinel MRU side
    private final Node<K, V> tail = new Node<>(null, null);  // sentinel LRU side
    private final ReentrantLock lock = new ReentrantLock();

    public ThreadSafeLRUCache(int capacity) {
        this.capacity = capacity;
        head.next = tail; tail.prev = head;
    }

    public V get(K key) {
        lock.lock();
        try {
            Node<K, V> n = map.get(key);
            if (n == null) return null;
            moveToFront(n);
            return n.value;
        } finally { lock.unlock(); }
    }

    public void put(K key, V value) {
        lock.lock();
        try {
            Node<K, V> n = map.get(key);
            if (n != null) { n.value = value; moveToFront(n); return; }
            if (map.size() == capacity) {
                Node<K, V> lru = tail.prev;
                unlink(lru);
                map.remove(lru.key);
            }
            Node<K, V> fresh = new Node<>(key, value);
            map.put(key, fresh);
            linkFirst(fresh);
        } finally { lock.unlock(); }
    }

    private void moveToFront(Node<K, V> n) { unlink(n); linkFirst(n); }
    private void unlink(Node<K, V> n) { n.prev.next = n.next; n.next.prev = n.prev; }
    private void linkFirst(Node<K, V> n) {
        n.next = head.next; n.prev = head;
        head.next.prev = n; head.next = n;
    }
}
```

Follow-ups you must be ready for: *Why one lock?* Because `get` mutates (recency), so a read-write lock buys nothing. *How to scale?* Segment the cache (stripe by `hash(key) % N` into N independent LRUs) — trades global LRU accuracy for throughput; or go lock-free like Caffeine (window-TinyLFU) — name it, don't build it. Stress-test today with 8 threads hammering `get`/`put` and assert `map.size() <= capacity` throughout.

### DSA Drills — linked lists

**LRU Cache (LeetCode 146)** — you just built the hard version; the single-threaded one is the same minus the lock. Do it from memory. **Merge k Sorted Lists** — min-heap of current heads, O(N log k). **Reverse Nodes in k-Group** — pointer surgery with a dummy head; count k, reverse the segment, reconnect.

### Review

Self-quiz: when does CAS beat a lock? · what breaks ABA? · why is `computeIfAbsent`'s lambda dangerous if it's slow? · why no nulls in CHM? · `AtomicLong` vs `LongAdder`?

---

## Day 5 — Async Composition with CompletableFuture

### Deep Work

The mental model: a `CompletableFuture<T>` is a promise plus a dependency graph of callbacks.

**The map/flatMap distinction (asked constantly):** `thenApply(fn)` transforms a value (`T → U`). `thenCompose(fn)` chains a dependent async call (`T → CompletableFuture<U>`) *without nesting* — using `thenApply` there yields the ugly `CompletableFuture<CompletableFuture<U>>`.

**Combining:** `thenCombine(other, biFn)` joins two independent futures; `allOf(f1, f2, …)` waits for all (returns `CompletableFuture<Void>` — you re-`join()` the originals to collect results); `anyOf` races.

**Errors:** `exceptionally(fn)` = catch-and-recover; `handle((val, err) -> …)` = always runs, sees both; `whenComplete` = side-effect only, doesn't swallow the error.

**Threading:** non-`Async` methods may run on the completing thread; `*Async(fn, executor)` variants run on your executor. Default async pool is `ForkJoinPool.commonPool()` — **never** block in it (it's shared process-wide); pass a dedicated executor for IO.

**Timeouts (Java 9+):** `orTimeout(d, unit)` fails after d; `completeOnTimeout(fallback, d, unit)` degrades gracefully. `cancel()` on a CF does not interrupt the running task — a common trick question.

**Kotlin equivalence** (one paragraph is all you need): `suspend` functions + structured concurrency (`coroutineScope { async { … } }`) replace CF chaining; `Dispatchers.IO` replaces the custom executor; cancellation is cooperative and propagates through the scope tree — an advantage worth naming if you code in Kotlin.

### Build Lab: parallel fetch-and-aggregate pipeline

```java
public final class AggregatorPipeline {
    private final ExecutorService io = Executors.newFixedThreadPool(16);

    /** Fan out to n fake services, apply per-call timeout + fallback, aggregate. */
    public Map<String, String> fetchAll(List<String> serviceIds) {
        List<CompletableFuture<Map.Entry<String, String>>> calls = serviceIds.stream()
            .map(id -> CompletableFuture
                .supplyAsync(() -> slowFetch(id), io)                    // IO on our pool
                .completeOnTimeout("FALLBACK", 800, TimeUnit.MILLISECONDS)
                .exceptionally(ex -> "ERROR:" + ex.getClass().getSimpleName())
                .thenApply(body -> Map.entry(id, body)))
            .toList();

        return CompletableFuture.allOf(calls.toArray(CompletableFuture[]::new))
            .thenApply(v -> calls.stream()
                .map(CompletableFuture::join)                            // safe: all done
                .collect(Collectors.toMap(Map.Entry::getKey, Map.Entry::getValue)))
            .join();
    }

    private String slowFetch(String id) {
        try { Thread.sleep(ThreadLocalRandom.current().nextInt(200, 1200)); }
        catch (InterruptedException e) { Thread.currentThread().interrupt(); throw new RuntimeException(e); }
        return "payload-" + id;
    }
}
```

Log per-call latency and total latency; observe that total ≈ slowest call, not the sum — that sentence is the whole point of async fan-out and worth saying in every design interview.

### DSA Drills — binary search

**Search in Rotated Sorted Array.** One half is always sorted; check which, check whether the target lies in it, discard the other. **Koko Eating Bananas** — the template for *binary search on the answer*: predicate `canFinish(speed)` is monotone, so binary-search the smallest true. Learn this template; it solves a whole family (ship capacity, split array, min days). **Find Peak Element** — move toward the rising neighbor; a peak must exist on that side.

### Review

Mind-map the week so far: JMM → tools (monitor, volatile, locks, atomics) → structures (queues, CHM) → executors → async. Mark your two weakest nodes; they're tomorrow's warm-up.

---

## Day 6 — ThreadLocal, Immutability, GC, Semaphores

### Deep Work

**`ThreadLocal`** gives each thread a private copy — per-request context (user id, trace id) and non-thread-safe helpers (the classic `SimpleDateFormat`). The trap: in *thread pools* the thread outlives the request, so stale values leak into the next request and referenced objects never GC. Always `remove()` in a `finally`. `InheritableThreadLocal` copies to child threads at creation — mostly useless with pools (workers were created long ago), which is exactly the follow-up they'll ask.

**Immutability** is the cheapest concurrency strategy: an object whose state cannot change needs no synchronization. Requirements: all fields `final`, no leaked `this` during construction, defensive copies of mutable inputs. **Safe publication** — making an object visible to other threads with its final-field guarantees intact — happens via: static initializer, `volatile` field, `final` field of a properly published object, or handing it to a concurrent collection / synchronized block. "Effectively immutable" objects (never mutated after publication) are safe *if* safely published.

**GC in ten sentences (the SDE-2 level).** The heap is generational because most objects die young. Allocation happens in the young gen (Eden); minor GCs copy survivors between survivor spaces and promote the long-lived to the old gen; major/full GCs collect the old gen and are the expensive ones. G1 (default) divides the heap into regions and targets a pause goal by collecting the garbage-richest regions first. All collectors still have stop-the-world phases; your job as a backend engineer is mostly to (a) not allocate garbage in hot loops, (b) not leak (caches without bounds, ThreadLocals, listeners), and (c) recognize GC pressure in metrics (rising pause times, promotion rate). ZGC/Shenandoah exist for sub-millisecond pauses — name-drop only.

**`Semaphore`** = a permit counter: `acquire()` blocks until a permit is free, `release()` returns one. It bounds *concurrency* (e.g., at most 10 in-flight calls to a fragile downstream). Fairness option exists. A semaphore initialized to 1 is a non-reentrant mutex — trick-question material.

### Build Lab: concurrency-limiting a client

```java
public final class ThrottledClient {
    private final Semaphore permits;
    public ThrottledClient(int maxConcurrent) { permits = new Semaphore(maxConcurrent, true); }

    public String call(String req) throws InterruptedException {
        if (!permits.tryAcquire(200, TimeUnit.MILLISECONDS))
            throw new RejectedExecutionException("downstream saturated");   // fail fast > pile up
        try {
            return doNetworkCall(req);
        } finally {
            permits.release();
        }
    }
    private String doNetworkCall(String req) { /* simulate IO */ return "ok:" + req; }
}
```

Experiment: fair vs unfair under 50 threads (watch starvation), and blocking `acquire` vs timed `tryAcquire` (queueing vs shedding). Note the conceptual bridge for tomorrow’s week-2 kickoff: a semaphore bounds *concurrent* work; a **rate limiter bounds work per unit time**. Different axes — systems often need both.

### DSA Drills — trees

**Validate BST.** Pass down `(min, max)` bounds; each node must sit strictly inside. (In-order-must-be-increasing is the alternative.)

```java
boolean valid(TreeNode n, long lo, long hi) {
    if (n == null) return true;
    if (n.val <= lo || n.val >= hi) return false;
    return valid(n.left, lo, n.val) && valid(n.right, n.val, hi);
}
```

**LCA in a BST:** walk from the root; the first node between the two values is the answer. **LCA in a binary tree:** post-order — return the node if found in exactly one subtree from each side, else bubble the non-null side. **Level-order traversal:** queue + per-level size loop.

### Review

Timed 20-minute closed-book quiz across days 1–6 (write questions from your flashcards, answer, grade). Anything below 80% goes on the gap list that Day 7's build block starts with.

---

## Day 7 — Warm-up Mock + Testing Concurrency

### Deep Work: timed mock (75 min) — Sliding-Window Hit Counter / Metrics Aggregator

**Prompt:** *Design a thread-safe `HitCounter` supporting `hit(timestampSec)` and `getHits(timestampSec)` returning hits in the trailing 300 seconds. Memory must be O(window), not O(hits).*

Attempt cold first. Reference solution — a **ring buffer of per-second buckets**:

```java
public final class HitCounter {
    private static final int WINDOW = 300;
    private final long[] counts = new long[WINDOW];
    private final long[] seconds = new long[WINDOW];   // which second each bucket holds

    public synchronized void hit(long sec) {
        int i = (int) (sec % WINDOW);
        if (seconds[i] != sec) {          // bucket is stale — reuse it
            seconds[i] = sec;
            counts[i] = 0;
        }
        counts[i]++;
    }

    public synchronized long getHits(long sec) {
        long total = 0;
        for (int i = 0; i < WINDOW; i++)
            if (sec - seconds[i] < WINDOW) total += counts[i];
        return total;
    }
}
```

Narrate: O(1) `hit`, O(window) `getHits`, O(window) memory; lazy bucket reuse avoids any sweeper thread; a single monitor is fine at this granularity, and the scale-up answer is striping (per-thread `LongAdder` buckets merged on read) — say it, don't build it. Grade yourself with the LLD rubric in week 4's appendix.

### Build Lab: concurrency test patterns + refactor

The **start-gun pattern** makes races reproducible-ish by maximizing simultaneity:

```java
@Test void lruNeverExceedsCapacityUnderContention() throws Exception {
    var cache = new ThreadSafeLRUCache<Integer, Integer>(64);
    int threads = 8, opsPerThread = 50_000;
    var ready = new CountDownLatch(threads);
    var go = new CountDownLatch(1);
    var done = new CountDownLatch(threads);
    var pool = Executors.newFixedThreadPool(threads);
    for (int t = 0; t < threads; t++) pool.submit(() -> {
        ready.countDown();
        go.await();
        var rnd = ThreadLocalRandom.current();
        for (int i = 0; i < opsPerThread; i++) {
            int k = rnd.nextInt(512);
            if (rnd.nextBoolean()) cache.put(k, k); else cache.get(k);
        }
        done.countDown();
        return null;
    });
    ready.await(); go.countDown();
    assertTrue(done.await(30, TimeUnit.SECONDS), "possible deadlock");
    // invariant checks after the storm:
    assertTrue(cache.size() <= 64);
}
```

Principles: assert **invariants**, not exact interleavings; repeat runs (races are probabilistic); always bound waits with timeouts so a deadlock fails the test instead of hanging CI; for serious lock-free code the real tool is OpenJDK's jcstress — know the name. Spend the rest of the block adding tests like this to every week-1 build and refactoring what they expose.

### DSA Drills

Timed set, 60 minutes, three unseen mediums from this week's topics. Simulate: no IDE autocomplete reliance, talk aloud, state complexity before coding.

### Review — weekly retro

Three questions in your log: What can I now build cold that I couldn't on Monday? Which topic still requires looking things up? What one change to next week's routine would help most? Carry the gap list into Week 2 — its warm-ups come from here.

**[→ Week 2 — Low-Level Design](week2-low-level-design.md)**
