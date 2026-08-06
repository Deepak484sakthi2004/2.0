# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 50 of 63 — Phase 2: System Design Track (Design 22 of 35)
**Topic:** Live Streaming Platform (Twitch) — low-latency ingest, transcode, fan-out + live chat
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Netflix (yesterday) is VOD: bytes exist before you watch. Twitch is the opposite — a streamer's webcam is being encoded *right now*, and a million viewers must see it a few seconds later while typing in a chat that flies past at hundreds of messages per second. The hard parts are glass-to-glass latency (ideally 2–5 seconds), live transcode fanout at unpredictable scale (a small streamer suddenly goes viral), and a real-time chat system that is itself a massive fan-out problem. This is the canonical "live" system design.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**CDN & edge delivery (taught in Phase 1, Lesson 18):** Same edge-caching idea as Netflix but the object is *ephemeral*: HLS/LL-HLS segments generated live, cached at the edge for seconds. Push vs pull matters less than segment TTL and cache-key design. Used to fan out one encoded stream to millions of viewers without every viewer hitting origin.

**WebSockets & long-lived connections (taught in Phase 1, Lesson 7 networking):** Chat and low-latency signaling need persistent bidirectional connections, not request/response polling. Used for the chat firehose and (in LL variants) for playback control. Contrast with HTTP-chunked video delivery which stays request/response for cacheability.

**Kafka / event-driven pub-sub (taught in Phase 1, Lesson 21):** Durable partitioned log for decoupling. Used for chat message fan-out backbone, moderation events, and telemetry (viewer counts, bitrate, dropped-frame metrics). Chat is fundamentally a pub-sub topic-per-channel problem.

**Load balancers (taught in Phase 1, Lesson 6):** L4/L7 distribution. Ingest uses L4 to route RTMP/SRT streams to ingest servers; chat uses connection-aware L7/consistent routing so a channel's subscribers land coherently.

**Consistent hashing (taught in Phase 1, Lesson 19):** Maps channels → chat servers / transcode workers with minimal churn on scale events. A channel's chat subscribers should converge on the same edge chat server set.

**Caching hierarchy (taught in Phase 1, Lesson 24):** For live, the "cache" is the segment buffer — edge holds the last few segments; there's also a rolling DVR buffer. Short TTLs, aggressive edge collapsing of duplicate requests (thundering herd on segment N).

**Message queues & backpressure (taught in Phase 1, Lesson 21/22):** When chat rate exceeds a client's ability to render, you must drop/coalesce, not block. Backpressure and load-shedding are first-class.

> **Recap box:** Segments are ephemeral, TTL in seconds (L18). Chat = WebSockets + pub-sub topic-per-channel (L7, L21). Ingest routed by L4 LB (L6). Channels → servers via consistent hashing (L19). Live "cache" = rolling segment buffer with request collapsing (L24). Chat overload → shed/coalesce, never block (L22).

---

## Part 2 — The Interview Session

### Warm-Up Questions (Easy)

Q1. What are the two independent latency problems in a live platform, and what protocols address each?
> **What a strong answer covers:** (a) *Ingest* latency: streamer → platform, over RTMP (legacy, ~2–5s) or SRT/WebRTC (sub-second). (b) *Distribution/glass-to-glass* latency: platform → viewer, over HLS (10–30s with default settings), Low-Latency HLS / LL-DASH (2–5s), or WebRTC (sub-second, expensive to fan out). Trade-off: WebRTC gives sub-second but is peer-oriented and hard to scale to millions; HLS/LL-HLS is CDN-cacheable and scales to millions but adds seconds. Twitch uses LL-HLS for most viewers. Chat is a separate real-time channel (WebSocket) and should feel instant (<1s).
> **Common weak answer:** "Use WebRTC for everything." Misses that WebRTC fan-out to 1M viewers is economically brutal vs CDN-cached HLS.
> **Mentor follow-up if they answer well:** Why does segment duration directly bound HLS latency, and how does LL-HLS use partial segments (chunked CMAF) + HTTP/2 push/preload hints to break the "latency ≈ 3× segment duration" rule?

