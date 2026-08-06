# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 29 of 63 — Phase 2: System Design Track (Design 1 of 35)
**Topic:** URL Shortener (TinyURL / Bitly)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
A URL shortener looks trivial — "just hash a string" — and that is exactly why it is the classic opener. The hard parts hide in plain sight: generating billions of collision-free short keys without a coordination bottleneck, serving 100:1 read-heavy traffic at single-digit-millisecond latency, and doing all of it while staying cheap. Bitly, TinyURL, and every social platform's `t.co`/`fb.me`/`lnkd.in` link wrapper solved this; the interesting engineering is in the ID-generation strategy, the read path caching, and analytics fan-out — not the redirect itself.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Base conversion & hashing (foundation from Lesson 4 & 5):** A short code is just an integer rendered in a higher base. Base62 (`[0-9a-zA-Z]`) gives 62^7 ≈ 3.5 trillion keys in 7 characters. Hashing (MD5/SHA) can also generate keys but requires collision handling. Today you'll choose between a counter-in-base62 approach and a hash-and-truncate approach.

**Consistent hashing (taught in Phase 1, Lesson 19):** Maps keys to nodes on a hash ring with virtual nodes so that adding/removing a shard only remaps ~1/N of keys. Here it is used to shard the key→URL mapping across the KV store and to distribute a pre-generated key pool.

**Caches / Redis / Memcached (taught in Phase 1, Lesson 24):** LRU/LFU eviction, TTLs, cache-aside vs read-through. The redirect read path is 100:1 read-heavy, so a Redis/Memcached layer in front of the datastore is the single most impactful component. Recap cache-aside: on miss, read DB, populate cache, return.

**SQL vs NoSQL / B-tree vs LSM (taught in Phase 1, Lesson 23):** A URL mapping is a pure point-lookup on an immutable key. A B-tree KV (or a hash-partitioned NoSQL like DynamoDB/Cassandra) fits better than a relational join model. LSM trees give fast writes for the analytics/click stream.

**Rate limiting (taught in Phase 1, Lesson 11):** Needed to stop abuse of the create endpoint and to protect the redirect path from scrapers. Token bucket per API key.

**CDN / edge (taught in Phase 1, Lesson 18):** 301/302 redirects can be served from the edge for hot links, cutting origin traffic dramatically.

**Capacity estimation (taught in Phase 1, Lesson 28):** Drives every sizing decision below — writes/sec, reads/sec, storage growth, cache working-set size.

> **Recap box:** Base62(counter) = collision-free keys · consistent hashing = shard without remapping everything · cache-aside on a 100:1 read ratio · point-lookup KV beats relational · rate-limit the create path · edge-cache hot redirects.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Why do we use Base62 for the short code instead of hex or Base64? How many characters do we need for 10 billion URLs?
> **What a strong answer covers:** Base62 = `0-9A-Za-z`, all URL-safe with no encoding needed. Base64 adds `+` and `/` which need percent-encoding in a URL, and `=` padding — bad for a short link. Hex only uses 16 symbols so codes get long. For 10B URLs: 62^6 ≈ 56.8B, so 6 chars technically suffice, but you pick **7 chars** (62^7 ≈ 3.5 trillion) for headroom and to avoid running out. Show the log math: ceil(log(1e10)/log(62)) = 6.
> **Common weak answer:** "Base64 because it's standard" — ignores the URL-unsafe characters.
> **Mentor follow-up if they answer well:** If short codes are sequential integers rendered in Base62, they're guessable and enumerable. How do you stop a competitor scraping your entire link corpus?

Q2. Estimate the write QPS, read QPS, and 5-year storage. Assume 100M new URLs/day and a 100:1 read:write ratio.
> **What a strong answer covers:** Writes: 100M/86400 ≈ **1,160 writes/sec** (call it ~1.2K, peak 2–3x ≈ 3.5K). Reads: 100:1 → **116K reads/sec** average, peak ~350K/sec. Storage per row ≈ short_code(7) + long_url(~500B avg) + metadata(~100B) ≈ ~600–700 bytes; round to ~1KB with index overhead. 100M/day × 365 × 5 = 182.5B rows × 1KB ≈ **~180 TB** over 5 years. That's why you shard and why you don't store analytics inline.
> **Mentor follow-up:** Where does the 180TB live — one big table or sharded? What's your shard count if a node holds ~2TB usable?

