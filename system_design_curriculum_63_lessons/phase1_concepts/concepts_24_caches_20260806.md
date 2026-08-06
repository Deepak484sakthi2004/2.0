# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 24 of 63 — Phase 1: Foundations (Module 24 of 28)
**Module:** Caches
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Caching is the single highest-leverage performance move in system design. Yesterday you learned RAM is ~1,000x faster than SSD and ~100,000x faster than an HDD seek; a cache is how you keep the hot data in that fast tier so users never pay the disk-read cliff. Nearly every design in Phase 2 — feeds, rate limiters, sessions, product pages — has a cache in front of the database, and interviewers probe hard on the details: which cache, which write strategy, what happens on eviction, and what stops a cache miss from stampeding your database into the ground. Get the write strategy or invalidation wrong and you serve stale money balances or take down the DB at 3 a.m. Today we make caching precise.

## Learning Objectives
By the end of this lesson you can:
- Compare Memcached and Redis on architecture, data types, and persistence, and pick between them.
- Distinguish cache-aside, write-through, and write-back and their consistency/durability trade-offs.
- Explain eviction policies (LRU/LFU) and size a cache with a hit-ratio calculation.
- Use TTL and invalidation correctly, and reason about staleness.
- Design thundering-herd protection so a hot-key expiry doesn't stampede the database.

## The Lesson

### Memcached vs Redis (Architecture, Advantages, Disadvantages)
**What it is (plain English):** Both are in-memory key-value stores used as caches. **Memcached** is a deliberately simple, multithreaded string cache. **Redis** is a richer single-threaded (per core) data-structure server — strings, hashes, sorted sets, streams — with optional persistence and replication.

**The problem it solves:** You need sub-millisecond reads for hot data instead of ~2–50 ms database queries. Both keep data in RAM and answer in **~0.1–1 ms**. The choice is about how much structure and durability you need.

**How it works (mechanics):** Memcached shards keys across a slab-allocated memory pool and uses multiple threads, so it scales reads across cores trivially and is very lean. Redis runs one command at a time per core (atomic, no locks) but ships data structures, Lua scripting, pub/sub, TTLs, and persistence (RDB snapshots / AOF log) plus replica sets and Cluster mode for sharding.
```
Memcached: multithreaded, strings only, no persistence, LRU eviction
Redis:     single-thread/core, rich types (SET/ZSET/HASH), RDB+AOF,
           replication, Cluster; ~100k+ ops/sec/core
```
**Trade-offs / when NOT to use it:** Memcached is faster to saturate many cores for pure string caching and uses slightly less memory per key, but has no persistence, no replication, and no data types — lose a node, lose that data. Redis's single-threaded model means one slow command (a big `KEYS *`) blocks everything; richer features add memory overhead. Choose Memcached for a simple, huge, ephemeral string cache; Redis when you need structures, atomic ops, persistence, or pub/sub.

**Where you'll see it:** Memcached at Facebook (one of the largest deployments ever). Redis nearly everywhere — sessions, leaderboards (sorted sets), rate limiters, queues, feed caches.

### Cache-Aside vs Write-Through vs Write-Back
**What it is (plain English):** Three strategies for how the cache and database stay coordinated on reads and writes. **Cache-aside**: the app manages the cache. **Write-through**: writes go through the cache to the DB synchronously. **Write-back**: writes hit the cache and are flushed to the DB later.

**The problem it solves:** A cache and a database are two copies of the truth; these patterns define who updates what and when, trading off consistency, durability, and write latency.

**How it works (mechanics):**
```
Cache-aside (read):  miss? -> read DB -> populate cache -> return
Cache-aside (write): write DB -> invalidate/update cache
Write-through:       write -> cache AND DB (sync) -> ack   (DB always fresh)
Write-back:          write -> cache -> ack; flush to DB async/batched
```
Cache-aside is the default: on a miss, load from DB and store in cache; on write, update the DB and delete the cache key. Write-through keeps cache and DB in lockstep (great read consistency) at the cost of write latency. Write-back acks after just the cache write — fastest writes (~0.2 ms), and it can coalesce many updates into one DB write.

