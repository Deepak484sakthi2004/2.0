# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 49 of 63 — Phase 2: System Design Track (Design 21 of 35)
**Topic:** Netflix — Video Streaming Platform + CDN (Open Connect)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Netflix streams ~250 million subscribers and, at peak, is responsible for roughly 15% of global downstream internet traffic. The hard part is not the app — it is delivering multi-terabit video at sub-second start times to devices on flaky home Wi-Fi, while paying almost nothing per delivered gigabyte. Netflix solved this with its own CDN (Open Connect), adaptive bitrate streaming (ABR), and a per-title encoding pipeline. This session is about the streaming data plane and the control plane that feeds it — not recommendations.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**CDN & edge delivery (taught in Phase 1, Lesson 18):** A CDN caches content close to users so bytes travel the shortest network path. Push CDNs pre-position content; pull CDNs cache on first miss. Netflix runs a *push* CDN (Open Connect Appliances, OCAs) embedded inside ISP networks and at internet exchange points (IXPs). The catalog is proactively filled overnight during off-peak hours. Today this is the entire delivery backbone: 90%+ of bytes are served from an OCA one network hop from the user.

**DNS & Anycast/GeoDNS (taught in Phase 1, Lesson 7):** DNS resolves a name to an IP; GeoDNS and Anycast route users to nearby endpoints. Netflix does NOT use DNS-based steering for the video itself — the control plane (`api.netflix.com`) uses GeoDNS to hit the nearest AWS region, but the *OCA is chosen by an application-level steering service* that returns ranked URLs. Understand the split: DNS gets you to the brain, the steering service picks the muscle.

**Caching / cache hierarchy (taught in Phase 1, Lesson 24):** LRU/LFU eviction, TTLs, hot vs cold data. Open Connect is fundamentally a giant tiered cache: SSD-based "flash" OCAs hold the hot head of the catalog (most-watched titles), HDD-based "storage" OCAs hold the long tail. Fill logic is a popularity-ranked prefetch, not demand-driven LRU.

**Consistent hashing (taught in Phase 1, Lesson 19):** Maps keys to nodes with minimal reshuffling on membership change. Used to distribute chunk requests across a cluster of OCAs at a site, and inside the encoding pipeline to shard the work queue.

**Kafka / event-driven (taught in Phase 1, Lesson 21):** Durable partitioned log for decoupled producers/consumers. Netflix runs one of the largest Kafka deployments on earth (Keystone pipeline, trillions of events/day) for playback telemetry, QoE metrics, and billing events. The client heartbeats playback state; those events drive ABR analytics and the "continue watching" bookmark.

**Object storage & durability (taught in Phase 1, Lesson 15 — GFS lineage):** S3 as the source-of-truth blob store with 11 nines durability. Master mezzanine files and all encoded renditions live in S3; OCAs pull fills from S3 over the fill network.

**Microservices / API gateway (taught in Phase 1, Lesson 9):** Zuul is Netflix's edge gateway; hundreds of backend microservices behind it. The playback path is a specific, latency-critical subset (playback API, license/DRM, steering).

> **Recap box:** CDN = push OCAs inside ISPs (L18). DNS gets you to the control plane only (L7). Open Connect is a popularity-prefetched tiered cache (L24). Consistent hashing shards OCAs and encoding work (L19). Kafka/Keystone carries playback telemetry (L21). S3 is source of truth (L15). Zuul is the edge gateway (L9).

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Explain adaptive bitrate streaming (ABR). Why does Netflix chop a movie into segments and encode each title at ~10–15 bitrates?
> **What a strong answer covers:** The master (mezzanine) is transcoded into a *bitrate ladder* — e.g., 235 kbps@320x240 up to 15+ Mbps@4K. Each rendition is split into 2–10 second segments aligned on the same GOP/keyframe boundaries so a player can switch renditions at any segment boundary without artifacts. A manifest (DASH MPD or HLS m3u8) lists all renditions and segment URLs. The client's ABR algorithm measures throughput and buffer occupancy and picks the highest sustainable rate per segment. Mentions HTTP-based delivery so any CDN/proxy works and segments are cacheable objects.
> **Common weak answer:** "It changes quality based on your internet" with no mention of segment alignment, manifests, buffer-based decisions, or that each rendition is a fully separate encode.
> **Mentor follow-up if they answer well:** Buffer-based (BOLA) vs throughput-based ABR — which does Netflix lean on and why does pure throughput estimation oscillate on shared home Wi-Fi?

