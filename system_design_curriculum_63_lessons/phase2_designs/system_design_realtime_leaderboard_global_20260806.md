# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 33 of 63 — Phase 2: System Design Track (Design 5 of 35)
**Topic:** Real-Time Leaderboard at Global Scale (Sharded)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Lesson 32 built a leaderboard that fits in one Redis sorted set: elegant, O(log N), and completely correct — until you have 500 million players and 2 million score updates per second, at which point a single sorted set is both a memory bottleneck (one box can't hold it) and a write bottleneck (one thread can't apply it). This is the problem PUBG, Fortnite, Clash of Clans, and Candy Crush actually solve: a **globally sharded** ranking system where the fundamental tension is that ranking is a *global* operation but data must be *partitioned*. You can shard a key-value store trivially; you cannot trivially shard "give me global rank 1–100" because rank requires knowing everyone. The entire session is about that contradiction — how to partition score data while still answering global top-K and arbitrary-rank queries in single-digit milliseconds.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Heaps & skip lists (taught in Phase 1, Lesson 5):** A Redis sorted set (ZSET) is a skip list keyed by score plus a hashmap from member→score. The skip list gives O(log N) insert, delete, and rank lookup (`ZRANK`), and O(log N + K) range scans (`ZREVRANGE`). Heaps give you O(log N) push/pop for merging top-K streams across shards. Today: each shard is a local ZSET; a merge heap combines shard tops into a global top-K.

