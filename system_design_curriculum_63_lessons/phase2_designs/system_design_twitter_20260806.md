# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 44 of 63 — Phase 2: System Design Track (Design 16 of 35)
**Topic:** Twitter/X — Social Feed + Fanout
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
The Twitter timeline is the canonical "read-heavy, write-amplified" problem: 500M+ tweets/day, but the home timeline is read ~300x more than it's written, and a single tweet from a celebrity must land in tens of millions of timelines. Twitter solved this with a hybrid fanout model — fanout-on-write for normal users, fanout-on-read for celebrities — after their original Ruby monolith and naive fanout melted under the "Bieber problem." What makes it hard: the write amplification of one high-follower tweet is O(followers), the read latency budget is ~200ms for a fully merged, ranked timeline, and the two paths must reconcile without showing duplicates or gaps.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Kafka / event-driven (taught in Phase 1, Lesson 21):** A durable, partitioned, append-only log where producers write and consumers read at their own offset. Partitions give ordering-per-key and parallelism; consumer groups let you scale fanout workers horizontally. Retention lets you replay. Here, every tweet becomes an event on a `tweets` topic partitioned by author_id; a pool of fanout workers consumes it and injects the tweet into follower timelines. If a worker dies, offsets let the replacement resume without losing tweets.

**Consistent hashing (taught in Phase 1, Lesson 19):** Map both keys and nodes onto a hash ring so adding/removing a node only remaps 1/N of keys; virtual nodes smooth out load. Used here to shard the timeline cache (Redis) and the tweet store by user_id, so we can add cache capacity during a spike without a full reshuffle.

**Caches / Redis (taught in Phase 1, Lesson 24):** In-memory KV with data structures (lists, sorted sets), TTLs, and eviction policies (LRU/LFU). The home timeline itself lives in Redis as a capped list of tweet IDs per user — the "timeline cache." We store IDs, not hydrated tweets, to keep memory bounded (~800 entries/user).

**SQL/NoSQL, B-tree vs LSM (taught in Phase 1, Lesson 23):** LSM-tree stores (Cassandra/Manhattan) absorb high write throughput via memtable + SSTable compaction; B-trees favor read-modify-update. Tweets are write-heavy and immutable → LSM store. The social graph (follows) is also LSM/wide-column, keyed for both "who I follow" and "who follows me."

**Load balancers (taught in Phase 1, Lesson 6):** L4/L7 distribution, health checks, connection draining. Fronts the API tier; L7 lets us route `/timeline` reads separately from `/tweet` writes so a write storm doesn't starve reads.

**CAP / eventual consistency (taught in Phase 1, Lesson 2):** In a partition you choose consistency or availability. The timeline is an AP system — a user seeing a tweet 2 seconds late is fine; the timeline being *down* is not. Follower counts and timelines are eventually consistent.

**Capacity estimation (taught in Phase 1, Lesson 28):** QPS = DAU × actions/day ÷ 86,400, times a peak factor. We'll size fanout writes, timeline cache memory, and read QPS from first principles below.

> **Recap box:**
> - Kafka = durable fanout pipeline, partition by author_id.
> - Consistent hashing = reshard timeline cache without a full remap.
> - Redis list of tweet IDs = the home timeline; store IDs not objects.
> - LSM store = immutable, write-heavy tweets.
> - Hybrid fanout exists *because* of the CAP choice: AP timeline, eventual consistency.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. What's the difference between fanout-on-write (push) and fanout-on-read (pull) for a home timeline, and why does Twitter need both?
> **What a strong answer covers:** Fanout-on-write: at tweet time, the tweet ID is pushed into every follower's precomputed timeline (Redis list) — reads are O(1), writes are O(followers). Fanout-on-read: at read time, you fetch the latest tweets from everyone the user follows and merge them — writes are O(1), reads are expensive. Push wins for the common case (fast reads, most users have modest follower counts); pull wins for celebrities where push would mean 50M writes per tweet. Hybrid: push for normal authors, pull for celebrity authors, merged at read time.
> **Common weak answer:** "Just precompute everyone's timeline" — ignores that a tweet from a 100M-follower account would trigger 100M writes and saturate the cache tier.
> **Mentor follow-up if they answer well:** Where's the follower-count threshold that flips an author from push to pull, and is it static or dynamic?

