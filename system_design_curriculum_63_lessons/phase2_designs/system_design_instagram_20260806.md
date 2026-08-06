# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 45 of 63 — Phase 2: System Design Track (Design 17 of 35)
**Topic:** Instagram — Photo Feed + Social Graph
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Instagram is Twitter's fanout problem plus a *heavy binary-object* problem: every feed item is backed by a multi-megabyte photo/video that must be transcoded into many renditions and served from the edge in under 100ms. Instagram famously scaled to 14M users with ~3 engineers on Postgres + a thin sharding layer, then to billions on a hybrid of sharded Postgres, Cassandra, TAO-style graph, and a massive CDN. What makes it hard: the metadata path (feed/graph) and the blob path (media) have completely different scaling characteristics, and you must keep them consistent — a feed entry pointing at a photo that isn't transcoded yet is a broken post.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**CDN / SaaS-PaaS-IaaS (taught in Phase 1, Lesson 18):** A CDN caches content at edge PoPs near users; origin is hit only on cache miss. Instagram media (photos, video renditions) is served almost entirely from CDN — origin (blob store) sees a small fraction of read traffic. The feed API returns *CDN URLs*, not bytes.

**Object/blob storage & GFS lineage (taught in Phase 1, Lesson 15):** Large immutable objects live in an object store (S3-like), not a database. Photos are written once, read millions of times, never mutated → perfect blob-store fit. Metadata (who, when, caption) lives in the DB; the DB stores only the blob key + rendition manifest.

**SQL sharding, B-tree vs LSM (taught in Phase 1, Lesson 23):** Instagram's origin story is sharded Postgres. Photos and the graph are sharded by user_id; each shard is a Postgres schema. LSM (Cassandra) is used for the feed/inbox where write throughput dominates.

**Consistent hashing (taught in Phase 1, Lesson 19):** Maps user_id → logical shard → physical DB, so you can split/move shards as capacity grows. Instagram used a fixed set of *logical* shards (thousands) mapped onto fewer physical machines — move logical shards, not rehash users.

**Kafka / event-driven (taught in Phase 1, Lesson 21):** Uploads emit events that drive async transcoding and feed fanout. Decouples the slow media pipeline from the fast metadata write.

**Caches / Redis, Memcached (taught in Phase 1, Lesson 24):** Instagram is one of the largest Memcached users. Feed IDs, media metadata, and counts are cached. The feed itself (list of media IDs) is a Redis/Cassandra-backed precomputed timeline like Twitter.

**Capacity estimation (taught in Phase 1, Lesson 28):** We'll size storage for photos, transcode compute, and read bandwidth from DAU × upload rate × rendition sizes.

> **Recap box:**
> - CDN serves the bytes; the DB serves the pointers.
> - Blob store = immutable photos; DB = metadata + rendition manifest.
> - Shard photos/graph by user_id into thousands of logical shards.
> - Kafka decouples slow transcode from fast metadata write.
> - Feed = precomputed list of media IDs (same fanout model as Twitter).

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Why do you store photo *bytes* in an object store but photo *metadata* in a database? What exactly goes where?
> **What a strong answer covers:** Object store: the immutable binary + its derived renditions (thumbnail, 640px, 1080px, WebP/AVIF variants), keyed by an opaque media_id. DB: media_id, author_id, caption, created_at, location, blob key, rendition manifest (which sizes exist), status (uploading/transcoding/ready), and counts. Rationale: databases are terrible at multi-MB blobs (bloats pages, kills buffer cache, replication overhead); object stores give cheap, infinitely scalable, CDN-frontable, immutable storage. The DB row is ~1KB and indexable; the photo is ~2MB and never queried by content.
> **Common weak answer:** "Store the photo as a BLOB column" — destroys DB cache locality, replication, and backup times.
> **Mentor follow-up if they answer well:** How does the feed avoid showing a post whose transcode hasn't finished?

Q2. Estimate daily storage for photos. 500M DAU, 10% upload/day, avg 3MB original, and you keep 4 renditions totaling ~1.5x the original.
> **What a strong answer covers:** Uploads/day = 500M × 0.10 = 50M. Original bytes = 50M × 3MB = 150TB/day. With renditions (×2.5 total incl. original) ≈ 375TB/day ≈ ~137PB/year before replication; with 3x durability replication → ~400PB/year. This is why tiered storage and aggressive compression (AVIF) matter. Read bandwidth dwarfs this: each photo read many times, mostly from CDN.
> **Mentor follow-up:** Now the read side — if the feed serves 50M reads/sec at peak and each photo is 200KB from CDN, what origin bandwidth do you actually need at a 98% CDN hit rate?