Q2. Back-of-envelope: 250M subscribers, average 2 hours/day streaming at an average 5 Mbps. Estimate peak egress bandwidth and yearly bytes served.
> **What a strong answer covers:** Concurrent streams at peak ≈ not 250M — estimate peak concurrency. 250M × 2h/day = 500M viewing-hours/day. Spread unevenly; peak-hour concurrency maybe 10–15% of subs in prime regions → assume ~30M concurrent streams at peak. 30M × 5 Mbps = 150 Tbps peak egress. Yearly bytes: 500M h/day × 3600s × 5 Mbps ≈ 500e6×3600×5e6 bits/day = 9e18 bits/day ≈ 1.1 EB/day → ~400 EB/year. The point: this is impossible to serve from AWS egress economically → hence Open Connect. Good candidates immediately tie the number to *why* the CDN exists.
> **Mentor follow-up:** At 150 Tbps, what fraction must be served from inside ISP networks for the transit bill to be survivable, and what does that imply for OCA placement?

Q3. Where do you store the actual video bytes vs the metadata (title name, cast, artwork, watch progress)? Why different stores?
> **What a strong answer covers:** Video bytes → S3 as source of truth, then pushed to OCAs (immutable, huge, sequential reads, served over HTTP). Metadata → a mix: catalog metadata in a document/relational store cached hard (Netflix uses Cassandra + EVCache heavily), watch progress/bookmarks in Cassandra (write-heavy, per-user, eventually consistent is fine). Separation because access patterns differ wildly: video is immutable cold-ish blobs; bookmarks are tiny high-write hot rows.
> **Red flag answer:** "Store the video in the database as a BLOB." Instant fail — video never belongs in an OLTP DB.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Draw the end-to-end Netflix architecture: from a user pressing Play to bytes arriving, and the offline pipeline that made those bytes exist.
> **Key components expected:** Client (ABR player), Zuul edge gateway, Playback API, Steering service (Open Connect Steering), DRM/license service, OCA fleet (flash + storage tiers), Fill network + S3, Encoding/transcoding pipeline (Archer/Cosmos), Cassandra + EVCache, Kafka/Keystone telemetry.
> **Architecture diagram (text):**
```
                          CONTROL PLANE (AWS)
  Client ──TLS──> Zuul ──> Playback API ──> Steering Svc ──> [OCA health/route DB]
    │                          │                    │
    │                          ├──> DRM/License Svc (Widevine/PlayReady/FairPlay)
    │                          ├──> Catalog Svc ──> Cassandra + EVCache
    │                          └──> Bookmark Svc ──> Cassandra
    │
    │  (Steering returns RANKED list of OCA URLs)
    │
    ▼   DATA PLANE (Open Connect, inside ISPs / IXPs)
  Client ══HTTPS segments══> OCA (flash SSD: hot head)
                               │  cache miss / fill
                               ▼
                             OCA (storage HDD: long tail) ──fill──> S3 (source of truth)

  OFFLINE PIPELINE:
  Studio master ─> Ingest ─> Chunked parallel transcode (per-title/per-shot ladder)
                 ─> Validate ─> Package (DASH/HLS + DRM) ─> S3 ─> nightly proactive fill to OCAs

  TELEMETRY: Client ──heartbeat/QoE events──> Kafka (Keystone) ──> Flink/Druid ──> QoE dashboards, ABR tuning
```
> **What separates SDE2 from SDE3 here:** SDE2 draws request→CDN→video. SDE3 separates control plane (AWS) from data plane (Open Connect), knows steering returns a *ranked* list so the client can fail over between OCAs mid-stream, and knows the fill is *proactive and popularity-ranked* rather than lazy pull. SDE3 also notes DRM license acquisition is a separate blocking call before the first segment.