Q2. Estimate the daily fanout write volume. Assume 250M DAU, average user tweets 0.5 times/day, average 400 followers.
> **What a strong answer covers:** Tweets/day = 250M × 0.5 = 125M tweets. Naive pure-push fanout writes = 125M × 400 = 50B timeline insertions/day ≈ 580K writes/sec average, and with a peak factor of ~3x → ~1.7M writes/sec. That's the number that justifies not pushing to everyone. Note the distribution is heavy-tailed: a handful of accounts contribute the bulk of the amplification, which is exactly why the hybrid model targets them.
> **Mentor follow-up:** Now recompute assuming the top 0.1% of accounts (avg 5M followers) are moved to pull. How much fanout write volume do you save?

Q3. Would you store the home timeline as fully-hydrated tweet objects or just tweet IDs in the cache? Defend the choice.
> **What a strong answer covers:** Store IDs. A hydrated tweet with text, media URLs, author, counts is ~1–2KB; an ID is 8 bytes. At 800 IDs/user × 250M users, IDs cost ~1.6TB vs ~1.6PB hydrated — infeasible. IDs also avoid staleness: like counts, author display name, and deletion status are resolved at read time by hydrating from the tweet-store cache. Trade-off: an extra hydration round-trip (mget of ~200 tweet objects) on the read path, easily cached.
> **Red flag answer:** "Store full tweets so reads need no joins" — blows memory budget and serves stale counts/deleted tweets.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Draw the end-to-end architecture for posting a tweet and rendering a home timeline, including the hybrid fanout path.
> **Key components expected:** API gateway/LB, Tweet Write service, Kafka fanout pipeline, Fanout workers, Timeline cache (Redis), Tweet store (Manhattan/Cassandra), Social graph service, Timeline read/mixer service, Ranking service, Media/CDN, celebrity-list service.
> **Architecture diagram (text):**
```
                         ┌─────────────┐
   Client ──HTTPS──▶ L7 LB ──▶ API Gateway (auth, rate limit)
                         └─────────────┘
             write │                          │ read
                   ▼                          ▼
        ┌───────────────────┐      ┌────────────────────────┐
        │ Tweet Write Svc   │      │ Timeline Read/Mixer Svc │
        │ - validate/store  │      │ 1. get pushed IDs (Redis)│
        └───┬───────────┬───┘      │ 2. get followed          │
            │           │          │    celebrities           │
   append   │           │ store    │ 3. pull their recent     │
            ▼           ▼          │    tweets                 │
      ┌──────────┐  ┌──────────┐   │ 4. merge + rank          │
      │  Kafka   │  │TweetStore│   │ 5. hydrate via mget      │
      │tweets tp │  │  (LSM)   │   └──────┬──────────┬────────┘
      └────┬─────┘  └──────────┘          ▼          ▼
           │ consume                 ┌─────────┐ ┌──────────┐
           ▼                         │ Ranking │ │ Tweet    │
   ┌───────────────┐                 │  Svc    │ │ hydration│
   │ Fanout Workers│──is author a────┘         │ │ cache    │
   │               │  celebrity? skip           └──────────┘
   │  else: LPUSH  │
   │  to N follower│───▶ ┌──────────────────────┐
   │  timelines    │     │ Timeline Cache (Redis)│  Media ──▶ CDN
   └───────────────┘     │  user → [tweetIDs]    │
        ▲                └──────────────────────┘
        │ getFollowers
   ┌────────────┐
   │Social Graph│
   └────────────┘
```
> **What separates SDE2 from SDE3 here:** SDE2 draws push fanout correctly. SDE3 explicitly handles the *merge boundary*: pushed tweets and pulled celebrity tweets must be deduplicated and ordered on a single monotonic key (Snowflake ID, which is time-sortable), and handles the "user just followed a celebrity" case where there are no pushed tweets yet — pull backfills it.