Q3. Should the feed API return image bytes or URLs? Defend it.
> **What a strong answer covers:** Return **signed CDN URLs** (plus width/height, blurhash placeholder, and available renditions). The client picks the rendition for its screen/DPR and network. This offloads all byte-serving to the CDN, keeps the feed API response tiny (~few KB for 20 posts), enables progressive loading (blurhash first), and lets the CDN handle range requests for video. Bytes-in-API would balloon payloads and bypass edge caching.
> **Red flag answer:** "Return base64 image bytes in JSON" — 33% size inflation, no CDN caching, huge payloads.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Draw the architecture for uploading a photo and rendering the home feed.
> **Key components expected:** Client, API gateway/LB, Upload service, Object store (originals + renditions), Transcoding pipeline (Kafka + workers), Metadata DB (sharded Postgres/Cassandra), Feed/fanout service, Feed cache, Social graph service (TAO-style), CDN, blurhash/thumbnail generator.
> **Architecture diagram (text):**
```
  Client ──HTTPS──▶ L7 LB ──▶ API Gateway (auth, rate limit)
     │ 1.get upload URL          │
     ▼                           │
 ┌──────────────┐                │
 │Upload Service│─signed PUT URL─┘
 └──────┬───────┘
   2.   │ client PUTs bytes directly
        ▼
 ┌──────────────────┐   3.event   ┌──────────┐   ┌────────────────┐
 │ Object Store     │────────────▶│  Kafka   │──▶│ Transcode      │
 │ (originals)      │             │ media tp │   │ Workers        │
 └──────────────────┘             └──────────┘   │ ffmpeg: 4 sizes│
        ▲ write renditions ─────────────────────  │ AVIF/WebP/h264 │
        │                                          └───────┬────────┘
 ┌──────────────────┐                                      │ 4.status=ready
 │ CDN (edge PoPs)  │◀── pull-through from object store     ▼
 └────────┬─────────┘                             ┌──────────────────┐
          │ serves bytes                          │ Metadata DB      │
          ▼                                        │ (sharded)        │
       Client ◀── feed JSON (CDN URLs) ── Feed Svc │ media, manifest  │
                                          │        └──────────────────┘
                                    ┌─────┴──────┐        │ 5.fanout event
                                    │ Feed Cache │◀───────┤
                                    │ user→[ids] │   ┌────▼─────┐  ┌──────────┐
                                    └────────────┘   │ Fanout   │──│  Social  │
                                                     │ Workers  │  │  Graph   │
                                                     └──────────┘  └──────────┘
```
> **What separates SDE2 from SDE3 here:** SDE3 uses **direct-to-object-store upload via pre-signed URL** — the bytes never transit the app tier. And they handle the *state machine*: a post is `uploading → transcoding → ready`; fanout to feeds only fires on `ready`, so no follower ever sees a post whose image 404s.

Q5. Trace an upload from tap-to-share to appearing in a follower's feed.
> **Expected trace:**
> 1. Client requests upload slot → Upload svc returns a pre-signed PUT URL + media_id.
> 2. Client PUTs the original bytes **directly to object store** (not through app tier); also uploads a client-computed blurhash for instant placeholder.
> 3. Object store emits an event → Kafka `media` topic.
> 4. Transcode workers pull it, run ffmpeg → generate 4 renditions (thumb, 640, 1080, video h264/AV1), write them back to the object store, update the rendition manifest.
> 5. On completion, media status → `ready` in metadata DB, which emits a **fanout** event.
> 6. Fanout workers push media_id into followers' feed lists (hybrid push/pull, same as Twitter for celebrities).
> 7. Follower's feed read returns the media_id → hydrated to CDN URLs.
> **Tricky part:** Candidates forget the ordering constraint — fanout must be gated on transcode completion. Also the *author's own* view should show the post immediately (optimistic UI with local bytes) before transcode finishes.