Q5. Trace exactly what happens when a user in Bangalore presses Play on a title, from tap to first frame.
> **Expected trace:**
> 1. Client → Zuul → Playback API with title ID, device profile, DRM capabilities.
> 2. Playback API authorizes (entitlement/subscription check), fetches the manifest set + resolves which bitrate ladder and codecs (AV1/HEVC/H.264) this device supports.
> 3. Playback API calls Steering: "user IP X, ISP Y, title Z" → Steering returns a ranked list of OCA URLs based on OCA proximity (same ISP > IXP > regional), current health/load, and whether that OCA actually holds this title.
> 4. Client requests a DRM license from the license service (device sends CDM challenge → gets content key). This is on the critical path — parallelize it with manifest fetch.
> 5. Client fetches the manifest, then requests the *first* segment at a conservatively low bitrate from the top-ranked OCA to minimize startup delay, filling the buffer.
> 6. ABR ramps up subsequent segments as it measures throughput/buffer. Bookmark service periodically gets progress updates; Kafka gets QoE heartbeats.
> **Tricky part:** Candidates forget the DRM license round-trip and forget that the *first* segment is deliberately low-bitrate to hit a sub-second start. They also forget the OCA-selection input includes "does this OCA actually have this title cached?" — a nearby OCA that lacks the title is useless.

Q6. Design the playback control-plane APIs.
> **Expected API design:**
> - `POST /playback/authorize` → `{titleId, deviceProfile, drmSchemes}` → `{sessionId, manifestUrl, ladder, drmToken}`; validates entitlement.
> - `GET /steering?sessionId&clientIp&titleId` → `{ranked:[{ocaUrl, weight, expiresAt}, ...]}`; short TTL so client can re-steer on OCA degradation.
> - `POST /drm/license` → CDM challenge blob → license/key response.
> - `PUT /bookmark` → `{titleId, positionMs, ts}` idempotent by `(userId,titleId)` last-writer-wins.
> - Manifest itself: DASH MPD / HLS m3u8 served as a cacheable object.
> **What to push on:** Idempotency of bookmark writes (LWW on timestamp — reject stale positions). Versioning the manifest/ladder (device capability changes → new ladder without breaking old clients; version in the URL path). Steering TTL and re-steer semantics (client must gracefully fail over mid-stream without a visible stall). Why steering is a *separate* call from authorize (steering is hot, cache-unfriendly, changes with OCA load; authorize is per-session).

---

### Data Modeling (Medium–Hard)
Q7. Design the data model for the catalog metadata and per-user watch state.
> **Expected schema:**
```sql
-- Catalog (read-heavy, cache-hard). Served from Cassandra + EVCache.
-- Denormalized per title for single-read fetch.
TitleMetadata (
  title_id       UUID PRIMARY KEY,
  type           TEXT,            -- movie | episode
  name           TEXT,
  runtime_ms     BIGINT,
  ladder_version INT,
  codecs         SET<TEXT>,       -- av1, hevc, h264
  drm_schemes    SET<TEXT>,
  artwork        MAP<TEXT,TEXT>,  -- locale -> url
  updated_at     TIMESTAMP
);

-- Watch progress: write-heavy, per user. Cassandra.
-- Partition by user so all of a user's bookmarks colocate.
Bookmark (
  user_id     UUID,
  title_id    UUID,
  position_ms BIGINT,
  updated_at  TIMESTAMP,
  PRIMARY KEY ((user_id), title_id)
) WITH CLUSTERING ORDER BY (title_id ASC);

-- Encoded asset registry: source of truth for what exists in S3 / on OCAs.
AssetRendition (
  title_id   UUID,
  rendition  TEXT,   -- e.g. 'av1-1080p-5000k'
  segment_ct INT,
  s3_prefix  TEXT,
  bytes      BIGINT,
  PRIMARY KEY ((title_id), rendition)
);
```
> **Index choices and why:** Catalog is fetched by `title_id` (PK point read) — no secondary indexes; browse/search is served by a separate search service (Elasticsearch) not by scanning Cassandra. Bookmark clustered by `title_id` under `user_id` gives "all my continue-watching" as one partition read.
> **Partitioning key and why:** Bookmark partitions on `user_id` — colocates a user's writes, and reads for "Continue Watching" hit one partition. Never partition bookmarks by `title_id` (a hit title like Squid Game becomes a single hot partition taking all writes).

