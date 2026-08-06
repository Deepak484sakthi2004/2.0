# Caching Strategy System

## 1. Problem Statement & Scope

Design the caching layer for a large read-heavy web platform (think product catalog + user profiles for an e-commerce site). The deliverable is not "add Redis" — it is a coherent caching *strategy*: which write/read policies to use per data class, how eviction works, how we avoid stampedes, how invalidation propagates, and how the cache tiers compose.

### Functional Requirements
- Serve reads for hot entities (products, profiles, sessions, rendered fragments) with p99 < 10 ms from cache.
- Support multiple caching patterns per data class: cache-aside, read-through, write-through, write-back, write-around.
- Configurable eviction (LRU / LFU / ARC) and TTL with jitter per namespace.
- Explicit invalidation API (`invalidate(key)`, `invalidate_by_tag(tag)`) for correctness-sensitive data.
- Multi-tier: in-process (L1) → distributed cache cluster (L2) → database (source of truth).
- Stampede protection: at most ~1 origin fetch per key per expiry event, cluster-wide.

### Non-Functional Requirements
- Cache hit ratio ≥ 95% for the hot working set; DB must survive a full cache flush (degraded but alive).
- Availability: cache cluster loss must not cause an outage, only elevated latency (cache is an optimization, not a dependency for correctness).
- Staleness budget: product prices ≤ 5 s stale; product descriptions ≤ 10 min; inventory counts must never be served from cache for the checkout path.
- Horizontal scalability of the cache tier without full rehash storms (consistent hashing).

### Back-of-Envelope Estimation
- 50 M DAU, average 40 page views/day → 2 B page views/day.
- Each page fans out to ~20 entity reads → 40 B entity reads/day.
- 40 B / 86,400 s ≈ 463 K reads/s average; peak = 3× average ≈ **1.4 M reads/s**.
- Write rate (catalog updates, profile edits): ~2% of reads → ~28 K writes/s peak.
- Object size: avg 2 KB serialized. Hot working set: 200 M distinct hot keys × 2 KB = **400 GB** → fits in a cluster of ~15 nodes × 32 GB usable (with replication ×2 → ~30 nodes) or fewer bigger nodes.
- Bandwidth at cache tier: 1.4 M reads/s × 2 KB = **2.8 GB/s** egress. This alone justifies an L1 in-process tier: if L1 absorbs 60% of reads, L2 egress drops to ~1.1 GB/s.
- DB capacity check: at 95% hit ratio, miss traffic = 5% × 1.4 M = 70 K QPS to the DB — that already requires read replicas. At 90% hit ratio it doubles to 140 K QPS. **Every point of hit ratio matters**; this is the argument you make numerically in the interview.

## 2. Brute-Force / Naive Design

Single app server, single PostgreSQL, no cache:

```
Client → App Server → PostgreSQL
```

Why it breaks, with numbers:
- A tuned Postgres box does ~10–50 K simple indexed reads/s. Peak demand is 1.4 M reads/s → you need ~30–100× the capacity of one box on the read path alone.
- p99 latency: even a fast indexed lookup is 1–5 ms under light load; under connection saturation (Postgres degrades badly past a few hundred active connections) p99 blows past 100 ms.
- Naive fix #1 — read replicas: 1.4 M QPS / 30 K per replica ≈ 47 replicas, plus replication lag issues, plus you're paying database prices for what is mostly repeated reads of the same 200 M hot rows. 40 B reads/day over 200 M hot keys means the average hot key is read **200 times/day** — redundant work screaming to be cached.
- Naive fix #2 — one global in-process dict per app server: no bound on memory (OOM), no coherence across servers (server A shows the old price, server B the new), cold caches on every deploy (deploy = self-inflicted stampede on the DB).

## 3. Evolving the Design

**Step 1 — Bottleneck: repeated identical reads hammer the DB.**
Fix: add a distributed cache (Redis cluster) with **cache-aside**: app checks cache, on miss reads DB and populates cache with TTL. Chosen first because it's the least invasive: no change to the write path, cache failure degrades to plain DB reads. Hit ratio ~90%+ immediately for Zipfian access patterns.

**Step 2 — Bottleneck: staleness after writes.**
Cache-aside + TTL alone means up to TTL-seconds of staleness. Fix: on write, **invalidate (delete) the cache key** rather than updating it. Delete-not-update because two concurrent writers updating the cache can interleave and leave the *older* value cached forever; a delete converges via the next read. Order: write DB first, then delete cache ("write-then-invalidate"). The residual race (reader loads old value between DB write and delete) is narrowed with a short delayed second delete ("double delete") or accepted within the staleness budget.

**Step 3 — Bottleneck: hot key expiry causes a stampede.**
When a key read 10 K times/s expires, thousands of concurrent misses all hit the DB ("cache stampede"; correlated expiry of many keys = "thundering herd"). Fixes, layered: (a) **TTL jitter** — `ttl = base × uniform(0.9, 1.1)` so keys populated together don't expire together; (b) **per-key mutex / request coalescing** — only one caller per key rebuilds, others wait or serve stale; (c) **probabilistic early expiry (XFetch)** — hot readers refresh *before* expiry with probability increasing near the deadline, so the herd never forms. Details in §7.

**Step 4 — Bottleneck: network round-trip to Redis (0.3–1 ms) × 20 keys/page adds up; Redis egress bandwidth.**
Fix: **L1 in-process cache** (Caffeine/Guava-style, ~1 GB per app server, short TTL 1–5 s) in front of L2 Redis. L1 hits cost ~100 ns. Coherence: L1 entries are short-TTL only, plus an **invalidation pub/sub channel** (Redis pub/sub or Kafka) that app servers subscribe to; on invalidate, every server drops its L1 entry. Accept that L1 gives bounded (seconds) staleness — that's why checkout-path inventory bypasses L1 entirely.