Q2. Estimate: a top streamer has 1M concurrent viewers at 1080p60 (~6 Mbps). What's the fan-out bandwidth, and how many origin transcodes do you need?
> **What a strong answer covers:** 1M × 6 Mbps = 6 Tbps of viewer egress for ONE channel — must be served from CDN edge, not origin. Origin transcode: you transcode the single ingest ONCE into a ladder (e.g., 1080p, 720p, 480p, 360p, audio-only) — so ~5 renditions total, independent of viewer count. The insight: transcode cost scales with *number of active streams*, egress scales with *number of viewers*. For the whole platform: if 100k channels are live concurrently, that's 100k × ~5 renditions of transcode work; viewer egress is the sum across all channels' viewers.
> **Mentor follow-up:** Most of those 100k channels have <10 viewers. Do you transcode a full ladder for a streamer with 3 viewers? (Answer: no — transcode lazily/on-demand, source passthrough for tiny channels, spin up the ladder only when viewership crosses a threshold.)

Q3. Should live video segments and chat messages share the same delivery path? Why or why not?
> **What a strong answer covers:** No. Video = HTTP-chunked, CDN-cacheable, one segment served to millions from edge cache (fan-out via caching). Chat = per-message fan-out over WebSocket pub-sub, not cacheable (each subscriber gets the same message pushed, but it's a live push not a cached GET). Different consistency, different latency budget, different scaling model. Coupling them means chat's connection state pollutes video's stateless CDN path.
> **Red flag answer:** "Put chat messages inside the video segment metadata / send chat over the HLS manifest." Breaks caching and couples two very different systems.

---

### High-Level Design (Medium)

Q4. Draw the full architecture: ingest → transcode → packaging → CDN fan-out, plus the chat plane.
> **Key components expected:** Streamer encoder (OBS), Ingest servers (RTMP/SRT), Transcode fleet (per-stream ladder), Packager (LL-HLS/CMAF segmenter), Origin/shield, CDN edges, Player. Chat: WS edge gateway, Chat pub-sub (Kafka/Redis), Moderation service, Presence/viewer-count service.
> **Architecture diagram (text):**
```
  Streamer(OBS) ──RTMP/SRT──> [L4 LB] ──> Ingest Server (per stream)
                                              │ (raw/passthrough)
                                              ▼
                                        Transcode Fleet ── ladder: 1080p/720p/480p/360p/audio
                                              │
                                              ▼
                                        Packager (CMAF/LL-HLS segmenter) ──> Origin/Shield
                                                                                │
                                        millions of viewers <══CDN edges<══════┘  (segment cache, TTL~sec)
                                              ▲
                                          Player (LL-HLS ABR)

  CHAT PLANE (independent):
  Viewer <==WebSocket==> [Chat Edge GW] <--sub/pub--> [Redis Pub/Sub or Kafka, topic per channel]
                              │                              │
                              ├──> Moderation Svc (spam/ban/slow-mode)  <── Kafka moderation events
                              └──> Presence/Viewer-count Svc (approx count, HLL)

  TELEMETRY: player + ingest ──> Kafka ──> Flink ──> QoE, concurrent-viewer, dropped-frame dashboards
```
> **What separates SDE2 from SDE3 here:** SDE2 draws ingest→transcode→CDN. SDE3 (a) separates the chat plane entirely, (b) notes transcode is lazy/threshold-based per channel size, (c) adds an origin *shield* tier so a viral segment-N request storm collapses at the shield instead of hitting the packager, and (d) treats ingest failover (streamer reconnect, backup ingest) as first-class.

Q5. Trace a viewer joining a live channel, from clicking the channel to seeing video + chat.
> **Expected trace:**
> 1. Player requests the channel's LL-HLS manifest from the nearest CDN edge. Manifest lists renditions + the current media playlist with the latest (partial) segments and preload hints.
> 2. ABR picks a starting rendition; player requests the latest few segments — edge serves from cache (already fetched for other viewers). Player targets the "live edge" (most recent segment) to minimize latency, sacrificing some buffer.
> 3. In parallel, chat: player opens a WebSocket to a chat edge gateway; gateway resolves which chat cluster owns this channel (consistent hashing on channel_id) and subscribes the connection to `channel:{id}` topic. Recent backlog (last N messages) sent on join.
> 4. Presence service increments an approximate viewer count (probabilistic — HyperLogLog / sampled) and updates every few seconds.
> 5. Player continuously requests the next segment as it's produced; LL-HLS blocking playlist reload / preload hints let it fetch partial segments as they're packaged.
> **Tricky part:** Candidates forget the "live edge" trade-off (join near the newest segment = low latency but higher rebuffer risk on jitter). They also forget chat backlog-on-join and that viewer count is *approximate by design* (exact distinct-count at 1M scale is wasteful).

Q6. Design the ingest and playback APIs / protocols.
> **Expected API design:**
> - Ingest: `rtmp://ingest.twitch.tv/app/{stream_key}` — the stream key authenticates + identifies the channel; SRT/`srt://...` for low-latency. Backup ingest URL for failover.
> - `POST /streams/start` (internal, ingest → control) → registers a live session, allocates transcode capacity, marks channel live.
> - Playback: `GET /{channel}/master.m3u8` (CDN-cached, short TTL) → media playlists per rendition; LL-HLS uses `#EXT-X-PART` partial segments + `#EXT-X-PRELOAD-HINT`.
> - Chat: WebSocket `wss://chat.twitch.tv` with subscribe/publish frames; `PRIVMSG #channel :text`. Rate-limited per user.
> **What to push on:** Stream-key security (leaked key = anyone can hijack a channel — rotate, scope, don't put in manifest). Manifest TTL vs latency (too long = stale live edge; too short = manifest request storm). Idempotency of `streams/start` on ingest reconnect (same session, not a new one). Chat message rate limits and idempotency (dedupe on client-msg-id to avoid double-send on WS reconnect).

---

### Data Modeling (Medium–Hard)
Q7. Model live-session state, chat messages, and channel metadata.
> **Expected schema:**
```sql
-- Live session (ephemeral, hot; Redis + durable copy). One row per active broadcast.
LiveSession (
  channel_id     BIGINT PRIMARY KEY,
  session_id     UUID,
  ingest_server  TEXT,
  started_at     TIMESTAMP,
  renditions     JSONB,         -- active ladder
  viewer_count   INT,           -- approximate, updated periodically
  status         TEXT           -- live | reconnecting | ended
);

-- Chat messages: append-only, per-channel, short retention. Cassandra / Kafka log.
ChatMessage (
  channel_id  BIGINT,
  bucket      INT,              -- time bucket (e.g., minute) to bound partition size
  msg_id      TIMEUUID,
  user_id     BIGINT,
  body        TEXT,
  flags       INT,             -- deleted, mod-only, etc.
  PRIMARY KEY ((channel_id, bucket), msg_id)
) WITH CLUSTERING ORDER BY (msg_id DESC);

-- Channel metadata (durable, read-heavy). Relational + cache.
Channel (
  channel_id BIGINT PRIMARY KEY,
  owner_id   BIGINT,
  title      TEXT,
  category   TEXT,
  is_live    BOOLEAN,
  followers  BIGINT
);
```
> **Index choices and why:** ChatMessage clustered `msg_id DESC` under `(channel_id, bucket)` gives fast "recent messages" reads for join backlog; time-bucketing prevents an unbounded hot partition on a mega-channel. Channel by category for "browse live in category X" is served by a search/index service, not by scanning.
> **Partitioning key and why:** Chat partitions on `(channel_id, bucket)` — colocates a channel's chat but *bounds* partition size by time so a channel with 500 msg/s doesn't create a multi-GB partition. LiveSession partitions on `channel_id` (natural, hot key lives in Redis anyway).

Q8. A viewer joins mid-stream; show them the last ~50 chat messages instantly, then live-tail. How?
> **Expected answer:** Backlog: single partition read `SELECT ... WHERE channel_id=? AND bucket=current LIMIT 50 ORDER BY msg_id DESC` from a hot store (Redis list/stream capped per channel is even better — `XREVRANGE chat:{ch} + - COUNT 50`). Live-tail: after sending backlog, subscribe the WebSocket to the channel's pub-sub topic; server pushes each new message. Bridge the gap with a "since msg_id" cursor so no message is missed or duplicated between backlog snapshot and live subscription start.
> **Trap:** Reading backlog from Cassandra with `ALLOW FILTERING` or scanning across buckets; or subscribing to live *before* reading backlog and getting duplicates/gaps. Use a capped Redis Stream per channel as the hot buffer — O(1) append, cheap ranged read, TTL-based trim.

Q9. Chat ordering and delivery guarantees — what consistency do viewers actually need? Apply CAP (Lesson 2).
> **Expected answer:** Chat is AP: availability and low latency dominate; you accept that two viewers may see messages in slightly different order across shards, and that an occasional message is dropped under overload. Within a *single channel topic*, provide per-channel total order by funneling through one ordered log (Kafka partition per channel, or a Redis stream) so all subscribers see the same sequence — total order per channel, but no cross-channel ordering. Delivery is at-most-once under load (drop rather than block) for the live tail, with the durable log for backlog. Don't attempt global strong ordering across channels — pointless and unscalable.
> **Mentor pushback:** A moderator deletes a message; some viewers already rendered it. Now you need a *retraction* that must reach everyone who saw it. Deletion is a new event on the same ordered topic (`DELETE msg_id`) — clients apply it idempotently. But a viewer who joins after deletion must not receive it in backlog — so the backlog read must filter `flags & DELETED`. This is where "AP chat" meets a correctness requirement (legal/ToS), and you handle it with tombstones + idempotent client apply, not by trying to make chat strongly consistent.

---

### Low-Level Design (Hard)
Q10. Design the live transcode fanout: one ingest stream → a bitrate ladder → millions of viewers, at unpredictable per-channel scale.
> **Problem statement:** Each live channel needs its single ingest transcoded into a ladder in real time (can't fall behind — it's live), then packaged and fanned out. 100k+ channels live, power-law viewership.
> **Naive solution:** Statically assign every live channel a full transcode ladder on dedicated hardware, fan out directly from origin.
> **Why naive fails at scale:** 100k channels × full 5-rendition ladder = enormous idle transcode cost, because 90% of channels have a handful of viewers who could watch source passthrough. And direct origin fan-out means segment N for a viral channel is requested by 1M viewers in the same second → origin meltdown (thundering herd).
> **Expected optimal approach:** (1) *Lazy/threshold transcoding*: small channels serve source passthrough (single rendition); spin up the full ABR ladder only when concurrent viewers cross a threshold or when a lower-bandwidth viewer requests a lower rendition. (2) *Real-time transcode must keep pace* — use hardware-accelerated encoders (GPU/ASIC), one worker per stream pinned via consistent hashing on channel_id. (3) *Fan-out via CDN with an origin shield*: viewers hit edge; edge misses collapse at a shield/mid-tier so origin sees ~1 request per segment regardless of viewer count (request coalescing / "collapsed forwarding"). (4) Segments are immutable, short TTL; the manifest updates as new segments are packaged.
> **Pseudo-code or class diagram:**
```
on_ingest_connect(channel_id, stream):
    worker = transcode_pool.assign(consistent_hash(channel_id))
    worker.start(stream, ladder=[passthrough])       # start cheap

on_viewership_signal(channel_id, concurrent):
    if concurrent > LADDER_THRESHOLD and not full_ladder(channel_id):
        worker.add_renditions([1080p,720p,480p,360p,audio])   # scale up lazily

# CDN edge segment fetch with request collapsing:
def edge_get(segment_key):
    if cache.has(segment_key): return cache.get(segment_key)
    with singleflight(segment_key):        # collapse concurrent misses -> 1 upstream fetch
        if cache.has(segment_key): return cache.get(segment_key)
        seg = shield.fetch(segment_key)    # shield does the same collapsing toward origin
        cache.put(segment_key, ttl=SECONDS)
        return seg
```

Q11. Concurrency/race: a streamer's connection drops and reconnects within 2 seconds. Handle it without ending the broadcast or double-provisioning.
> **Scenario:** RTMP TCP resets; encoder auto-reconnects to a (possibly different) ingest server. Two ingest sessions could briefly claim the same channel; viewers must not see the stream "end."
> **Expected fix:** Session identity keyed by `(channel_id, stream_key)` with a lease/lock in a coordination store (e.g., a short-TTL lock in Redis/etcd — leader election, Lesson 17). New ingest connection must acquire the channel lease; if the old session's lease hasn't expired, reject or fence it (fencing token so a zombie old session's writes are ignored). Mark session `reconnecting` rather than `ended`; keep the manifest alive with a grace window (e.g., 30–60s) so the player doesn't tear down. On successful reconnect, resume the same `session_id`; only after grace expiry with no reconnect do you mark `ended`.
> **Follow-up:** What if the ingest server holding the lease *dies* (not the streamer)? The lease TTL expires → another ingest can take over when the encoder reconnects; the fencing token prevents the dead server's late-arriving segments from corrupting the live edge. Viewers see at most a brief buffering blip covered by their playback buffer.

Q12. Edge case: chat firehose — a channel spikes to 1000 messages/second and a client on mobile can't render that fast. What happens?
> **Scenario:** Message production rate ≫ client render/network capacity; naive push would back up the server's send buffer and OOM the connection.
> **Expected handling:** Backpressure + load-shedding (Lesson 22), never block the producer: (1) per-connection outbound buffer with a bound; when full, *drop/coalesce* messages for that slow client — chat is lossy by design. (2) Server-side rate features: slow-mode (limit user post rate), sub-only mode, and sampling — under extreme load, show a representative sample rather than every message (a human can't read 1000 msg/s anyway). (3) Fan-out is done once per topic to per-server subscriber lists, not per-message-per-user from origin. (4) Moderation/spam filtering upstream reduces volume. DLQ isn't used for live chat (stale messages are worthless) — you shed, not retry.

---

### Scaling to 10x / 100x (Hard)
Q13. Where does the platform break first under a record-breaking event (e.g., 5M concurrent on one channel)?
> **Expected answer:** Not transcode (still ~5 renditions for that one channel) and not raw CDN egress if the shield collapses correctly. First break points: (1) the *origin shield / segment origin* if request collapsing isn't perfect — 5M viewers requesting segment N within its ~2s lifetime must collapse to ~1 origin fetch; any collapsing miss = origin storm. (2) The *chat plane* — 5M WebSocket connections + high message rate on one topic is a massive fan-out; a single chat server can hold ~10k–100k connections, so you need thousands of chat edge servers subscribed to the channel topic, arranged in a fan-out tree. (3) Presence/viewer-count if computed exactly.
> **Numbers to ground the answer:** 5M viewers × 6 Mbps = 30 Tbps egress (CDN-served). Chat: 5M connections / 50k per server ≈ 100 chat servers *just for connections* on one channel; message fan-out of 200 msg/s × 5M = 1e9 message-deliveries/sec if naive — hence topic-based fan-out trees, not per-user delivery from a single origin.

Q14. Shard the chat system across servers so a mega-channel doesn't overwhelm one box. Apply consistent hashing (Lesson 19).
> **Expected sharding strategy:** Consistent-hash channels → chat clusters for the *common* case (most channels fit on one cluster). For a mega-channel that exceeds one server's connection capacity, you can't put all its subscribers on one box — use a *fan-out tree*: one authoritative ordered log (single Kafka partition / Redis stream) for the channel produces the sequence; a tier of relay servers each subscribe once to that log and re-broadcast to thousands of edge servers, which each push to ~50k viewer connections. This is hierarchical fan-out: one publish → tree of relays → millions of pushes.
> **Hot spot problem:** The mega-channel is the hot spot by definition. Detect via connection count + message rate per channel; fix by dynamically promoting the channel from "single cluster" to "fan-out tree" mode and spreading its edge servers across many hosts/AZs. Virtual nodes keep the *rest* of the channels balanced when you add capacity.

Q15. Design caching for live video where every segment is new.
> **Expected layered cache design:**
> - Edge cache: last few segments per rendition, TTL ≈ segment duration; short but high hit-rate because thousands request the same current segment.
> - Origin shield / mid-tier: collapses edge misses so origin sees ~1 fetch per segment (collapsed forwarding / singleflight).
> - Player buffer: small (live edge) — trade latency vs rebuffer.
> - DVR/VOD buffer: a rolling window (e.g., last 2 hours) stored for "rewind live" and later VOD; this is a longer-TTL cache backed by object storage.
> **Cache invalidation trap:** Live segments are immutable so invalidation isn't the issue — the trap is the *manifest*. The media playlist changes every segment; caching it too long makes viewers stuck behind the live edge, caching it too short creates a manifest-request storm at 1M viewers. LL-HLS solves this with *blocking playlist reload* + preload hints so the player efficiently long-polls for the next partial segment instead of hammering the manifest. Cache-key must include rendition + segment number; never let two channels' segments collide.

Q16. Cost/efficiency at scale.
> **Expected answer:** (1) Lazy transcoding + source passthrough for the long tail of tiny channels — don't burn GPU on a 3-viewer stream. (2) Hardware-accelerated (ASIC/GPU) transcode to cut per-stream cost and hit real-time. (3) Aggressive CDN offload with shield collapsing — origin does ~1 fetch per segment regardless of viewers. (4) Right-size the ladder per content (fast-motion gaming needs more bits than a talk show). (5) Chat: fan-out trees + message sampling under load reduce per-message cost; presence via HyperLogLog instead of exact distinct counts. (6) Tier the DVR/VOD buffer to cheap object storage and expire aggressively.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** LL-HLS internals: explain partial segments (`EXT-X-PART`), preload hints, and blocking playlist reload. How do these break the classic "HLS latency ≈ 3 × target segment duration" and get you to ~2s glass-to-glass while staying CDN-cacheable? Contrast with WebRTC's sub-second-but-unscalable fan-out.

**H2.** Multi-region + failover: a streamer in Europe, viewers worldwide. Where do you transcode (near ingest), where do you package, and how does the origin shield replicate across regions so a US viewer doesn't cross the Atlantic per segment? What happens to in-flight viewers if the ingest region fails?

**H3.** Operational: deploy a new transcoder version without dropping live streams. (Answer: you cannot restart a worker mid-broadcast without a blip — drain by not assigning *new* streams to old-version workers, let existing streams finish or migrate on natural reconnect; canary on low-viewership channels first; blue-green the packager behind the shield.)

**H4.** Observability: instrument glass-to-glass latency, rebuffer ratio, ingest frame drops, transcode lag (are you keeping real-time?), and chat delivery latency. How do you measure end-to-end latency when producer and consumer clocks differ? (Embed producer timestamps / SCTE-like markers in segments; player computes delta.)

**H5.** 'Undo a bad decision': you lowered target segment duration to cut latency, but rebuffering spiked on mobile networks. How do you detect (QoE A/B by network type), roll back per-cohort behind a server-controlled config (not a client redeploy), and design segment duration to be adaptively negotiated per viewer rather than globally fixed?

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Confusing VOD with live. Live means transcode must keep real-time pace, segments are ephemeral, and you can't pre-position anything — the "cache" is a few seconds of rolling buffer.
2. Trying to make chat strongly consistent or globally ordered. Chat is AP, per-channel-ordered, lossy under load — and that's correct. The subtlety is tombstone-based deletion for moderation.
3. Ignoring request collapsing / origin shield. Without singleflight at the edge and shield, a viral segment turns 1M viewer requests into 1M origin fetches and melts the origin.

**The one insight that makes an answer truly impressive:**
Decouple the three scaling axes: *transcode cost scales with number of live streams*, *video egress scales with viewers (and is CDN-offloaded via shield collapsing)*, and *chat fan-out scales with viewers-per-channel and needs a hierarchical fan-out tree for mega-channels*. Candidates who name these three independent axes and design each separately stand out.

**Suggested follow-up reading:**
- Apple's Low-Latency HLS spec + "LL-HLS" engineering write-ups.
- Twitch Engineering blog: live video ingest/transcode and the "IRLTK"/chat scaling posts; Discord's "How we handle millions of concurrent voice/chat" for fan-out-tree patterns.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