Q3. 301 vs 302 redirect — which do you return and why?
> **What a strong answer covers:** 301 = permanent, browsers and proxies cache it aggressively, so subsequent clicks may **never hit your server** — great for load, terrible for analytics because you lose click counts. 302 = temporary, browser re-requests each time, so you capture every click but carry full redirect traffic. Bitly uses 301 for some, but analytics-driven shorteners lean **302** (or 307) to keep click tracking. Answer: 302 by default when analytics matter; 301 only when you want max cache offload and don't need per-click data.
> **Red flag answer:** "301 because permanent sounds better" without mentioning the analytics/caching trade-off.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Draw the full architecture: create path and redirect path, including the ID generation service.
> **Key components expected:** Client → CDN/edge → LB (Lesson 6) → API service (create) + Redirect service (read) → Key Generation Service (KGS) → KV store (sharded) + Cache (Redis) → async click pipeline (Kafka → analytics store).
> **Architecture diagram (text):**
```
                    ┌─────────── CDN / Edge (hot 302s) ───────────┐
                    │                                              │
  Client ──HTTP──► Load Balancer (L7) ──┬──► Create API ──► KGS (key pool)
                                        │        │              │
                                        │        ▼              ▼
                                        │   Rate Limiter   ┌──────────────┐
                                        │                  │ ID counter / │
                                        │                  │ Zookeeper or │
                                        │                  │ range alloc  │
                                        │                  └──────────────┘
                                        │        │
                                        │        ▼
                                        │   ┌─────────────────────┐
                                        └──►│ Redirect Service     │
                                            └──────────┬──────────┘
                                             miss│      │hit
                                                 ▼      ▼
                                        ┌────────────┐ ┌───────────┐
                                        │ Redis (LRU)│ │  return   │
                                        └─────┬──────┘ └───────────┘
                                              │ miss
                                              ▼
                                 ┌───────────────────────────┐
                                 │  Sharded KV (DynamoDB /    │
                                 │  Cassandra) key→long_url   │
                                 └───────────────────────────┘
   Every redirect ──async──► Kafka (click events) ──► Flink/Spark ──► Analytics DB
```
> **What separates SDE2 from SDE3 here:** SDE2 draws the boxes. SDE3 calls out that the KGS is the only stateful coordination point and pre-generates keys in **batches (ranges)** so the create path never blocks on a global counter — and that the click pipeline is fully async so a redirect never waits on analytics.

Q5. Trace a single redirect from click to response, end-to-end.
> **Expected trace:** (1) Browser requests `short.ly/aZ3xK9p`. (2) Edge/CDN checks its cache — if a cached 302 exists, respond immediately. (3) Else L7 LB routes to a Redirect service instance. (4) Service looks up `aZ3xK9p` in Redis (cache-aside, Lesson 24). (5) Hit → return `302 Location: <long_url>` in <1ms server time. (6) Miss → read from sharded KV (consistent-hash the key to a shard, Lesson 19), populate Redis with TTL, return 302. (7) Fire-and-forget a click event to Kafka (Lesson 21) — do NOT block the response on it.
> **Tricky part:** Candidates block the redirect on the analytics write. The click event must be async (local buffer → batched produce to Kafka); if Kafka is down, the redirect still succeeds and events are dropped or buffered to disk, never failing the user request.

Q6. Design the API. Define the create and redirect endpoints.
> **Expected API design:**
> - `POST /api/v1/urls` body `{ "long_url": "...", "custom_alias": "optional", "expiry": "optional ISO8601" }` → `201 { "short_url": "https://short.ly/aZ3xK9p", "code": "aZ3xK9p" }`. Requires API key; rate-limited (Lesson 11).
> - `GET /{code}` → `302` with `Location` header (the redirect itself, served by the redirect service, not `/api`).
> - `GET /api/v1/urls/{code}` → metadata (owner, created_at, click_count).
> - `DELETE /api/v1/urls/{code}` → soft-delete.
> **What to push on:** **Idempotency** — a retried `POST` for the same long_url + same api key should return the same short code, not mint a new one (idempotency key or a `long_url` hash lookup). **Versioning** via `/v1/`. **Pagination** for listing a user's links (cursor-based, not offset). Custom alias must check uniqueness atomically against the KV store.