**Consistent hashing (taught in Phase 1, Lesson 19):** How we assign 500M players to N shards without a full reshuffle when N changes — hash(playerId) → ring → shard, with virtual nodes to smooth load. Used for the *write* path (which shard owns a player's score). Critically, hashing by player scatters players randomly across shards, which is exactly what breaks naive global ranking — remember that tension.

**Caching / Redis / Memcached (taught in Phase 1, Lesson 24):** Redis is the primary store here, not a cache in front of a DB — the ZSET *is* the source of truth for live ranks, with async persistence to a durable store. TTLs, `EXPIRE`, and eviction policies (`volatile-lru` vs `noeviction`) matter because a leaderboard with unbounded seasons will OOM. L1/L2 cache layering covers the read-heavy top-K.

**Kafka / event-driven (taught in Phase 1, Lesson 21):** Score updates arrive as an event stream. Kafka partitioned by playerId gives per-player ordering (a player's score deltas apply in order) and lets us decouple game servers from the ranking tier, absorb 10x spikes, and replay to rebuild shards. Idempotent consumers via event IDs.

**CAP / PACELC (taught in Phase 1, Lesson 2):** A leaderboard is the textbook **AP** system — you never block a score write because ranking is momentarily stale. We consciously choose eventual consistency for rank display (a rank that's 200ms behind is fine) but need read-your-writes for "your own score/rank." PACELC: even without partitions we trade latency for consistency by serving cached approximate ranks.

**Capacity estimation (taught in Phase 1, Lesson 28):** We will size shards, memory, and QPS from first principles: bytes-per-entry in a ZSET, updates/sec, fan-out. Grounding every design choice in numbers is the SDE3 differentiator.

**Load balancers (taught in Phase 1, Lesson 6):** The query tier is stateless and sits behind an L7 LB; the write tier routes by shard key. Scatter-gather for global queries fans out across shards.

> **Recap box:**
> - ZSET = skip list + hashmap → O(log N) rank, O(log N + K) range.
> - Consistent hashing by playerId spreads writes evenly but destroys score locality (the core problem).
> - Merge K sorted shard-tops with a min-heap of size K → O(N log K) for global top-K.
> - Leaderboard is AP: never block a write; stale ranks are acceptable, read-your-writes is not.
> - Kafka partitioned by playerId = per-player ordering + replayable rebuild.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. In Lesson 32 a single Redis sorted set held the whole leaderboard. Give me two concrete reasons a single ZSET cannot serve a 500M-player global leaderboard, with numbers.
> **What a strong answer covers:** (1) **Memory:** A ZSET entry costs roughly 60–90 bytes in Redis (skip-list node with multiple forward pointers + the dict entry + member string overhead). At ~80 bytes × 500M = ~40 GB *just for the sorted set*, before RDB fork copy-on-write doubling and OS overhead — well past a single instance's comfortable working set and past the point where an RDB fork or failover is fast. (2) **Write throughput:** Redis is single-threaded for command execution; one instance tops out around 100k–200k writes/sec for `ZADD` on a large set (each `ZADD` is O(log N) ≈ 29 comparisons at N=500M). At 2M updates/sec you're 10–20x over a single core. Add (3) blast radius: one box = one failure domain for the entire game.
> **Common weak answer:** "Redis can't hold that much data" — vague, no bytes, and ignores that the *write* ceiling bites before memory does.
> **Mentor follow-up if they answer well:** Given single-threaded execution, would Redis Cluster on one big box with 16 shard-processes fix the write ceiling? (Partly — it's really horizontal sharding within a box; you still need cross-shard merge for global rank, which is the actual hard problem.)

Q2. Estimate the memory and shard count for 500M players, and the write QPS for a game where each of 50M DAU submits a score every 30s on average, spiking 8x during events.
> **What a strong answer covers:** Memory: 500M × ~80 B ≈ 40 GB for the ranking ZSETs; keep shards ≤ ~4–6 GB of ZSET data each for fast failover/fork → ~8–12 shards minimum, round to **16 shards** for headroom and clean hashing. Baseline write QPS: 50M ÷ 30s ≈ **1.67M updates/sec**, spiking to ~**13M/sec**. Per shard at 16 shards: ~104k/sec baseline, ~830k/sec peak — over a single Redis core at peak, so either go to 32–64 shards or absorb spikes in Kafka and apply at a bounded rate. Read QPS (top-K + "my rank") is typically 5–20x writes → design for ~10–30M reads/sec, served mostly from cache.
> **Mentor follow-up:** Peak per-shard write is over one core. What's cheaper — double the shard count, or batch/coalesce writes? (Coalesce: within a 100ms window keep only a player's latest score → collapses 8x spike dramatically since the same players re-submit.)

Q3. Why is a leaderboard almost always an AP system, and what is the one part of it that must feel strongly consistent to the user?
> **What a strong answer covers:** Ranking is inherently eventual — with millions of concurrent writers, any displayed rank is stale the instant it's computed, and users tolerate a board that lags a few hundred ms. So we choose availability: never reject or block a score submission. The exception is **read-your-own-writes**: after I score, *my* new score and *my* rank must reflect it immediately, or the game feels broken. You get this by routing a player's writes and their own-rank reads to the same shard (same owner), so their update is visible locally even while the global merge lags.
> **Red flag answer:** "Make it strongly consistent so ranks are always exact." A globally consistent, always-exact rank across 500M players under 13M writes/sec is neither achievable at single-digit-ms latency nor necessary — it's a misread of the requirements.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the high-level architecture for a globally sharded, real-time leaderboard. Draw it. Show write path, global read path, and the real-time push.
> **Key components expected:** Game servers → Kafka (partitioned by playerId) → score-ingestion consumers → N **shard owners** (each a Redis ZSET, hash-partitioned by playerId) → durable store (Cassandra/DynamoDB) for cold data & rebuild → a **query/aggregator tier** doing scatter-gather + heap merge for global top-K → a materialized **global top-K cache** → a real-time push tier (WebSocket/SSE) for live boards.
> **Architecture diagram (text):**
```
 Game Servers ──emit score events──▶ Kafka (partitioned by playerId, per-player order)
                                          │
                                 ┌────────┴─────────┐  (coalesce within 100ms window)
                                 ▼                  ▼
                        Ingestion Consumers  Ingestion Consumers   ...(scaled by partition)
                                 │  ZADD(idempotent, event-id dedup)
        ┌────────────┬──────────┼───────────┬────────────┐
        ▼            ▼          ▼           ▼            ▼
    Shard 0      Shard 1     Shard 2  ...  Shard 15   (Redis ZSET each; replica per shard)
    (hash(pid)→shard via consistent-hash ring w/ vnodes)
        │            │          │           │            │
        └───────async persist (WAL/CDC)─────┴────────────┘──▶ Cassandra (durable, rebuild source)
                                 ▲
   Query/Aggregator Tier ───scatter-gather ZREVRANGE top-K'──┘
        │  (min-heap merge of shard tops → global top-K)
        ├──▶ Global Top-K Cache (Redis, TTL ~1s, versioned)
        │
   Clients ──"my rank" (route to owning shard)──▶ owning Shard
   Clients ──"top 100" ──▶ Global Top-K Cache ──miss──▶ Aggregator
        ▲
        └── Real-time push tier (WebSocket/SSE) ◀── top-K change stream / delta events
```
> **What separates SDE2 from SDE3 here:** The SDE2 draws the shards. The SDE3 immediately flags that **top-K is scatter-gather but arbitrary-rank ("what's rank #4,238,001?") is not solvable by top-K merge at all** — it needs a rank-approximation structure (score histogram / bucketed counts per shard). They also separate "my rank" (route to one shard, cheap) from "global top 100" (fan out to all, cache hard) from "arbitrary rank" (histogram estimate) as three different query classes with different SLAs.

Q5. Trace a single score submission from the game client to it being visible on the global top-100 board.
> **Expected trace:**
> 1. Game server validates the score (anti-cheat, server-authoritative — never trust the client score) and emits `{eventId, playerId, gameId, score, ts}` to Kafka, keyed by `playerId` → lands in a fixed partition preserving per-player order.
> 2. Ingestion consumer reads the batch, **coalesces** multiple events for the same player in the window to the latest, and computes the shard via `hash(playerId)` on the consistent-hash ring.
> 3. Consumer checks idempotency (`eventId` seen? via a short-TTL set or a per-player last-applied version) and issues `ZADD shard:{i} score playerId` (or a Lua script doing "max of current/new" if scores only increase). Commits Kafka offset only after the ZADD acks → at-least-once + idempotent = effectively-once.
> 4. The shard asynchronously ships the change to Cassandra (CDC/WAL) for durability and to the top-K delta pipeline.
> 5. If the new score cracks that shard's local top-K', it's pushed to the aggregator, which merges it into the global top-K via the min-heap; if it displaces the current global #100, the global top-K cache is updated (new version) and a delta is pushed over WebSocket to subscribed clients.
> **Tricky part:** Candidates get vague at "how does a local top-K change become a global top-K change without re-fanning-out to all shards every time?" The answer: each shard only forwards changes to its *local top-K'* (K' slightly > K, e.g., 150 for global 100), and the aggregator keeps a running merged heap; only local-top-K' movements can ever affect the global top-K, so 99.99% of writes (mid-pack players) never touch the global path at all.

Q6. Design the API. Cover the three query classes and the write path.
> **Expected API design:**
> - `POST /v1/scores` — body `{playerId, gameId, score, eventId}`. Idempotent on `eventId`. Returns 202 (accepted to Kafka), not the new rank (rank is async). Fire-and-forget from the client's view.
> - `GET /v1/leaderboard/{gameId}/top?limit=100&window=daily` — global top-K, served from cache, `ETag`/version header for delta polling.
> - `GET /v1/leaderboard/{gameId}/players/{playerId}/rank` — "my rank," routed to owning shard; returns `{score, exactRankWithinShard, approxGlobalRank, percentile}`.
> - `GET /v1/leaderboard/{gameId}/around/{playerId}?range=5` — the 5-above/5-below "relative" board; served from owning shard for the neighbor scores but global rank is approximate.
> - `WS /v1/leaderboard/{gameId}/stream?window=daily` — subscribe to top-K deltas.
> **What to push on:** **Versioning** — put `/v1/` in the path; leaderboards outlive API changes (seasons span years). **Idempotency** — `eventId` is mandatory on writes; without it a Kafka redelivery double-counts. **Pagination** — top-K is bounded (nobody paginates to rank 400M), so offer cursor-based "around me" instead of `OFFSET` (deep `ZRANGE` offsets are O(offset)). **Why 202 not the rank:** returning an exact global rank synchronously would force a scatter-gather on every write — the classic trap.

---

### Data Modeling (Medium–Hard)
Q7. Design the data model: the live ranking store, the durable store, and whatever structure answers arbitrary-rank queries.
> **Expected schema:**
```
# LIVE (Redis, per shard i, per game+window)
ZSET  lb:{gameId}:{window}:shard:{i}   member=playerId   score=score
HASH  player:{playerId}:{gameId}       fields: bestScore, lastEventId, lastTs   # dedup + read-your-writes
STRING lb:{gameId}:{window}:topk       serialized global top-K + version        # aggregator cache

# ARBITRARY-RANK APPROXIMATION (per shard, per window)
# score histogram: fixed buckets, count of players per bucket
ZSET/HASH  hist:{gameId}:{window}:shard:{i}   bucket=floor(score/B)  value=count
# global rank(scoreX) ≈ Σ_shards (players with score > scoreX)  ← summed from histograms

# DURABLE (Cassandra) — source of truth for rebuild + history
CREATE TABLE scores (
  game_id      text,
  window       text,          -- daily / weekly / all_time / season_2026
  player_id    text,
  score        bigint,
  updated_at   timestamp,
  PRIMARY KEY ((game_id, window), player_id)   -- partition per game+window
);
CREATE TABLE score_events (   -- append-only audit / replay
  game_id text, player_id text, event_id timeuuid, score bigint, ts timestamp,
  PRIMARY KEY ((game_id, player_id), event_id)
);
```
> **Index choices and why:** The ZSET *is* the index (skip list sorted by score). In Cassandra, partition by `(game_id, window)` so a rebuild reads one game-window's players with clustering on `player_id` for point upserts. The **histogram** is the key insight: it trades exactness for O(#buckets) global-rank estimation instead of O(N) counting.
> **Partitioning key and why:** Partition the live ranking by `hash(playerId)` (consistent hashing, vnodes) — spreads write load evenly and gives a stable home for "my rank" / read-your-writes. Do *not* partition by score range (see Q14 for why score-range sharding creates a moving hot spot at the top).

Q8. A player opens the app and wants to see "you are rank 4,238,117 of 500M, top 0.8%." How do you answer arbitrary global rank efficiently? Naive answer?
> **Expected answer:** You cannot `ZRANK` globally — the player is scattered on one shard and their global rank depends on all other shards. Use the **per-shard score histogram**: to get global rank of score X, query each shard's histogram for "count of players with score > X" and sum, then add the exact within-bucket offset from the owning shard. Buckets of width B (say B=10 for scores up to millions) → each shard answers in O(#buckets_above_X) ≈ O(log range) with a cumulative-count structure, and the fan-out is N cheap counts, not N range scans. Result is approximate to ±(bucket width) but that's fine for "top 0.8%." Cache the histogram aggregates and refresh every 1–5s.
> **Trap:** `ZCARD`/`ZRANK` on every shard then summing — `ZRANK` gives *within-shard* rank, and summing within-shard ranks is meaningless (rank 5 on shard A and rank 5 on shard B don't compose). The naive candidate sums ranks; the correct approach sums *counts above a score threshold*, which is additive across shards.

Q9. Where do you accept eventual consistency and where must you not? Give a scenario that breaks naive eventual consistency and how you handle it.
> **Expected answer:** Eventual is fine for: global top-K display (1–5s staleness), arbitrary-rank percentile (approximate anyway), and other players' ranks. Must be immediate: a player's own score and own rank after they submit (read-your-writes). Achieve it by routing the player's write and their own-rank read to the same shard owner and serving own-score from `player:{id}` hash updated synchronously with the ZADD (Lua atomic).
> **Mentor pushback:** Scenario — two events for the same player arrive out of order across a Kafka rebalance (consumer A had partition, dies, consumer B takes over and reprocesses): a lower old score overwrites a higher new one. Fix: make ZADD conditional — Lua script `if new.version > stored.version then ZADD`, tracking a monotonic per-player version/timestamp in `player:{id}` hash. Since Kafka guarantees order *within a partition* and we key by playerId, in-order is the norm; the version guard handles the redelivery/rebalance edge. If scores are monotonic (only increase), use `ZADD GT` (greater-than) which Redis supports natively — even simpler and idempotent.

---

### Low-Level Design (Hard)
Q10. The hardest sub-problem: global top-100 across 16 shards, refreshed in real time, at single-digit-ms read latency. Design it.
> **Problem statement:** Continuously maintain the global top-100 for a game-window while each shard receives up to ~830k writes/sec, and serve `GET top?limit=100` in <5ms at 30M reads/sec.
> **Naive solution:** On each read, scatter `ZREVRANGE 0 99` to all 16 shards and merge — 16 network round-trips + a merge, per read, 30M times/sec.
> **Why naive fails at scale:** 30M reads × 16 fan-out = 480M internal RPCs/sec, and tail latency is bounded by the *slowest* of 16 shards per request (the fan-out tail-latency amplification problem — p99 of the max-of-16 is far worse than p99 of one). It also hammers shards with read traffic competing against writes on a single-threaded Redis.
> **Expected optimal approach:** **Push-based materialization, not pull.** (1) Each shard maintains its local top-K' (K'=150) and emits a delta only when its local top-K' changes. (2) A stateful aggregator holds a min-heap of size 100 merged across shard contributions; applying a delta is O(log 100). (3) The materialized global top-100 lives in a cache (`lb:...:topk`, versioned). (4) Reads hit the cache — O(1), no fan-out. (5) Only cold-start / cache-loss triggers a one-time scatter-gather rebuild. This converts per-read fan-out into per-*meaningful-write* maintenance; since only top-K' movements matter, the maintenance rate is tiny relative to total writes.
> **Pseudo-code or class diagram:**
```
# Per-shard (runs where the ZSET lives)
on_score_apply(playerId, newScore):
    ZADD GT shard_zset newScore playerId
    localTopK = ZREVRANGE shard_zset 0 149 WITHSCORES   # K'=150
    if localTopK changed at/above position affecting merge:
        emit_delta(shardId, localTopK_snapshot_or_diff)

# Aggregator (stateful, single owner per game-window, HA via leader election - Lesson 17)
state: shardTops[shardId] -> sorted list; globalHeap: min-heap size 100
on_delta(shardId, top):
    shardTops[shardId] = top
    rebuild_or_patch:
        candidates = merge_all(shardTops)     # 16 * 150 = 2400 entries, tiny
        globalTop100 = heap_select_top(candidates, 100)   # O(2400 log 100)
    if globalTop100 != cached:
        version += 1
        SET lb:...:topk = (version, globalTop100)
        publish_delta_to_websocket_tier(diff)

# Read path
GET top(limit=100): return cache.get(lb:...:topk)   # O(1)
```
> Note: 16×150 = 2,400 candidate entries is trivially small — the merge is microseconds. The magic is that mid-pack writes never reach here.

Q11. Concurrency: two events for the same player, and the "score can only go up" invariant. Where's the race and how do you fix it?
> **Scenario:** Player finishes two matches near-simultaneously; game servers emit score=5000 then score=5200. Under at-least-once Kafka with a consumer rebalance, these can be applied out of order or redelivered, leaving the ZSET at 5000 (stale) even though 5200 is correct.
> **Expected fix:** Since we key Kafka by playerId, both land on the same partition → same consumer → in-order under normal operation. For the rebalance/redelivery edge, make the apply **conditional and atomic**: use `ZADD GT` (only updates if the new score is greater) for monotonic-increasing scores, or a Lua script comparing a monotonic version when scores can decrease. Both make the operation idempotent and commutative w.r.t. redelivery. Commit the Kafka offset only after the ZADD acks (at-least-once + idempotent apply = effectively-once).
> **Follow-up (what if the aggregator/lock holder dies?):** The aggregator is a single stateful owner per game-window — elect it via ZooKeeper/etcd (Lesson 17). On death, a standby wins leader election and **rebuilds state from scratch** by one scatter-gather of all shards' top-K' (2,400 entries — sub-second), then resumes consuming deltas. Shard writes never block on the aggregator (writes go to shards directly), so an aggregator outage degrades only top-K freshness, not the write path — exactly the AP posture we chose.

Q12. Failure deep-dive: a shard's Redis primary crashes. Walk through what breaks and how you recover without losing live ranks.
> **Scenario:** Shard 7's Redis primary dies mid-traffic. It owns ~31M players' live scores.
> **Expected handling:** (1) **Replica failover:** each shard runs primary + ≥1 replica with Redis Sentinel/Cluster; failover in seconds. Async replication means the replica may lag by a few writes — acceptable (AP), and Kafka can replay the tail. (2) **Rebuild from Kafka + Cassandra:** if both primary and replica are lost, spin a new shard instance, bulk-load current scores from Cassandra (`scores` table partition), then replay Kafka from the last committed offset for that shard's players to catch up the tail — this is why we keep the durable store and commit offsets carefully. (3) **During the gap:** writes for shard 7 buffer in Kafka (no data loss — Kafka is the durability boundary); "my rank" for those players returns `503`/stale-with-flag rather than wrong data; global top-K keeps serving from cache minus shard 7's contribution (slightly stale, flagged). (4) **Idempotent replay** (ZADD GT / version guard) makes re-consuming the Kafka tail safe. DLQ any poison events. The key property: **Kafka is the source of truth for durability, Redis is the source of truth for latency** — losing Redis is a rebuild, not a data loss.

---

### Scaling to 10x / 100x (Hard)
Q13. At 10x (5B players, 130M writes/sec peak), where does this break first?
> **Expected answer:** The **write coalescing + Kafka ingestion tier** and **per-shard Redis write ceiling** break first, not memory. At 130M/sec across even 64 shards that's ~2M writes/sec/shard — 10x over a single Redis core. The fix is (a) aggressive coalescing (same player re-submits; a 200ms window collapses spikes massively since active players dominate), (b) more shards (256+), and (c) sharded Kafka with enough partitions (partition count ≥ consumer parallelism; you need thousands of partitions). Secondary breakage: the **real-time push tier** — 5B players can't all subscribe to live top-K; connection count, not rank compute, becomes the wall.
> **Numbers to ground the answer:** 5B × 80 B ≈ 400 GB ZSET data → 256 shards ≈ 1.6 GB each (fine for fork/failover). Write: 130M/sec ÷ 256 = ~508k/sec/shard, still over one core at peak → coalescing is mandatory, not optional. Reads: ~1B/sec top-K, but served from a cache with CDN/edge fan-out, so effectively free at origin.

Q14. Sharding strategy: you chose hash-by-player. Someone proposes score-range sharding (shard 0 = scores 0–1k, shard 1 = 1k–2k…) so top-K is just "read shard N." Argue it, then find the hot spot.
> **Expected sharding strategy:** **Hash-by-player** is correct for the write path: uniform load, stable home per player (read-your-writes), no rebalancing storm. Score-range sharding is seductive because global top-K becomes "read the highest-range shard" (no fan-out) — but it has two fatal flaws.
> **Hot spot problem:** (1) **Score is not uniformly distributed** — most players cluster at low/mid scores (a power-law/normal hump), so the mid-range shards are enormous and the top-range shard tiny; load is wildly skewed. (2) **A player's score changes → the player must migrate shards** on every score cross into a new range, so a single active player generates cross-shard moves (delete from shard A, insert to shard B) — turning one write into a distributed transaction, at millions/sec. That's catastrophic. **Detect** skew via per-shard key-count and CPU metrics; **fix** by staying with hash-by-player and solving top-K via the push-merge from Q10. If you *must* have range locality for top-K only, keep a *separate* small "elite" ZSET (top ~10k globally) maintained by the aggregator — range-like benefits without migrating 5B mid-pack players.

Q15. Caching strategy for the three query classes. What/where/how to invalidate.
> **Expected layered cache design:**
> - **Global top-K:** materialized in Redis (`lb:...:topk`, version-stamped), pushed (not pulled). Served at edge/CDN with ~1s TTL + version ETag for the *public* board; internal clients use the WebSocket delta stream so they never poll. Invalidation is *replacement*: aggregator writes a new version; readers with a stale ETag get the new blob. No hard invalidation race because it's monotonic-version.
> - **Arbitrary rank / percentile:** cache the per-shard histogram aggregates for 1–5s (approximate anyway); recompute on TTL. This is the cheapest cache — a few KB per game-window.
> - **My rank / around-me:** *don't* cache aggressively (read-your-writes); serve from the owning shard directly, it's already O(log N). Optionally L1 in the API node for 200ms.
> **Cache invalidation trap:** The hard one is the top-K under rapid churn near position #100 — the boundary player flips in/out repeatedly, thrashing the cached blob and the WebSocket deltas. Fix with **hysteresis/K' band**: only promote into top-100 when you exceed #100's score by a margin, and only demote below a lower threshold, damping the flapping. Also, never invalidate by TTL-expiry-then-recompute (thundering herd of scatter-gathers on expiry) — use the push/version model so there's always a warm value.

Q16. Cost/efficiency at scale. How do you not spend a fortune on RAM and network?
> **Expected answer:** (1) **Tiered storage:** only *active* players (played in the last N days) live in Redis; inactive players age out to Cassandra with `EXPIRE`, and are lazily rehydrated on next play. A game with 5B installs but 100M actives keeps Redis at ~8 GB, not 400 GB — 50x saving. (2) **Coalesce writes** (Q13) — the single biggest CPU/network saver; collapsing redundant updates cuts the write tier by an order of magnitude during events. (3) **Bounded windows:** daily/weekly leaderboards get a hard `EXPIRE` at rollover; only `all_time`/`season` persists — don't keep every window forever in RAM. (4) **Delta compression on the push tier:** send only rank diffs over WebSocket, not full boards; batch deltas at ~4–10 Hz (humans don't perceive faster). (5) **Approximate where allowed:** histograms for rank instead of exact counting saves the entire arbitrary-rank fan-out. (6) Serve public top-K from CDN edge — origin does O(1) work regardless of viewer count.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Your per-shard histogram gives approximate global rank. Quantify the error and make it tunable. (Answer direction: error is bounded by bucket width B — within a bucket you can't distinguish. Use *non-uniform* buckets: fine buckets near the top (where players care about exact rank) and coarse buckets in the long tail (where ±1000 is invisible). Or use a t-digest / CKMS quantile sketch per shard for relative-error guarantees — e.g., 1% relative error at any quantile with a few KB. Merging t-digests across shards is associative, so scatter-gather sum works.)

**H2.** Multi-region: players in NA, EU, APAC all on one global board. Where do writes and the aggregator live, and how do you avoid a cross-Pacific write on every score? (Answer direction: shard *within* region for write locality — a player writes to their home-region shard. Regional aggregators compute regional top-K; a global aggregator merges the K'-tops of the 3 regional aggregators — a 3-way merge of 150-entry lists, cheap and infrequent-cross-region. Own-rank stays regional (read-your-writes local). Accept that global top-K crosses regions but only for ~450 entries, not per-write. GDPR: EU player data physically stays in EU shards; only anonymized scores cross for the global board.)

**H3.** Deploy a new sharding scheme (16→256 shards) with zero downtime and no wrong ranks during migration. (Answer direction: consistent hashing with vnodes limits the fraction of keys that move to ~(1 - 16/256). Dual-write to old and new topology during migration, backfill new shards from Cassandra, verify counts match, then flip reads shard-by-shard behind a version flag. Because apply is idempotent (ZADD GT), replaying into new shards is safe. Never a stop-the-world reshuffle.)

**H4.** What do you instrument? Give the four metrics whose alerts would page you. (Answer direction: (1) **write-apply lag** = now − event.ts at apply, p99 — the freshness SLO; (2) **Kafka consumer lag** per partition — the leading indicator of ingestion falling behind; (3) **aggregator delta-merge latency** and staleness of `topk` version — top-K freshness; (4) **per-shard Redis CPU / cmd latency** — the single-core write ceiling. Plus anti-cheat: sudden score deltas beyond physical possibility → flag. Trace a score end-to-end with the eventId as the trace/span key.)

**H5.** 'Undo a bad decision': you shipped score-range sharding (Q14) and it's melting under player migrations. Migrate to hash-by-player live. (Answer direction: stand up the new hash-partitioned topology in parallel, dual-write both from the ingestion tier, backfill hash shards from Cassandra, run a shadow read comparing old vs new top-K for a week, then cut reads over behind a flag and decommission range shards. The append-only `score_events` table lets you rebuild the new topology from scratch and prove equivalence. Idempotent applies make the dual-write safe. This is a data-migration, not a rewrite — the durable store is what makes it survivable.)

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. **Summing per-shard ranks** to get global rank — ranks aren't additive; only *counts above a threshold* are. This single error signals someone who memorized "use Redis ZSET" without understanding what rank means under partitioning.
2. **Pull-based scatter-gather on every read** for global top-K, ignoring tail-latency amplification (p99 of max-of-N shards) and the fact that only top-K' movements matter. The right answer is push/materialize.
3. **Score-range sharding** for "easy top-K," missing that it makes every score change a cross-shard migration and creates load skew from non-uniform score distributions.

**The one insight that makes an answer truly impressive:**
Recognizing that a leaderboard is really *three different systems* wearing one name — (a) a partitioned KV write path (hash-by-player, trivially shardable), (b) a global top-K aggregation (push-merge of tiny K'-tops, not a fan-out), and (c) an arbitrary-rank estimator (mergeable quantile sketches / histograms, approximate by design). Candidates who name these three query classes and give each its own data structure and SLA are operating at SDE3+. The unifying idea: *partition the writes, materialize the reads, and approximate the ranks.*

**Suggested follow-up reading:**
- Redis docs on Sorted Sets internals (skip list + ziplist/listpack thresholds) and `ZADD GT/LT/NX` semantics.
- "Computing Extremely Accurate Quantiles Using t-Digests" (Dunning & Ertl) — the mergeable sketch for arbitrary-rank at scale; and the CKMS quantile paper.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate — especially Lessons 5 (skip lists/heaps), 19 (consistent hashing), and 21 (Kafka).
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to a globally sharded real-time leaderboard. Expected answers include real algorithms (heap-merge top-K, t-digest, ZADD GT), data structures (skip list, histogram, min-heap), specific failure modes (scatter tail amplification, score-range migration storm), and real numbers. Cross-referenced Phase 1 lessons throughout.
