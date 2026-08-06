# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 31 of 63 — Phase 2: System Design Track (Design 3 of 35)
**Topic:** Distributed Cache (Redis-like / Memcached-like)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
This is the "build Redis/Memcached yourself" interview — and it's a favorite because it forces you to reason about memory, eviction, sharding, and replication *simultaneously*. A distributed cache is deceptively simple as a KV `get/set`, but the design pressure comes from operating at 500K+ ops/sec per node with sub-millisecond p99, surviving node loss without a cold-start thundering herd on the origin DB, and keeping a distributed set of nodes coherent enough to be useful without paying full-consistency latency. Amazon (ElastiCache), Meta (memcached at trillions of ops/day), and Twitter all published hard-won lessons here.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Caches / Redis / Memcached (taught in Phase 1, Lesson 24):** Cache-aside, read-through, write-through, write-back. Eviction policies (LRU, LFU, TTL, random). Redis (single-threaded, rich data types, persistence via RDB/AOF) vs Memcached (multi-threaded, slab allocator, pure KV). This *is* the system — you're building one.

**LRU & data structures (taught in Phase 1, Lesson 4 & 5):** LRU = hashmap + doubly-linked list for O(1) get/put/evict. LFU adds frequency counts. Bloom filters (Lesson 5) guard against cache-penetration lookups of non-existent keys. Skip lists / hash tables for the index.

**Consistent hashing (taught in Phase 1, Lesson 19):** The core of distributing keys across cache nodes with virtual nodes, so adding/removing a node remaps only ~1/N of keys instead of rehashing everything. Central to the sharding answer.

**HA / replication / quorum (taught in Phase 1, Lesson 3):** Primary-replica replication, failover, read replicas, quorum reads/writes. Determines how the cache survives node death.

**CAP / consistency (taught in Phase 1, Lesson 2):** A replicated cache is eventually consistent by default; you decide where you need more.

**Kafka / event-driven (taught in Phase 1, Lesson 21):** For cross-region cache invalidation fan-out and change-data-capture-driven invalidation.

> **Recap box:** LRU = hashmap + DLL (O(1)) · consistent hashing spreads keys & limits remap · primary-replica for HA · cache-aside is the default pattern · Bloom filter stops penetration · replicated cache = eventually consistent.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Implement an in-memory LRU cache with O(1) get and put. What data structures?
> **What a strong answer covers:** **Hashmap + doubly-linked list.** Hashmap: key → node pointer for O(1) lookup. DLL: maintains recency order; most-recently-used at head, LRU victim at tail. `get`: hashmap lookup, move node to head. `put`: insert at head; if over capacity, remove tail node and its hashmap entry. Both O(1) because DLL splice is pointer surgery, no scan. Mention thread-safety needs a lock or sharded locks under concurrency.
> **Common weak answer:** Using an array/list and scanning for LRU (O(n) eviction) or a timestamp + sort (O(n log n)).
> **Mentor follow-up if they answer well:** LRU evicts a hot key if it was briefly idle during a scan (sequential flooding). How does LFU or ARC / Redis's LRU-approximation-with-sampling fix that?

Q2. A node holds 64GB RAM. With ~1KB average values and ~50 bytes overhead per entry, how many keys fit, and what ops/sec should one node sustain?
> **What a strong answer covers:** ~1KB + ~50B overhead + DLL pointers (~16–48B) ≈ ~1.1KB effective per entry. Reserve ~25% headroom (fragmentation, OS, replication buffers) → ~48GB usable → **~43–45M keys**. Ops/sec: an in-memory single-threaded server (Redis-style) does ~100–200K ops/sec/core; a multi-threaded one (Memcached-style) scales to **500K–1M+ ops/sec** on a multi-core box for small values, network-bound before CPU-bound. Ground p99 at sub-millisecond in-DC.
> **Mentor follow-up:** Memory fragmentation eats real capacity. How does a slab allocator (Memcached) vs jemalloc (Redis) change your usable-memory math?