Q6. Design the key API endpoints, including the upload handshake.
> **Expected API design:**
> - `POST /v1/media/upload-url` → `{media_id, put_url, expires_at}` (pre-signed, short TTL).
> - `POST /v1/media/{id}/publish` body `{caption, location, blurhash, idempotency_key}` → 202 (kicks transcode+fanout).
> - `GET /v1/feed/home?cursor=<id>&limit=15` → `{items:[{media_id, author, caption, renditions{...}, blurhash, counts}], next_cursor}`.
> - `GET /v1/users/{id}/media?cursor=...` (profile grid).
> **What to push on:** (1) Pre-signed URLs decouple bytes from the API and have TTLs. (2) Idempotency key on publish so retries don't create duplicate posts. (3) Cursor pagination on media_id (Snowflake-like), never offset — profile grids drift as new posts arrive. (4) `renditions` map lets clients choose by DPR/network. (5) Video needs HLS/DASH manifest URLs for adaptive bitrate, not a single file.

---

### Data Modeling (Medium–Hard)

Q7. Design the media, social graph, and feed schemas.
> **Expected schema:**
```sql
-- Media metadata: sharded by author_id (thousands of logical shards)
CREATE TABLE media (
  media_id    bigint PRIMARY KEY,     -- Snowflake, time-sortable
  author_id   bigint,
  caption     text,
  blob_key    text,                   -- object-store key of original
  renditions  jsonb,                  -- {thumb:key, r640:key, r1080:key, hls:key}
  blurhash    text,
  status      smallint,               -- 0=uploading 1=transcoding 2=ready 3=failed
  location    geography,
  created_at  timestamptz
);

-- Social graph (TAO-style association store); both directions
CREATE TABLE follow_edges (
  from_id bigint, to_id bigint, created_at timestamptz,
  PRIMARY KEY (from_id, to_id)          -- "following"; inverse index on to_id
);

-- Feed / inbox: LSM (Cassandra) — one row-key per user, wide columns of media_ids
--   feed[user_id] : { media_id (clustering, desc) -> author_id }
```
```
# Counts (likes/comments) — separate, high-write counter store (Redis/Cassandra counters)
count:media:{id}:likes -> N
```
> **Index choices and why:** media_id is the time-sortable PK → feed range scans need no extra index. `follow_edges` needs a secondary index on `to_id` (or a mirror table) for "who follows me" (fanout). Counts are split out because likes update orders of magnitude more often than the media row — you don't want to rewrite a 1KB row for every like.
> **Partitioning key and why:** Media & graph sharded by author_id/user_id → a user's posts and edges co-locate. Feed sharded by user_id (reader). Counts sharded by media_id.