Q8. "Continue Watching" must show a user's in-progress titles sorted by most-recent. How do you serve it efficiently?
> **Expected answer:** Don't sort Cassandra by `updated_at` inside the partition (mutable, can't be a clustering key that reorders on update without delete+insert). Maintain a per-user "recent activity" materialized list: on each bookmark write, also upsert into a small `RecentlyWatched(user_id) -> list<title_id, ts>` capped to N (e.g., 100) entries, or keep a Redis sorted set `ZADD cw:{user} ts titleId` and trim. Read path is one ZRANGE + a multi-get of title metadata from EVCache. Eventual consistency is fine — a bookmark showing a few seconds stale is invisible.
> **Trap:** Doing `SELECT * FROM Bookmark WHERE user_id=? ORDER BY updated_at` — Cassandra can only order by clustering columns, and `updated_at` isn't one; you'd end up scanning and sorting in the app for users with thousands of titles. Also trap: a global secondary index on `updated_at` (hotspots, tombstone hell).

Q9. Watch progress and entitlement — where do you demand strong consistency and where is eventual fine? Apply CAP (Lesson 2).
> **Expected answer:** Entitlement/billing state → needs strong-ish consistency (you shouldn't stream after a hard cancel/chargeback), but Netflix tolerates a short window: it's AP-leaning with an authoritative periodic reconciliation; the cost of one extra streamed movie is trivial vs blocking legitimate playback. Watch progress → firmly AP/eventual: Cassandra tunable consistency, writes at `LOCAL_ONE` or `LOCAL_QUORUM`, LWW on timestamp; if two devices update, latest timestamp wins. Catalog → AP with cache; staleness of minutes is fine. The design principle: *availability of playback beats correctness of a bookmark*. Never make the Play button depend on a strongly-consistent global transaction.
> **Mentor pushback:** User watches on the phone (offline, on a plane) to minute 40, then opens TV which had it at minute 10. LWW by timestamp is right *only if* clocks/monotonic session counters are trustworthy. What if the phone was offline and its cached bookmark timestamp is older wall-clock but represents newer progress? Answer: carry a per-session monotonically increasing counter + device clock, and resolve conflicts by "furthest credible position" heuristics, not naive wall-clock LWW.

---

### Low-Level Design (Hard)
Q10. Design the OCA fill and steering: given a title becomes newly popular in a region, how do OCAs get filled and how does a client get routed to one that has it?
> **Problem statement:** OCAs have finite SSD/HDD. You must (a) decide which titles live on which OCA (fill), and (b) at play time return OCA URLs that both are near the user AND hold the requested title AND aren't overloaded.
> **Naive solution:** Lazy pull-through cache — client hits nearest OCA, on miss the OCA fetches from S3 and caches (classic CDN). Round-robin or pure-geo steering.
> **Why naive fails at scale:** A new global release (millions pressing Play in the same hour) would cause a thundering-herd of simultaneous misses → every OCA hammers S3/origin at once → fill network saturates during peak viewing hours (exactly when you have no spare capacity). Pure-geo steering routes users to a near OCA that may not hold the title → miss → origin hit.
> **Expected optimal approach:** Proactive, popularity-ranked fill during the *off-peak fill window* (predicted demand per region from historical + pre-release signals). Titles ranked by predicted regional popularity; hot head → flash SSD OCAs, long tail → storage HDD OCAs, cold → regional origin. Steering is a scoring function over `(proximity, has_title, health, load)` returning a ranked list; client tries #1, fails over to #2 on stall. Use consistent hashing (Lesson 19) to spread a title's segments across multiple OCAs at a site so no single box is the hot spot for a mega-hit.
> **Pseudo-code or class diagram:**
```
def steer(client_ip, title_id):
    site = geo_lookup(client_ip)         # ISP-embedded OCAs first, then IXP, then regional
    candidates = ocas_in_scope(site)     # ordered by network proximity
    scored = []
    for oca in candidates:
        if not oca.has(title_id):        # fill catalog check — critical
            continue
        score = w1*proximity(oca, client_ip) \
              - w2*oca.load_pct \
              - w3*oca.error_rate
        scored.append((score, oca.url_for(title_id)))
    ranked = top_k(sort_desc(scored), k=3)
    if not ranked:                       # nobody near has it -> regional/origin fallback
        ranked = [regional_origin_url(title_id)]
    return ranked  # short TTL so client re-steers on degradation

# Fill (nightly, off-peak):
def plan_fill(region):
    predicted = popularity_model(region)          # ranked titles
    for oca in region.ocas:
        cap = oca.capacity_bytes
        for title in predicted:                   # greedy by predicted demand
            for seg in shard_segments(title, key=consistent_hash):
                if cap - seg.bytes < 0: break
                oca.stage(seg); cap -= seg.bytes
```

Q11. Concurrency: two of a user's devices update the same bookmark near-simultaneously, and the encoding pipeline may double-publish a rendition. Handle both races.
> **Scenario:** (a) Phone writes position=2400s@t1, TV writes position=600s@t2 where t2>t1 but represents older progress; (b) a transcode worker retried and wrote `AssetRendition` twice.
> **Expected fix:** (a) Bookmark is idempotent by `(user_id, title_id)`, conflict-resolved by a monotonic session sequence + "furthest credible position," NOT naive wall-clock — see Q9. Cassandra LWW alone is insufficient; wrap with app-level resolution. (b) Encoding writes are idempotent by a content-addressed key: `rendition_id = hash(title_id, ladder_version, rendition_name)`; the packager writes to a deterministic S3 path, so a retry overwrites the identical object (same bytes) — no duplication. Publish to the asset registry with a conditional "IF NOT EXISTS" (Cassandra LWT) or upsert-by-PK so a double-publish is a no-op.
> **Follow-up:** What if the transcode worker dies mid-job? Chunked transcoding means each *shot/chunk* is an independent idempotent task on a work queue (SQS/Kafka); a dead worker's chunk is redelivered after visibility timeout and re-encoded deterministically. The final "stitch" step only proceeds when all chunks report complete — a barrier keyed on the job manifest.

Q12. Edge case: an OCA inside a major ISP fails at 8pm peak, dropping 500 Gbps of serving capacity instantly. What happens and how is it absorbed?
> **Scenario:** Hardware/PSU failure or the ISP link flaps; thousands of in-flight streams are pinned to that OCA.
> **Expected handling:** Clients hold a *ranked* steering list with short TTL, so on segment failures/timeouts they immediately request from OCA #2 (peer at the same site or IXP) — mid-stream failover is invisible if the buffer (typically tens of seconds) covers the re-request latency. Steering health-checks mark the OCA unhealthy and stop returning it. Load spills to peer OCAs and the IXP tier; if regional capacity is exceeded, ABR gracefully steps down bitrates (serving 720p to more users beats stalling). This is a circuit-breaker + graceful-degradation pattern (Lesson 10): degrade quality, never hard-fail the stream. Keystone telemetry spikes QoE alerts; capacity engineering pre-provisions N+2 headroom at each site precisely for this.

---

### Scaling to 10x / 100x (Hard)
Q13. Where does Netflix break first as you scale — control plane or data plane?
> **Expected answer:** The *data plane* bandwidth is the dominant physical constraint but it's already handled by Open Connect embedding capacity inside ISPs — it scales by shipping more appliances, near-linearly. The first *software* bottleneck under a viral spike is the DRM license service and the steering service (both on the synchronous play path, both hard to cache because they're per-session/per-device). Second is the fill network during off-peak if catalog churn is high. Cassandra bookmark writes scale horizontally, so they're rarely the bottleneck.
> **Numbers to ground the answer:** ~30M concurrent streams at peak → if each new play = 1 license + 1 steering call, a global synchronized release can spike to millions of license/steering RPS in a minute. License service must be regionally sharded, stateless, and pre-scaled; steering results cached per (ISP, title) with short TTL to collapse duplicate requests.