Q3. Cache-aside vs write-through vs write-back — pick one for a read-heavy product catalog and defend it.
> **What a strong answer covers:** **Cache-aside** (lazy loading, Lesson 24): app reads cache, on miss reads DB and populates. Best for read-heavy where not all data is hot — you only cache what's actually requested, cache failure just means DB reads (degraded, not broken). Write-through adds write latency (every write hits cache+DB) but keeps cache fresh — good when reads immediately follow writes. Write-back (cache first, async DB flush) risks data loss on node death — only for tolerable-loss counters/metrics. For a catalog: cache-aside + short TTL.
> **Red flag answer:** Write-back for a system of record — a node crash loses unflushed writes.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design a distributed cache cluster: many nodes, clients, sharding, replication. Draw it.
> **Key components expected:** Client library (smart client with hash ring) OR a proxy tier → cache nodes (sharded by consistent hashing) → per-shard primary + replicas → config/topology service (cluster membership) → origin DB behind it.
> **Architecture diagram (text):**
```
   App servers (smart clients hold the hash ring)
        │  get(k) → hash(k) → vnode → shard
        ▼
 ┌──────────────── Consistent-hash ring (vnodes) ────────────────┐
 │                                                               │
 │  Shard A            Shard B            Shard C                 │
 │  ┌─────────┐        ┌─────────┐        ┌─────────┐             │
 │  │primary  │        │primary  │        │primary  │             │
 │  │ + repl1 │        │ + repl1 │        │ + repl1 │             │
 │  │ + repl2 │        │ + repl2 │        │ + repl2 │             │
 │  └─────────┘        └─────────┘        └─────────┘             │
 └───────────────────────────┬───────────────────────────────────┘
                             │ membership / gossip
                    ┌────────┴─────────┐
                    │ Topology / Config │  (or Redis Cluster gossip)
                    │  service (etcd)   │
                    └───────────────────┘
   miss ──► Origin DB   (populate on read; invalidation events via Kafka)
```
> **What separates SDE2 from SDE3 here:** SDE2 shards and replicates. SDE3 decides **client-side hashing vs proxy tier** (smart client = one fewer hop but fat clients + topology-change coordination; proxy like Twemproxy/Envoy = thin clients but extra hop + proxy scaling), specifies **how topology changes propagate** without a coordinated stop-the-world remap, and designs the **thundering-herd protection** for when a shard dies cold.

Q5. Trace a `get(key)` and a `set(key,val)` end-to-end including a miss.
> **Expected trace (get):** (1) Client hashes key → locates vnode → maps to shard primary (or a replica for reads). (2) Send `GET` over the wire protocol (RESP-like). (3) Node hashes into its local hashtable, finds LRU node, splices to head, returns value — sub-ms. (4) Miss → return nil; app reads origin DB, then `SET`s with TTL (cache-aside). **set:** client hashes → primary → write to local store + append to replication stream → replicas apply async → ack. Optionally write-through to DB.
> **Tricky part:** On a **miss under high concurrency** for the same key, thousands of clients simultaneously read the DB (cache stampede). The trace must include request coalescing / a mutex-per-key so only one client repopulates.

Q6. Define the wire protocol / client API.
> **Expected API design:** Core: `GET k`, `SET k v [EX ttl]`, `DEL k`, `MGET k1 k2...` (batch to amortize RTT), `INCR k`, `EXPIRE k ttl`, `CAS k v version` (compare-and-set for optimistic writes). A compact binary or RESP-style text protocol over persistent TCP connections (connection pooling — never per-request connect). Pipelining to batch commands on one RTT.
> **What to push on:** **MGET batching** and **pipelining** are the throughput levers (amortize network RTT over many keys). **CAS/versioning** for concurrent updates. Consistent-hash-aware clients so `MGET` across shards fans out correctly. TTL semantics (lazy vs active expiry). No pagination concern; the concern is round-trip amortization.

---

