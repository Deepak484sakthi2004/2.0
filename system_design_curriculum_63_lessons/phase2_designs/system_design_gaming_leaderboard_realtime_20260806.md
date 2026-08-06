# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 32 of 63 — Phase 2: System Design Track (Design 4 of 35)
**Topic:** Gaming Leaderboard with Real-Time Updates (Redis Sorted Sets)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
A leaderboard is the canonical "why sorted sets exist" problem: you must answer *"what is player X's rank?"* and *"show me ranks 1–100"* in constant/log time while millions of score updates stream in and every viewer expects the board to move in real time. The naive `SELECT COUNT(*) WHERE score > mine` is O(n) per query and dies instantly. Redis sorted sets (skip list + hashmap) turn rank queries into O(log N), and the real-time push layer (WebSockets) turns it into a live experience. This session focuses on a **single-game / bounded-scale** leaderboard done right — the sorted-set internals, the real-time fan-out, and the write path. Lesson 33 will shatter it into global shards.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Skip lists & heaps (taught in Phase 1, Lesson 5):** A Redis sorted set (ZSET) is a **skip list + hashmap**. The skip list keeps members ordered by score for O(log N) insert/delete/rank-range; the hashmap gives O(1) member→score lookup. This is the beating heart of today's design — know why it's a skip list and not a balanced BST (simpler, lock-friendly, probabilistic O(log N)).

**Caches / Redis (taught in Phase 1, Lesson 24):** ZADD/ZRANGE/ZREVRANK/ZSCORE, in-memory speed, persistence (AOF/RDB), single-threaded atomicity. The leaderboard lives in Redis.

**Databases / SQL vs NoSQL (taught in Phase 1, Lesson 23):** The durable source of truth (scores, match history) sits in a DB; Redis is the fast rank index. B-tree indexes vs the in-memory skip list.

**Kafka / event-driven (taught in Phase 1, Lesson 21):** Score events flow through a queue so the write path is decoupled, ordered, and replayable.

**Networking / WebSockets (taught in Phase 1, Lesson 7):** Real-time updates need a persistent bidirectional channel (WebSocket) or SSE, not polling. OSI/transport recap matters for connection scaling.

**Rate limiting (taught in Phase 1, Lesson 11):** Score-submission endpoint must be limited to stop cheating/score-flooding.