---

### Data Modeling (Medium–Hard)
Q7. Design the primary storage schema for the mapping.
> **Expected schema:**
```sql
-- Primary mapping (NoSQL point-lookup; shown relationally for clarity)
CREATE TABLE url_mapping (
  short_code   VARCHAR(8)  PRIMARY KEY,   -- Base62, partition key
  long_url     TEXT        NOT NULL,
  owner_id     BIGINT,
  created_at   TIMESTAMP   NOT NULL,
  expiry_at    TIMESTAMP,                 -- NULL = never
  is_active    BOOLEAN     DEFAULT TRUE
);
-- Reverse lookup for idempotent creation (dedup same long_url per owner)
CREATE TABLE url_by_hash (
  url_hash     CHAR(32),                  -- md5(owner_id + long_url)
  short_code   VARCHAR(8),
  PRIMARY KEY (url_hash)
);
```
> **Index choices and why:** `short_code` is the partition/primary key — every redirect is a single-key point lookup, no secondary index needed on the hot path. `url_by_hash` is a separate table (not a secondary index) so idempotency lookups don't add write amplification to the hot mapping table. Avoid a global secondary index on `long_url` (huge, low-cardinality-per-value).
> **Partitioning key and why:** Partition on `short_code` with a hash partitioner (Lesson 19). It's high-cardinality and uniformly random (if keys come from a Base62 counter with bit-scrambling), so load spreads evenly. Partitioning on `owner_id` would hot-spot on power users.

Q8. How do you serve the redirect lookup efficiently at 116K reads/sec?
> **Expected answer:** Cache-aside with Redis in front of the KV store. Working set: not all 180TB is hot — link clicks follow a power law (Zipfian), so the top few million codes serve the vast majority of traffic. Size Redis for the hot set (say 50–100GB holds tens of millions of hot codes), giving >95% hit rate. On hit: sub-millisecond. On miss: single-partition KV read ~5–10ms, then populate. Add an edge CDN layer for the very hottest links so they never reach the origin.
> **Trap:** Doing a DB read on every redirect. At 116K/sec even a 5ms read needs ~580 concurrent DB connections continuously — the DB becomes the bottleneck and cost balloons. The cache is not optional; it's the design.

Q9. Consistency vs availability: what consistency model does the mapping need?
> **Expected answer:** The mapping is **write-once, read-many, immutable**. Once `aZ3xK9p → long_url` is written it never changes, so you can lean fully toward **availability + eventual consistency** (AP in CAP, Lesson 2) on the read path. A newly created short link tolerating a few hundred ms of replication lag before it's globally resolvable is acceptable. Redirects should never fail due to a consistency stall.
> **Mentor pushback:** Custom aliases break this. Two users racing to claim `/promo` need a **strongly consistent** uniqueness check — a conditional write (`PUT if not exists` / compare-and-set) on that single key. So the system is AP for random codes but needs CP-style CAS specifically for custom-alias creation. Name that split and you've nailed it.

---

### Low-Level Design (Hard)
Q10. Design the key generation so two servers never mint the same short code, without a global lock per request.
> **Problem statement:** At 3.5K writes/sec peak across many stateless app servers, generate unique 7-char Base62 codes with no collisions and no per-request coordination bottleneck.
> **Naive solution:** Hash the long URL with MD5 and take the first 7 chars. Or increment a single shared counter row in the DB per request.
> **Why naive fails at scale:** MD5-truncate → **collisions** (birthday bound: with 62^7 space, collisions start appearing well before you fill it, and each collision needs a read-check-retry, amplifying DB load). A single shared counter → every write serializes on one row, a hard throughput ceiling and a single point of failure.
> **Expected optimal approach:** **Key Generation Service with range allocation.** A central allocator (backed by Zookeeper/etcd or a DB row using atomic increment, Lesson 17 leader election) hands each app server a *range* of, say, 100,000 counter values at a time. The server renders each integer as Base62 locally — zero coordination per key. When its range is exhausted it grabs the next. Optionally **bit-scramble** the counter (Feistel/multiply-by-large-coprime mod 62^7) before Base62 so codes aren't sequentially guessable. Alternative: a **pre-generated key pool** — a background job fills a table of unused codes; app servers claim keys in batches.
> **Pseudo-code or class diagram:**
```
class KeyGenService:
    def get_range():                       # called rarely, atomic
        start = atomic_fetch_and_add(GLOBAL_COUNTER, BATCH=100_000)
        return (start, start + BATCH)

class AppServer:
    range = None
    def next_code():
        if range is exhausted:
            range = KeyGenService.get_range()
        n = range.next()                    # local, no network
        scrambled = feistel_encrypt(n)      # optional anti-enumeration
        return base62_encode(scrambled)     # 7 chars
```
Each server crash "wastes" at most one unused range (100K codes out of 3.5 trillion) — a rounding error.

