# Real-Time Leaderboard

## 1. Problem Statement & Scope

Design a gaming-scale real-time leaderboard: players submit scores, the system serves global and windowed rankings with low latency.

### Functional Requirements

1. **Submit score** — a game server reports a score for `(user_id, leaderboard_id)` at match end. Semantics are configurable per game: *only-improve* (keep max score) or *accumulate* (increment total points).
2. **Get top-K** — return the top K entries (K ≤ 100 typical, K ≤ 1000 hard cap) with user, score, rank.
3. **Get my rank + neighbors** — exact (or bounded-error) rank for any user, plus the ±2..±5 surrounding entries ("you are #48,213, here are #48,211–#48,215").
4. **Time windows** — daily, weekly, and all-time leaderboards, each independently rankable. Daily resets at 00:00 UTC.
5. **Live-ish updates** — top-K views should reflect new scores within ~1–2 s; a user's own rank should reflect their own write read-your-writes style.

### Non-Functional Requirements

- **Read-heavy**: reads (top-K + rank lookups) outnumber writes ~10:1.
- **Latency**: p99 < 50 ms for top-K, < 100 ms for rank+neighbors.
- **Scale**: 50M DAU per title; must have a story for 500M users all-time.
- **Durability**: a lost score is a support ticket; ranks may be rebuilt, scores may not be lost.
- **Consistency**: eventual consistency on ranks is acceptable (seconds); a user's own submitted score must not be silently dropped.

### Back-of-Envelope

- **Writes**: 50M DAU × 5 games/day = 250M score submissions/day.
  250M / 86,400 s ≈ **2,900 writes/s average**; peak (evening, tournament end) ×5–10 → **~15k–30k writes/s**. A single Redis node does 100k+ ops/s, so write *throughput* is not the problem — memory and rank-query cost are.
- **Reads**: 10× writes → ~29k reads/s avg, ~300k/s peak. But most reads are the *same* top-10/top-100 — highly cacheable.
- **Memory per zset entry**: Redis sorted set = hash table entry + skiplist node. Roughly: member string (say 16-byte user id) + robj/SDS overhead + skiplist node (score double, backward ptr, avg ~1.33 level ptrs) + dict entry ≈ **100–130 bytes/entry**.
  50M entries × ~120 B ≈ **6 GB per window**. Daily + weekly + all-time = ~18 GB, plus replication ×2 → ~36 GB fleet-wide for one title. Fits in a few Redis nodes but **not** comfortably on one, and all-time at 500M users → 60 GB for one zset → *must* shard.
- **Kafka persistence stream**: 250M events/day × ~200 B ≈ 50 GB/day — trivial.

Scope out: matchmaking, anti-cheat internals (we place a hook), social/friend leaderboards (mention as a filter variant at the end).

## 2. Brute-Force / Naive Design

One SQL table:

```sql
CREATE TABLE scores (
  leaderboard_id BIGINT,
  user_id        BIGINT,
  score          BIGINT,
  updated_at     TIMESTAMP,
  PRIMARY KEY (leaderboard_id, user_id)
);
CREATE INDEX idx_lb_score ON scores (leaderboard_id, score DESC);
```

- **Top-K**: `SELECT ... ORDER BY score DESC LIMIT 100` — fine with the index, until pagination: `LIMIT 100 OFFSET 1000000` walks 1M index entries per query, O(offset).
- **My rank**: `SELECT COUNT(*) FROM scores WHERE leaderboard_id=? AND score > :myScore` — an index *range scan* counting every row above you. For a median player that is **O(N/2) ≈ 25M index entries per query**. At even 1k rank-QPS this is 25 *billion* index touches/s. Dead on arrival.
- **Writes**: 15–30k updates/s against a secondary index on a hot, monotonically shuffled column → constant B-tree page splits, buffer pool churn, replication lag. Postgres/MySQL survive this only with heavy partitioning, and it buys you nothing for the rank query.
- **Windowing**: `WHERE updated_at > midnight` breaks the score index entirely (composite index per window, table bloat, vacuum storms on daily deletes).

Numbers verdict: rank query is O(N) per read at ~10³ QPS with N = 5×10⁷ → the design breaks on reads long before writes. We need a data structure where **rank is a first-class O(log N) operation**.

## 3. Evolving the Design

