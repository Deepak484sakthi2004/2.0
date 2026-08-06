# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 22 of 63 — Phase 1: Foundations (Module 22 of 28)
**Module:** Performance Tuning Techniques
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Every system-design interview eventually turns into "it works, now make it fast and cheap." The candidates who get the SDE3 offer don't reach for "add more servers" first — they reach for the ten levers in this lesson, each of which can cut latency or cost by an order of magnitude on the *same* hardware. A single connection pool can turn a 30 ms-per-query overhead into 0.1 ms. One batch call can replace 1,000 network round-trips with one. These are the moves that separate "I've read blog posts" from "I've tuned production." We group the ten techniques into four families: **do work concurrently**, **do fewer/cheaper round-trips**, **defer or precompute work**, and **reshape data and reads**.

## Learning Objectives
By the end of this lesson you can:
- Explain when multithreading vs async I/O wins, using the CPU-bound vs I/O-bound distinction.
- Quantify why connection pooling, batching, compression, and pagination cut latency, with real numbers.
- Decide when lazy loading and precomputation move work off the critical path.
- Use read replicas to scale reads and explain the replication-lag trade-off.
- Justify denormalization as a read-optimization and name what it costs on writes.
- Combine these levers to take an endpoint from seconds to tens of milliseconds.

## The Lesson

--- FAMILY 1: DO WORK CONCURRENTLY ---

### Multithreading
**What it is (plain English):** Running multiple threads so work proceeds in parallel. On a multi-core CPU, threads can run truly simultaneously — 8 cores can crunch 8 chunks of a computation at once. A thread is a lightweight unit of execution inside a process, sharing memory with its siblings.

**The problem it solves:** A single thread uses one core; the other 7 sit idle. For CPU-bound work (image resizing, encryption, aggregation), parallelizing across cores gives near-linear speedup.

**How it works (mechanics):** Split the work, run it on a thread pool sized to core count, then join results. Resizing 800 images at 25 ms each: single-threaded = 800 × 25 = 20,000 ms. On 8 cores with a pool of 8 = 20,000 / 8 ≈ **2,500 ms**, an 8x win.
```
1 thread:  [img1][img2]...[img800]  20.0 s
8 threads: [img1..100]  (per core)   2.5 s
           [img101..200]
           ... x8 cores in parallel
```
**Trade-offs / when NOT to use it:** Shared mutable state needs locks → risk of race conditions, deadlocks, and lock contention that erases the gains. Threads cost memory (~1 MB stack each) and context-switch overhead. Spawning 10,000 threads for I/O-bound waiting is wasteful — that's async I/O's job. In Python, the GIL blocks true CPU parallelism in one process (use multiprocessing).

**Where you'll see it:** Web-server request handling (Tomcat thread pools), parallel stream processing, video transcoding, ML training data loaders.

### Async I/O
**What it is (plain English):** Instead of a thread blocking while it waits for the network or disk, async I/O lets a single thread fire off a request and move on to other work, handling the response via a callback/event loop when it arrives. One thread juggles thousands of in-flight waits.

**The problem it solves:** For I/O-bound work, threads spend ~99% of their time *waiting*, not computing. A thread-per-connection model with 10,000 connections needs 10,000 threads (~10 GB of stack memory) mostly sleeping. Async serves the same load with a handful of threads.

**How it works (mechanics):** An event loop (epoll/kqueue under the hood) registers interest in many sockets and processes whichever becomes ready. Node.js, Netty, Go, and Python asyncio use this.
```
Blocking:  thread waits 50 ms per call, does nothing else.
Async: one loop, 2,000 sockets, each ~50 ms wait
  effective throughput ~ 2,000 / 0.05 s = 40,000 req/s per loop thread
```
Nginx serving **10,000+ concurrent connections** on a handful of worker processes is the canonical example (the "C10K" problem, solved with async).

**Trade-offs / when NOT to use it:** Async gives you no help on CPU-bound work — a long computation on the event-loop thread blocks *everything* (a stalled Node.js server). Code is harder to reason about (callback/promise chains, "colored functions"). Use threads/processes for CPU work, async for I/O fan-out.

**Where you'll see it:** Nginx, Node.js, Netflix's Netty gateways, Go's goroutine scheduler (async-like), Redis's single-threaded event loop.

--- FAMILY 2: FEWER / CHEAPER ROUND-TRIPS ---