Q11. Two requests try to claim the same custom alias `/black-friday` at the same instant. Prevent a double-write.
> **Scenario:** User A and User B both `POST` with `custom_alias=black-friday` within microseconds. Both read "not taken," both write.
> **Expected fix:** **Conditional write / compare-and-set** on the KV store: `PUT short_code=black-friday IF NOT EXISTS`. DynamoDB's conditional expression, Cassandra's `IF NOT EXISTS` (Paxos-backed lightweight transaction), or a unique constraint in SQL. Exactly one succeeds; the other gets a 409 Conflict. Never do read-then-write in app code — that's the race.
> **Follow-up:** What if the lock holder dies? There's no long-held lock here — CAS is atomic and instantaneous, so no orphaned-lock problem. Contrast with the range allocator, where a crashed server just abandons its range; the allocator never blocks waiting on it because ranges are handed out, not leased-with-heartbeat.

Q12. A redirect service instance produces click events but Kafka is temporarily unreachable. What happens?
> **Scenario:** Network partition (Lesson 2) between redirect service and the Kafka cluster during peak traffic.
> **Expected handling:** Click events are **best-effort, non-blocking**. The producer buffers to an in-memory ring buffer with a bounded size and a local disk spillover; on Kafka recovery it flushes. If the buffer fills, drop events (analytics tolerate loss; redirects must not). The redirect response is **never** coupled to the produce ack (`acks=0` or fire-and-forget with a background flusher). Use a circuit breaker (Lesson 10) around the producer so a Kafka outage doesn't add latency to redirects. Idempotent consumer downstream dedupes on `event_id` if events replay.

---

### Scaling to 10x / 100x (Hard)
Q13. At 100x traffic (~11.6M reads/sec), where does it break first?
> **Expected answer:** The **cache tier and the origin fan-out**, not the KV store directly. First failure: cache hot-key concentration — a single viral link (one code) getting millions of req/sec overwhelms the one Redis shard holding it (hot partition). Second: the create-path KGS if you didn't do range allocation. The KV store is last to break because point lookups on a hash partition scale near-linearly with shards.
> **Numbers to ground the answer:** 11.6M reads/sec, 95% cache hit → still ~580K reads/sec hitting KV. At ~10K point-reads/sec per node, that's ~60 shards minimum plus replicas. A single hot code at 1M req/sec needs edge/CDN caching and client-side caching (301 for those) — one Redis node caps around 100–200K ops/sec.
> 
Q14. Shard the KV store. What key, and how do you handle hot spots?
> **Expected sharding strategy:** **Hash partitioning on `short_code`** using consistent hashing with virtual nodes (Lesson 19), so adding a shard remaps only ~1/N of keys. Range partitioning on the code would hot-spot because sequential codes (recent, popular links) cluster on one range.
> **Hot spot problem:** Even with hashing, a single viral code is one key on one shard — hashing doesn't split a single key. Detect via per-key request-rate metrics; fix by **promoting hot keys to a replicated read-only tier / CDN edge** (serve from many edge PoPs) and by client/browser caching (301). For hot *shards* (not keys), split the vnode range and rebalance.