Each step: bottleneck → fix.

1. **O(N) rank in SQL → Redis sorted set.** A zset is a hash map (member → score, O(1) score lookup) plus a skiplist ordered by (score, member). `ZADD`/`ZINCRBY` are O(log N); `ZREVRANK` is O(log N) because skiplist nodes carry *span* counts, so rank is computed while descending levels; `ZREVRANGE 0 99` is O(log N + K). One command replaces the 25M-row count.
2. **Redis is volatile → write-behind durability.** Redis holds *rank truth*; every accepted score is also produced to Kafka, and a sink consumer upserts Postgres/DynamoDB. On Redis loss: restore from RDB/AOF, replay Kafka from the last snapshot offset. DB is *score* truth; ranks are always recomputable.
3. **One 6–60 GB zset = single-node memory + hot key → shard by `hash(user_id) % S`.** Writes spread across S nodes. Top-K becomes scatter-gather: fetch top-K from each shard, k-way merge. Exact rank becomes `1 + Σ_shards ZCOUNT(shard, (myScore, +inf)` — S parallel O(log n_s) ops.
4. **Exact rank for the long tail is S round-trips per lookup at high QPS → approximate rank.** Maintain a score histogram (fixed or exponential buckets, or a t-digest). Rank ≈ cumulative count of buckets above yours + intra-bucket interpolation. #48,213,772 doesn't need the last three digits right; top-1000 stays exact.
5. **Time windows → key-per-window with TTL.** `lb:{game}:daily:2026-08-06`, `lb:{game}:weekly:2026-W32`. Writes go to current daily + weekly + all-time keys. Old windows expire via TTL (kept ~7 d for "yesterday's results"). Pre-create/warm next window before midnight; no delete storms, no timestamp filtering.
6. **Top-10 read spike (300k/s all asking the same thing) → cached top-K snapshot.** A refresher pulls merged top-K every 1–2 s into local memory / a plain Redis string on each API node; CDN/edge cache with `max-age=1` for public boards. Read amplification on the zsets drops to ~1 QPS per window regardless of client load.

End state: sharded Redis zsets per window (rank), Kafka → DB (durability), histogram (approximate tail rank), snapshot cache (top-K reads).

## 4. Protocol & Technology Choices — Why This, Not That

### Rank storage

| Option | Rank op cost | Verdict | When it would win |
|---|---|---|---|
| **Redis sorted set** | O(log N) rank, O(log N + K) top-K, in-memory | **Chosen** — the only mainstream store where rank is a native primitive | — |
| SQL (Postgres/MySQL) | O(N) count per rank; OFFSET pagination O(offset) | Rejected for serving; **kept as durable system of record** | Small boards (<100k rows), or batch-computed ranks refreshed hourly |
| Cassandra counters | No ordering across partitions; counters aren't idempotent, can't index by value | Rejected | Pure "total points" storage with ranks computed offline (Spark) |
| DynamoDB (GSI score as sort key) | Top-K per partition OK; global rank = full scan; write-sharded GSIs get complex | Rejected for ranks | Serverless shop wanting managed durability tier; pair with in-memory ranker |
| Aerospike / KeyDB / Dragonfly | Same sorted-structure idea, better memory/threading | Viable substitutes | >1 TB in-memory footprint or when Redis single-threaded CPU is the wall |

### Sharding topology

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| Single zset, one node | Exact ranks in one op, trivial | 60 GB key, one node's CPU/NIC takes all traffic; failover story is scary | Only for ≤ ~10M entries |
| **Application-level sharded zsets (hash(user_id) % S)** | Spreads memory+write CPU; shard count under our control; scatter-gather is S parallel O(log n) ops | Exact global rank costs S round-trips; resharding is a migration | **Chosen** |
| Redis Cluster, hash-tags to co-locate | Managed slot migration | Hash-tagging all shards of one board to one slot **defeats the purpose** (one key = one slot = one node; Cluster never splits a key); without tags you can't multi-key anyway | Use Cluster as the *fleet*, but shard logic stays in the app |

### Rank fidelity