Q14. How do you shard the encoding pipeline and the OCA fleet so no hotspot forms? Apply consistent hashing (Lesson 19).
> **Expected sharding strategy:** Encoding: shard the work by *shot/chunk* — a title splits into hundreds of independently-encodable chunks distributed across a worker fleet via a partitioned Kafka/SQS queue; consistent hashing on `chunk_id` balances load and lets workers scale elastically. OCA content: shard a title's segments across multiple OCAs at a site using consistent hashing on `(title_id, segment_index)` so a mega-hit isn't concentrated on one box; virtual nodes smooth the distribution.
> **Hot spot problem:** A single blockbuster (e.g., a finale) can overwhelm one OCA if the whole title is on one box. Detect via per-OCA egress and error-rate telemetry; fix by (a) fanning the title's segments across peer OCAs (consistent-hash with vnodes), and (b) replicating the hot head onto *all* flash OCAs at the site so any of them can serve it. The long tail stays on fewer boxes.

Q15. Design the caching hierarchy for both metadata and video.
> **Expected layered cache design:**
> - Video: L1 = OCA flash SSD (hot head, sub-catalog), L2 = OCA storage HDD (long tail), L3 = regional origin, source of truth = S3. Fill is push, not pull.
> - Metadata: L1 = client-side manifest/catalog cache; L2 = EVCache (Netflix's memcached-based tier, multi-AZ replicated); backing store = Cassandra. Search served separately by Elasticsearch.
> - Steering/DRM: short-TTL edge caches keyed by (ISP, title) for steering; DRM licenses are per-device and largely uncacheable (intentionally, for security).
> **Cache invalidation trap:** The hard one is *artwork and metadata versioning* combined with the encoded ladder: if you re-encode a title (new AV1 ladder) or swap artwork, in-flight sessions holding an old manifest must not break. Solution: content-address renditions and version the manifest; never mutate an existing rendition's URL — publish a new versioned path and let steering/catalog point new sessions at it, expiring old objects only after sessions drain. Immutable objects make invalidation a non-problem for video; the trap is thinking you can overwrite in place.

Q16. Cost/efficiency: how does Netflix keep per-delivered-GB cost near zero?
> **Expected answer:** (1) Open Connect embeds free-to-Netflix appliances inside ISPs — the ISP saves transit too, so both win; ~90%+ of bytes never touch paid transit. (2) Per-title / per-shot encoding: instead of a fixed bitrate ladder for every title, Netflix analyzes each title's complexity and assigns an optimal ladder — a cartoon needs far fewer bits than an action film for the same quality, cutting average bitrate ~20%. (3) Next-gen codecs: AV1/HEVC cut bytes ~30–50% vs H.264 for the same VMAF quality. (4) Off-peak proactive fill uses cheap idle bandwidth. (5) Tiered storage: flash for hot head, HDD for tail, drop truly cold content back to regional origin. VMAF (perceptual quality metric) lets them minimize bits at a target *perceived* quality rather than a fixed bitrate.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Explain per-shot (per-title) encoding and VMAF. Why is a fixed bitrate ladder wasteful, and how does convex-hull optimization pick the ladder points that maximize quality-per-bit for each individual title?

**H2.** DRM & content protection: walk through Widevine L1 vs L3, hardware-backed key handling, and why the license service must never be cacheable at the CDN. How do you serve 4K only to devices with a trusted hardware CDM while degrading others to 720p?

**H3.** Open Connect deployment: how do you push firmware/catalog updates to tens of thousands of appliances sitting inside third-party ISP networks with no direct access, without disrupting peak-hour serving? (Answer touches: off-peak windows, staged/canary fills, health-gated rollout, the appliance pulling config from control plane rather than being pushed.)

**H4.** Observability: which metrics define streaming quality? Expect the QoE quartet — startup latency (time-to-first-frame), rebuffer ratio (% of viewing time spent stalled), average delivered bitrate/VMAF, and play-delay/error rate. How do you build the Keystone→Flink→Druid pipeline to alert on regional rebuffer spikes within minutes and attribute them to a specific ISP or OCA?

**H5.** 'Undo a bad decision': you shipped a new ABR algorithm that improved average bitrate but increased rebuffers 3% for low-bandwidth users. How do you detect it (A/B via QoE metrics), roll it back safely behind a feature flag without a client redeploy, and design future ABR changes to be server-tunable rather than baked into the client binary?

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Treating Netflix as "just a CDN + video player." They miss the control-plane / data-plane split and that Open Connect is a *push* CDN embedded in ISPs, not a generic pull CDN.
2. Forgetting the offline encoding pipeline entirely. The reason streaming is cheap and fast is per-title encoding + proactive popularity-based fill — that's the actual engineering, and it happens hours before any user presses Play.
3. Over-engineering consistency for bookmarks and under-engineering it for the play path's DRM/steering. Bookmarks are eventual; the license round-trip is the real latency risk on time-to-first-frame.

**The one insight that makes an answer truly impressive:**
Netflix optimizes for *perceived* quality per bit, not bitrate. Mentioning VMAF-driven per-shot encoding with convex-hull ladder selection — and that the "bitrate ladder" is title-specific, not universal — signals you understand the economics that make 150 Tbps affordable.

**Suggested follow-up reading:**
- Netflix TechBlog: "Open Connect" architecture + "Per-Title Encode Optimization" and "Optimized shot-based encodes."
- Netflix TechBlog: "VMAF: The Journey Continues" and the Keystone real-time data pipeline posts.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