Q8. How do you render a user's profile grid (their last 30 posts) efficiently, and their feed?
> **Expected answer:** Profile grid: single-partition query on the author_id shard, `WHERE author_id=? ORDER BY media_id DESC LIMIT 30`, media rows cached in Memcached (hot for popular accounts). Feed: `feed[uid]` clustering-column slice for top 15 media_ids from Cassandra, then `MGET` hydrate media rows from Memcached, then construct CDN rendition URLs. Blurhash comes back in the same media row for instant placeholders while CDN fetches the real image.
> **Trap:** Doing a cross-shard scatter-gather to build the feed on every read (querying each followed user's media shard live). That's O(following) cross-shard queries — fine at 14M users, catastrophic at 500M. Precompute the feed via fanout instead.

Q9. Where is Instagram eventually consistent, and where can't it be?
> **Expected answer:** Eventual: feed propagation, like/comment counts (approximate, TTL'd), follower counts, search/explore indexing. It's AP for the feed. Stronger guarantees: (1) a post must not appear until `status=ready` (no broken images) — a correctness gate, not eventual; (2) blocked/private-account visibility must be enforced at read time (privacy = safety); (3) read-your-writes for the author's own profile grid.
> **Mentor pushback:** A private account approves a follow request — the new follower's feed must *not* retroactively get past posts they weren't allowed to see, but *should* get future ones. Fanout-on-approval must respect the approval timestamp, and read-time checks must verify the follow edge still exists (unfollow mid-session).

---

### Low-Level Design (Hard)

Q10. Design the transcoding pipeline for a video upload — the hardest sub-problem here.
> **Problem statement:** Turn one uploaded video into an adaptive-bitrate ladder (multiple resolutions/bitrates) reliably, at scale, without blocking the metadata path, and gate feed visibility on completion.
> **Naive solution:** Transcode synchronously in the upload request; one worker does all sizes serially.
> **Why naive fails at scale:** A 60s 4K video takes tens of seconds to transcode — the HTTP request times out, the app tier is blocked, and at 50M uploads/day you'd need an absurd synchronous fleet. One slow video head-of-lines everyone.
> **Expected optimal approach:** Async, **chunked/segment-parallel** transcode. Split the video into GOP-aligned segments; distribute segments across workers; each produces the rendition ladder for its segment; reassemble into an HLS/DASH manifest. Idempotent per-segment (retriable). Priority queues so short videos / high-profile creators aren't stuck behind a giant upload. Emit `ready` only when all segments of all renditions land.
> **Pseudo-code or class diagram:**
```
publish(media):
    db.insert(media, status=TRANSCODING)
    kafka.publish("transcode", media_id, key=media_id)

transcode_orchestrator.consume(media_id):
    segments = split_by_GOP(original)          # e.g., 6s segments
    for seg in segments:
        for rendition in [240p,480p,720p,1080p]:
            kafka.publish("seg-transcode", {media_id, seg, rendition})

seg_worker.consume(job):                        # idempotent
    out_key = key(job.media_id, job.seg, job.rendition)
    if object_store.exists(out_key): return     # already done (retry-safe)
    bytes = ffmpeg(job.seg, job.rendition)
    object_store.put(out_key, bytes)
    completion_tracker.mark(job)                 # atomic counter

on_all_segments_done(media_id):
    manifest = build_hls_manifest(media_id)
    db.update(media_id, renditions=manifest, status=READY)
    kafka.publish("fanout", media_id)            # NOW eligible for feeds
```

Q11. Concurrency: a user double-taps "share," or a transcode job is redelivered by Kafka. Prevent duplicate posts / duplicate transcodes.
> **Scenario:** Client retries `publish` after a network blip → two media rows; or a seg-worker crashes after writing bytes but before committing offset → job redelivered.
> **Expected fix:** (1) `idempotency_key` on publish, stored uniquely; the second call returns the same media_id. (2) Transcode idempotency via **content-addressed output keys** — `exists(out_key)` short-circuits a redelivered segment (see pseudo-code). Completion tracked with an atomic counter/set of segment keys, so double-marking is a no-op.
> **Follow-up:** What if the orchestrator dies mid-split? The `media.status=TRANSCODING` row with a heartbeat/lease lets a reaper detect stalled transcodes (no progress in N minutes) and re-enqueue; because every step is idempotent, re-running is safe and never duplicates output.

Q12. Failure deep-dive: the CDN edge has the old rendition cached, but you had to re-transcode (corrupt output); or the object store write of a rendition silently fails.
> **Scenario:** Stale/broken image served from edge; or feed shows a post whose 1080 rendition is missing.
> **Expected handling:** (1) **Immutable, versioned rendition keys** — a re-transcode writes a *new* key and the manifest points to it; old edge cache entries for the old key are simply orphaned (and purged async). Never overwrite a rendition in place — that's what causes stale-CDN bugs. (2) Missing rendition: the client requests a rendition not in the manifest → API only advertises renditions confirmed present; if a rendition is missing, serve the next-best available and enqueue a repair job. (3) Transcode failure → `status=failed`, don't fanout, surface a retry to the author, DLQ the poison media after N attempts.

---

### Scaling to 10x / 100x (Hard)

Q13. At 100x, where does Instagram break first?
> **Expected answer:** Two places, different subsystems. (1) **Transcode compute** — it's CPU-bound and scales with upload volume × rendition ladder; video makes it explode. (2) **CDN egress bandwidth / cost** on the read side. The metadata DB and feed cache scale more gracefully via sharding. So the first pain is the transcode fleet queue depth and the CDN bill.
> **Numbers to ground the answer:** 50M uploads/day → 5B at 100x. If 10% are video at ~10 CPU-seconds/video to build the ladder, that's ~500M CPU-seconds/day for video alone ≈ ~5,800 cores continuously, before headroom. CDN: 50M photo reads/sec × 200KB = 10TB/sec edge egress — the origin only survives because of a 98%+ hit rate.

Q14. How do you shard the metadata DB and graph, and handle hot accounts?
> **Expected sharding strategy:** Thousands of **logical shards** by user_id, mapped onto physical DBs; move logical shards to rebalance without rehashing users (Instagram's actual approach). Snowflake-style IDs embed the shard, so you resolve a media_id → shard without a lookup.
> **Hot spot problem:** A celebrity's profile grid + a viral post concentrate reads on one shard/key. Detect via hot-key counters. Fix: aggressive Memcached caching of that account's media rows (read replicas + fan-out reads to replicas), and for the viral *image*, the CDN absorbs the byte load. For write hotspots (a creator posting a burst), Snowflake IDs spread writes across time bits; the real amplification (fanout) uses the celebrity pull path like Twitter.

Q15. Design the caching layers and name the nastiest invalidation.
> **Expected layered cache design:** L1 = client-side (blurhash placeholder + already-downloaded renditions cached on device). L2 = CDN for all media bytes (the dominant layer; 98%+ hit rate). L3 = Memcached for media metadata rows, feed lists, counts, and graph edges. Source of truth = sharded DB + object store.
> **Cache invalidation trap:** **Counts and the "thundering herd" on a viral post's metadata.** Likes/comments change thousands of times/sec on a viral post — you can't invalidate the media row cache per like. Split counts into a dedicated counter store with short TTL and read-through; the media row itself is near-immutable (caption edits are rare, versioned). Second trap: privacy changes — a user flips to private, and their photos are already cached at CDN edges under public URLs. Solution: **signed, expiring CDN URLs**; visibility is re-checked at feed-read time and URLs are minted per-request with short expiry, so a privacy flip stops new URL issuance quickly.

Q16. Cost/efficiency levers at scale?
> **Expected answer:** (1) Modern codecs — AVIF/HEIC for photos, AV1 for video — 30–50% smaller than JPEG/h264 for the same quality; huge CDN bill reduction. (2) Rendition-on-demand for long-tail sizes instead of pre-generating every size for every photo (most photos are viewed at 1–2 sizes). (3) Tiered object storage — recent media on hot/standard tier, old media to cold/archive (Instagram photos have a sharp recency-of-access curve). (4) Don't transcode the full ladder for low-reach accounts until first view (lazy). (5) Feed stores media_ids only (bytes-in-cache would be insane). (6) Deduplicate identical uploads via content hashing.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Explain the graph store internals (TAO-style): objects and associations, the two-tier cache (leader/follower cache regions over sharded MySQL), why it's *read-optimized* with eventual consistency, and how "who liked this" and "is A following B" are both association queries. Contrast with a native graph DB and why Meta chose an association store over Neo4j at their scale.

**H2.** Multi-region: a user in the EU uploads; a follower in the US must see it. How do you replicate object-store blobs and metadata cross-region, keep CDN warm globally, and comply with data-residency (EU photos may need to stay in-region while still being viewable elsewhere)? Discuss write-region affinity + async cross-region replication + edge caching.

**H3.** Roll out a new AV1 rendition to the ladder without breaking old clients or double-storing everything. Feature-flag by client capability, generate AV1 lazily on first request from a capable client, and let old clients keep pulling h264 — coexistence, not a big-bang re-transcode of the entire catalog.

**H4.** Observability: transcode queue depth + p99 transcode latency, CDN hit ratio (the single most cost-sensitive metric), feed-read p99, "broken post" rate (feed items whose renditions 404 — should be ~0), fanout lag. Alert threshold reasoning: a dip in CDN hit ratio from 98→95% *triples* origin egress.

**H5.** "Undo a bad decision": you stored photos as Postgres BLOBs in v1 (like the naive answer in Q1). Migrate 100PB to an object store live. Dual-write new uploads to object store, lazily migrate on read (copy-on-access) + a background backfill, flip the read path per-photo once the blob key exists, then drop the BLOB columns — all without downtime or serving 404s.

---

### Mentor's Closing Notes

**Top 3 things most candidates get wrong on this topic:**
1. Routing photo bytes through the app tier instead of pre-signed direct-to-object-store upload and CDN reads.
2. Not gating fanout on transcode completion → followers see posts with broken/absent images.
3. Overwriting renditions in place, causing stale-CDN bugs — renditions must be immutable and versioned.

**The one insight that makes an answer truly impressive:**
Instagram is two loosely-coupled systems with a state machine between them: a fast, small **metadata/feed plane** and a slow, huge **binary plane**, joined by the `uploading→transcoding→ready` lifecycle. Every hard problem — broken posts, re-transcode, privacy on the CDN, cost — is really a question of *keeping those two planes consistent* through that state machine, with immutability and idempotency as the enforcement tools.

**Suggested follow-up reading:**
- "What Powers Instagram: Hundreds of Instances, Dozens of Technologies" (Instagram Engineering).
- Meta's TAO paper ("TAO: Facebook's Distributed Data Store for the Social Graph"); Netflix/YouTube transcoding pipeline writeups for the ABR ladder.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