Q5. Trace exactly what happens when a user with 2,000 followers (300 of them currently online) posts a tweet.
> **Expected trace:**
> 1. Client → API gateway (authN via JWT, rate-limit check).
> 2. Tweet Write svc assigns a **Snowflake ID** (timestamp-ordered 64-bit), persists the tweet to the LSM store, uploads media pointers.
> 3. Emits event to Kafka `tweets` topic, partitioned by author_id (preserves per-author order).
> 4. Fanout worker consumes; queries social graph for follower list; checks author's follower count against celebrity threshold — 2,000 < threshold, so **push path**.
> 5. Worker batches `LPUSH`+`LTRIM` (cap ~800) into the 2,000 follower timeline lists in Redis. Optionally prioritizes the ~300 online followers first for perceived freshness.
> 6. Tweet becomes visible in followers' home timelines within ~1–5s.
> **Tricky part:** Candidates get vague on ordering. The timeline is sorted by Snowflake ID, not insert time — otherwise a delayed fanout worker would insert an old tweet at the top. Also: LTRIM must be atomic with LPUSH (Lua script or pipeline) to avoid unbounded lists.

Q6. Design the core API endpoints for posting and reading.
> **Expected API design:**
> - `POST /v1/tweets` body `{text, media_ids[], reply_to?, idempotency_key}` → 201 `{tweet_id}`. Idempotency key dedupes retries.
> - `GET /v1/timeline/home?cursor=<snowflake>&limit=20` → `{tweets[], next_cursor}`. Cursor pagination on Snowflake ID, **not** offset pagination.
> - `GET /v1/tweets/{id}` → hydrated tweet.
> - `POST /v1/follows` / `DELETE /v1/follows/{target}`.
> **What to push on:** (1) Cursor vs offset — offset pagination breaks when tweets are inserted at the head (page drift, duplicates). Cursor on the monotonic ID is stable. (2) Idempotency — double-tap/retry must not create two tweets; the idempotency key is stored with a short TTL. (3) Versioning via `/v1/`. (4) Pagination limit caps to bound hydration mget size.

---

### Data Modeling (Medium–Hard)

Q7. Design the primary data stores: tweets, the social graph, and the timeline cache.
> **Expected schema:**
```sql
-- Tweets: immutable, write-heavy → wide-column / LSM (Cassandra/Manhattan)
-- Partition key = tweet_id (Snowflake). No updates except soft-delete flag.
CREATE TABLE tweets (
  tweet_id   bigint PRIMARY KEY,   -- Snowflake: [41b ts | 10b machine | 12b seq]
  author_id  bigint,
  text       text,
  media_ids  list<bigint>,
  reply_to   bigint,
  created_at timestamp,
  deleted    boolean
);

-- Social graph — stored BOTH directions for O(1) lookups
CREATE TABLE followers (          -- "who follows X"  (for fanout)
  user_id    bigint,              -- partition key
  follower_id bigint,
  PRIMARY KEY (user_id, follower_id)
);
CREATE TABLE following (          -- "who X follows"  (for pull path)
  user_id    bigint,              -- partition key
  followee_id bigint,
  PRIMARY KEY (user_id, followee_id)
);
```
```
# Timeline cache (Redis), one capped list per user
timeline:{user_id}  ->  [ tweet_id, tweet_id, ... ]   # LPUSH + LTRIM 0 799
```
> **Index choices and why:** Snowflake ID is *itself* a time-sorted primary key, so range scans "tweets before cursor X" need no secondary index. The dual follower/following tables are a deliberate denormalization — fanout needs "followers of author" and the pull path needs "who this reader follows," and you can't get both cheaply from one table in a wide-column store.
> **Partitioning key and why:** Tweets partitioned by tweet_id → even write distribution, no hot author partition. Social graph partitioned by user_id → all of a user's edges co-located for a single-partition read. Timeline cache sharded by user_id via consistent hashing.