| Option | Error | Cost per lookup | Verdict |
|---|---|---|---|
| Exact (Σ ZCOUNT across shards) | 0 | S round-trips, S × O(log n) | Top-1000 and "my rank" on demand |
| **Score-histogram buckets** | ≤ bucket width count | O(1) from cached CDF + 1 ZCOUNT | **Default for long tail** |
| t-digest (Redis Stack `TDIGEST.RANK`) | ~0.1–1% relative, best at the extremes | O(1) | Percentile displays ("top 3%"); mention, don't build first |

### Client update delivery

| Option | Verdict |
|---|---|
| Polling (1–2 s, ETag) | **Chosen default** — top-K only changes ~1/s post-cache anyway; infinitely CDN-friendly |
| SSE | Chosen for "watch the board" tournament screens — one-way, auto-reconnect, HTTP-native |
| WebSocket | Rejected here — bidirectional plumbing for a read-only feed; wins only if the game already holds a WS for gameplay (then piggyback) |

### Persistence path

| Option | Verdict |
|---|---|
| **Kafka write-behind (API → Redis + Kafka; consumer → DB)** | **Chosen** — absorbs DB downtime, replayable, feeds analytics/anti-cheat for free |
| Direct dual-write (API → Redis and DB synchronously) | Rejected — no atomicity across the two; partial-failure divergence with no repair log; DB p99 leaks into submit latency |
| CDC from DB → Redis | Inverts truth: DB can't answer "did I improve?" cheaply per write; adds rank staleness. Wins when DB is *already* the mandated system of record |

### Atomic only-if-higher update

| Option | Verdict |
|---|---|
| **`ZADD key GT CH member`** (Redis ≥ 6.2) | **Chosen** — server-side compare-and-set in one command, no round trips |
| Lua script | Chosen when logic exceeds GT (composite tie-break score must be rebuilt from parts, dedup check, multi-key window fan-out) — atomic, one RTT |
| WATCH/MULTI/EXEC | Rejected — optimistic retry loops under contention on a hot member; strictly worse than Lua here |
| Plain ZSCORE-then-ZADD | Rejected — read-modify-write race loses the higher of two concurrent scores |

## 5. High-Level Design (HLD)

```mermaid
flowchart LR
  GS[Game Server] -->|"POST /scores (event_id)"| API[Score API]
  API --> AC[Anti-cheat / validation hook]
  AC -->|reject/quarantine| Q[(Quarantine store)]
  AC -->|accept| W[Write path]
  W -->|"ZADD GT (Lua, per window keys)"| RS[(Redis shards\nlb:daily / weekly / alltime : s0..sN)]
  W -->|produce score-event| K[[Kafka: score-events]]
  K --> SINK[DB sink consumer] --> PG[(Postgres scores\nsystem of record)]
  K --> AN[Analytics / anti-cheat offline]
  RS --> HIST[Histogram maintainer\nbucket counts CDF]
  RS --> SNAP[Top-K refresher\nscatter-gather every 1-2s]
  SNAP --> C[(Snapshot cache\ntopK JSON, per window)]
  RD[Read API] -->|top-K| C
  RD -->|"my rank: ZSCORE + sum ZCOUNT / histogram"| RS
  RD -->|neighbors: ZREVRANGE r-2..r+2| RS
  CL[Clients] -->|"GET (poll/SSE, CDN 1s TTL)"| RD
  CRON[Window manager] -->|pre-create next window keys, set TTL, snapshot+freeze on close| RS
```

### Write path

1. Game server (never the client directly) calls `POST /scores` with an idempotency `event_id`.
2. Validation/anti-cheat hook: schema, score bounds per game mode, z-score outlier check; suspicious scores go to quarantine, not the board.
3. Lua script per shard: dedup `event_id` (SET NX EX 86400), then `ZADD GT` (or composite compare-and-set) into `daily`, `weekly`, `alltime` keys for the user's shard.
4. Produce the event to Kafka (acks=all). Redis-write and Kafka-produce both succeed before 200; on Kafka failure, buffer locally and retry (Redis already has it; DB catches up on replay).
5. Sink consumer upserts `scores` table with `GREATEST(score, excluded.score)` semantics; idempotent by `event_id`.

### Read path