**Trade-offs / when NOT to use it:** Cache-aside can serve stale data between DB write and cache invalidation, and cold misses hit the DB. Write-through makes every write pay both cache and DB latency and caches data that may never be read. Write-back is fastest but **risks data loss** — a crash before flush loses unpersisted writes — so never use it for a bank balance; it's for high-throughput, loss-tolerant counters/metrics.

**Where you'll see it:** Cache-aside is the web default (Redis + Postgres). Write-through in read-heavy config caches. Write-back in metrics aggregators, write-heavy buffers, and CPU caches (the hardware analogy).

### Eviction
**What it is (plain English):** A cache has finite RAM, so when it fills, it must evict something to make room. The **eviction policy** decides which victim to drop — ideally something you won't need soon.

**The problem it solves:** Without eviction, a full cache either rejects new writes or crashes. A good policy maximizes **hit ratio** — the fraction of reads served from cache — which is the whole point.

**How it works (mechanics):** Common policies: **LRU** (evict least recently used — a hashmap + doubly linked list gives O(1); Redis approximates it by sampling keys), **LFU** (evict least frequently used — better against scan pollution), **FIFO**, and **random**. Hit ratio drives the math:
```
DB read 10 ms, cache read 0.5 ms. Hit ratio 95%:
avg = 0.95*0.5 + 0.05*10 = 0.475 + 0.5 = ~0.98 ms
Drop to 80% hit ratio:
avg = 0.8*0.5 + 0.2*10 = 0.4 + 2.0 = 2.4 ms  (2.4x worse)
```
So each percent of hit ratio matters enormously. Sizing: if the hot working set is ~50 GB and you cache 40 GB, you'll miss on the coldest 20% — bump RAM or improve the policy.