> **Recap box:** ZSET = skip list + hashmap → O(log N) rank · Redis is the index, DB is truth · Kafka decouples the write path · WebSockets for live push, not polling · rate-limit submissions.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. You learned skip lists in Lesson 5. Explain exactly why a Redis sorted set gives O(log N) rank queries, and what two structures back it.
> **What a strong answer covers:** A ZSET is a **skip list ordered by score** plus a **hashmap (member → score)**. The skip list has multiple express lanes (levels); search/insert/delete traverse ~log N levels → O(log N). Redis augments each skip-list node with a **span** (number of nodes it skips) so `ZRANK`/`ZREVRANK` compute rank by summing spans along the search path — also O(log N), no full scan. The hashmap makes `ZSCORE` O(1). Together: O(log N) rank/range, O(1) score lookup.
> **Common weak answer:** "Redis just sorts them" or claiming O(1) rank (it's O(log N)).
> **Mentor follow-up if they answer well:** Two players have the same score. How does the skip list break the tie deterministically, and why does that matter for stable ranking?

Q2. 10M players, 100 score updates/sec per active player during peak with 50K concurrent players. Estimate write QPS and the memory for the ZSET.
> **What a strong answer covers:** Writes: 50K concurrent × modest submit rate — say each active player submits ~1 score/sec → **~50K ZADD/sec** peak (if 100/sec each, that's 5M/sec — flag that as needing batching/aggregation, not raw). Memory: a ZSET entry ≈ member (player_id ~ 20B) + score (8B double) + skip-list node overhead (~30–60B) ≈ ~100B/entry. 10M players → **~1GB** for the ZSET. Trivial for one Redis node — memory is not the constraint here; **update throughput and rank-query fan-out** are.
> **Mentor follow-up:** If it's 5M ZADD/sec you can't do raw — how do you aggregate? (Client/edge batching, take max/last per player per window.)

Q3. Why not just query the SQL database with `ORDER BY score DESC LIMIT 100` and `COUNT(*) WHERE score > mine`?
> **What a strong answer covers:** `ORDER BY score DESC LIMIT 100` with a B-tree index on score is fine for the *top-K* (index scan, cheap). But **rank of an arbitrary player** = `COUNT(*) WHERE score > mine` is **O(n)** — it scans/counts a huge portion of the index on every request, and at millions of players and thousands of rank queries/sec it melts the DB. Redis ZSET's span-augmented skip list gives that rank in O(log N). Also, scores update constantly, so the "index" is a moving target — Redis handles in-place reordering cheaply.
> **Red flag answer:** "Add an index, it'll be fast" — an index speeds top-K but not arbitrary-rank counting.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the leaderboard: score ingestion, Redis ZSET, rank queries, and real-time push. Draw it.
> **Key components expected:** Game clients → API (submit score, rate-limited) → Kafka (score events) → scoring workers → Redis ZSET (rank index) + durable DB (source of truth) → Read API (ZREVRANK/ZREVRANGE) → WebSocket/SSE fan-out service for live updates.
> **Architecture diagram (text):**
```
  Game clients
     │ submit score (rate-limited, Lesson 11)
     ▼
  ┌────────────┐    ┌──────────┐   ┌────────────────┐
  │ Submit API │──► │  Kafka   │──►│ Scoring worker │
  └────────────┘    │ (ordered │   │  validate/agg  │
     ▲              │ per-player) │  └───────┬────────┘
     │ WS/SSE               └──────┘         │ ZADD
     │                                       ▼
  ┌──────────────┐   ZREVRANK/ZREVRANGE  ┌──────────────┐
  │ Read/Rank API│◄─────────────────────►│ Redis ZSET   │
  └──────┬───────┘                       │ (skiplist)   │
         │                               └──────┬───────┘
         ▼ push deltas                          │ persist
  ┌────────────────────┐                 ┌──────────────┐
  │ WebSocket fan-out  │                 │ Durable DB   │
  │ (pub/sub, N conns) │                 │ (scores,truth)│
  └────────────────────┘                 └──────────────┘
```
> **What separates SDE2 from SDE3 here:** SDE2 wires client→Redis→client. SDE3 puts **Kafka between submit and Redis** (decouple, order per-player, absorb bursts, enable replay/rebuild of the ZSET), makes Redis a **rebuildable index over a durable DB** (never the only copy of truth), and separates the **WebSocket fan-out tier** so connection scaling is independent of rank compute. SDE3 also notes you push **deltas** (only changed ranks near a viewer), not the whole board.

Q5. Trace a score submission all the way to other players' screens updating.
> **Expected trace:** (1) Client `POST /score {player, points}` — rate-limited, authenticated (JWT, Lesson 12). (2) Event to Kafka, keyed by `player_id` so a player's events stay ordered on one partition. (3) Scoring worker consumes, validates (anti-cheat bounds), updates durable DB, then `ZADD leaderboard <newscore> player`. (4) Redis reorders the skip list in O(log N). (5) Worker publishes a "rank-changed" event to a pub/sub channel. (6) WebSocket fan-out service, holding open connections, computes affected viewers (those watching a window this player crossed) and pushes a delta. (7) Client re-renders that row.
> **Tricky part:** You can't recompute and push the *entire* board to everyone on every update (that's O(viewers × board) fan-out). The trace must show **who actually needs the update** — only viewers whose visible window changed — and that you push a small delta, not a full snapshot.

Q6. Design the API for rank queries and updates.
> **Expected API design:**
> - `POST /v1/leaderboard/{board}/scores` `{player_id, score, idempotency_key}` → 202.
> - `GET /v1/leaderboard/{board}/top?limit=100` → `ZREVRANGE ... WITHSCORES`.
> - `GET /v1/leaderboard/{board}/rank/{player}` → `{rank: ZREVRANK+1, score: ZSCORE}`.
> - `GET /v1/leaderboard/{board}/around/{player}?window=5` → the player's neighbors (rank−5 .. rank+5) via `ZREVRANK` then `ZREVRANGE`.
> - `WS /v1/leaderboard/{board}/subscribe` → live deltas.
> **What to push on:** **Idempotency** on submit (a retried score must not double-apply — dedup on idempotency_key or use last-write / max semantics). **Pagination** for deep browsing via rank offset (cursor by rank). **The "around me" query** is the interesting one — it's `ZREVRANK` (find my rank) then `ZREVRANGE start stop` (fetch the window), two O(log N) ops.

---

### Data Modeling (Medium–Hard)
Q7. Model the Redis structures and the durable schema.
> **Expected schema:**
```
# Redis (fast index)
ZADD  lb:{board}   <score>  <player_id>          # sorted set, skiplist
HSET  player:{id}  name ...  avatar ...          # metadata for display
# Optional per-tie ordering: encode (score, -timestamp) into the double
#   or use lexicographic member to break ties by earliest-achieved.

-- Durable DB (source of truth; ZSET is rebuildable from this)
CREATE TABLE scores (
  board_id   VARCHAR,
  player_id  BIGINT,
  score      BIGINT,
  updated_at TIMESTAMP,
  PRIMARY KEY (board_id, player_id)
);
CREATE INDEX idx_board_score ON scores(board_id, score DESC);
```
> **Index choices and why:** In Redis the skip list *is* the index (score-ordered) + hashmap (member→score). In the DB, a composite `(board_id, score DESC)` B-tree supports top-K reads and full ZSET rebuild; PK `(board_id, player_id)` for point upserts.
> **Partitioning key and why:** Partition by `board_id` (one ZSET per board/game-mode/season). A single board fits comfortably in one Redis instance at this scale, so `board_id` is the natural unit — it also isolates blast radius and lets you shard *by board* later (Lesson 33 shards *within* a board).

Q8. Efficiently answer "give me the 10 players just above and below me."
> **Expected answer:** Two operations: `rank = ZREVRANK lb player` (O(log N)), then `ZREVRANGE lb (rank-10) (rank+10) WITHSCORES` (O(log N + window)). Cache the result briefly (200–500ms) since neighbors rarely change every millisecond. This "around me" view is what players actually stare at, so optimize it, not just top-100.
> **Trap:** Fetching the whole board to the app and slicing in code (O(n) transfer + compute), or issuing a rank query per neighbor. Let Redis do the range in one call.

Q9. Consistency: does the displayed rank need to be exact and instantly consistent?
> **Expected answer:** **Eventual/approximate is fine** (Lesson 2, AP). A rank shown as #4,213 vs #4,210 for a few hundred ms doesn't matter to a player deep in the pack. So you can serve rank reads from a **replica** with slight lag, cache "around me" windows briefly, and update the far reaches of the board lazily. Redis single-threaded ZADD keeps the primary internally consistent; replicas lag by ms.
> **Mentor pushback:** The **top ranks and tournament payouts** need exactness — being #1 vs #2 decides prize money. For those top-N positions and any decisions with real stakes, read from the primary and treat updates as strongly consistent (or settle from the durable DB at tournament close). So: eventual for the long tail, strong for the top and for money. Name that split.

---

### Low-Level Design (Hard)
Q10. Design real-time fan-out so 50K concurrent viewers see live rank changes without O(viewers × board) work per update.
> **Problem statement:** Every ZADD can change ranks; naively you'd recompute and push the board to all 50K viewers on every one of thousands of updates/sec.
> **Naive solution:** On each score update, broadcast the full top-100 (or the whole board) to every connected client.
> **Why naive fails at scale:** 50K viewers × 100 rows × thousands of updates/sec = billions of messages/sec — network and CPU meltdown; clients get flooded with redundant frames.
> **Expected optimal approach:** (1) **Interest-based routing** — each viewer subscribes to a *window* (top-N and/or their "around me" band). An update only fans out to viewers whose window it intersects. (2) **Delta push** — send only the changed row(s), not the board. (3) **Coalesce + tick** — batch updates on a fixed cadence (e.g., 4–10 Hz), so a row changing 100×/sec still pushes ~5×/sec. (4) **Redis pub/sub or a fan-out tier** decouples ZSET writers from connection holders.
> **Pseudo-code or class diagram:**
```
on_score_update(player, new_rank, old_rank):
    for band in affected_bands(old_rank, new_rank):   # e.g., top100, nearby
        deltas[band].add({player, new_rank, score})   # buffer

every 200ms (tick):                                    # coalesce
    for band, changes in deltas:
        subscribers = fanout.subscribers_of(band)
        for conn in subscribers:
            conn.send(diff(changes))                   # small delta only
    deltas.clear()
```

Q11. A player submits two scores nearly simultaneously (double-tap / retry); or two workers process the same player. Prevent a wrong/lost update.
> **Scenario:** Events for the same player interleave; a lower score could overwrite a higher one, or a retry double-counts.
> **Expected fix:** (1) **Partition Kafka by player_id** (Lesson 21) so one player's events are ordered and processed by a single consumer — no cross-worker race. (2) Use **max semantics**: `ZADD GT` (Redis 6.2+ update only if greater) so a stale/lower score never demotes the player; for cumulative games use idempotent increment keyed by idempotency_key. (3) Dedup retries via idempotency_key stored with a short TTL.
> **Follow-up:** What if the consumer dies mid-batch after ZADD but before committing the Kafka offset? On restart it reprocesses — safe *because* ZADD GT and idempotency_key make reprocessing a no-op. That's why idempotency + ordering, not locks, is the right tool here.

Q12. Redis primary crashes. What's lost and how do you rebuild the ZSET?
> **Scenario:** The ZSET-holding Redis node dies; last few seconds of ZADDs may be unpersisted (AOF fsync lag).
> **Expected handling:** Redis is a **rebuildable index**, not the truth (durable DB is). On failure: promote a replica (Lesson 3) — near-warm, minimal loss. If the ZSET is truly lost, **rebuild it from the durable DB** (`scores` table) with a batch `ZADD` load, and/or **replay recent Kafka score events** (retained, e.g., 24h) to re-apply anything after the last DB checkpoint. Set AOF `everysec` for a ≤1s loss window. During rebuild, serve top-K reads from the DB's `(board, score DESC)` index (degraded but available).

---

### Scaling to 10x / 100x (Hard)
Q13. At 10x, where does it break first — the ZSET or the fan-out?
> **Expected answer:** The **real-time fan-out and WebSocket connection tier** break before the ZSET. A single Redis node handles millions of ZADD/ZRANGE ops/sec and a 10M-member ZSET is only ~1GB. But holding 500K+ live WebSocket connections and pushing deltas is a connection-count and egress problem — one server holds ~50–100K connections, so you need a horizontally scaled fan-out fleet with a pub/sub backbone. Second bottleneck: hot write path if updates aren't coalesced.
> **Numbers to ground the answer:** 500K concurrent connections ÷ ~65K/server ≈ **~8–10 fan-out servers** minimum, more for delta compute. ZSET ops: one Redis primary ~100–200K ops/sec — if rank reads exceed that, add **read replicas** (rank reads are replica-safe). Push cadence 5–10 Hz keeps per-connection egress sane.

Q14. You've outgrown one Redis node for a single massive board. How do you shard one leaderboard? (Preview of Lesson 33.)
> **Expected sharding strategy:** A single ZSET can't be trivially split because **global rank spans all members**. Options: (a) **Range/bucket sharding by score** — shard 0 holds scores 0–1000, shard 1 holds 1000–2000, etc.; global rank = sum of higher-shard counts + local rank. Hot-score-range skew is the risk. (b) **Hash shard members, keep per-shard sorted sets, merge at query** — top-K = merge top-K from each shard (heap merge, Lesson 5); arbitrary rank needs `ZCOUNT(score > mine)` summed across shards. This is exactly Lesson 33's problem.
> **Hot spot problem:** Score distributions cluster (many players near the mode), so range-sharding by score hot-spots the popular band. Detect via per-shard op rate; fix by **non-uniform range boundaries** (narrow ranges where players cluster) or hash-sharding with a cross-shard rank aggregation.

Q15. Caching strategy for rank reads.
> **Expected layered cache design:** **L1** — app-local micro-cache of `top-100` and hot "around me" windows, TTL 200–500ms (ranks deep in the pack barely move; huge read-amplification win). **L2** — Redis ZSET itself is already the fast tier; add **read replicas** for rank-read scale-out. **Edge/CDN** — the top-100 board is highly cacheable for spectators (public view), serve as a periodically-refreshed JSON blob from CDN.
> **Cache invalidation trap:** The **top-100 changes constantly**, so a too-long TTL shows stale leaders while a too-short TTL kills the cache's value. Fix: push-based invalidation — when a top-N position changes, invalidate/refresh that cached blob via the same fan-out event; use TTL only as a safety net. The "around me" cache is easier — short TTL is fine because small errors don't matter there.

Q16. Cost/efficiency at scale.
> **Expected answer:** (1) **Coalesce updates** (tick-based push) — the single biggest cost lever for fan-out egress and CPU. (2) **Delta encoding** — send changed rows, not full boards. (3) **Replica-served reads** so the primary only writes. (4) **CDN the public top-100** to offload spectator traffic. (5) **Aggregate/throttle submissions** at the client/edge (max-per-window) instead of raw floods. (6) **TTL/season-expire old boards** — daily/weekly boards roll off; don't keep every historical ZSET in RAM, snapshot to the DB and drop from Redis.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Redis ZSET stores rank via skip-list **spans**. Explain how `ZREVRANK` computes rank in O(log N) using spans, and how ties (equal scores) are ordered. (Rank = sum of spans skipped along the descending search path. Equal scores are ordered **lexicographically by member**; to break ties by achievement time, encode a timestamp into the score's low bits or the member, so `score = points*1e9 - timestamp`.)