- **Top-K**: served from the snapshot cache (JSON blob refreshed every 1–2 s by scatter-gather merge). CDN-cacheable.
- **My rank (exact)**: `ZSCORE` on the user's shard → parallel `ZCOUNT (score, +inf)` on all shards → rank = 1 + Σ. ~1–2 ms with pipelining.
- **My rank (approx, long tail)**: cached bucket CDF lookup + one `ZCOUNT` in own shard's bucket range.
- **Neighbors**: cheap only within a shard; for global neighbors, use rank r from above, then `ZREVRANGEBYSCORE` a narrow score band on all shards and merge ±2 around the user's score (band chosen from histogram density).

### Data model

Redis (per shard s, per window):
```
lb:{game}:daily:2026-08-06:{s}    ZSET member=user_id score=composite   TTL 7d
lb:{game}:weekly:2026-W32:{s}     ZSET                                  TTL 30d
lb:{game}:alltime:{s}             ZSET                                  no TTL
lbhist:{game}:daily:2026-08-06    HASH bucket_id -> count (or t-digest)
dedup:{event_id}                  STRING NX, TTL 24h
lbsnap:{game}:daily:2026-08-06    STRING frozen top-K JSON (post-close)
```

Postgres:
```sql
scores(leaderboard_id, window_id, user_id, score BIGINT, achieved_at, event_id, PRIMARY KEY(leaderboard_id, window_id, user_id));
leaderboard_snapshots(leaderboard_id, window_id, rank, user_id, score, frozen_at);  -- final results, prizes/audit
```

### API

```
POST /v1/leaderboards/{lbId}/scores        {user_id, score, event_id, achieved_at} -> {accepted, new_best, score}
GET  /v1/leaderboards/{lbId}/top?window=daily&k=100&cursor=...                      -> [{rank,user,score}...]
GET  /v1/leaderboards/{lbId}/users/{uid}/rank?window=daily&exact=false&neighbors=2  -> {rank, approx, score, neighbors[]}
```

## 6. Low-Level Design (LLD)

```mermaid
classDiagram
  class LeaderboardService {
    +submitScore(lbId, userId, score, eventId)
    +topK(lbId, window, k) List~Entry~
    +rankOf(lbId, window, userId, exact) RankResult
  }
  class RankingStore {
    <<interface>>
    +upsertIfHigher(key, member, compositeScore) bool
    +topK(key, k) List~Entry~
    +rank(key, member) long
    +countAbove(key, score) long
    +scoreOf(key, member) double
  }
  class RedisRankingStore
  class InMemoryRankingStore
  class ShardRouter {
    +shardFor(userId) int
    +allShardKeys(windowKey) List~String~
  }
  class RankStrategy {
    <<interface>>
    +rankOf(windowKey, userId) RankResult
  }
  class ExactRankStrategy
  class ApproximateRankStrategy {
    -HistogramIndex hist
  }
  class ScoreCombiner {
    +pack(score, achievedAtSec) long
    +unpackScore(composite) long
  }
  class WindowManager {
    +currentKeys(lbId) List~WindowKey~
    +precreateNext(lbId)
    +freeze(windowKey)
  }
  class WriteBehindPersister {
    +onScoreAccepted(event)
  }
  LeaderboardService --> RankingStore
  LeaderboardService --> ShardRouter
  LeaderboardService --> RankStrategy
  LeaderboardService --> ScoreCombiner
  LeaderboardService --> WindowManager
  RankingStore <|.. RedisRankingStore
  RankingStore <|.. InMemoryRankingStore
  RankStrategy <|.. ExactRankStrategy
  RankStrategy <|.. ApproximateRankStrategy
  LeaderboardService ..> WriteBehindPersister : publishes ScoreAccepted
```

Patterns, named:
- **Repository** — `RankingStore` abstracts the store; `InMemoryRankingStore` (TreeMap + HashMap) makes unit tests hermetic and is the machine-coding fallback.
- **Strategy** — `RankStrategy` swaps exact vs approximate per request (`exact=` flag, or by rank threshold: exact if likely top-10k).
- **Factory** — `WindowManager` mints window keys (`daily:2026-08-06`), owns TTLs, pre-creation, and freeze-on-close.
- **Observer** — `WriteBehindPersister` subscribes to `ScoreAccepted` events (in-process bus → Kafka producer); write path doesn't know about the DB.
- **Adapter/Sharding** — `ShardRouter` maps userId → shard suffix and enumerates shard keys for scatter-gather.