### Connection Pooling
**What it is (plain English):** Keep a set of already-open database (or HTTP) connections in a pool and reuse them, instead of opening a fresh connection per request and tearing it down after.

**The problem it solves:** Establishing a new DB connection is expensive — TCP handshake + TLS + auth can be **20–50 ms** and consume server resources. At 1,000 requests/second, opening a connection each time is both slow and can exhaust the database's connection limit.

**How it works (mechanics):** On startup, open (say) 20 connections and park them. A request borrows one, runs its query, returns it. Borrow/return is a near-instant handoff (~0.05 ms).
```
No pool:  connect(30ms) + query(2ms) + close = 32 ms/request
Pool:     borrow(0.05ms) + query(2ms) + return = ~2.05 ms/request
=> ~15x lower per-request overhead
```
**Trade-offs / when NOT to use it:** A pool that's too small becomes a bottleneck (requests queue for a free connection); too large overwhelms the DB (Postgres each connection ~ a backend process, ~5–10 MB). Idle connections can go stale and need validation. Sizing rule of thumb: pool ≈ cores × 2–4, not "hundreds."

**Where you'll see it:** HikariCP (Java), PgBouncer, SQLAlchemy pools, every ORM. Serverless functions famously break pooling (each invocation is isolated) → PgBouncer/RDS Proxy exist to fix it.

### Batching
**What it is (plain English):** Group many small operations into one larger request. Instead of 1,000 individual inserts or API calls, send one call carrying 1,000 items.

**The problem it solves:** Every request pays fixed overhead — network round-trip (~0.5–1 ms in-datacenter, ~50 ms cross-region), query parsing, TLS. That per-call tax dominates when payloads are tiny. Batching amortizes it across many items.

**How it works (mechanics):** Buffer items and flush when the batch hits a size or time threshold (e.g., 500 items or 10 ms, whichever first).
```
1,000 inserts, 1 ms round-trip each = 1,000 ms
1 batch insert of 1,000 rows       = ~15 ms
=> ~65x faster
```
Kafka producers, DB bulk inserts, and GraphQL DataLoader all batch. Redis pipelining sends N commands without waiting for each reply.

**Trade-offs / when NOT to use it:** Batching adds latency for the *first* item (it waits for the batch to fill) — bad for low-latency single-item paths. Bigger batches raise memory use and make partial failures messier ("which of the 1,000 failed?"). Real-time chat wants small/no batching; bulk imports want large batches.

**Where you'll see it:** Kafka, Elasticsearch `_bulk`, DynamoDB `BatchWriteItem`, GraphQL DataLoader, logging agents.

### Pagination
**What it is (plain English):** Return large result sets in small pages instead of all at once — "give me 50 rows starting after this point" rather than "give me all 2 million."

**The problem it solves:** Loading a huge result blows up memory, latency, and payload size. Returning 2M rows might be 500 MB and 30 seconds; nobody scrolls that far anyway.

**How it works (mechanics):** Two styles. **Offset pagination**: `LIMIT 50 OFFSET 100000` — simple but the DB scans and discards the first 100,000 rows, so deep pages get slow (O(offset)). **Cursor (keyset) pagination**: `WHERE id > last_seen_id ORDER BY id LIMIT 50` — uses the index to jump straight to the spot, O(log n), constant per page.
```
OFFSET 1,000,000 LIMIT 50  -> scans 1,000,050 rows (~slow, secs)
WHERE id > 1,000,000 LIMIT 50 -> index seek + 50 rows (~1 ms)
```
**Trade-offs / when NOT to use it:** Offset pagination is easy and allows "jump to page 500," but degrades on deep pages and can skip/duplicate rows if data changes mid-scroll. Cursor pagination is fast and stable but only supports next/prev, not random page jumps.

**Where you'll see it:** Every API with lists — Twitter/X timelines (cursor), Stripe (`starting_after`), GitHub API, infinite-scroll feeds.

### Compression
**What it is (plain English):** Shrink data before sending or storing it, then decompress on the other side. Fewer bytes over the wire and on disk.

**The problem it solves:** Network bandwidth and disk are finite and often the bottleneck. A 1 MB JSON response over a 10 Mbps mobile link takes ~800 ms just to transfer; compressed to 150 KB it's ~120 ms.