### Data Modeling (Medium–Hard)
Q7. Design the in-node storage: index + eviction + expiry structures.
> **Expected schema:**
```
Node internal:
  hashtable:  key -> Entry*                     # O(1) lookup
  Entry { key, value_ptr, ttl_expire_at,
          lru_prev, lru_next,                   # DLL for LRU
          freq }                                # for LFU option
  lru_list:  doubly-linked list (head=MRU, tail=LRU victim)
  expiry:    (a) lazy — check ttl on GET, evict if expired
             (b) active — sampled background sweep (Redis: sample 20
                 keys with TTL, evict expired, repeat if >25% expired)
  memory:    slab allocator (fixed size classes) OR jemalloc arenas
```
> **Index choices and why:** Open-addressing or chained hashtable for O(1) average lookup; DLL for O(1) LRU reorder/evict. Slab allocation groups same-size objects to cut fragmentation (Memcached); the trade-off is slab calcification (a size class hoarding memory).
> **Partitioning key and why:** Across the cluster, partition by `hash(key)` on a consistent-hash ring with **virtual nodes** (Lesson 19) — ~100–300 vnodes per physical node so load is even and a node join/leave remaps only its vnode slices (~1/N of keys), not the whole space.

Q8. How do you avoid a cache stampede when a hot key expires and 10K requests miss simultaneously?
> **Expected answer:** Multiple layers: (1) **Request coalescing / single-flight** — a per-key in-flight lock so only the first miss recomputes from DB and the rest wait for that result (Go's singleflight, or a short-lived Redis lock). (2) **Probabilistic early expiration** — recompute the value slightly before TTL with a probability increasing as expiry nears (XFetch algorithm), so it never expires under load for everyone at once. (3) **Stale-while-revalidate** — serve the stale value while one worker refreshes. (4) **Bloom filter** (Lesson 5) to short-circuit lookups of keys that don't exist at all (penetration).
> **Trap:** Naive cache-aside where every missing request independently hits the DB — a single hot-key expiry becomes a **10K-QPS spike on the origin**, which can topple the DB (a real Facebook incident pattern).

Q9. Replicated cache consistency: primary vs replica reads — what can go wrong?
> **Expected answer:** Replication is **async** by default (Lesson 3), so a replica lags the primary by some ms. Reading from a replica after writing to the primary can return a **stale value** (read-your-writes violation). This is fine for most cache use (eventual consistency, Lesson 2 — AP). Where read-your-writes matters (user just updated their profile), route those reads to the primary or use a session-sticky/"read-from-primary-after-write" window.
> **Mentor pushback:** During a **failover**, an async replica promoted to primary may be missing the last few writes the old primary acked but didn't replicate — **lost writes**. For a pure cache that's tolerable (repopulate from DB). But if you used write-back, those lost writes are lost *data*. So replication mode (async vs semi-sync) and cache-mode (cache-aside vs write-back) interact — name that coupling.

---

### Low-Level Design (Hard)
Q10. A cache node dies. Its ~45M keys vanish. Design so the cluster survives without melting the origin DB.
> **Problem statement:** One shard's primary crashes at 500K ops/sec. Its keys are now cold; every request for them misses and hits the DB.
> **Naive solution:** Consistent hashing just remaps that node's keys to the next node on the ring — but the next node doesn't *have* the data, so it's all misses → DB stampede.
> **Why naive fails at scale:** 45M keys × even a fraction being hot = a massive synchronized miss storm on the origin the instant the node drops. The DB, sized for cache-shielded load, gets hit with full traffic and cascades.
> **Expected optimal approach:** (1) **Replicas per shard** (Lesson 3): promote a warm replica on failure — near-zero cold miss because the replica already holds the data. (2) **Consistent hashing with replication factor** so keys live on N successor nodes; a death shifts reads to a node that already has the data. (3) **Gradual rewarming + request coalescing** if data truly must be reloaded, so at most one DB read per key. (4) Origin-side rate limiting / load shedding as a backstop.
> **Pseudo-code or class diagram:**
```
on_node_failure(dead):
    ring.remove_vnodes(dead)                 # remap dead's ranges
    for shard in shards_of(dead):
        promote(replica_of(shard))           # replica already warm
    # reads now hit warm replica → no cold DB stampede
get(k):
    shard = ring.locate(k)
    v = shard.primary_or_replica.get(k)
    if v is None:
        v = singleflight(k, lambda: db.read(k))   # one DB read per key
        shard.set(k, v, ttl)
    return v
```

Q11. Two clients concurrently `SET` the same key with different values; you also have async replication. Which wins, and how do you make it deterministic?
> **Scenario:** Client A `SET k=1`, Client B `SET k=2` arrive near-simultaneously at the primary; replicas may apply in different orders.
> **Expected fix:** The **single-threaded primary serializes** writes (Redis) — last-writer-wins by arrival order at the primary, and replicas apply the primary's ordered replication stream, so all replicas converge to the same value. For app-level correctness (avoid clobbering), use **CAS with a version/CFlag**: `SET k=v IF version==N`; the loser retries. For multi-primary/active-active, you need **CRDTs or vector clocks** (last-write-wins register with timestamps) to converge deterministically.
> **Follow-up:** What if the primary acks a write then dies before replicating? With async replication that write is lost on failover. Use **semi-synchronous replication** (wait for ≥1 replica ack) when write durability matters — at the cost of write latency.

Q12. Network partition splits the cluster; a shard's primary and replica can't see each other. Both think they're primary. What happens?
> **Scenario:** Split-brain (Lesson 2, Lesson 3) — two nodes accept writes for the same shard.
> **Expected handling:** Use a **quorum-based / consensus membership** (Redis Cluster's gossip + majority, or etcd/Zookeeper leader election, Lesson 17): a primary that can't reach a majority **steps down** and refuses writes (fail-closed on the minority side) to prevent divergent writes. On heal, the minority side's writes are discarded/reconciled. Configure `min-replicas-to-write` so a primary with no reachable replicas stops accepting writes. Accept reduced availability on the minority partition to preserve consistency — the CP choice for the write path. Reads may continue serving stale on both sides (tolerable for a cache).

---

### Scaling to 10x / 100x (Hard)
Q13. At 100x (say 50M ops/sec cluster-wide), where does it break first?
> **Expected answer:** **Hot keys and network/NIC saturation**, before total memory. A single celebrity key at millions of req/s pins one node's CPU/NIC — consistent hashing can't split one key across nodes. Second: cross-shard `MGET` fan-out amplifying connection counts. Memory is usually *not* the first wall if you shard enough; the walls are per-node throughput (single-threaded ~200K ops/sec) and hot-key concentration.
> **Numbers to ground the answer:** 50M ops/sec ÷ ~200K ops/sec/node (Redis single-thread) ≈ **250 nodes** minimum for even load — but a hot key can need far more just for itself. A 10Gbps NIC at ~1KB values caps ~1.25M ops/sec on bandwidth alone. p99 target stays sub-ms; tail latency from GC/fork (RDB snapshot) is the sneaky killer.

Q14. Shard across 250 nodes. Key, and hot-key handling?
> **Expected sharding strategy:** Consistent hashing with ~150 virtual nodes per physical node (Lesson 19) — even distribution, minimal remap on scale events. Hash the key (e.g., CRC16 mod 16384 slots like Redis Cluster) → slot → node.
> **Hot spot problem:** A single hot key is unsplittable by hashing. Detect via per-key request sampling / top-N heavy-hitter tracking. Fix: (a) **client-side local caching (L1)** of the hot key with a short TTL so most reads never leave the app process; (b) **key replication/fan-out** — store copies as `hotkey#0..N` across nodes and have clients pick a random suffix to spread reads; (c) dedicated replica fleet for the hot key serving reads. For hot *slots* (not single keys), migrate slots to less-loaded nodes.

Q15. Design the layered caching around this distributed cache.
> **Expected layered cache design:** **L1** — in-process near-cache in the app (Caffeine/Guava LRU), microsecond access, absorbs hot keys and cuts network hops; small, short TTL. **L2** — this distributed cache cluster (the shared tier). **Origin** — the DB. Optionally an **L0 CDN/edge** for cacheable HTTP responses. Reads: L1 → L2 → DB, populating upward.
> **Cache invalidation trap:** The **L1 near-cache is the hardest to invalidate** — it's spread across thousands of app processes with no central control. A write invalidates L2 easily (DEL) but L1 copies stay stale until their TTL. Fix: keep L1 TTL short (seconds), and for correctness-critical keys publish invalidation events via **Kafka/pub-sub (Lesson 21)** that every app node subscribes to and evicts its L1 entry. This is the classic multi-layer coherence problem — acknowledge you're trading a small staleness window for the huge hit-rate and latency win.

Q16. Keep it cost-efficient at 100x.
> **Expected answer:** (1) **Tiered memory** — hot data in RAM, warm in NVMe/SSD (Redis on flash / extstore in Memcached) at a fraction of RAM cost. (2) **Compression** of large values (LZ4) — trade CPU for memory. (3) **Right-size TTLs** so dead data self-evicts and you buy less RAM. (4) **Slab/allocator tuning** to cut fragmentation waste (real 20–30% recoverable). (5) **L1 near-cache** reduces L2 node count needed. (6) **Smaller values** — store IDs/deltas not whole blobs; normalize. (7) Reserved-instance / spot for replicas.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Explain Redis's *approximate* LRU (sampled eviction) vs true LRU, and why it exists. (True LRU needs a global DLL touched on every access — memory + write-amp cost. Redis samples N keys (default 5, tunable) and evicts the oldest among them — O(1)-ish, near-LRU quality, far cheaper. LFU mode (Morris counters) approximates frequency in a few bits. This is the memory-vs-accuracy trade-off in eviction.)

**H2.** Active-active multi-region cache: same key written in us-east and eu-west. How do you converge? (CRDTs — a LWW-register with hybrid logical clocks, or PN-counters for counters. Or designate a per-key home region. Explain why naive async replication both directions causes permanent divergence without conflict resolution.)

**H3.** Resize the cluster from 250 → 400 nodes with zero downtime and minimal miss spike. (Add nodes, migrate slots gradually (Redis Cluster `MIGRATE` slot-by-slot), clients follow `MOVED`/`ASK` redirects; consistent hashing means only ~1/N keys move. Warm new nodes before cutting traffic to avoid a cold-miss wave.)

**H4.** What do you instrument? (Hit ratio (the #1 health metric), p50/p99/p999 latency, per-node ops/sec & memory used/frag ratio, eviction rate (rising = undersized), hot-key top-N, replication lag, connection counts. Alert on hit-ratio drop (the leading indicator of DB overload) and eviction-rate spikes.)

**H5.** You launched with client-side consistent hashing baked into a fat client library; now every topology change requires redeploying every app. Migration story to a proxy/managed-cluster model. (Introduce a proxy tier (Envoy/Twemproxy) or move to Redis Cluster with smart-but-thin clients; dual-run, shift reads gradually, then decommission the fat-client hashing. Lesson: putting topology in the client couples deploys to cluster ops — a decision you pay for at scale.)

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. **Ignoring the cache-stampede / thundering-herd** on hot-key expiry or node death — the failure that actually takes down production (via the origin DB), not the cache.
2. **Hand-waving eviction** — saying "LRU" without the hashmap+DLL O(1) mechanics or knowing real systems use *approximate* sampled LRU for cost.
3. **Assuming replicated caches are consistent** — missing async lag, read-your-writes violations, and lost-writes-on-failover.

**The one insight that makes an answer truly impressive:**
State that a distributed cache's real job is **shielding the origin**, so the design must be evaluated by what happens to the DB when the cache degrades — stampede protection (single-flight, probabilistic expiry, warm replicas) is more important than the happy-path get/set. Candidates who optimize the hit path but ignore the miss-storm miss the point.

**Suggested follow-up reading:**
- "Scaling Memcache at Facebook" (NSDI 2013) — leases, the thundering-herd fix, regional pools; the definitive paper.
- Redis Cluster specification + "A CRDT approach" (Redis Enterprise Active-Active / Roshi from SoundCloud).

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