**H2.** Anti-cheat / integrity: a client submits an impossible score. Where and how do you validate, and how do you keep the leaderboard tamper-proof? (Server-authoritative scoring only — never trust client scores; validate against game-physics bounds and rate limits (Lesson 11) in the scoring worker; sign/replay-verify match results; keep the durable DB as audit truth so a compromised Redis can be rebuilt clean.)

**H3.** Deploy a scoring-logic change (new anti-cheat rule) with zero downtime and no rank corruption. (Version the scoring worker; roll out behind a feature flag; the ZSET is rebuildable so you can recompute from the durable DB with the new logic in a shadow ZSET, validate, then swap. Never mutate the live ZSET with two logics at once.)

**H4.** What do you instrument? (ZADD/ZRANK op latency & rate, ZSET cardinality, fan-out push latency & dropped-frame rate, WebSocket connection count & churn, Kafka consumer lag (the leading indicator of stale ranks), replica lag, submission rate-limit rejections. Alert on consumer lag rising — means the board is falling behind reality.)

**H5.** You launched pushing the **full board to everyone** and it melts at 100K viewers. Tell the migration to interest-based delta fan-out. (Introduce band subscriptions + coalescing tick + delta diffs incrementally: first add coalescing (immediate 10x cut), then band-scoped routing, then delta encoding. Measure egress drop at each step. Lesson: real-time cost is dominated by *fan-out shape*, not by the rank computation.)

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. **Pushing the whole board to everyone on every update** — the real-time fan-out, not the ZSET, is what breaks; interest-based delta + coalescing is the fix.
2. **Treating Redis as the source of truth** — it's a rebuildable index; the durable DB (or Kafka replay) is truth, which is what makes crash recovery trivial.
3. **Claiming O(1) or O(n) for rank** — it's O(log N) via skip-list spans; not knowing the span mechanism signals shallow ZSET understanding.

**The one insight that makes an answer truly impressive:**
Separate the two hard problems that look like one: the **rank index** (solved elegantly by ZSET skip-list spans, O(log N), cheap) and the **real-time distribution** (an interest-routing + coalescing + delta problem whose cost scales with viewers, not players). Most candidates conflate them and over-engineer the index while under-designing the fan-out.

**Suggested follow-up reading:**
- Redis documentation — sorted sets internals (ziplist/listpack vs skiplist encoding, `ZADD GT/LT`, spans).
- "Building a real-time leaderboard" engineering posts (e.g., Discord/Riot on WebSocket fan-out and coalescing).

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