### (a) Composite score packing (tie-break: earlier scorer wins)

```java
// Redis zset scores are IEEE-754 doubles: integers are exact only up to 2^53.
// Layout: [ raw score : high bits ][ inverted time : low 20 bits ]
static final long TS_BITS   = 20;                    // 2^20 = 1,048,576 buckets
static final long MAX_TS    = (1L << TS_BITS) - 1;   // ~12 days at 1s buckets; use epoch-relative
static final long EPOCH     = 1_754_006_400L;        // window/season start, seconds

long pack(long score, long achievedAtSec) {
    long tsBucket = clamp(achievedAtSec - EPOCH, 0, MAX_TS);   // seconds since season start
    long composite = (score << TS_BITS) | (MAX_TS - tsBucket); // earlier => larger low bits
    // Safety: composite must be <= 2^53 for exact double representation.
    // => score <= 2^(53-20) = 2^33 ≈ 8.59e9. Assert at ingest.
    if (score >= (1L << (53 - TS_BITS))) throw new ScoreOverflow();
    return composite;
}
long unpackScore(long composite) { return composite >>> TS_BITS; }
```

Analysis: doubles have a 52-bit mantissa (53 significant bits with the implicit leading 1). Any packed value > 2^53 silently rounds — two different composites collapse to the same double and tie-breaking (or worse, ordering) corrupts. Budget: 53 = score bits + time bits. 20 time bits at 1 s granularity covers a ~12-day season; for all-time boards use coarser buckets (e.g. 64 s buckets → 2^20 ≈ 2.1 years) or 16 bits at hour granularity. If score itself needs > 33 bits, drop to minute buckets or store tie-break rank out-of-band. Same tie ordering *within* equal composites falls back to Redis's lexicographic member order — acceptable, deterministic.

### (b) Sharded top-K scatter-gather (k-way merge with a heap)

```java
List<Entry> topK(String windowKey, int k) {
    // Parallel: per-shard ZREVRANGE 0 k-1 WITHSCORES  -> S sorted lists (each O(log n + k))
    List<Deque<Entry>> lists = shards.parallelFetchTopK(windowKey, k);

    PriorityQueue<Cursor> heap = new PriorityQueue<>(   // max-heap by composite score
        (a, b) -> Double.compare(b.head().score, a.head().score));
    for (Deque<Entry> l : lists) if (!l.isEmpty()) heap.add(new Cursor(l));

    List<Entry> out = new ArrayList<>(k);
    while (out.size() < k && !heap.isEmpty()) {
        Cursor c = heap.poll();
        out.add(c.pop());                                // global next-best
        if (c.hasNext()) heap.add(c);
    }
    return out;                                          // O(k log S) merge; ranks = 1..k
}
```

Correctness: each shard's top-k must be fetched (not top-k/S) because one shard could hold all global top-k. Cost: S × O(log n_s + k) Redis work + O(k log S) merge; with S=16, k=100 this is microseconds of CPU.

### (c) Approximate rank via histogram buckets

```java
// HistogramIndex: bucket_id -> count, exponential bucket width; CDF cached in-process,
// rebuilt every ~5s from the maintained hash (incremented/decremented on score moves).
long approxRank(String windowKey, long userId) {
    long composite = store.scoreOf(shardKey(windowKey, userId), userId);
    long score     = combiner.unpackScore(composite);
    int  b         = hist.bucketOf(score);

    long above = hist.countAboveBucket(b);               // O(1): precomputed suffix sums (CDF)
    // Refine within own bucket, own shard only, then extrapolate by shard fanout:
    long inBucketAboveMe = store.countBetween(            // ZCOUNT (composite, bucketUpperComposite]
        shardKey(windowKey, userId), composite, hist.upperComposite(b));
    return 1 + above + inBucketAboveMe * shardCount;      // error <= ~bucket width; exact for singleton buckets
}
```

Error bound: at most the in-bucket population misestimate, ≈ bucket_count × (1 − 1/S) worst case; with ~2,000 exponential buckets over 50M users, typical bucket ≈ 25k users → tail rank error ≪ 0.1%. Top ranks bypass this via `ExactRankStrategy`. (Redis Stack alternative: `TDIGEST.ADD` on ingest, `TDIGEST.RANK`/`CDF` for percentile — better tails, no per-bucket ZCOUNT.)