Q8. A user opens the app. How do you fetch and render their home timeline efficiently?
> **Expected answer:** (1) `LRANGE timeline:{uid} 0 19` → 20 tweet IDs from Redis (O(1) list slice). (2) In parallel, get the user's followed *celebrities* (small set, cached) and pull their recent tweets via a per-author "user timeline" cache. (3) Merge-sort both lists by Snowflake ID, dedupe, take top 20. (4) `MGET` hydrate those 20 tweet objects from the tweet-hydration cache (fallback to LSM store on miss). (5) Attach live counts (likes/retweets) from a counter service. Total: 2–3 Redis round-trips + one batched hydration, ~10–30ms.
> **Trap:** Fetching each followed user's tweets individually on every read (pure pull for everyone) — O(following) queries per timeline load, kills p99. Pull is only for the small celebrity subset.

Q9. Where do you accept eventual consistency in this system, and where must you not?
> **Expected answer:** Eventual is fine for: timeline propagation (seconds of lag OK), follower/like counts (approximate, reconciled), and search indexing. It's an AP system per CAP — availability of the timeline beats strict freshness. Must be stronger: a user must *see their own tweet immediately* after posting (read-your-writes) — solved by injecting the just-posted tweet into the author's own timeline synchronously; and block/mute must apply immediately (safety), so blocked-author tweets are filtered at read time, not relied upon to be absent from the cache.
> **Mentor pushback:** User A blocks user B, then B's already-fanned-out tweet sits in A's Redis timeline. Eventual consistency won't remove it. → Filter at read/hydration time against the block list; never trust the cache to be block-clean.

---

### Low-Level Design (Hard)

Q10. Design the hybrid fanout mechanism in detail — deciding push vs pull, and merging at read.
> **Problem statement:** Minimize write amplification for high-follower authors while keeping timeline reads O(1)-ish, with correct ordering and no duplicates across the two paths.
> **Naive solution:** Push every tweet to every follower's list.
> **Why naive fails at scale:** One 50M-follower tweet = 50M Redis LPUSHes. At ~200K ops/sec/shard, that's a multi-minute fanout that starves normal users and can OOM/hotspot shards — the classic "Bieber problem." Deletes/edits multiply the cost.
> **Expected optimal approach:** Maintain a `celebrity_threshold` (e.g., >1M followers, tuned). Authors above it are **excluded from push**; their tweets are read-time pulled from a per-author "user timeline" cache. On read, mixer fetches pushed IDs + the reader's followed-celebrity tweets, merges by Snowflake ID, dedupes, ranks. Threshold is dynamic per author — recompute on follower-count crossing.
> **Pseudo-code or class diagram:**
```
on_tweet(tweet):
    id = snowflake()
    store.put(tweet, id)
    kafka.publish("tweets", key=tweet.author_id, tweet)

fanout_worker.consume(tweet):
    if follower_count(tweet.author_id) > CELEB_THRESHOLD:
        userTimelineCache.lpush(tweet.author_id, tweet.id)  # pull source
        return                                              # skip fanout
    for batch in chunks(followers(tweet.author_id), 1000):
        redis.pipeline:
          for f in batch:
            LPUSH timeline:f tweet.id
            LTRIM timeline:f 0 799

read_home(uid, cursor, limit):
    pushed  = LRANGE timeline:uid 0 (limit*2)
    celebs  = followed_celebrities(uid)          # cached small set
    pulled  = [userTimelineCache.range(c, cursor, limit) for c in celebs]
    merged  = merge_by_snowflake(pushed, flatten(pulled))
    merged  = dedupe(merged)
    ranked  = ranking.score(merged)[:limit]
    return hydrate(ranked), next_cursor(ranked)
```