**How it works (mechanics):** Algorithms find and encode redundancy. **gzip** typically compresses text/JSON **3–5x**; **Brotli** a bit more for web assets; **Snappy/LZ4** compress less (~2x) but are extremely fast (used inside databases and Kafka where CPU time matters more than ratio); **Zstandard (zstd)** offers tunable ratio-vs-speed.
```
JSON payload: 1,000 KB
gzip (ratio ~5x) -> 200 KB
transfer over 10 Mbps: 800 ms -> 160 ms
CPU cost to gzip: ~5-15 ms  => net big win
```
**Trade-offs / when NOT to use it:** Compression burns CPU on both ends and adds latency for the compress/decompress step — pointless for tiny payloads (<1 KB, overhead exceeds savings) or already-compressed data (JPEG, MP4, encrypted blobs won't shrink). Pick fast codecs (LZ4/Snappy) when CPU is scarce, high-ratio (Brotli/gzip-9) when bandwidth is scarce.

**Where you'll see it:** HTTP `Content-Encoding: gzip/br`, Kafka `compression.type=snappy`, Parquet/ORC columnar files, database page compression.

--- FAMILY 3: DEFER OR PRECOMPUTE WORK ---

### Lazy Loading
**What it is (plain English):** Don't fetch or compute something until it's actually needed. Load the visible part now; load the rest on demand.

**The problem it solves:** Eager loading everything upfront wastes time and bandwidth on data the user may never look at. A product page with 200 reviews and 50 images shouldn't fetch all of them before first paint.

**How it works (mechanics):** Load the critical view first; trigger further fetches on interaction/scroll. On the web, `<img loading="lazy">` fetches images only as they near the viewport; in ORMs, a lazily-loaded relation issues its query only when the field is accessed.
```
Eager: fetch product + 200 reviews + 50 imgs = 3.2 s to first paint
Lazy:  fetch product + first 10 reviews + 6 imgs = 0.4 s;
       rest loads on scroll
```
**Trade-offs / when NOT to use it:** In ORMs, naive lazy loading causes the infamous **N+1 query problem** — loading 100 orders then lazily loading each order's customer = 1 + 100 = 101 queries. Fix with eager `JOIN`/batch fetch when you know you'll need the data. Lazy loading also adds latency at the moment of access (a stutter mid-scroll).

**Where you'll see it:** Hibernate/JPA lazy associations, React `lazy()`/code-splitting, image lazy-loading, infinite scroll.

### Precomputation
**What it is (plain English):** Do expensive work *ahead of time* and store the result, so reads are cheap lookups instead of live computation. Compute once, serve many times.

**The problem it solves:** Recomputing the same expensive aggregate (a leaderboard, a daily report, a feed) on every request is wasteful and slow. If 1M users request a leaderboard that takes 2 s to compute, computing it live is a disaster; computing it once per minute and caching it is trivial.

**How it works (mechanics):** A background job (cron/stream processor) computes the result and writes it to a cache or table; reads become O(1) lookups. Feed **fan-out-on-write** (precompute each follower's feed when a post is made) is a classic example.
```
Live: leaderboard = ORDER BY score over 50M rows = ~2,000 ms/request
Precomputed: job runs every 60 s, writes top-100 to Redis
Read = 1 lookup = ~1 ms  => ~2,000x faster reads
```
**Trade-offs / when NOT to use it:** Precomputed data is **stale** between refreshes (the leaderboard lags up to 60 s) and costs storage + compute even for results nobody reads. For a celebrity with 100M followers, fan-out-on-write precomputes into 100M feeds — sometimes worse than computing on read (hybrid models fix this). Don't precompute rarely-read or highly-personalized-and-volatile data.

**Where you'll see it:** Materialized views, feed fan-out (Twitter/X, Instagram), dashboard rollups, `COUNT` caches, ML feature stores.

--- FAMILY 4: RESHAPE DATA AND READS ---

### Read Replicas
**What it is (plain English):** Copies of your database that serve read queries, so the primary handles writes and replicas absorb the read load. Reads usually outnumber writes 10:1 or more, so this scales the bottleneck.

**The problem it solves:** A single primary caps your read throughput and competes reads against writes. Add 3 replicas and you roughly 4x read capacity without sharding.

**How it works (mechanics):** The primary streams its write-ahead log to replicas, which apply it. The app routes writes to the primary and reads to replicas (often round-robin).
```
Primary: 5,000 writes/s + 45,000 reads/s = overloaded
With 3 replicas: primary 5,000 w/s; reads spread 15,000/s each
Replication lag: typically 10-500 ms behind primary
```
**Trade-offs / when NOT to use it:** **Replication lag** means a replica can serve stale data — write to primary, immediately read from a replica, and you might not see your own write (read-your-writes violation). Mitigate by reading from the primary for just-written data or routing a user's session to the primary briefly. Replicas add cost and don't help write scaling (that needs sharding).

**Where you'll see it:** Postgres/MySQL read replicas, Amazon RDS/Aurora, Vitess, virtually every read-heavy web app.

### Denormalization
**What it is (plain English):** Deliberately store redundant data (or pre-joined data) so a read doesn't need expensive joins. The opposite of textbook normalization — you trade storage and write-complexity for read speed.

**The problem it solves:** Joins across large tables are expensive. If every profile view joins users × posts × comments × likes, that's slow at scale. Storing a denormalized "profile document" makes the read a single lookup.

**How it works (mechanics):** Duplicate the needed fields into the read table. E.g., store `author_name` alongside each post instead of joining to `users` every read.
```
Normalized: SELECT ... FROM posts JOIN users JOIN likes  (~50 ms, 3 joins)
Denormalized: SELECT * FROM post_view WHERE id=?         (~2 ms, 0 joins)
Cost: when a user renames, update author_name in N posts
```
**Trade-offs / when NOT to use it:** Redundancy risks **inconsistency** — the duplicated `author_name` can drift if you forget to update every copy. Writes get more expensive and must fan out to all copies. Storage grows. Don't denormalize write-heavy, frequently-changing data or where strong consistency is paramount (ledgers). It shines on read-heavy, rarely-changing data.

**Where you'll see it:** NoSQL data modeling (Cassandra, DynamoDB, MongoDB), analytics star schemas, precomputed feed rows, any read-optimized denormalized view table.

## Comparison Table

| Technique | Primary win | Real number | Chief cost |
|---|---|---|---|
| Multithreading | CPU parallelism | 8x on 8 cores | Lock complexity, races |
| Async I/O | I/O concurrency | 10k conns, few threads | No CPU help; harder code |
| Connection pooling | Skip handshakes | 32 ms → 2 ms | Sizing / DB limits |
| Batching | Amortize round-trips | 1,000 ms → 15 ms | First-item latency |
| Pagination | Bounded payloads | secs → ~1 ms (cursor) | No random page jump |
| Compression | Fewer bytes | 1 MB → 200 KB | CPU on both ends |
| Lazy loading | Skip unused work | 3.2 s → 0.4 s paint | N+1 queries |
| Precomputation | Cheap reads | 2,000 ms → 1 ms | Staleness, storage |
| Read replicas | Scale reads | ~4x with 3 replicas | Replication lag |
| Denormalization | Kill joins | 50 ms → 2 ms | Write fan-out, drift |

**Verdict:** Measure first, then pick the lever that attacks *your* actual bottleneck — CPU, I/O wait, round-trips, bytes, or read amplification. Stacking two or three of these is how a 2 s endpoint becomes 20 ms.

## Common Misconceptions
- **Myth:** More threads always means faster. → **Reality:** Beyond core count for CPU work, threads just add context-switch and lock overhead; for I/O, async scales far better.
- **Myth:** Async I/O speeds up computation. → **Reality:** It only helps waiting; a CPU-bound task on an event loop blocks everything.
- **Myth:** Compression is always worth it. → **Reality:** For tiny or already-compressed payloads, it costs more CPU than it saves.
- **Myth:** OFFSET pagination scales. → **Reality:** Deep offsets scan and discard everything before them; use keyset/cursor pagination.
- **Myth:** Read replicas give you a consistent read. → **Reality:** Replication lag means replicas can be stale; you may not read your own writes.
- **Myth:** Denormalization is just bad design. → **Reality:** At read-heavy scale it's a deliberate, correct trade — you pay on writes to win on reads.

## Real-World Case
When Instagram scaled its feed, computing each user's timeline live by querying all followees' recent posts became impossibly slow at hundreds of millions of users. The team leaned on **precomputation via fan-out-on-write**: when you post, the system pushes that post ID into each follower's precomputed feed list (in Redis/Cassandra), so reading a feed is a cheap lookup instead of a giant live query. But this broke for celebrities — fanning one post out to 100M+ follower feeds is enormous write amplification. The industry answer became a **hybrid**: fan-out-on-write for normal users, fan-out-on-read (compute live) for a handful of mega-accounts, merged at read time. The lesson: the same lever (precomputation) that saves you at one scale becomes the bottleneck at another — you tune per access pattern, not with one rule.

## Self-Test (answers at the bottom)
1. You have a CPU-bound image pipeline and an I/O-bound API aggregator. Which technique (threads vs async) fits each and why?
2. Why does `LIMIT 50 OFFSET 1000000` get slow, and what's the fix?
3. A batch endpoint buffers 500 items or flushes every 10 ms. Why the time limit in addition to the size limit?
4. You add 3 read replicas, then a user updates their profile and immediately reloads and sees the old name. What happened and how do you fix it?
5. Design sketch: An endpoint currently takes 2.1 s: it opens a new DB connection (30 ms), runs 1 query per item for 100 items (N+1, ~1 ms each round-trip = ~200 ms with overhead... plus join cost), returns 1 MB of uncompressed JSON, and recomputes a leaderboard live (1.5 s). Apply four techniques from this lesson and estimate the new latency.

## Interview Soundbites
- "Threads for CPU-bound work up to core count; async I/O for I/O-bound fan-out — mixing them up either starves cores or blocks the event loop."
- "Connection pooling and batching both attack fixed per-call overhead: reuse the handshake, amortize the round-trip. Together they turn milliseconds of tax into microseconds."
- "Denormalization and precomputation are the same bet — pay more on writes to make reads trivial — and they're correct exactly when reads dominate and data changes rarely."

## Mini-Assignment
Take a slow endpoint (real or hypothetical) that returns a paginated list of 50 orders, each with its customer and line items, as uncompressed JSON, opening a fresh DB connection per call. In ~30 minutes: (1) List every source of latency with an estimated number. (2) Apply connection pooling, fix the N+1 with a batch/JOIN, add gzip, and switch offset→cursor pagination. (3) Write the before/after latency budget as a table and compute the total speedup. (4) Note one correctness risk each change introduces (e.g., stale connection, cursor stability).

## Recap & Tomorrow
- **Multithreading / async I/O:** parallelize CPU work across cores; use an event loop for I/O-bound concurrency.
- **Connection pooling / batching:** reuse connections and amortize round-trips to erase fixed per-call overhead.
- **Pagination / compression:** bound payload size and shrink bytes on the wire.
- **Lazy loading / precomputation:** defer unneeded work; precompute expensive results for cheap reads.
- **Read replicas / denormalization:** scale reads out and reshape data to kill joins — paying on writes to win on reads.

Tomorrow, **Lesson 23 — The Database Landscape**: SQL vs NoSQL, Cassandra, MongoDB, graph and time-series databases, Elasticsearch, the memory/SSD/disk speed hierarchy, B-tree vs LSM storage engines, and a framework for picking the right database.

## Self-Test Answers
1. The image pipeline is CPU-bound → use multithreading (or multiprocessing) to spread work across cores for near-linear speedup. The API aggregator is I/O-bound (mostly waiting on network) → use async I/O so one thread juggles thousands of in-flight requests without wasting memory on blocked threads.
2. The database must scan and discard the first 1,000,000 rows before returning 50, so cost grows with the offset (O(offset)). Fix with keyset/cursor pagination: `WHERE id > last_seen_id ORDER BY id LIMIT 50`, which uses the index to seek directly and is O(log n) per page.
3. Without a time limit, a slow trickle of items (say 3 per second) would wait a very long time to reach 500, adding huge latency to those items. The 10 ms flush caps worst-case latency; the 500-item cap caps batch size/memory. You flush on whichever fires first.
4. Replication lag: the write went to the primary, but the reload read from a replica that hadn't yet applied the change, so it served stale data. Fix with read-your-writes routing — send that user's reads to the primary for a short window after a write (or read the just-written record from the primary).
5. Pool the connection (30→~0.05 ms), fix N+1 with one batched query/JOIN (~5 ms), gzip the response (1 MB→200 KB, transfer and serialization cheaper), and precompute the leaderboard in Redis (1,500→~1 ms). New total roughly 5 + 5 + 1 + a few ms overhead ≈ **~15–20 ms**, down from 2,100 ms — about a 100x improvement, with staleness on the leaderboard as the main new trade-off.