### (d) Only-improve semantics: ZADD GT / Lua CAS

```java
// Simple case (raw scores): single command, atomic, returns 1 if changed.
// ZADD lb:daily:2026-08-06:{s} GT CH <score> <userId>
```

```lua
-- Composite case: GT on the packed value would let a LATER equal score win
-- (its inverted-ts low bits are smaller... actually lose; but a later HIGHER raw score
-- must win even though packing math needs rebuilding). Compare on unpacked raw score:
-- KEYS[1]=zset key  ARGV[1]=userId ARGV[2]=newComposite ARGV[3]=tsBits ARGV[4]=eventDedupKey
if redis.call('SET', ARGV[4], 1, 'NX', 'EX', 86400) == false then return -1 end  -- duplicate
local cur = redis.call('ZSCORE', KEYS[1], ARGV[1])
local newRaw = math.floor(tonumber(ARGV[2]) / 2^tonumber(ARGV[3]))
if cur and math.floor(tonumber(cur) / 2^tonumber(ARGV[3])) >= newRaw then return 0 end
redis.call('ZADD', KEYS[1], ARGV[2], ARGV[1])
return 1
```

Note: for pure raw scores without tie-breaking, plain `ZADD GT` suffices and GT also correctly rejects equal scores (keeping the earlier one implicitly). The Lua path exists precisely because composite packing changes what "greater" means; it also folds in dedup atomically. For accumulate-mode games, replace with `ZINCRBY` guarded by dedup only.

## 7. Deep Dives & Failure Modes

**Hot key: the giant zset lives on one node.** Redis Cluster shards *keys across slots*; it never splits a single key, so one 50M-entry zset pins its memory and all its ops to one primary. Mitigations, layered: (1) application-level sharding (S sub-keys) spreads *writes* and memory; (2) read replicas take `ZREVRANGE`/`ZCOUNT` traffic (`READONLY` routing), tolerating replica lag of ms; (3) the top-K snapshot cache absorbs the overwhelming majority of reads so the zsets see refresher traffic only. Know the failure smell: one node at 100% CPU while the rest of the cluster idles.

**Thundering herd at tournament close.** At T=end, millions fetch final standings and their prize rank simultaneously. Fix: *freeze semantics* — WindowManager stops accepting writes for the window (grace period for in-flight matches), computes the full final ranking once (or top-N + per-user ranks materialized to a plain hash/DB `leaderboard_snapshots`), and all "final results" reads hit the immutable snapshot behind CDN. Never serve finals from live zsets.

**Idempotent submits.** Game servers retry on timeout; with `ZINCRBY` (accumulate mode) a retry double-counts, and even with `ZADD GT` a retry can resurrect a score after an admin rollback. Every event carries a UUID `event_id`; the Lua script does `SET dedup:{event_id} NX EX 86400` before mutating. The DB sink dedups by the same id (unique index). Dedup TTL must exceed max retry horizon.

**Redis failover loses recent writes.** Async replication means promotion can drop the last N ms of writes; AOF `everysec` bounds crash loss on a node to ≤ 1 s. Recovery: the Kafka topic is the repair log — a reconciler replays events since the last known-good offset, and `ZADD GT`/dedup makes replay idempotent. This is the payoff of write-behind: Redis is *rebuildable state*, not the only copy. Bound RPO explicitly in the interview: "≤1 s local loss, 0 after Kafka replay."

**Clock issues in tie-breaking.** `achieved_at` comes from game servers; skew makes a later scorer "earlier". Use server-receive time at the Score API (single NTP-disciplined fleet) rather than client/game-server timestamps, bucket to ≥ 1 s so sub-second skew is invisible, and clamp any timestamp outside [window_start, now+ε]. Accept that tie-order within one bucket is arbitrary-but-deterministic.

**Window rollover race at midnight.** A score at 23:59:59.9 processed at 00:00:00.1 must not land in the wrong day. WindowManager pre-creates tomorrow's keys minutes early; during a ±few-second cutover band writes are routed by the event's `achieved_at`, not wall-clock, and the API double-writes to both windows when the timestamp is inside the ambiguity band (GT semantics make the extra write harmless). Yesterday's key stays writable for a short grace window, then freezes.