Q15. Design the caching layers and invalidation.
> **Expected layered cache design:** **L1** — in-process LRU cache (Lesson 4) on each redirect instance, tiny TTL (seconds), catches repeated hot codes with zero network hop. **L2** — Redis cluster, cache-aside, LRU/LFU eviction, TTL ~24h. **L3/edge** — CDN caches 302/301 responses for the hottest links at PoPs near users.
> **Cache invalidation trap:** Mappings are immutable, so invalidation is *easy* for creates — but **deletions and expiries** are the trap. When a link is deleted or its `expiry_at` passes, a cached entry could still serve a redirect to a dead/malicious target. Fix: short-ish TTLs so stale entries self-heal, plus an explicit cache-purge (DEL by key across L1/L2/edge) on delete. For expiry, store `expiry_at` in the cached value and check it on read, or set the cache TTL to `min(24h, expiry_at - now)`.

Q16. How do you keep this cheap at 180TB and 100x traffic?
> **Expected answer:** (1) **Tiered storage** — recent/active links on fast SSD-backed KV, cold links (most of the 180TB, rarely clicked) on cheaper cold storage; power-law access makes this a big win. (2) **Compress long URLs** and dedupe identical targets. (3) **Don't store analytics inline** — click events go to a columnar/OLAP store (append-only, compressed ~10:1). (4) **Batch** Kafka produces and analytics aggregation windows instead of per-click writes. (5) **Edge caching** offloads the majority of hot reads from origin, cutting egress and compute. (6) TTL-expire and hard-delete expired links to reclaim storage.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Explain the range-allocation KGS failure semantics precisely: with 100K-code ranges and app-server crashes, what's your maximum wasted key space over a year, and at what point does key exhaustion become real? (Answer: waste is bounded by servers×batch across restarts — negligible vs 3.5T; exhaustion only matters if you undersized code length. Show why you'd rather waste keys than coordinate per-request.)

**H2.** Multi-tenancy & multi-region: a European tenant's links must resolve from EU PoPs under GDPR data-residency. How do you partition storage by region while keeping a single global short domain? (Geo-partition the KV by owner region, encode a region hint or route by GeoDNS (Lesson 7/8), replicate hot read-only mappings globally but keep PII/click-analytics in-region.)

**H3.** Zero-downtime deploy of a new code-scrambling algorithm: old codes used sequential Base62, new ones use Feistel-scrambled. How do you migrate without breaking existing links? (You don't re-key existing links — they're immutable and must keep resolving. Change applies only to newly minted codes. Version the generation logic; decode is a pure table lookup regardless of how the code was made, so old and new coexist forever.)

**H4.** What do you instrument? (Cache hit ratio per layer, redirect p50/p99 latency, KGS range-allocation rate & remaining pool, per-shard QPS for hot-shard detection, hot-key top-N, create-path 409 rate for alias contention, Kafka producer buffer depth / drop count. Alert on cache hit ratio dropping below ~90% — signals a cache-miss storm.)

**H5.** You launched with MD5-truncate keys and now have millions of collisions causing retry storms on the create path. Tell the migration story to range-allocated counters. (Freeze new-code generation on the old path, stand up the KGS, dual-write during cutover, backfill nothing (old codes stay valid), monitor create-path retry rate dropping to zero. The lesson: never pick collision-prone generation for an immutable-key space.)

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. **Blocking the redirect on analytics.** The click write must be fully async; a redirect is a read path and should never wait on a write.
2. **Treating key generation as an afterthought** ("just hash it") — collisions and the single-counter bottleneck are the actual hard problem. Range allocation or a key pool is the SDE3 answer.
3. **Forgetting the custom-alias consistency split** — the system is AP for random codes but needs CAS/CP for alias uniqueness. Missing this shows shallow CAP understanding.

**The one insight that makes an answer truly impressive:**
Recognize that a URL shortener is really *two systems with opposite characteristics glued together*: an availability-optimized, immutable, cache-dominated read path, and a coordination-sensitive, uniqueness-enforcing write path. Design them separately and the whole thing falls into place.

**Suggested follow-up reading:**
- Bitly Engineering blog — "Enterprise-scale link management" and their NSQ/queue architecture.
- "System Design Interview Vol 1" (Alex Xu), URL shortener chapter — for the KGS range-allocation pattern.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