**Step 5 — Bottleneck: single Redis node caps at ~100 K–1 M ops/s and 400 GB doesn't fit.**
Fix: shard by key with **consistent hashing** (Redis Cluster's 16,384 hash slots). Adding a node moves only ~1/N of slots — no global rehash storm. Replicate each shard (1 replica) for failover; reads stay on primaries to avoid replica-lag surprises unless we explicitly opt into stale reads.

**Step 6 — Bottleneck: heavy-write, read-later data (view counters, like counts) makes write-through wasteful and DB-write-bound.**
Fix: **write-back (write-behind)** for tolerant data: increment in Redis, flush batched deltas to DB every few seconds via a drainer. Accept bounded data loss on cache node crash (mitigate with AOF persistence / replication) — acceptable for counters, never for orders.

**Step 7 — Bottleneck: bulk imports / analytics writes pollute the cache with never-read keys, evicting hot ones.**
Fix: **write-around** for those paths — write straight to DB, don't populate cache; the rare subsequent read misses once and cache-asides it in.

End state: pattern-per-data-class, two tiers, sharded L2, stampede protection, pub/sub invalidation.

## 4. Protocol & Technology Choices — Why This, Not That

### Redis vs Memcached (L2 distributed cache)

| Dimension | Redis (chosen) | Memcached |
|---|---|---|
| Data structures | Hashes, sorted sets, sets, streams — enables counters, leaderboards, coalescing locks (`SET NX PX`) | Strings/blobs only |
| Threading | Single-threaded core (I/O threads ≥6.0); predictable latency | Multithreaded; better raw throughput per node for flat GET/SET |
| Persistence / replication | RDB/AOF, replicas, Cluster mode, failover | None built in (client-side sharding only) |
| Pub/sub | Yes — powers L1 invalidation channel | No |
| Memory efficiency | Slightly worse per byte (structure overhead) | Slab allocator, very efficient for uniform small values |
| Eviction | 8 policies incl. `allkeys-lru`, `allkeys-lfu` | LRU per slab class |

**Why Redis:** we need `SET NX` locks for stampede control, pub/sub for L1 invalidation, atomic `INCR` for write-back counters, and replication for shard failover. **When Memcached wins:** pure ephemeral blob cache with a multithreaded hot box and maximum GET/SET throughput per dollar, no need for durability or structures (Facebook's classic mcrouter deployment).

### Cache-aside vs Read-through vs Write-through vs Write-back vs Write-around

| Pattern | Read path | Write path | Staleness | Failure blast radius | Use for |
|---|---|---|---|---|---|
| Cache-aside (default) | App: cache → miss → DB → populate | App: DB write → cache delete | Bounded by TTL + invalidate | Cache down ⇒ DB pressure only | Most entities |
| Read-through | Cache library/proxy loads on miss | Same as aside typically | Same | Loader is a new SPOF/complexity | Uniform loading logic, DAX-style |
| Write-through | Cache always warm | Write cache + DB synchronously | ~0 for cached keys | Write latency = max(cache, DB); double-write consistency | Read-after-write critical, moderate write rate |
| Write-back | Cache is freshest | Write cache, async flush to DB | DB is stale, cache fresh | **Data loss** window on cache crash | Counters, metrics, tolerant aggregates |
| Write-around | Miss on first read | DB only | First read is a miss | None extra | Bulk loads, write-once-read-rarely |

**Why cache-aside as default:** simplest failure semantics — the cache is strictly an optimization; every alternative couples writes or reads to cache availability. **When write-through wins:** strong read-after-write on the same key with modest write volume. **When write-back wins:** write rate ≫ DB write capacity and loss of a few seconds of deltas is acceptable.

### Eviction: LRU vs LFU vs ARC

| Policy | Mechanism | Strength | Weakness | When it wins |
|---|---|---|---|---|
| LRU | Evict least-recently-used; O(1) with hashmap + doubly linked list | Simple, great for temporal locality | One sequential scan flushes the whole cache | General default; recency-dominated traffic |
| LFU | Evict least-frequently-used (Redis: 8-bit log counter + decay) | Protects perennially hot keys from scan pollution | New keys die young without aging; more bookkeeping | Stable popularity distribution (top-N products) |
| ARC | Two LRU lists (recent T1, frequent T2) + ghost lists (B1/B2) self-tune the split | Adapts between recency and frequency automatically, scan-resistant | Patented history (IBM; expired 2024-ish), ~2× metadata, rarely off-the-shelf | Mixed/shifting workloads; ZFS uses it |
| TinyLFU/W-TinyLFU (mention) | Frequency sketch admission filter in front of LRU | Best hit ratios in practice (Caffeine's default) | Complexity | L1 in-process caches |

**Choice:** Redis `allkeys-lfu` for the catalog namespace (stable Zipfian popularity), `allkeys-lru` for sessions (pure recency); Caffeine/W-TinyLFU for L1.

### L1 invalidation transport: Redis pub/sub vs Kafka

| | Redis pub/sub (chosen) | Kafka |
|---|---|---|
| Delivery | Fire-and-forget, at-most-once | Durable, replayable, at-least-once |
| Latency | Sub-ms | ms–tens of ms |
| Miss consequence | L1 serves stale until its short TTL (≤5 s) expires — acceptable | Guaranteed delivery but heavier |

**Why pub/sub:** the short L1 TTL is the correctness backstop, so lost invalidations self-heal within seconds; we buy simplicity and latency. **Kafka wins** when invalidations must be durable/audited or drive downstream materialized views (then it's a CDC pipeline, not a cache ping).

### Serialization: JSON vs Protobuf

| | JSON | Protobuf (chosen for hot namespaces) |
|---|---|---|
| Size | 2 KB avg | ~0.8–1.2 KB (40–60% smaller) → directly cuts the 2.8 GB/s egress |
| CPU | Slower parse | Fast, schema-checked |
| Debuggability | `redis-cli GET` readable | Needs tooling |

## 5. High-Level Design (HLD)

```mermaid
flowchart TB
    C[Clients] --> LB[Load Balancer]
    LB --> A1[App Server + L1 Caffeine]
    LB --> A2[App Server + L1 Caffeine]
    A1 -->|miss| P[Cache Client Library<br/>consistent hashing, coalescing]
    A2 -->|miss| P
    P --> R1[(Redis Shard 1 + replica)]
    P --> R2[(Redis Shard 2 + replica)]
    P --> R3[(Redis Shard N + replica)]
    P -->|L2 miss, singleflight| DB[(Primary DB + read replicas)]
    W[Write API] -->|1. write| DB
    W -->|2. DEL key| R1
    W -->|3. publish invalidate| PS[[Redis Pub/Sub<br/>invalidation channel]]
    PS -.->|drop L1 entry| A1
    PS -.->|drop L1 entry| A2
    WB[Write-back Drainer] -->|batched flush counters| DB
    R1 --> WB
```

### Read Path (product page, key `prod:{id}:v3`)
1. App checks L1 (Caffeine, TTL 2 s + jitter). Hit → return (~100 ns). Expected 60% of reads.
2. L1 miss → cache client hashes key to shard, `GET` from Redis. Hit → deserialize, populate L1, return (~0.5 ms). Expected ~37% of reads.
3. L2 miss → **singleflight**: acquire per-key in-process latch and cluster lock `SET lock:{key} {token} NX PX 3000`. Winner queries DB, `SET key val EX ttl±jitter`, releases lock, returns. Losers wait on the latch (bounded 50 ms) then re-read cache; on timeout, serve stale-if-available or go to DB with a concurrency cap.
4. Near-expiry hits may trigger probabilistic early refresh (§7) — refresh happens async, the caller still gets the cached value.

### Write Path (price update)
1. Write to DB (transactional, source of truth).
2. In the same request path: `DEL prod:{id}:v3` on L2; publish `{key}` on the invalidation channel → all app servers evict L1.
3. Schedule a delayed re-delete (+500 ms) to close the read-modify race window.
4. For counters (write-back namespace): `HINCRBY` in Redis only; drainer flushes deltas to DB every 5 s with idempotent upserts.

### Data Model / Key Schema
```
prod:{product_id}:v{schema_ver}     -> protobuf blob, TTL 600s ±10%, LFU namespace
price:{product_id}                  -> string, TTL 5s ±10%   (tight staleness budget)
sess:{session_id}                   -> hash, TTL 1800s, LRU namespace, sliding
cnt:views:{product_id}              -> int, write-back, no TTL (drainer-owned)
tag:{tag} -> SET of keys            (tag-based invalidation index)
lock:{key} -> owner token, PX 3000  (stampede lock)
```
Schema version in the key = free "invalidation" on deploys that change the serialized shape (old keys age out; no poisoned deserialization).

### API (cache client library)
```
get(ns, key) -> Optional<V>
getOrLoad(ns, key, loader, ttl) -> V        // coalesced, stampede-safe
put(ns, key, v, ttl)
invalidate(ns, key)                          // L2 DEL + pub/sub broadcast
invalidateByTag(tag)                         // fan-out DEL over tag set
increment(ns, key, delta)                    // write-back namespace only
```

## 6. Low-Level Design (LLD)

Patterns used: **Strategy** (pluggable eviction), **Template Method / Decorator** (tiered cache composing L1 over L2), **Repository** (loader abstraction over DB), **Singleton-per-namespace Factory** (CacheFactory builds configured instances), **Singleflight** (request coalescing).

```mermaid
classDiagram
    class Cache~K,V~ {
        <<interface>>
        +get(K key) Optional~V~
        +put(K key, V val)
        +remove(K key)
        +size() int
    }
    class EvictionStrategy~K~ {
        <<interface>>
        +onAccess(K key)
        +onInsert(K key)
        +evictCandidate() K
        +onRemove(K key)
    }
    class LRUEviction~K~ {
        -DoublyLinkedList~K~ order
        -Map~K,Node~ index
    }
    class LFUEviction~K~ {
        -Map~K,int~ freq
        -Map~int, LinkedHashSet~K~~ buckets
        -int minFreq
    }
    class ARCEviction~K~ {
        -LinkedHashSet~K~ t1
        -LinkedHashSet~K~ t2
        -LinkedHashSet~K~ b1ghost
        -LinkedHashSet~K~ b2ghost
        -double targetP
    }
    class InMemoryCache~K,V~ {
        -Map~K, Entry~V~~ store
        -EvictionStrategy~K~ eviction
        -int capacity
        -Clock clock
        +get(K) Optional~V~
        +put(K, V)
    }
    class TieredCache~K,V~ {
        -Cache~K,V~ l1
        -Cache~K,V~ l2
        -Singleflight~K,V~ flight
        -CacheLoader~K,V~ loader
        +getOrLoad(K) V
    }
    class CacheLoader~K,V~ {
        <<interface>>
        +load(K key) V
    }
    class ProductRepository {
        +load(String id) Product
    }
    class Singleflight~K,V~ {
        -ConcurrentMap~K, CompletableFuture~V~~ inflight
        +run(K key, Supplier~V~ fn) V
    }
    class InvalidationBus {
        <<interface>>
        +publish(String key)
        +subscribe(Consumer~String~ onInvalidate)
    }
    class RedisPubSubBus
    class CacheFactory {
        +build(NamespaceConfig cfg) Cache
    }
    Cache <|.. InMemoryCache
    Cache <|.. TieredCache
    EvictionStrategy <|.. LRUEviction
    EvictionStrategy <|.. LFUEviction
    EvictionStrategy <|.. ARCEviction
    InMemoryCache o-- EvictionStrategy : strategy
    TieredCache o-- Singleflight
    TieredCache o-- CacheLoader
    CacheLoader <|.. ProductRepository : repository
    InvalidationBus <|.. RedisPubSubBus
    TieredCache ..> InvalidationBus : subscribes
    CacheFactory ..> InMemoryCache : creates
```

Why these patterns: **Strategy** lets one `InMemoryCache` host LRU/LFU/ARC without subclass explosion and makes eviction unit-testable in isolation; **Repository** keeps the cache ignorant of SQL so loaders are mockable; **Factory** centralizes per-namespace config (capacity, TTL, jitter, eviction) so product code can't misconfigure; **Singleflight** is the coalescing primitive reused by both tiers.

### Hardest algorithm 1 — O(1) LRU cache (the classic machine-coding ask)
```java
class LRUCache<K, V> {
    private final int capacity;
    private final Map<K, Node<K, V>> map = new HashMap<>();
    private final Node<K, V> head = new Node<>(null, null); // MRU sentinel
    private final Node<K, V> tail = new Node<>(null, null); // LRU sentinel

    LRUCache(int capacity) {
        this.capacity = capacity;
        head.next = tail; tail.prev = head;
    }

    synchronized V get(K key) {
        Node<K, V> n = map.get(key);
        if (n == null) return null;
        unlink(n); linkFront(n);            // move-to-front on access
        return n.val;
    }

    synchronized void put(K key, V val) {
        Node<K, V> n = map.get(key);
        if (n != null) { n.val = val; unlink(n); linkFront(n); return; }
        if (map.size() == capacity) {       // evict from tail
            Node<K, V> lru = tail.prev;
            unlink(lru); map.remove(lru.key);
        }
        n = new Node<>(key, val);
        map.put(key, n); linkFront(n);
    }

    private void unlink(Node<K, V> n) { n.prev.next = n.next; n.next.prev = n.prev; }
    private void linkFront(Node<K, V> n) {
        n.next = head.next; n.prev = head;
        head.next.prev = n; head.next = n;
    }
    static final class Node<K, V> {
        K key; V val; Node<K, V> prev, next;
        Node(K k, V v) { key = k; val = v; }
    }
}
```
Interview notes: both ops O(1); sentinels remove null-edge cases; `synchronized` is fine for L1 per-segment — production caches (Caffeine) use lock-free reads + striped write buffers instead of a global lock.

### Hardest algorithm 2 — Singleflight + probabilistic early expiry
```java
V getOrLoad(K key) {
    Entry<V> e = l1.getEntry(key);
    if (e != null && !e.expired(clock)) {
        // XFetch: refresh early with prob rising near expiry
        // refresh if: now - delta * beta * ln(rand()) >= expiry
        double delta = e.lastLoadMillis;          // observed recompute cost
        if (clock.now() - delta * BETA * Math.log(rng.nextDouble()) >= e.expiry) {
            asyncRefresh(key);                    // caller still gets cached value
        }
        return e.value;
    }
    return flight.run(key, () -> {                // one loader per key per process
        V v2 = l2get(key);
        if (v2 != null) { l1.put(key, v2); return v2; }
        String token = UUID.randomUUID().toString();
        boolean won = redis.set("lock:" + key, token, "NX", "PX", 3000);
        if (!won) {
            V v = pollL2(key, 50 /*ms*/);          // wait for winner's SET
            if (v != null) return v;
            if (e != null) return e.value;         // serve stale as last resort
        }
        long t0 = clock.now();
        V fresh = loader.load(key);                // repository → DB
        long cost = clock.now() - t0;
        l2setWithJitter(key, fresh, baseTtl, cost);
        releaseIfOwner("lock:" + key, token);      // Lua: GET==token then DEL
        l1.put(key, fresh);
        return fresh;
    });
}
```
Note `BETA = 1.0` default; log of uniform(0,1] is negative, so the term pulls "now" forward — hotter keys sample more often and almost surely refresh before expiry, cold keys rarely pay early refresh. Lock release must be compare-and-delete (Lua script) so a slow winner can't delete a successor's lock.

## 7. Deep Dives & Failure Modes

**Consistency model.** This design is deliberately eventually consistent with bounded staleness: DB-write → L2 delete → pub/sub L1 evict. Windows of staleness: (a) between DB commit and DEL (~ms), (b) the read-repopulate race (a reader read the old DB value pre-commit and SETs it post-DEL) — closed by delayed double-delete or CDC-driven invalidation (Debezium reading the binlog and issuing deletes — removes the "app forgot to invalidate" class entirely), (c) lost pub/sub message — bounded by L1's ≤5 s TTL. Anything needing linearizability (inventory decrement at checkout) bypasses cache and hits the DB with `SELECT ... FOR UPDATE` or a Redis-atomic reservation.

**Cache stampede vs thundering herd (name the difference).** Stampede = many concurrent misses on *one* key (hot key expired). Herd = many keys/clients synchronized (mass expiry after deploy, cache flush, or every client retrying on the same timer). Defenses map accordingly: per-key locks/coalescing and XFetch for stampede; TTL jitter, warm-up scripts on deploy, and retry-with-jitter for herd. Also "stale-while-revalidate": serve the expired value for a grace window while one refresher runs — best p99 protection of all, if staleness budget allows.

**Hot keys.** A celebrity product can push one Redis shard past its ops ceiling while others idle. Detect: per-key hit counters via client-side sampling or `redis-cli --hotkeys`. Mitigate: (1) L1 absorbs most hot-key reads (this is the single best fix — the hotter the key, the higher its L1 hit ratio); (2) key replication: write `key#0..key#9` to 10 shards, readers pick a random suffix — trades 10× memory and 10× invalidation fan-out for 10× read capacity; (3) for hot *write* keys (counters), shard the counter and sum on read.

**Big keys.** A 5 MB serialized object at 1 K reads/s = 5 GB/s from one shard and blocks Redis's single thread during serialization. Enforce a max value size (e.g., 100 KB) in the client library; split or compress above it.

**Cache penetration** (queries for keys that don't exist — attacker or bug — every one a guaranteed DB hit). Fix: cache negative results (`"NULL"` sentinel, short TTL 30 s) and/or a Bloom filter of valid IDs in front of the DB path; reject non-members without touching the DB. Trade-off: Bloom false positives (~1%) still pass through; deletions need a counting filter or periodic rebuild.

**Negative caching, done properly.** The sentinel approach has three sharp edges worth naming: (1) *TTL asymmetry* — negative entries must have a much shorter TTL than positives (30 s vs 600 s) because "not found" flips to "found" the moment the row is inserted, and there is often no write-path invalidation for a key that didn't exist when the writer looked (the `INSERT` path must also DEL the negative key — easy to forget, so short TTL is the backstop); (2) *memory abuse* — an attacker enumerating random IDs fills the cache with negative entries and evicts real ones; cap the negative namespace with its own small LRU region or store negatives only in L1, and rate-limit by source before caching; (3) *sentinel typing* — use a distinct marker (`0x00` byte, or a wrapper `{found:false}`) that the deserializer handles explicitly; a stringly-typed `"NULL"` will eventually be returned to a user as a product description. DNS resolvers formalized all of this decades ago (RFC 2308 negative TTL from the SOA record) — worth citing as prior art.

**Cache warming strategies.** Cold caches appear on: new node join, cluster restart, region failover, generation bump (schema-version flush), and big deploys. Four techniques, in increasing sophistication: (1) **replay warming** — continuously log a sampled key-frequency stream (say 1-in-100 GETs to Kafka); on cold start, a warmer replays the top-K keys through the normal `getOrLoad` path at a controlled rate (e.g., 5 K loads/s, well under DB headroom); (2) **snapshot shipping** — for Redis, seed a new replica from an RDB snapshot of a healthy peer, then promote — the cache starts ~minutes stale but hot, and short TTLs converge it; (3) **shadow traffic** — before cutting a new cluster into service, tee live read traffic to it (populate-only, responses discarded) for 10–30 min until its hit ratio crosses a threshold gate (e.g., ≥ 85%) that the LB checks before admitting it; (4) **priming on deploy** — the app on boot fetches its own critical config/entity set before reporting healthy. State the gate explicitly in the interview: *a cache node should not take traffic until its measured hit ratio clears a floor* — admitting a cold node is a self-inflicted partial stampede.

**Consistent hashing vs Redis Cluster slots — know both mechanisms.** Classic consistent hashing (memcached/Ketama): hash each server to ~100–200 virtual nodes on a ring; a key goes to the first vnode clockwise from `hash(key)`. Adding a server steals ~1/N of keyspace, spread evenly thanks to vnodes; removing one spills its arc to successors. Weaknesses: keyspace ownership is implicit (no authoritative map, so clients can disagree during config rollout — brief split-brain caching, tolerable for a cache, fatal for a store), and load variance is ~±10% even with vnodes. Redis Cluster instead uses **16,384 fixed hash slots**: `slot = CRC16(key) mod 16384`, and an explicit slot→node assignment gossiped cluster-wide. Migration is per-slot and stateful (`MIGRATING`/`IMPORTING` flags; clients chase `-MOVED`/`-ASK` redirects), so resharding is precise, observable, and can be throttled. **Hash tags** (`{user123}.profile`, `{user123}.orders`) force related keys into one slot to permit multi-key ops/Lua — but a popular hash tag is a self-made hot slot. Interview line: *the ring gives you statistical placement with zero coordination; slots give you exact placement with a small coordination protocol — Redis chose slots so that resharding is an explicit operation, not an emergent behavior.*

**Write-behind failure & durability handling.** Write-back's loss window deserves more than a hand-wave. Layered mitigations: (1) *don't buffer in bare memory* — deltas live in Redis with `appendfsync everysec` AOF plus a replica, so a primary crash loses ≤ 1 s, not the whole buffer; (2) *bound the buffer* — cap unflushed deltas per namespace (e.g., 5 s of writes or 100 MB); when the drainer falls behind the cap, degrade to write-through for new writes rather than growing the loss window silently; (3) *idempotent, ordered flush* — drainer reads deltas with a monotonically increasing epoch, upserts `applied_epoch` per key in the DB, and skips deltas ≤ applied epoch, so crash-and-retry never double-applies; (4) *reconciliation* — a nightly job compares DB aggregates against a recount from source events and emits drift metrics; write-back without reconciliation is slow silent corruption; (5) *upgrade path* — if the loss window ever becomes unacceptable, the namespace graduates from write-back to a real log (Kafka + consumer), which is write-behind with durability — say this to show you know where the pattern's ceiling is.

**Cache coherence across regions.** Multi-region deployments add a third tier problem: each region has its own L1+L2, with a single-writer-region DB (or multi-master). Options: (1) **regional independence (chosen default)** — each region's cache fills from its local DB read replica; invalidations ride the same channel as DB replication via CDC — the invalidator in each region tails the *local* replica's stream, so invalidation ordering matches data arrival and you never invalidate before the new value is locally readable (invalidating off the primary's stream races replica lag: reader misses, loads the *old* value from the lagging replica, and caches it — a classic bug worth naming); (2) **global invalidation bus** — broadcast DELs cross-region over Kafka MirrorMaker or a global pub/sub; simpler mental model, but the race above bites unless deletes are delayed past max replica lag; (3) **lease-based** (Facebook's memcache paper): the cache hands a reader a lease token on miss; a DEL invalidates outstanding leases so a stale SET is refused — the strongest defense against the read-repopulate race, worth citing by name. Staleness across regions is bounded by replica lag + TTL; the budget per namespace decides whether cross-region reads must instead pin to the home region (checkout does; browsing doesn't).

**Memcached slab allocation vs Redis memory model.** Memcached carves memory into 1 MB pages assigned to **slab classes** of fixed chunk sizes (~growth factor 1.25: 96 B, 120 B, 152 B…). A value occupies one chunk of the smallest fitting class — internal fragmentation is bounded and predictable (~20% worst case), allocation is O(1), there is no compaction, and LRU is *per slab class*. Failure mode: **slab calcification** — if traffic shifts from 100 B values to 1 KB values, pages already assigned to the small class aren't reclaimed and the 1 KB class evicts furiously while small-class memory idles (modern memcached's automover reassigns pages, but slowly). Redis instead uses a general-purpose allocator (jemalloc) with per-type encodings (listpack for small hashes, intset, embstr) — flexible, but subject to **external fragmentation**: `mem_fragmentation_ratio` = RSS / used_memory; healthy ~1.0–1.3; > 1.5 after churny workloads means the allocator holds pages it can't return — fix with `activedefrag yes` or a rolling restart. Also name the fork cost: RDB/AOF-rewrite forks copy-on-write the heap, so a write-heavy 32 GB instance can transiently need ~2× memory — a reason to prefer many medium nodes over few huge ones.

**Monitoring & alerting, concretely.** Per namespace × tier, export: **hit ratio** (alert on *derivative*, not just level — a 95%→92% drop in 5 min is a leading indicator of a bad deploy or key-schema change, long before the DB pages); **eviction rate vs insert rate** (sustained evictions of keys younger than their TTL = under-capacity; Redis `evicted_keys` counter); **expired vs evicted** split (mostly-expired is healthy churn, mostly-evicted is memory pressure); **p50/p99 latency per tier** (L2 p99 creeping from 0.5→5 ms = big keys, slow Lua, or a saturated node's single thread); **memory**: `used_memory` vs `maxmemory` headroom and `mem_fragmentation_ratio`; **connections/ops per shard** to spot hot shards (max/median shard ops > 3× = hot-key or bad hash tag); **stampede telemetry**: lock acquisition failures, singleflight queue depth, stale-serves/s; **invalidation channel**: subscriber count (a server missing from the channel serves stale L1 silently) and publish→observe lag; **DB miss-path QPS** with an alert at the DB's provisioned shed threshold — this is the "cache is failing open" alarm. Dashboard rule of thumb: the top row answers *"is the DB about to die?"* (miss QPS, hit ratio), not cache vanity metrics.

**Idempotency & retries.** Cache ops are naturally idempotent (SET/DEL), so client retries are safe. The dangerous retry is the *loader*: on lock-acquire timeout, losers must not stampede the DB — cap loader concurrency with a semaphore (e.g., ≤ 2× shard count) and prefer serving stale. Write-back drainer must flush idempotently: store deltas with a flush epoch, upsert `SET count = count + :delta WHERE epoch < :e` semantics, so a crash-and-retry doesn't double-count.

**Backpressure.** If DB latency rises, misses queue behind singleflight latches; bound the wait (50 ms) and the inflight map size. If Redis latency rises, the client circuit-breaks per shard (rolling error rate > 50% → open for 5 s) and falls through to DB *with a concurrency limiter* — a fallen-open circuit without a limiter converts a cache brownout into a DB outage.

**Failure walk-through by component:**
- **One Redis shard dies:** replica promoted in ~1–2 s (Cluster failover). During the window, that shard's keys miss → coalesced DB reads, limiter caps damage. Data loss on that shard = fine (cache) except write-back counters → run write-back namespaces with `appendfsync everysec` AOF + replica; worst case lose ≤1 s of deltas.
- **Whole cache cluster dies (cold start):** the scariest scenario. Full miss traffic = 1.4 M QPS at the DB → certain outage without protection. Mitigations: DB-side global concurrency limit + load shedding (serve degraded pages), cache warmer that replays the top-N key list (kept as a periodically snapshotted key-frequency log), and gradual traffic ramp via LB. State in the interview: "the system must be *provisioned* so the DB survives at max shed rate, not at full traffic."
- **Pub/sub gap (network partition):** L1s serve ≤5 s stale; correctness backstop is L1 TTL. Monitor invalidation-channel lag/subscriber count.
- **App server deploy:** L1s start cold → transient 60%-of-traffic shift to L2 (sized for it: L2 must handle 100% of read traffic on its own). Rolling deploys keep the aggregate L1 hit ratio smooth.
- **Clock skew:** TTLs are server-side in Redis (safe); L1 uses monotonic clock for expiry, never wall clock.
- **Poisoned cache entry (bad deploy wrote garbage):** schema version in key namespace lets you bump `v3 → v4` and abandon the poisoned generation instantly — O(1) "flush" without touching Redis.

### Reference tables for the deep dives above

**Ring vs slots, side by side:**

| Dimension | Consistent-hash ring (Ketama/memcached) | Redis Cluster slots |
|---|---|---|
| Placement function | `hash(key)` → first vnode clockwise | `CRC16(key) mod 16384` → slot → node map |
| Ownership authority | Implicit — each client computes independently | Explicit slot map, gossiped; authoritative |
| Rebalance granularity | Statistical (~1/N of keyspace per node change) | Exact, per-slot, throttleable |
| Client disagreement window | Possible during config rollout (split-brain caching) | Resolved by `-MOVED`/`-ASK` redirects |
| Multi-key operations | No affinity control | Hash tags `{...}` pin keys to one slot |
| Load variance | ±10% even with 150 vnodes/node | Deterministic per slot; hot *slots* still possible |
| Coordination cost | Zero | Gossip + slot-migration protocol |

**Memcached slab classes vs Redis memory, side by side:**

| | Memcached slabs | Redis (jemalloc) |
|---|---|---|
| Allocation unit | Fixed chunks per class (96 B, 120 B, … ×1.25) | Arbitrary; per-type encodings (listpack, embstr) |
| Fragmentation type | Internal, bounded (~20% worst case) | External; watch `mem_fragmentation_ratio` |
| Reclaim / compaction | None (page automover only) | `activedefrag`, or rolling restart |
| Eviction scope | LRU per slab class | Global policy across keyspace (LRU/LFU sampling) |
| Pathology | Slab calcification on size-mix shift | Fork copy-on-write spike during RDB/AOF rewrite |
| Predictability | Very high — no allocator surprises | Good, but requires monitoring |

**Warming technique selection:**

| Scenario | Technique | Why |
|---|---|---|
| Single node replaced | Snapshot-seed from peer replica | Fastest to hot; staleness converges via TTL |
| New cluster / region | Shadow traffic + hit-ratio admission gate | Realistic working set; no guessing top-K |
| Post-flush (generation bump) | Replay top-K frequency log, rate-limited | Controlled DB load; hits the true hot set first |
| App deploy (L1 cold) | Rolling deploy + boot-time priming of critical keys | Keeps aggregate L1 hit ratio smooth |

**Cross-region invalidation sequence (regional-CDC option), step by step:**

```
1. Writer (region A, home region) commits UPDATE to primary DB.
2. Primary streams the change to region B's read replica (lag: 50–500 ms).
3. Region B's invalidator tails ITS OWN replica's CDC stream.
4. Change becomes visible on B's replica  →  invalidator sees it  →  DELs
   B's L2 key and publishes on B's L1 bus.
5. Next read in B misses, loads the NEW value from B's replica (guaranteed
   present — the CDC event and the row arrived on the same stream).
```
The invariant bought in step 3: *invalidation can never outrun the data it invalidates for*. A global bus breaks this — the DEL can arrive in B before replication does, and the repopulating read caches the old value for a full TTL.

**Lease-based anti-stale-set (Facebook memcache), sketch:**

```
on GET miss(key):
    lease_token = cache.miss_with_lease(key)     # cache remembers token
    val = db.read(key)
    cache.set_if_lease_valid(key, val, lease_token)  # refused if a DEL
                                                     # invalidated the lease
on WRITE(key):
    db.write(key)
    cache.delete(key)        # also invalidates all outstanding lease tokens
```
The lease closes the read-repopulate race exactly: any SET whose read began before the invalidating write is refused, because its token died with the DEL. Bonus: handing out one lease per key per interval is also stampede control — non-holders briefly wait or serve stale.

**Metrics that must exist** (per namespace × tier):

| Metric | Healthy | Alert condition | What it catches |
|---|---|---|---|
| Hit ratio | ≥ 95% hot namespaces | Drop > 3 pts in 10 min (derivative alert) | Bad deploy, key-schema change, bot scans |
| Miss-path DB QPS | Under shed threshold | > 80% of DB provisioned ceiling | Cache failing open; the outage precursor |
| Evicted vs expired split | Mostly expired | Evictions of keys younger than TTL | Under-capacity / memory pressure |
| p99 per tier | L1 µs, L2 < 1 ms | L2 p99 > 5 ms | Big keys, hot shard, slow Lua |
| `mem_fragmentation_ratio` | 1.0–1.3 | > 1.5 sustained | Allocator fragmentation → defrag/restart |
| Max/median shard ops | < 2× | > 3× | Hot key or bad hash tag |
| Singleflight queue depth, lock failures | ~0 | Sustained growth | Stampede forming; DB slowness |
| Stale-serves/s | Low, bounded | Spike | Origin unhealthy; grace-mode active |
| Invalidation subscriber count | = fleet size | Any shortfall | Server silently serving stale L1 |
| Invalidation publish→observe lag | < 100 ms | > 1 s | Partition; L1 TTL becomes the only backstop |

## 8. Trade-off Summary & Interview Soundbites

| Decision | Trade-off accepted |
|---|---|
| Cache-aside as default | Extra app-side logic + first-miss latency, for the cleanest failure isolation |
| Delete-on-write, not update-on-write | One extra miss per write, to eliminate concurrent-writer stale-forever bug |
| L1 with 2–5 s TTL + pub/sub | Bounded seconds of staleness, for ~60% traffic offload and ns latency |
| TTL jitter everywhere | Slightly unpredictable expiry, kills synchronized mass expiry |
| Singleflight + distributed lock + serve-stale | Added latency/complexity on miss path, for DB protection at expiry |
| LFU for catalog, LRU for sessions | Per-namespace tuning burden, for hit-ratio gains matched to access pattern |
| Write-back for counters only | Bounded loss window on crash, for 100× DB write reduction |
| Redis over Memcached | Some throughput/memory efficiency, for structures, locks, pub/sub, replication |
| Negative caching + Bloom filter | Memory + false positives, to defeat penetration attacks |
| Slot-based sharding (Redis Cluster) over a bare ring | A coordination protocol to run, for exact, observable, throttleable resharding |
| Regional caches fed by local CDC, not a global bus | Per-region invalidator infrastructure, to eliminate the replica-lag repopulate race |
| Hit-ratio admission gate on cold nodes | Slower node turn-up, to prevent self-inflicted partial stampedes |
| Bounded write-back buffer with degrade-to-write-through | Occasional write-latency spikes under drainer lag, for a hard cap on the loss window |
| Short TTLs even on "static" data (≤ 24 h) | Some avoidable misses, for bounded staleness, compliance-friendly deletion, and forgotten-invalidation insurance |

**Soundbites:**
1. "The cache is an optimization, never a dependency — every failure mode must degrade to the database plus a load shedder, not to an outage."
2. "On writes I delete the cache key rather than update it: concurrent updates can interleave and pin a stale value forever; a delete always converges."
3. "At 95% hit ratio the DB sees 70 K QPS; at 90% it sees 140 K — each point of hit ratio is a fleet of database replicas."
4. "Jitter breaks herds, coalescing breaks stampedes, and stale-while-revalidate makes both invisible to p99."
5. "The hotter the key, the better L1 handles it — in-process caching is the only hot-key fix that gets *stronger* as the key gets hotter."
6. "L1's short TTL is the correctness backstop, so the invalidation bus can be fast and lossy instead of durable and slow."
7. "Schema version in the key is a free O(1) cache flush."
8. "A cold cache node admitted to the pool is a stampede you scheduled yourself — gate admission on measured hit ratio."
9. "In multi-region, invalidate off the local replica's CDC stream, not the primary's — otherwise you race replication lag and cache the value you just deleted."
10. "Write-back without reconciliation is slow silent corruption; the nightly recount is part of the pattern, not an extra."
11. "Alert on the derivative of hit ratio — by the time the absolute number looks bad, the database already knows."

**Common follow-ups:**
- *"Why not write-through everywhere?"* Couples every write's latency and availability to the cache, and warms keys nobody reads; reserve it for strict read-after-write namespaces.
- *"How do you invalidate a list/page cache when one item changes?"* Tag-based invalidation: maintain `tag:{product_id} → {page keys}`, DEL the set members; or version the item and embed versions in the page key.
- *"TTL vs explicit invalidation?"* Both: explicit invalidation for correctness on known write paths, TTL as the safety net for the write paths you forgot.
- *"Redis Cluster vs client-side sharding vs proxy (Twemproxy/Envoy)?"* Cluster for built-in failover and slot migration; proxy when clients are polyglot and you want thin clients; client-side when you need per-key routing tricks (hot-key replication).
- *"How would CDC-based invalidation work?"* Debezium tails the DB binlog, an invalidator service maps table rows → cache keys and issues DELs; removes reliance on app code remembering to invalidate, at the cost of an async pipeline (~100 ms lag).
- *"What breaks first at 10× traffic?"* L2 egress bandwidth and hot shards — answer: raise L1 TTL/size, protobuf compression, hot-key replication, and more shards via slot rebalancing.
- *"How do you cache paginated or filtered query results, not just entities?"* Two-level composition: cache the *ID list* per query signature (`q:{hash(filters,page)} → [id1..id20]`, short TTL 30–60 s) and hydrate each ID through the entity cache. One item edit invalidates only that entity; the list refreshes on its short TTL. Never cache fully rendered result pages for mutable data — invalidation fan-in is unbounded. If list freshness matters (inventory search), tag the list keys by the dominant filter dimension and use tag invalidation.
- *"Your hit ratio dropped from 96% to 80% overnight — how do you debug it?"* Ordered checklist: (1) deploy diff — did a key schema/serialization change silently create a new namespace (v3→v4 bump doubles the working set)? (2) eviction metrics — did a new feature's keys blow the memory budget and evict the old hot set (evicted-young count)? (3) traffic mix — a bot/crawler scanning the long tail destroys LRU (fix: LFU or admission filter); (4) TTL regression — someone "fixed" staleness by dropping a TTL from 600 s to 5 s; (5) shard health — one shard flapping means 1/N of keyspace is effectively uncached. The metric that discriminates fastest is evictions-vs-expiries split plus per-namespace hit ratio, which is why both must pre-exist the incident.
- *"When is it correct to NOT cache something?"* When any of these hold: read rate ≲ write rate (invalidation churn exceeds the benefit — the cache is mostly a delete stream); strict linearizability required (checkout inventory, auth token revocation checks — a stale allow is a security bug); values are huge and read once (bulk export); or the DB read is already ~sub-ms and the added cache hop, coherence machinery, and failure modes don't pay for themselves. Framing: caching buys latency and DB offload at the price of a *second system that can disagree with the first* — if the purchase price exceeds the benefit, decline.
- *"How does caching interact with GDPR-style deletion?"* Explicit invalidation is now a compliance operation, not an optimization: on erasure, DEL all keys for the subject (tag index `tag:user:{id}` makes this tractable), purge L1 via the bus, and bound worst-case residency by max TTL — which becomes a documented compliance number. This is a strong argument for capping TTLs (≤ 24 h) even on "static" data, and for never caching personal data with no TTL.