**Write-behind consumer lag / backpressure.** If the DB sink lags hours, a Redis loss inside that gap is still recoverable (Kafka retains events) but DB-derived features (profiles, prize audit) go stale. Monitor consumer lag as an SLO; sink is horizontally scalable (partition by user_id for per-user ordering); batch upserts (COPY / multi-row) to keep DB write amplification down. Kafka absorbs bursts — that is why it exists in the path; never throttle score *acceptance* on DB health.

**Anti-cheat / outlier quarantine.** Impossible scores (beyond per-mode max, > μ+kσ, impossible rate) are quarantined: persisted to Kafka/DB flagged, *not* written to Redis. Async review can promote them (replay into zsets) or ban. Boards must also support *removal*: `ZREM` across window keys + snapshot invalidation — design the admin path up front, cheaters at #1 are a certainty.

## 8. Trade-off Summary & Interview Soundbites

| Decision | Trade-off accepted |
|---|---|
| Redis zsets as rank truth | Memory cost (~120 B/entry) and an in-memory tier to operate, for O(log N) ranks |
| Write-behind via Kafka (not dual-write) | Seconds of DB staleness for burst absorption, replayability, and no partial-write divergence |
| App-level sharding of one board | Exact global rank costs S ZCOUNTs; resharding is ours to run — in exchange for write/memory scale-out |
| Approximate rank for long tail | Bounded rank error (< bucket width) for O(1) lookups; exact reserved for top ranks |
| Key-per-window + TTL | Multiple concurrent zsets (3× memory) instead of timestamp filtering and delete storms |
| Composite score tie-breaks | Score capped at 2^33 (double 53-bit safe-integer budget) to encode time in low bits |
| 1–2 s snapshot cache for top-K | Deliberate staleness so read QPS decouples entirely from zset load |
| Freeze-and-snapshot at window close | Finals are immutable and cacheable; no live reads at the moment of peak demand |

**Soundbites**
- "Rank is the whole problem: SQL counts O(N) rows to answer it; a skiplist with span counts answers it in O(log N) — that one line justifies Redis."
- "Redis is the source of truth for *ranks*, the DB for *scores*; Kafka is the repair log that lets me treat Redis as rebuildable."
- "Redis Cluster shards keys, not the inside of a key — a 50M-member zset is a hot key no matter how big the cluster is, so I shard it myself and scatter-gather."
- "Exact rank across shards is just 1 + the sum of ZCOUNTs above my score."
- "Nobody at rank 48 million needs the last three digits — histogram CDF for the tail, exact for the top."
- "Tie-breaking is bit-packing: score in the high bits, inverted timestamp in the low bits — and I must stay under 2^53 because zset scores are doubles."
- "Windows are keys, not filters: `lb:daily:2026-08-06` with a TTL; rollover is creating a key, not deleting rows."
- "ZADD GT is compare-and-set on the server — the naive ZSCORE-then-ZADD read-modify-write loses races."

**Common follow-ups**
- *Rank of a user far outside top-K, cheaply?* Histogram CDF: O(1) suffix-sum above the user's bucket + one ZCOUNT to refine; or t-digest for a percentile. Exact fallback: parallel ZCOUNT per shard (~S ops, still ms).
- *500M users?* Same shape, more shards: ~60 GB → S=32–64 shards across a cluster; top-K path unchanged (k-way merge is O(k log S)); push all-time long-tail fully to approximate ranks; consider Dragonfly/Aerospike if memory economics bite.
- *Why not ZINCRBY for high-score games?* ZINCRBY is accumulate semantics; high-score boards need max semantics — an increment on retry or a lower second run corrupts the board. ZINCRBY is right only for total-points modes, and then only behind event-id dedup because increments aren't idempotent.
- *Paging through ranks?* Never OFFSET-style deep `ZREVRANGE` for arbitrary pages from clients; use cursor pagination keyed by `(score, member)` with `ZREVRANGEBYSCORE (lastScore -inf LIMIT 0 pageSize` (exclusive bound), which is O(log N + page) regardless of depth. Cap page depth product-side — nobody browses page 40,000.
- *Friends-only leaderboard?* Don't rank globally then filter; fetch friends' scores (MGET-style ZSCORE pipeline, friend lists are ~hundreds) and sort in the service — O(F log F) beats any global structure.
