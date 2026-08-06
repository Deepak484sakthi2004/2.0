# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 48 of 63 — Phase 2: System Design Track (Design 20 of 35)
**Topic:** YouTube — Video Upload, Processing, and Streaming
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
YouTube ingests ~500+ hours of video *every minute* and serves billions of hours of watch time daily, which decomposes into three loosely-coupled problems: a resumable **upload** path for huge files over flaky networks, an offline **transcode pipeline** that turns one source file into a ladder of resolutions/codecs/segments, and a **streaming** path (adaptive bitrate over CDN) that serves petabytes at the edge. What makes it hard: the transcode is a massively parallel batch job measured in CPU-hours per video, the storage is a hot/cold problem where 90% of watch time hits <10% of videos (heavy tail), and the streaming must adapt per-viewer to bandwidth changes mid-playback without stalls (rebuffer ratio is *the* quality metric).

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**CDN / edge caching (taught in Phase 1, Lesson 18):** A geographically distributed cache of static content near users; origin offload + low RTT. YouTube pushes video segments to edge caches (Google's own CDN + ISP-embedded caches / peering). The entire streaming path is "serve immutable segments from the nearest edge"; origin (blob store) is only hit on a cold miss.

**GFS/Hadoop/MapReduce/Spark — batch processing (taught in Phase 1, Lesson 15):** Split a big job into independent chunks processed in parallel across a worker pool, then combine. Transcoding is exactly this: split the source into GOP-aligned chunks, transcode each chunk on a separate worker, reassemble — embarrassingly parallel batch compute.

**Kafka / event-driven (taught in Phase 1, Lesson 21):** Durable job queue decoupling upload from processing. An uploaded video emits an event; a fleet of transcode workers consumes it. Retries, backpressure, and DLQ for failed encodes all come from this.

**Object/blob storage + tiered storage (taught in Phase 1, Lessons 15/22):** Cheap, durable, virtually infinite storage for immutable blobs, with hot/warm/cold tiers. Source masters and rendition segments live in blob storage; cold videos migrate to cheaper archival tiers.

**Consistent hashing (taught in Phase 1, Lesson 19):** Distribute blobs/metadata across nodes with minimal remap on scaling. Used to shard video metadata and to place segments across storage nodes.

**Caches / Redis (taught in Phase 1, Lesson 24):** Hot metadata (video → manifest, view counts, watch state) in memory. View counting and the "resume where you left off" state ride Redis.

**Capacity estimation (taught in Phase 1, Lesson 28):** Ingest rate → transcode CPU-hours → storage growth; watch QPS → egress bandwidth → CDN sizing. We compute all below.

> **Recap box:**
> - CDN edge = the streaming path; origin blob store only on cold miss.
> - Transcode = MapReduce-style chunked parallel batch job.
> - Kafka = durable transcode job queue with retries/DLQ.
> - Blob store + tiered storage = immutable masters + renditions, hot→cold.
> - Heavy tail: 90% of watch time on <10% of videos drives all caching/tiering decisions.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Why is video upload decoupled from video processing, and what does "your video is processing" actually mean?
> **What a strong answer covers:** Upload writes the raw source master to blob storage and returns fast; processing (transcoding into multiple renditions) is a separate, slow, async batch job triggered via a queue. Decoupling means the user isn't blocked for the minutes-to-hours of encode time, uploads and encodes scale independently, and a transcode failure can retry without re-uploading. "Processing" = the source is stored, an event is on the queue, and workers are producing the resolution ladder (144p→4K), generating thumbnails, running content-ID/moderation, and writing the HLS/DASH manifest. The video becomes watchable progressively (lower resolutions first).
> **Common weak answer:** "Upload directly into a database / process synchronously" — a multi-GB blob doesn't belong in a DB, and synchronous transcode ties up the request for hours.
> **Mentor follow-up if they answer well:** How do you let a video be *watchable* before all renditions finish encoding?

Q2. Estimate ingest volume and storage growth. Assume 500 hours uploaded/minute, source ~1 GB per 10-min video-equivalent, and a 5-rendition ladder that roughly doubles total stored bytes.
> **What a strong answer covers:** 500 hours/min = 30,000 hours/hour = 720,000 hours/day. In 10-min units that's ~4.3M source videos/day. At ~1GB per 10 min → ~4.3 PB/day of *source*. Renditions (144p/360p/480p/720p/1080p/4K ladder) add roughly 1–2× the source size depending on ladder → call it ~10 PB/day of *new stored bytes*. Over a year that's multi-exabyte growth — which is exactly why tiered/cold storage and dedup matter. Transcode compute: if each video-hour takes ~a few CPU-hours to encode the full ladder, 720K hours/day × (few) = **millions of CPU-hours/day**, i.e., a large elastic worker fleet.
> **Mentor follow-up:** Of that stored data, what fraction is ever watched again? How does that change your storage tier strategy?

Q3. Would you serve video by streaming whole files or in segments? Defend the choice.
> **What a strong answer covers:** Segments. Each rendition is chopped into small (2–10s) independently-downloadable segments (HLS `.ts`/fMP4 or DASH), described by a manifest (`.m3u8`/MPD). This enables **adaptive bitrate (ABR)**: the player picks the next segment's quality based on measured bandwidth/buffer, switching mid-stream without re-requesting the whole file. Segments are immutable → perfectly CDN-cacheable. Seeking jumps to a segment boundary. Whole-file serving can't adapt to bandwidth changes, wastes bytes on abandoned views, and caches poorly.
> **Red flag answer:** "Serve one MP4 via HTTP range requests at a fixed resolution" — no adaptation, stalls on bandwidth drops, and one giant object per video hurts cache granularity.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Draw the end-to-end architecture: upload → transcode → publish → stream.
> **Key components expected:** Client, resumable Upload service, Blob store (source masters), Kafka job queue, Transcode orchestrator + worker fleet (splitter/encoders/assembler), Thumbnail + moderation/Content-ID services, Rendition blob store, Manifest generator, Metadata DB, CDN (edge caches), Player, View-count/analytics pipeline.
> **Architecture diagram (text):**
```
  Uploader ──resumable(chunked)──▶ ┌───────────────┐   store source
                                   │ Upload Service│───────────────▶ ┌──────────────┐
                                   └──────┬────────┘                 │  Blob Store  │
                                          │ emit "new_video" event    │  (masters)   │
                                          ▼                           └──────┬───────┘
                                   ┌───────────────┐                         │ read source
                                   │  Kafka  queue │                         ▼
                                   └──────┬────────┘              ┌────────────────────┐
                                          │ consume              │ Transcode Pipeline  │
                                          ▼                      │  ┌───────┐          │
                              ┌──────────────────────┐           │  │Splitter│ GOP     │
                              │ Transcode Orchestrator│──────────▶│  └───┬───┘ chunks   │
                              │  (DAG per video)      │           │  ┌───▼────────────┐ │
                              └──────────┬────────────┘           │  │ Encoder workers│ │ (parallel:
                                         │                        │  │ per rendition  │ │  144p..4K)
              thumbnails / Content-ID /  │                        │  └───┬────────────┘ │
              moderation (parallel)      │                        │  ┌───▼───┐          │
                                         │                        │  │Assembler+segment│
                                         ▼                        │  └───┬───┘          │
                                 ┌───────────────┐                └──────┼──────────────┘
                                 │  Metadata DB  │◀── manifest,          │ write segments
                                 │ (video, status│    status             ▼
                                 │  manifest ptr)│              ┌──────────────────┐
                                 └───────┬───────┘              │ Rendition store  │
                                         │ read manifest         │ + Manifest gen   │
             Player ──GET manifest──▶ ┌──┴─────┐                └────────┬─────────┘
                     ──GET segments──▶│  CDN    │◀── origin pull ────────┘
                                      │ (edge)  │
                                      └─────────┘  ──▶ View/analytics ──▶ Kafka ──▶ counters
```
> **What separates SDE2 from SDE3 here:** SDE2 draws the linear pipeline. SDE3 models transcoding as a **DAG of tasks per video** (split → N parallel encodes → per-rendition segmentation → manifest assembly → publish), handles **partial publish** (make 360p/720p watchable while 4K is still encoding), and separates the *control plane* (orchestrator tracking task state) from the *data plane* (workers moving PB of bytes). SDE3 also puts moderation/Content-ID as a *gate* before public availability, not an afterthought.

Q5. Trace what happens when a creator uploads a 2 GB video on a flaky connection.
> **Expected trace:**
> 1. Client requests an upload session: `POST /v1/uploads` → server returns an `upload_id` + a resumable URL.
> 2. Client uploads in **chunks** (e.g., 8–32 MB) with `Content-Range` headers; each chunk is stored and acked. On network drop, client queries bytes-received and **resumes from the last committed offset** (no restart).
> 3. On final chunk, upload service assembles the source master in blob storage, computes a content hash (for dedup), sets video status = `uploaded`, and emits `new_video` to Kafka.
> 4. Transcode orchestrator builds the DAG: splitter cuts the source at GOP/keyframe boundaries into chunks; encoder workers transcode each chunk into each rendition in parallel; thumbnails + Content-ID + moderation run concurrently.
> 5. As each rendition's segments finish, the manifest generator writes/updates the `.m3u8`/MPD; status flips to `playable` for available renditions.
> 6. When moderation passes and renditions are ready, status = `published`; segments are pushed/pullable to CDN; view path goes live.
> **Tricky part:** Candidates skip the **GOP-aligned split**. You can't cut a video at an arbitrary byte — segments must start on a keyframe (IDR frame) or the decoder can't start there. The splitter is codec-aware. Also: resumable upload requires the server to track committed byte offset per upload_id durably, not in memory.

Q6. Design the key APIs across upload, status, and playback.
> **Expected API design:**
> - `POST /v1/uploads` → `{upload_id, resumable_url}`; `PUT resumable_url` with `Content-Range: bytes X-Y/Total` (chunked, resumable); `GET resumable_url` returns received range for resume.
> - `GET /v1/videos/{id}/status` → `{state: uploaded|processing|playable|published|failed, ready_renditions[]}`.
> - `GET /v1/videos/{id}/manifest.m3u8` → adaptive manifest listing rendition playlists (CDN-served).
> - `GET /cdn/.../seg_00042.ts` → immutable segment (long cache TTL, edge-served).
> - `POST /v1/videos/{id}/views` (or fire-and-forget beacon) for view/heartbeat analytics.
> **What to push on:** (1) **Resumability + idempotency** — re-`PUT`ting an already-committed chunk must be a no-op (offset-based). (2) **Immutability + cache headers** — segments get `Cache-Control: max-age=1yr, immutable`; the manifest gets a short TTL so new renditions appear. (3) **Manifest versioning** for codec/DRM negotiation. (4) View endpoint must be cheap + dedup'd (heartbeat, not per-frame).

---

### Data Modeling (Medium–Hard)

Q7. Design the video metadata model and how segments/renditions are referenced.
> **Expected schema:**
```sql
-- Video metadata: read-heavy, needs rich queries → SQL (sharded) or wide-column
CREATE TABLE videos (
  video_id     bigint PRIMARY KEY,   -- Snowflake-style, time-sortable
  channel_id   bigint,
  title        text,
  description  text,
  duration_ms  bigint,
  status       text,                 -- uploaded/processing/playable/published/failed
  visibility   text,                 -- public/unlisted/private
  source_uri   text,                 -- blob path to master
  manifest_uri text,                 -- blob/CDN path to .m3u8/MPD
  created_at   timestamp,
  published_at timestamp
);

-- Renditions: one row per (video, quality/codec)
CREATE TABLE renditions (
  video_id   bigint,     -- partition key
  rendition  text,       -- e.g., "1080p-h264", "720p-vp9", "2160p-av1"
  width int, height int, bitrate_kbps int,
  segment_count int,
  base_uri   text,        -- prefix for segments
  state      text,        -- encoding/ready/failed
  PRIMARY KEY (video_id, rendition)
);

-- Transcode task state (control plane) — the DAG
CREATE TABLE transcode_tasks (
  video_id bigint, task_id uuid,
  kind text,          -- split/encode/segment/manifest/thumbnail/contentid
  rendition text, chunk_index int,
  state text,         -- pending/running/done/failed, attempts int
  worker_id text, updated_at timestamp,
  PRIMARY KEY (video_id, task_id)
);
```
```
# Hot metadata cache (Redis)
video:{id}:manifest -> manifest URI + ready renditions   # so player resolves fast
video:{id}:views    -> approximate counter (HyperLogLog / sharded counter)
watch:{user}:{video}-> last_position_ms                  # resume playback
```
> **Index choices and why:** Videos indexed by channel_id (list a channel's uploads) and by published_at for recency feeds. Renditions clustered by video_id → one-partition read to build a manifest. Transcode_tasks partitioned by video_id so the orchestrator reads/updates a whole video's DAG in one partition.
> **Partitioning key and why:** video_id everywhere for co-location of a single video's renditions/tasks/segments; channel_id secondary index for channel pages. Segments themselves aren't in the DB — they're blob objects named by `{video_id}/{rendition}/seg_N`, referenced by the manifest.

Q8. A hugely popular video is being watched by 50M concurrent viewers. How do you serve segments efficiently?
> **Expected answer:** This is a **CDN cache-hit problem**, not a DB problem. The manifest and all segments are immutable and named deterministically, so the first viewer in a region pulls a segment from origin into the edge; the next 49,999,999 in that region are pure edge cache hits — origin sees ~one request per segment per edge PoP, not 50M. Use tiered CDN (edge → regional shield → origin) so even edge misses collapse at the shield before hitting origin blob store. The player fetches the manifest (short TTL, also cached) then segments (year-long TTL). View counting is decoupled: fire-and-forget beacons aggregated async (Redis sharded counters / HLL), never a synchronous DB write per view.
> **Trap:** Counting a view with a synchronous DB increment per viewer — 50M writes hammering a row is a classic hot-key meltdown. Views are approximate and batched.

Q9. Where do you accept eventual consistency, and where must it be strong?
> **Expected answer:** Eventual is fine for: **view counts** (approximate, reconciled — YouTube deliberately shows lagged/rounded counts), recommendation freshness, thumbnail propagation, and CDN segment propagation (new renditions appearing a bit late). Strong-ish: **video status/visibility** — if a creator sets a video to *private* or it fails moderation, it must *not* be servable; visibility is enforced at the manifest/token-authorization layer, not left to eventual cache. Also **billing/monetization** events (ad impressions, revenue) need at-least-once with dedup. It's an AP system for the read path (availability of playback wins), CP-leaning for access control.
> **Mentor pushback:** A creator deletes a video, but its segments are cached at thousands of edges with year-long TTLs. Eventual consistency won't purge them. → Access is gated by **signed/tokenized URLs** or an authorization check on the manifest; revoke there and the segment cache becomes unreachable even if bytes linger. Plus an explicit **CDN purge/invalidation** for hard takedowns (legal/DMCA).

---

### Low-Level Design (Hard)

Q10. Design the transcoding pipeline to encode a 1-hour 4K source into the full ladder as fast and cheaply as possible.
> **Problem statement:** Turn one large source into ~6 resolutions × multiple codecs (H.264/VP9/AV1) × segmented output, minimizing wall-clock latency and CPU cost, resilient to worker failure.
> **Naive solution:** One worker encodes the whole file sequentially, rendition by rendition.
> **Why naive fails at scale:** A single-machine full-ladder encode of a 1-hour 4K video (especially AV1, which is ~10× slower than H.264) can take *many hours* — unacceptable latency, and one machine failure loses all progress.
> **Expected optimal approach:** **Chunked parallel transcode (MapReduce-style, Lesson 15).** Splitter cuts the source at keyframe/GOP boundaries into N independent chunks (e.g., 30–60s each). Each chunk × each rendition is a task dispatched to the elastic worker fleet — thousands of chunks encode in parallel, so wall-clock ≈ single-chunk encode time regardless of video length. Assembler concatenates encoded chunks per rendition and segments them for HLS/DASH; manifest generator writes playlists. Codecs chosen by tier: fast H.264 for immediate availability, then VP9/AV1 for popular videos where the bandwidth savings pay off. Two-pass encoding for quality on important renditions.
> **Pseudo-code or class diagram:**
```
transcode(video):
    chunks = splitter.split(source, boundary=KEYFRAME, target=45s)   # GOP-aligned
    dag = DAG()
    for c in chunks:
        for r in RENDITION_LADDER:               # 144p..2160p × codecs
            dag.add_task(encode, chunk=c, rendition=r)
    dag.on_all(encode) -> for r in RENDITION_LADDER:
        dag.add_task(assemble_and_segment, rendition=r)
    dag.on_all(assemble) -> dag.add_task(write_manifest)
    dag.add_task(thumbnails); dag.add_task(content_id); dag.add_task(moderation)  # parallel
    orchestrator.run(dag)                          # dispatch to worker pool via Kafka

encode_worker.on(task):
    src_chunk = blob.get(task.chunk)
    out = ffmpeg(src_chunk, task.rendition, two_pass=task.rendition.important)
    blob.put(out, key(task)); task.done()          # idempotent by deterministic key

# Progressive publish: as soon as (360p,720p) assemble+manifest done -> status=playable
```

Q11. Concurrency/failure: an encoder worker dies mid-chunk, or the same chunk gets dispatched to two workers on a Kafka rebalance.
> **Scenario:** Worker W1 picks up chunk 17/rendition 720p, starts encoding (2 min job), then crashes at 90%. Meanwhile a consumer-group rebalance redelivers chunk 17 to W2. Now two workers may both write chunk 17's output.
> **Expected fix:** Tasks are **idempotent** via deterministic output keys: both workers write to the same blob path `{video_id}/720p/chunk_17`, so a duplicate is a harmless overwrite of identical content (encoding is deterministic given the same input + params). The orchestrator tracks task state with **attempts** and only advances the DAG when a chunk's output object exists — it doesn't matter *which* worker produced it. Kafka offset is committed only after the output is durably written, so a crash before commit → redelivery → re-encode (at-least-once). A per-chunk lease/heartbeat lets the orchestrator reclaim a chunk from a silent worker after a timeout.
> **Follow-up:** What if a chunk *repeatedly* fails (corrupt segment / unsupported codec feature)? After N attempts → DLQ; mark that rendition `failed` but still publish the renditions that succeeded (a video with 720p/1080p but no AV1 is fine). Don't let one poison chunk block the whole video.

Q12. Failure deep-dive: the CDN origin (blob store region) has an outage during a traffic spike on a viral video.
> **Scenario:** A newly viral video's less-common renditions aren't yet widely cached; an origin region goes down, causing edge misses to fail to fetch from origin.
> **Expected handling:** (1) **Multi-region origin + shield tier** — segments are replicated across regions; edge falls back to a healthy origin region on failure. (2) **Cache the hot set aggressively** — for a viral video, pre-warm/push segments to edges (predictive pre-positioning) so origin dependency drops to near zero. (3) **Graceful ABR degradation** — if a high rendition's segments are unfetchable, the player's ABR logic falls back to a lower rendition that *is* cached rather than stalling. (4) Circuit-break origin fetches (Lesson 10) and serve stale-while-revalidate from edge where possible. (5) For the *upload/transcode* side, an origin outage just delays processing (jobs stay durably queued in Kafka) — no data loss, it catches up.

---

### Scaling to 10x / 100x (Hard)

Q13. At 10x, where does YouTube break first?
> **Expected answer:** **Egress bandwidth / CDN capacity** on the read path and **transcode compute** on the write path — in that order, because reads vastly outnumber writes. Watch egress is the dominant cost and capacity constraint (petabits/sec globally); a 10x in watch time is a 10x in edge bandwidth and cache footprint. Transcode is elastic batch compute — you scale the worker fleet — but 10x ingest = 10x CPU-hours, and AV1/4K encoding is CPU-brutal. Metadata DB is *not* the first bottleneck (it's small relative to bytes).
> **Numbers to ground the answer:** From Q2, ~10 PB/day new storage and millions of CPU-hours/day for transcode at 1x. At 10x that's ~100 PB/day storage growth (forces aggressive tiering/dedup) and tens of millions of CPU-hours/day (forces hardware encoders/ASICs — Google built VCUs, video-transcode ASICs, exactly for this). Egress: billions of hours watched × ~a few Mbps → sustained multi-Tbps, requiring ISP-embedded caches and peering, not just cloud CDN.

Q14. How do you shard storage and metadata, and where are the hotspots?
> **Expected sharding strategy:** Segments/blobs: content-addressed or `video_id`-prefixed and spread across storage cells via consistent hashing; renditions of one video co-located for locality but the *serving* path is CDN, so origin sharding mainly affects cold reads. Metadata: shard videos by video_id (Snowflake) for even writes; secondary index by channel_id. Transcode tasks by video_id.
> **Hot spot problem:** A **viral video** is a read hotspot — but CDN edge caching absorbs it (one origin fetch per segment per PoP). The real origin hotspot is the *transition window* before a video is widely cached; fix with **predictive pre-warming** based on early view velocity. On the write side, a **channel that mass-uploads** (or a coordinated upload flood) is a transcode-queue hotspot — fair-share scheduling across channels so one uploader can't starve the fleet. Hot metadata key (a viral video's manifest) → cache in Redis + edge, and it's immutable-ish so it caches trivially.

Q15. Design the caching layers and the hardest invalidation problem.
> **Expected layered cache design:** L1 = player-local buffer + browser cache (segments cached client-side during playback). L2 = ISP-embedded / edge CDN caches (the bulk of egress, immutable segments, year-long TTL). L3 = regional shield caches (collapse edge misses). Origin = blob store (source of truth). Metadata: Redis hot cache (manifest URIs, watch position, view counts) in front of the metadata DB.
> **Cache invalidation trap:** **Re-encoding / replacing a rendition or a takedown.** Segments are cached immutably at thousands of edges with long TTLs; if you re-encode 1080p (bug fix, better codec) you can't wait for TTL expiry. Solution: **content-addressed / versioned segment URLs** — a new encode gets a new path/version, the manifest points to the new URLs, and old cached segments simply age out (nobody references them). For hard takedowns (DMCA/legal), you *do* issue an explicit CDN purge/invalidation *and* revoke the manifest authorization so even un-purged edge bytes are unreachable. The manifest's short TTL is the pivot point — it's the one mutable thing, so it controls what segments are reachable.

Q16. How do you keep this cost-efficient at scale?
> **Expected answer:** (1) **Tiered storage** — 90% of watch time hits a tiny hot set, so keep only hot videos on fast storage and migrate the long tail to cold/archival tiers; the master source of a never-watched video can go to the cheapest tier. (2) **Lazy/on-demand rendition generation** — don't encode AV1/4K for videos that get 3 views; generate expensive codecs only for videos that gain traction, saving enormous transcode CPU. (3) **Better codecs where it pays** — VP9/AV1 cut bandwidth ~30–50%, and since egress is the #1 cost, encoding popular videos in AV1 saves more than it costs. (4) **Hardware transcoding (ASICs/VCUs)** — orders-of-magnitude cheaper per encode than general CPUs at YouTube scale. (5) **Dedup identical uploads** via content hash. (6) **Segment-level caching** so abandoned views (most views watch <30s) only cost the segments actually fetched.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Explain **adaptive bitrate (ABR)** decision-making inside the player: how does it choose the next segment's quality? (Answer: buffer-based + throughput-based hybrid — estimate available bandwidth from recent segment download times, keep a target buffer level, step down a rendition when buffer drains or throughput drops, step up cautiously to avoid oscillation. Discuss the rebuffer-vs-quality trade-off and why segment granularity (2–10s) bounds reaction time.) Why can't the *server* make this decision?

**H2.** DRM and content protection across the E2E pipeline: how do you deliver premium/paid content so it can't be trivially ripped, and how does that interact with CDN caching? (Answer: encrypted segments (AES-128 / CENC), keys served via a licensing service (Widevine/FairPlay/PlayReady) separate from segments; the *ciphertext* segments are still CDN-cacheable, only the per-session key is gated. Discuss the tension: caching wants shared bytes, DRM wants per-user control → solved by shared encrypted segments + per-session keys.)

**H3.** Roll out a new codec (AV1) or a re-encode of the entire catalog with **zero disruption** to current viewers. (Answer: additive/versioned renditions — add AV1 as new rendition paths, update manifests to advertise it, let capable players negotiate it; old H.264 renditions stay until AV1 coverage is high; feature-flag by device capability; backfill the long tail lazily by view velocity rather than all at once.)

**H4.** What do you instrument? **Rebuffer ratio** (the #1 QoE metric — % of playback time spent stalling), startup latency (join time / time-to-first-frame), ABR switch rate, CDN cache-hit ratio per PoP, origin egress, transcode queue depth + per-stage latency, transcode failure/DLQ rate, cost-per-encoded-hour. The single SLO that matters for watch experience: **rebuffer ratio + join time p95**.

**H5.** "Undo a bad decision": you launched encoding *every* upload into the full 6-rendition × 3-codec ladder eagerly, and transcode compute cost is now unsustainable because most videos are barely watched. Migrate to lazy/tiered encoding without breaking playback. (Answer: encode a minimal base ladder (360p/720p H.264) eagerly for instant playability; generate higher/expensive renditions on-demand triggered by view thresholds; keep the source master so any rendition can be produced later; monitor for the rare case a video goes viral before its high renditions exist and prioritize its encode.)

---

### Mentor's Closing Notes

**Top 3 things most candidates get wrong on this topic:**
1. Conflating the three separate systems — upload, transcode, and streaming have completely different scaling profiles (network+durability, batch compute, egress+caching) and must be decoupled.
2. Missing that streaming is fundamentally a **CDN cache-hit** problem — they try to scale the origin/DB for reads instead of pushing immutable segments to the edge.
3. Encoding everything eagerly and treating storage/compute as free — ignoring the heavy-tail (90/10) that makes lazy encoding and tiered storage the dominant cost levers.

**The one insight that makes an answer truly impressive:**
The whole system is organized around **immutability + heavy-tail economics**. Segments are immutable → CDN solves reads almost for free; the catalog is heavy-tailed → you should *never* do expensive work (4K/AV1 encode, hot storage) eagerly for content that won't be watched. Great answers make every decision — encode lazily, tier storage, version segment URLs instead of invalidating — fall out of those two facts.

**Suggested follow-up reading:**
- Google's "Warehouse-scale video transcoding with VCUs" (Argos ASIC) paper and the YouTube engineering blog on transcoding at scale.
- Apple HLS / MPEG-DASH specs and Netflix's "Per-Title / Per-Shot Encoding" and ABR writeups (directly applicable QoE + encoding-efficiency material).

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