**Trade-offs / when NOT to use it:** Strict LRU suffers **cache pollution** — a one-time scan of cold data evicts genuinely hot keys (as in Lesson 4's outage). LFU resists that but adapts slowly to changing hotness and needs frequency counters. There's no universally best policy; match it to the access pattern.

**Where you'll see it:** Redis `maxmemory-policy` (`allkeys-lru`, `allkeys-lfu`, `volatile-ttl`), OS page cache, CPU caches, CDN edge eviction.

### TTL & Invalidation
**What it is (plain English):** **TTL** (time-to-live) is an expiry stamped on a cache entry so it auto-deletes after N seconds. **Invalidation** is actively removing/updating an entry when the underlying data changes, so the cache doesn't serve stale data.

**The problem it solves:** Cached data goes stale the moment the DB changes. TTL bounds *how* stale ("at most 60 seconds old"); explicit invalidation makes it fresh immediately on writes. Phil Karlton's famous line — "there are only two hard things in CS: cache invalidation and naming things" — is about exactly this.

**How it works (mechanics):** Set a TTL on write (`SET key val EX 300` = expire in 300 s). Redis lazily expires (checks on access) plus a background sampler proactively deletes expired keys. On a data change, either **delete** the key (next read repopulates via cache-aside) or **update** it in place.
```
SET product:42 {...} EX 300      # tolerate up to 5 min staleness
On price change: DEL product:42  # force fresh reload next read
Staleness window with TTL-only = up to 300 s
```
**Trade-offs / when NOT to use it:** Short TTLs mean freshness but more DB misses; long TTLs mean fewer misses but staler data. Pure invalidation is precise but hard to get right across many caches and event paths — a missed invalidation serves stale data indefinitely. Deleting (not updating) on write is safer (avoids a stale-write race) but causes a miss. Money/inventory needs tight invalidation; a trending list tolerates a 60 s TTL.

**Where you'll see it:** HTTP `Cache-Control: max-age`, CDN TTLs, Redis key expiry, every "why am I seeing old data?" bug ticket.

### Thundering-Herd Protection
**What it is (plain English):** When a very popular cache key expires or misses, thousands of concurrent requests all miss at once and stampede the database to recompute the same value simultaneously — a "thundering herd" (a.k.a. cache stampede). Protection ensures only one request does the recompute while the rest wait or serve stale.

**The problem it solves:** Without it, a single hot-key expiry can spike DB load by 10,000x in one instant and cause a cascading outage — the cache that was protecting the DB becomes the trigger that kills it.

**How it works (mechanics):** Three standard defenses:
```
1. Locking / single-flight: first miss takes a lock (SETNX),
   recomputes, repopulates; others wait or get the old value.
2. Early/probabilistic recompute: refresh a hot key *before* it
   expires (jittered), so it never all-expires at once.
3. Stale-while-revalidate: serve the stale value instantly and
   refresh in the background.
Example: 10,000 req/s hit an expired key.
 With single-flight: 1 DB query recomputes; 9,999 wait ~5 ms.
 Without: 10,000 identical DB queries -> DB melts.
```
Also add **TTL jitter** (e.g., 300 s ± random 30 s) so many keys don't expire in the same tick.

**Trade-offs / when NOT to use it:** Locking adds coordination overhead and a small wait; stale-while-revalidate serves briefly stale data (unacceptable for strict-consistency values). For cold, low-traffic keys the machinery is unnecessary — protect only genuinely hot keys.

**Where you'll see it:** Facebook's "leases," Go's `singleflight`, Netflix's EVCache, CDN request coalescing, any large Redis deployment with hot keys.

## Comparison Table

| Dimension | Memcached | Redis |
|---|---|---|
| Threading | Multithreaded | Single-threaded per core |
| Data types | Strings only | Strings, hashes, sets, sorted sets, streams |
| Persistence | None | RDB snapshots + AOF |
| Replication / cluster | Limited | Built-in replicas + Cluster |
| Eviction | LRU | LRU / LFU / TTL policies |
| Best for | Lean ephemeral string cache | Rich structures, durability, pub/sub |

| Write strategy | Read consistency | Write latency | Durability risk |
|---|---|---|---|
| Cache-aside | Can be briefly stale | DB latency | Low (DB is source) |
| Write-through | Always fresh | Cache + DB (higher) | Low |
| Write-back | Fresh in cache | Lowest (cache only) | High (loss before flush) |

**Verdict:** Default to Redis + cache-aside + TTL for most web systems; reach for write-through when reads must never be stale, write-back only for loss-tolerant high-write workloads, and always protect hot keys from stampedes.

## Common Misconceptions
- **Myth:** Redis is always better than Memcached. → **Reality:** For a pure, huge, ephemeral string cache, Memcached's multithreading and lower per-key overhead can win; Redis wins on features and durability.
- **Myth:** Write-back is just a faster write-through. → **Reality:** Write-back acks before the DB is updated, so a crash can lose data — never use it for money.
- **Myth:** A TTL guarantees fresh data. → **Reality:** It bounds staleness to the TTL window; between changes and expiry you still serve stale data unless you invalidate.
- **Myth:** Higher hit ratio is a minor optimization. → **Reality:** Dropping from 95% to 80% can more than double average latency because misses are ~20x costlier.
- **Myth:** Caching removes load from the DB, so it can't hurt it. → **Reality:** A hot-key stampede can spike DB load 10,000x in an instant — you must guard against thundering herds.

## Real-World Case
Facebook's Memcached tier is one of the most studied caching systems in history. At their scale, a single popular key expiring could unleash a thundering herd of thousands of simultaneous database reads for the same value. Their fix was **leases**: on a miss, the cache hands exactly one client a short-lived "lease" token authorizing it to recompute and repopulate the key; every other client for that key either waits briefly or is served the slightly stale prior value. This turned a potential 10,000x database spike into a single recompute. They also faced cross-region consistency — invalidations had to propagate so a user didn't see their own stale writes — solved with careful invalidation-on-write and versioning. The takeaway drilled into every engineer there: a cache doesn't just make things fast, it changes your failure modes, and hot-key protection is not optional at scale.

## Self-Test (answers at the bottom)
1. Give one workload where Memcached is the better pick than Redis, and one where Redis clearly wins.
2. In cache-aside, what are the two steps on a write, and why delete (not update) the cache key?
3. A cache read is 0.5 ms and a DB read is 10 ms. Compute average read latency at 90% vs 99% hit ratio.
4. Why is write-back dangerous for a bank balance but fine for a page-view counter?
5. Design sketch: A product page for a flash sale has one item viewed 50,000 times/second. Its cache entry has a 60 s TTL. Describe exactly what happens the instant it expires under naive cache-aside, and design the protection so the database survives.

## Interview Soundbites
- "Default is Redis with cache-aside and a jittered TTL; I only go write-through when reads can't be stale and write-back only for loss-tolerant, write-heavy counters."
- "Hit ratio is everything — going from 95% to 80% more than doubles average latency because a miss is roughly 20x costlier than a hit."
- "The cache's most dangerous moment is a hot key expiring; single-flight plus TTL jitter turns a 10,000x database stampede into one recompute."

## Mini-Assignment
Design the caching layer for a product-detail service (Redis + Postgres) in ~30 minutes. (1) Choose a write strategy and justify it for a mix of price updates (must be fresh) and description text (can be stale). (2) Pick a TTL and eviction policy and size the cache from an assumed 100 GB catalog with a 20 GB hot set. (3) Compute average read latency at 92% hit ratio (cache 0.4 ms, DB 8 ms). (4) Write out the exact thundering-herd protection (single-flight with `SETNX` + jittered TTL) for the top 10 hottest SKUs, and state the staleness you accept.

## Recap & Tomorrow
- **Memcached vs Redis:** lean multithreaded string cache vs rich single-threaded data-structure server with persistence.
- **Write strategies:** cache-aside (default), write-through (always-fresh reads), write-back (fastest writes, data-loss risk).
- **Eviction:** LRU/LFU to maximize hit ratio; guard against scan pollution.
- **TTL & invalidation:** bound staleness with TTL, force freshness with delete-on-write.
- **Thundering herd:** single-flight, early recompute, stale-while-revalidate, and TTL jitter keep a hot-key expiry from stampeding the DB.

Tomorrow, **Lesson 25 — Content Delivery Networks & Edge Caching**: pushing cached content to the network edge so users worldwide get sub-50 ms responses, and how CDNs, cache hierarchies, and consistent hashing extend everything you learned today across the globe.

## Self-Test Answers
1. Memcached wins for a very large, purely ephemeral string cache where multithreading across many cores and minimal per-key overhead matter (e.g., a massive fragment cache). Redis wins when you need data structures (leaderboards via sorted sets), atomic operations, persistence, replication, or pub/sub — e.g., a rate limiter or session store.
2. On a write you update the database, then delete (invalidate) the cache key. Deleting rather than updating avoids a race where two concurrent writers set the cache in the wrong order and leave a stale value; the next read simply repopulates from the DB with the correct current value.
3. At 90%: 0.9×0.5 + 0.1×10 = 0.45 + 1.0 = 1.45 ms. At 99%: 0.99×0.5 + 0.01×10 = 0.495 + 0.1 = ~0.6 ms. The extra 9 points of hit ratio roughly halve average latency.
4. Write-back acks after writing only the cache and flushes to the DB later, so a crash before flush loses those writes — catastrophic for a bank balance (lost money, no audit trail). A page-view counter is loss-tolerant and write-heavy, so coalescing thousands of increments in cache and flushing periodically is a big win with acceptable risk.
5. Naively, at expiry all ~50,000 concurrent requests miss simultaneously and each fires the same DB query — a 50,000x spike that can crash the database. Protect it with single-flight: the first miss acquires a lock (`SETNX`), recomputes and repopulates while others briefly wait or receive the last-known value (stale-while-revalidate); add jittered TTL and proactively refresh the key before expiry so it never all-expires at once. Result: one DB query instead of 50,000.