Q11. Concurrency: two fanout workers process two tweets from the same author out of order, or a follow happens mid-fanout. Handle it.
> **Scenario:** Worker A (older tweet) is slow; Worker B (newer tweet) finishes first and LPUSHes to the front. Then A LPUSHes the older tweet on top → out-of-order list.
> **Expected fix:** Don't rely on LPUSH position for order — the timeline is *sorted by Snowflake ID at read time* (or stored as a Redis **sorted set** scored by tweet_id). Then insertion order is irrelevant; the mixer/LRANGE-then-sort always yields correct chronology. For the "follow mid-fanout" case: a new follow triggers a **synchronous backfill** — pull the followee's recent N tweets into the follower's timeline — so a tweet in flight during the follow can't create a gap.
> **Follow-up:** What if the lock holder dies? There's no long-lived lock; Kafka offsets are the safety net. A worker crash before committing its offset means the tweet is redelivered → the LPUSH/LTRIM must be **idempotent** (using a sorted set keyed by tweet_id makes re-insertion a no-op).

Q12. Failure deep-dive: the Kafka fanout pipeline backs up during a spike; the timeline cache shard for a hot region goes down.
> **Scenario:** Consumer lag climbs to millions; timelines go stale by minutes. Separately a Redis shard dies, taking those users' precomputed timelines with it.
> **Expected handling:** (1) Fanout lag → autoscale fanout workers (more partitions/consumers), shed load by temporarily raising the celebrity threshold (push fewer, pull more at read), and prioritize online-user fanout. Kafka retention means no data loss — timelines just lag, then catch up. (2) Redis shard down → treat the timeline cache as **rebuildable derived state**: on cache miss, fall back to pull-mode for those users (fetch from followees' user-timeline caches / tweet store and reconstruct). Warm the shard from the LSM store + graph. Consistent hashing limits blast radius to 1/N of users. Poison messages (a tweet that repeatedly crashes a worker) go to a **DLQ** after N retries.

---

### Scaling to 10x / 100x (Hard)

Q13. At 10x load, where does this break first?
> **Expected answer:** The **fanout write path** and the **timeline cache memory/ops**. Fanout is O(followers) writes; a 10x growth in high-follower accounts multiplies Redis LPUSH ops super-linearly. The cache tier hits its ops/sec ceiling before the read path does (reads are O(1) list slices). Secondary bottleneck: the social-graph `getFollowers` fan-in read for large accounts.
> **Numbers to ground the answer:** ~1.7M fanout writes/sec at peak (from Q2) → 17M/sec at 10x. A single Redis instance does ~100–200K ops/sec, so you need ~100+ shards just for fanout writes, and the celebrity threshold must drop to shift more load to pull.

Q14. How do you shard the timeline cache and the tweet store, and how do you handle hotspots?
> **Expected sharding strategy:** Timeline cache: consistent hashing by user_id (each user's list on one shard; even distribution; add shards with minimal remap). Tweet store: partition by tweet_id (Snowflake) so writes spread across time and machine bits — no single hot partition even for one author's burst.
> **Hot spot problem:** A viral tweet's *hydration* becomes hot — everyone loading it hits one tweet_id partition and one cache key. Detect via per-key request counters (hot-key detection). Fix: replicate the hot tweet object to a small local in-process (L1) cache on every read node, or serve it from CDN/edge. For the fanout write hotspot (celebrity), the pull path already avoids it. For a hot *reader* (bot scraping), rate-limit per user (Lesson 11).

Q15. Design the layered caching and the hardest invalidation problem.
> **Expected layered cache design:** L1 = in-process LRU on read nodes for ultra-hot tweet objects and celebrity user-timelines (ms, tiny TTL 1–5s). L2 = Redis timeline cache (the precomputed home timelines) + Redis tweet-hydration cache. L3 = CDN for media and for public, cacheable profile/tweet render fragments. The LSM tweet store is the source of truth behind L2.
> **Cache invalidation trap:** **Tweet deletion / edit.** A deleted tweet may already be fanned out into millions of timeline lists — you cannot afford to scan and remove it from every list. Solution: mark `deleted=true` in the store and **filter at hydration time** (the ID stays in the list but hydration returns a tombstone, dropped from the response). Same for edits — hydration always fetches the latest version, so the list-of-IDs model makes edits "free." The genuinely hard case is *count* invalidation (likes/retweets) which changes constantly — served from a separate counter service with approximate, TTL'd values, never baked into the cached timeline.

Q16. How do you keep this cost-efficient at scale?
> **Expected answer:** (1) Store IDs not objects in timelines (1.6TB vs 1.6PB — the single biggest lever). (2) Cap timeline length (~800) — nobody scrolls past that; older reads fall back to pull. (3) Tiered storage for tweets: hot (recent, in cache/SSD-backed LSM) vs cold (old tweets to cheaper object storage, rarely read). (4) Batch fanout LPUSHes via Redis pipelines to amortize round-trips. (5) Don't fanout to *inactive* users — lazily materialize a dormant user's timeline on their next login (pull-on-first-read) rather than pushing to accounts that never open the app. This alone cuts fanout volume dramatically because most DAU-eligible accounts are inactive on a given day.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Snowflake IDs are the backbone of ordering and cursor pagination. Explain the 64-bit layout (sign + 41-bit ms timestamp + 10-bit machine + 12-bit sequence), why the timestamp is the high-order bits (so IDs are k-sortable), what happens at 4096 IDs/ms/machine (sequence overflow → spin to next ms), and how clock skew / NTP backward jumps can produce duplicate or out-of-order IDs — and how you guard against it (reject-and-wait on backward clock).

**H2.** GDPR "right to be forgotten": a user deletes their account, but their tweet IDs are embedded in millions of followers' timeline caches and in others' reply threads. How do you guarantee deletion without scanning every timeline? (Tombstone-at-hydration + async purge of source-of-truth + a suppression list; the derived caches are allowed to lag but must never *serve* the content.)

**H3.** Deploy the fanout worker fleet with zero timeline loss during a rollout. Kafka consumer-group rebalance on deploy causes partition reassignment — how do you avoid double-fanout or gaps? (Commit offsets only after idempotent LPUSH; use sticky/cooperative rebalancing; idempotent sorted-set inserts make redelivery safe.)

**H4.** What do you instrument? Fanout lag (Kafka consumer offset lag per partition), fanout latency p50/p99 (tweet-created → landed-in-timeline), timeline read p99, hydration cache hit rate, celebrity-threshold crossings/min, hot-key alerts. The single most important SLO: **time-to-timeline** for the online-follower cohort.

**H5.** "Undo a bad decision": you launched with pure fanout-on-write and now a growth in mega-accounts is melting the cache tier. Migrate to hybrid *without downtime*. (Introduce celebrity threshold behind a flag; start pulling for accounts above it while their already-pushed tweets age out of the 800-cap naturally; dual-read/merge during transition; verify no gaps via shadow comparison before removing their push path.)

---

### Mentor's Closing Notes

**Top 3 things most candidates get wrong on this topic:**
1. Proposing pure push *or* pure pull — the entire interesting problem is the hybrid boundary and the merge.
2. Storing hydrated tweets in the timeline cache — blows memory and serves stale counts/deleted content.
3. Ordering timelines by insertion/fanout time instead of a monotonic tweet ID — leads to out-of-order, gappy timelines under concurrent fanout.

**The one insight that makes an answer truly impressive:**
Fanout is not really "push vs pull" — it's *lazy materialization of derived state*. The home timeline is a cache that can always be rebuilt from tweets + the graph. Once you frame it that way, deletion (tombstone-at-read), inactive-user savings (materialize on login), shard failure (rebuild via pull), and edits (free, IDs only) all fall out of the same principle.

**Suggested follow-up reading:**
- Twitter Engineering — "The Infrastructure Behind Twitter: Scale" and the classic "Timelines at Scale" (Raffi Krikorian) talk.
- Discord/Instagram fanout writeups for contrast; Twitter's Manhattan and Snowflake blog posts.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
