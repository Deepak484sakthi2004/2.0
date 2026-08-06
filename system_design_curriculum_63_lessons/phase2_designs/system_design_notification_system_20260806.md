# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 34 of 63 — Phase 2: System Design Track (Design 6 of 35)
**Topic:** Notification System (Push / Email / SMS at Scale)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
A notification system is the connective tissue of every consumer product: it fans a single business event ("your order shipped," "someone liked your post," "your OTP is 483920") out to millions of users across push (APNs/FCM), email (SES/SendGrid), SMS (Twilio), and in-app channels. It looks trivial until you meet the real constraints — third-party providers rate-limit and fail independently, users have per-channel preferences and quiet hours, regulators (TCPA, GDPR, CAN-SPAM) impose consent and opt-out, and a single misconfigured retry loop can send the same push five times or blow your Twilio bill by $50k overnight. Meta, Uber, LinkedIn, and Slack all run dedicated platform teams for exactly this. What makes it hard is not the send — it's idempotency, deduplication, provider abstraction, preference enforcement, and observability at fan-out scale.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Kafka / event-driven architecture (taught in Phase 1, Lesson 21):** Kafka is a partitioned, append-only commit log. Producers write to topic partitions; consumers in a group each own a subset of partitions and track a committed offset. Ordering is guaranteed only within a partition, delivery is at-least-once by default, and consumer lag is your primary backpressure signal. In today's design Kafka is the ingestion buffer and the per-channel work queue — it decouples the burst of business events from the throttled reality of downstream providers.

**Rate limiting (taught in Phase 1, Lesson 11):** Token bucket allows bursts up to bucket size then refills at a fixed rate; sliding-window log is exact but memory-heavy; sliding-window counter approximates cheaply. We apply rate limiting in two places today: outbound provider throttling (APNs ~ per-connection HTTP/2 stream limits, Twilio ~1 msg/sec per long code) and inbound per-user frequency capping ("no more than 3 marketing pushes/day").

**Idempotency & at-least-once semantics (taught in Phase 1, Lessons 9 & 21):** Because Kafka and HTTP retries are at-least-once, exactly-once *delivery* is impossible end-to-end; you engineer exactly-once *effect* with an idempotency key and a dedup store. Every notification carries a deterministic `notification_id`; a Redis SETNX or a DB unique constraint on `(notification_id, channel)` guarantees a single send even if the message is reprocessed. This is the single most important concept for this system.

**Caching / Redis (taught in Phase 1, Lesson 24):** Redis gives sub-millisecond reads for hot data and atomic primitives (SETNX, INCR, sorted sets, TTL). Here Redis holds the dedup set, per-user rate-limit counters, device-token cache, and (via ZSET) the scheduled/delayed notification queue.

**Circuit breaker & observability (taught in Phase 1, Lesson 10):** A circuit breaker trips open after an error-rate threshold, short-circuits calls to a failing dependency, and half-opens to probe recovery. Providers fail independently — when SendGrid degrades you must stop hammering it, fail over to a secondary ESP, and keep push/SMS unaffected. Per-provider breakers + RED metrics (Rate, Errors, Duration) are non-negotiable.

**Consistent hashing (taught in Phase 1, Lesson 19):** Maps keys to nodes on a hash ring with virtual nodes so adding/removing a node reshuffles only ~1/N of keys. Used to shard the dedup/rate-limit Redis cluster by `user_id` and to keep per-user state colocated.

**SQL vs NoSQL / LSM vs B-tree (taught in Phase 1, Lesson 23):** Wide-column stores (Cassandra, LSM-based) excel at high write throughput and time-series-shaped data. The notification log — billions of append-only records queried by `user_id` + time — is a textbook Cassandra/DynamoDB case, not a normalized RDBMS.

> **Recap box:**
> - Kafka = decoupling buffer + per-channel queue; ordering only within a partition.
> - Rate limiting = token bucket outbound (provider caps) + frequency cap inbound (per user).
> - Idempotency key + dedup store = exactly-once *effect* on top of at-least-once transport.
> - Redis = dedup SETNX, rate counters, device-token cache, ZSET scheduler.
> - Per-provider circuit breakers + failover; never let one ESP take down all channels.
> - Consistent hashing shards per-user state; wide-column store for the append-only log.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Why do we put a message queue between the event producers and the channel senders instead of calling APNs/Twilio synchronously from the API that generated the event?
> **What a strong answer covers:** Decoupling of producer throughput from consumer (provider) throughput; absorbing bursts (a marketing blast of 10M pushes shouldn't block the order service); independent retry and DLQ without holding request threads; back-pressure via consumer lag; enabling multiple consumer groups (analytics, dedup, send) off one event; provider latency (Twilio can take 300ms–2s) must never sit on a user request path.
> **Common weak answer:** "Queues make it async and faster." No mention of burst absorption, retry isolation, or provider latency — just repeats a buzzword.
> **Mentor follow-up if they answer well:** If the send service crashes after pulling a message but before calling the provider, do you lose the notification or double-send it? (Answer: with at-least-once + manual offset commit *after* successful send you may double-send, hence dedup; with commit-before-send you may drop — always commit after.)

Q2. Estimate the daily volume and the queue/storage footprint for a social app with 100M DAU, where the average user triggers 5 notifiable events/day and each event fans out to ~2 recipients on average.
> **What a strong answer covers:** Events/day = 100M × 5 = 500M events. Fan-out ×2 = 1B notification sends/day ≈ **11,600 sends/sec average**, with a peak factor of ~5x → ~58k/sec peak. Payload ~1KB per notification record → 1B × 1KB = **1TB/day** of log data; at 90-day retention ≈ 90TB (before compression/replication). Push tokens: 100M users × ~2 devices × ~200 bytes ≈ 40GB token store, easily cached. Kafka: 1B msgs/day, if each 1KB and RF=3 → 3TB/day churn on a topic with 7-day retention ≈ 21TB.
> **Mentor follow-up:** Where does the 5x peak come from and why does it dominate your provisioning? (Time-zone-aligned morning digest sends + product-launch blasts; you provision consumers and provider connections for peak, not average — this is why scheduled sends need smoothing/jitter.)

Q3. A candidate proposes storing user notification preferences in the same row as the user profile in the main product Postgres. Good idea or not?
> **What a strong answer covers:** Coupling the notification platform to the product's primary DB creates a cross-service dependency and read amplification (every send reads the product DB). Preferences (channel opt-in, quiet hours, frequency caps, per-category toggles) belong to the notification service, cached in Redis with the user DB as source of truth, and must be consultable at ~10k+ QPS without touching the product's transactional store. It's also a bounded-context violation (DDD, Lesson 9).
> **Red flag answer:** "Yes, it's one join, simpler." Ignores service ownership, read amplification on the product DB, and the latency budget of the send path.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the end-to-end notification platform. Draw the architecture — from a service emitting "order.shipped" to the push landing on a phone.
> **Key components expected:** Ingestion API / event gateway; Kafka ingestion topic; Notification service (template rendering, preference + consent check, dedup, rate/frequency cap); per-channel Kafka topics; channel workers (push/email/SMS/in-app) each with provider adapters + circuit breakers; provider abstraction layer with failover; scheduler (ZSET/Kafka-delay) for future/quiet-hour sends; dedup + rate store (Redis); notification log (Cassandra); status/receipt ingestion (webhooks from providers) back into the log; analytics pipeline.
> **Architecture diagram (text):**
```
 Producing services            ┌─────────────────────────────────────────┐
 (order, social, auth) ──POST──▶│  Notification API / Ingestion Gateway    │
                                │  - validate, auth, idempotency-key       │
                                └───────────────┬──────────────────────────┘
                                                │ produce
                                    ┌───────────▼───────────┐
                                    │  Kafka: raw_events     │  (partition by user_id)
                                    └───────────┬───────────┘
                                                │
                     ┌──────────────────────────▼───────────────────────────┐
                     │  Notification Processing Service (consumer group)     │
                     │  1. Load template + render (Handlebars)               │
                     │  2. Preference & consent check  ── Redis (prefs cache)│
                     │  3. Quiet-hours / schedule?  ── ZSET scheduler        │
                     │  4. Frequency cap  ── Redis INCR (token bucket)       │
                     │  5. Dedup  ── Redis SETNX(notif_id)                   │
                     └───────┬───────────┬───────────┬───────────┬───────────┘
                             │ push      │ email     │ sms       │ in-app
                     ┌───────▼──┐ ┌──────▼───┐ ┌─────▼────┐ ┌────▼─────┐   Kafka
                     │push topic│ │email topic│ │sms topic │ │inapp topic│  per-channel
                     └───┬──────┘ └────┬─────┘ └────┬─────┘ └────┬─────┘
                     ┌───▼──────┐ ┌────▼─────┐ ┌────▼─────┐ ┌────▼─────┐
                     │Push Wkr  │ │Email Wkr │ │SMS Wkr   │ │InApp Wkr │  workers
                     │+breaker  │ │+breaker  │ │+breaker  │ │(WS/store)│
                     └───┬──────┘ └────┬─────┘ └────┬─────┘ └──────────┘
              ┌──────────▼───┐  ┌──────▼──────┐ ┌───▼─────┐
              │APNs / FCM     │  │SES/SendGrid │ │Twilio   │  3rd-party providers
              └──────┬────────┘  └──────┬──────┘ └───┬─────┘
                     │ delivery receipts / bounces / clicks (webhooks)
                     └────────────┬──────────────────┘
                          ┌───────▼────────┐        ┌──────────────────┐
                          │ Receipt ingest │──────▶ │ Notification Log  │
                          └────────────────┘        │ (Cassandra)       │
                                                     └────────┬─────────┘
                                                              ▼  analytics/BI
```
> **What separates SDE2 from SDE3 here:** An SDE2 draws the boxes and the happy path. An SDE3 puts the *provider abstraction layer* front and center (adapters normalizing APNs/FCM/SES/Twilio behind one interface with health/failover), makes dedup and preference-check happen **before** the per-channel fan-out (so you don't rate-check four times), and treats *delivery receipts as first-class inbound events* that update state and feed retries — most candidates forget the return path entirely.

Q5. Trace a single "order.shipped" event end-to-end for a user who wants push + email but has quiet hours 22:00–08:00 and it's currently 23:30 local.
> **Expected trace:**
> 1. Order service POSTs `{event_type:"order.shipped", user_id, order_id, idempotency_key}` to the ingestion API.
> 2. API validates, attaches a deterministic `notification_id = hash(user_id, event_type, order_id)`, produces to `raw_events` (partitioned by user_id → per-user ordering).
> 3. Processing consumer picks it up, loads the `order.shipped` template, renders push + email bodies.
> 4. Preference lookup (Redis, fallback DB): user opted into push+email, SMS off. Consent OK.
> 5. Quiet-hours check: 23:30 falls in the window → **do not send now.** Compute next allowed time 08:00 local, enqueue into the ZSET scheduler with score = epoch(08:00). (Transactional/OTP notifications would bypass quiet hours — category matters.)
> 6. At 08:00 a scheduler poller pops due entries, re-runs frequency cap + dedup (SETNX on `notification_id:push` and `:email`), then produces to `push_topic` and `email_topic`.
> 7. Push worker pulls, calls FCM/APNs over HTTP/2, gets a message-id, commits offset **after** success, writes SENT to the log.
> 8. FCM later posts a delivery receipt webhook → receipt ingest updates log to DELIVERED. If APNs returns `Unregistered`, mark token dead and suppress future sends.
> **Tricky part:** Candidates get vague at step 5/6 — *where does the delay live* and *do you re-check dedup and rate limits after the delay*. Yes: state can change during the wait (user may opt out, another notification may have sent), so preference/cap/dedup must be re-evaluated at dispatch time, not only at ingest.

Q6. Design the notification ingestion API and the preferences API.
> **Expected API design:**
> ```
> POST /v1/notifications
>   Headers: Idempotency-Key: <uuid>        (required)
>   Body: { user_id, template_id, channels?: ["push","email"],
>           category: "transactional"|"marketing"|"otp",
>           data: {...template vars}, dedup_key?, priority, ttl_seconds }
>   → 202 Accepted { notification_id, status:"queued" }
>
> GET  /v1/notifications/{notification_id}      → status + per-channel receipts
> GET  /v1/users/{id}/preferences
> PUT  /v1/users/{id}/preferences
>   Body: { channels:{push:true,email:true,sms:false},
>           categories:{marketing:false,...}, quiet_hours:{start,end,tz},
>           frequency_caps:{marketing:3} }
> POST /v1/users/{id}/opt-out    (channel or category; honors List-Unsubscribe)
> ```
> **What to push on:** **Idempotency-Key** header → the API dedups retried POSTs (client timeout + retry must not double-enqueue); store key→notification_id in Redis with TTL. **Versioning** via `/v1/` and template versioning so a template change doesn't break in-flight sends. **`category` is load-bearing** — it drives quiet-hours bypass, consent rules, and frequency caps; OTP must never be dropped by a marketing cap. **TTL** so a stale notification ("your ride is here") isn't delivered 2 hours late. Return **202** (accepted, async), never 200 with a fake "delivered."

---

### Data Modeling (Medium–Hard)

Q7. Design the primary data stores: the notification log, device tokens, and templates.
> **Expected schema:**
```sql
-- Notification log (Cassandra / DynamoDB — append-heavy, queried by user+time)
CREATE TABLE notifications (
  user_id        uuid,
  bucket         text,          -- e.g. '2026-08'  (time bucketing to bound partitions)
  notification_id timeuuid,
  channel        text,          -- push|email|sms|inapp
  category       text,
  status         text,          -- QUEUED|SENT|DELIVERED|FAILED|BOUNCED|CLICKED
  provider       text,
  provider_msg_id text,
  template_id    text,
  created_at     timestamp,
  updated_at     timestamp,
  PRIMARY KEY ((user_id, bucket), notification_id)
) WITH CLUSTERING ORDER BY (notification_id DESC);

-- Device tokens (per user, per device) — Postgres/Dynamo, source of truth
CREATE TABLE device_tokens (
  user_id     uuid,
  device_id   text,
  platform    text,          -- ios|android|web
  token       text,          -- APNs/FCM token
  app_version text,
  active      boolean,
  last_seen   timestamp,
  PRIMARY KEY (user_id, device_id)
);

-- Templates (versioned) — small, cached aggressively
CREATE TABLE templates (
  template_id text,
  version     int,
  channel     text,
  locale      text,
  subject     text,
  body        text,           -- Handlebars/MJML source
  PRIMARY KEY ((template_id), version, channel, locale)
);
```
> **Index choices and why:** The log's partition key is `(user_id, bucket)` so all of a user's notifications for a month live together and "show my notifications" is a single-partition read in reverse-time order (clustering by `notification_id DESC`). Bucketing by month bounds partition size (unbounded `user_id`-only partitions become multi-GB hot rows). A secondary lookup `provider_msg_id → notification_id` is needed to process delivery-receipt webhooks — build it as a separate table/GSI, never a Cassandra `ALLOW FILTERING` scan.
> **Partitioning key and why:** `user_id` (hashed, via consistent hashing) — it's the natural access unit, spreads evenly, and colocates a user's dedup/rate/log state. Device tokens partition by `user_id` for the same reason. Templates partition by `template_id` because they're tiny and read-mostly (cache in every worker).

Q8. At send time you need each recipient's active device tokens plus their preferences. That's a read on the hot path for every one of ~58k sends/sec at peak. How do you serve it without melting a database?
> **Expected answer:** Two-tier cache. Preferences and active tokens live in Redis (sharded by user_id via consistent hashing), keyed `prefs:{user_id}` and `tokens:{user_id}`, TTL ~1h, populated on write-through when the user updates prefs/registers a device and lazily on cache-miss from the source DB. Hit rate should be >99% because sends cluster on active users. Batch the reads: a fan-out job for a marketing blast does an `MGET`/pipeline of thousands of users rather than N round-trips. Invalidate on preference change (write-through updates the cache synchronously). Dead tokens discovered via APNs/FCM error responses are removed immediately so you stop paying to send to them.
> **Trap:** Reading the source-of-truth DB per send. At 58k/sec you'd need a DB doing 58k QPS of point reads *just for preferences* — either an over-provisioned RDBMS or a guaranteed tail-latency spike that stalls consumers and grows Kafka lag. The naive answer also forgets to cache the *absence* (user has no tokens) — otherwise every send for a token-less user is a cache miss + DB read.

Q9. What's the consistency model for preferences and for the dedup/status data? Where is eventual consistency acceptable and where is it dangerous?
> **Expected answer:** Preferences and consent/opt-out need **read-your-writes and strong-enough** semantics for the *opt-out* case specifically — if a user unsubscribes, continuing to send is a legal/compliance violation (CAN-SPAM 10-day grace is the ceiling, but you aim for immediate). So opt-out writes are write-through to Redis + DB synchronously and checked at dispatch time, not just ingest. The notification **log/status** is fine eventually consistent — a receipt updating DELIVERED a few seconds late harms nothing (Cassandra with `LOCAL_QUORUM` writes/reads). **Dedup must be strongly consistent within a shard**: the SETNX and the send must be effectively atomic per `notification_id`, or you double-send.
> **Mentor pushback:** Eventual consistency breaks on quiet-hours + opt-out during a scheduled delay: a user opts out at 07:59, the 08:00 scheduler fires reading a stale prefs cache and sends anyway. Fix: re-read prefs at dispatch with a short TTL / write-through invalidation, and treat opt-out as a suppression list checked with strong consistency (a Redis set or Bloom filter fronting a durable store) at the last moment before the provider call.

---

### Low-Level Design (Hard)

Q10. The hardest sub-problem: fan-out for a broadcast. Marketing wants to send a push to a 50M-user segment at 9:00 AM. Design the fan-out so it doesn't (a) stampede the providers, (b) miss users, or (c) double-send.
> **Problem statement:** Turn one campaign into 50M individual, deduped, preference-respecting, rate-smoothed sends, resumable after a crash, completing within an SLA (say 30 min) while staying under APNs/FCM connection limits.
> **Naive solution:** Loop over 50M user_ids in one job, for each: check prefs, produce to push topic. Single-threaded or a fat for-loop.
> **Why naive fails at scale:** 50M sequential sends at even 5ms each = 69 hours. Parallelize naively and you hit APNs/FCM throttling (per-app rate limits, HTTP/2 stream caps) → 429s and provider-side blocks. A crash at user 30M has no idempotent resume → you either restart (double-send 30M) or lose 20M. All 50M hit at 9:00:00 exactly → a thundering herd on your Redis prefs cache and the provider.
> **Expected optimal approach:** *Segmentation + chunked, checkpointed fan-out + rate-smoothing.* Materialize the segment offline into an ordered, chunked list (e.g., 50M → 50k chunks of 1k users, stored in Kafka/object storage). A pool of fan-out workers claims chunks (each chunk is an idempotent unit with a `campaign_id:chunk_id` completion marker in Redis/DB — a crashed chunk is simply re-claimed and re-run; per-user dedup via SETNX makes re-runs safe). Each per-user send still passes prefs/cap/dedup. Apply **jitter**: spread the 50M over the 30-min window (add random 0–1800s offset per chunk) to smooth provider load rather than a 9:00:00 spike. A token-bucket rate limiter per provider connection caps outbound throughput to the negotiated APNs/FCM limit.
> **Pseudo-code or class diagram:**
```
def run_campaign(campaign_id, segment_query):
    chunks = materialize_segment(segment_query, chunk_size=1000)   # → object store
    for chunk_id, user_ids in chunks:
        produce("fanout_topic", {campaign_id, chunk_id, user_ids,
                                 dispatch_at: now + jitter(0, 1800)})

# fan-out worker (consumer group, many instances)
def handle_chunk(msg):
    if redis.sismember(f"campaign:{msg.campaign_id}:done", msg.chunk_id):
        return                                   # idempotent skip
    for uid in msg.user_ids:
        prefs = get_prefs(uid)                    # cached
        if not prefs.push or opted_out(uid, 'marketing'): continue
        if not freq_cap_ok(uid, 'marketing'): continue
        nid = f"{msg.campaign_id}:{uid}"
        if redis.set(f"dedup:{nid}", 1, nx=True, ex=DAY):   # SETNX
            rate_limiter.acquire('fcm')          # token bucket, blocks
            produce("push_topic", build_push(uid, prefs))
    redis.sadd(f"campaign:{msg.campaign_id}:done", msg.chunk_id)  # checkpoint
    commit_offset()
```

Q11. Concurrency: the same event gets processed twice (Kafka rebalance replays uncommitted offsets) at nearly the same instant on two consumer instances. Both check dedup and both send. Walk the exact race and fix it.
> **Scenario:** Consumer A reads message M (notification_id N), does `GET dedup:N` → miss. A GC pause hits A. Rebalance moves M's partition to consumer B (offset never committed). B reads M, `GET dedup:N` → miss, sends, `SET dedup:N`. A resumes, its earlier check said miss, A sends too → **double push.**
> **Expected fix:** Never do check-then-act with separate GET/SET — that's TOCTOU. Use a single atomic `SET dedup:N 1 NX EX 86400`; only the caller that *wins the SETNX* proceeds to send. B wins, A's SETNX returns nil → A skips. Commit the Kafka offset **only after** the provider call succeeds, but rely on SETNX (not offset commit) for correctness. For durability beyond Redis TTL, back it with a DB unique constraint on `(notification_id, channel)` — INSERT-then-send; a duplicate key means "already sent." Partitioning `raw_events` by `user_id` also keeps a user's events on one partition, reducing (not eliminating) cross-consumer races.
> **Follow-up — what if the lock/SETNX winner dies after SET but before the provider call?** Then dedup:N exists but nothing was sent → a silent drop. Mitigation: make the dedup entry a *two-phase* record — write status QUEUED atomically, and only after provider ACK flip to SENT. A reconciliation/sweeper job finds QUEUED entries older than a threshold with no receipt and re-drives them (idempotent because the provider msg-id / dedup still gates). This is the classic "at-least-once with idempotent effect beats exactly-once fantasy."

Q12. Failure deep-dive: Twilio starts returning 500s and 5-second latencies at 20% error rate during peak. What happens and how does the system protect itself?
> **Scenario:** SMS worker calls Twilio; 20% error, latency 5s. Without protection, worker threads block on slow calls, throughput collapses, Kafka lag on `sms_topic` explodes, retries pile on, and you may double-bill for messages Twilio actually accepted but timed out on the response.
> **Expected handling:** Per-provider **circuit breaker** (Lesson 10): after error-rate >50% over a rolling window, trip **open** — stop calling Twilio, fast-fail. Route new SMS to a **secondary provider** (e.g., MessageBird) via the provider-abstraction failover, if the message is provider-agnostic. Failed messages go to a **retry topic with exponential backoff + jitter** (e.g., 1s, 4s, 16s, cap), and after N attempts to a **DLQ** for inspection/alerting — never an infinite retry loop (that's how you get the $50k bill and duplicate sends). **Idempotency toward the provider:** send with a client-generated idempotency key / dedup so a timed-out-but-accepted message isn't re-sent. TTL-drop messages whose `ttl_seconds` expired while queued (a 2-hour-late OTP is worse than none). Alert on breaker-open + DLQ depth. Half-open probes restore Twilio when it recovers.

---

### Scaling to 10x / 100x (Hard)

Q13. You 100x to 100B sends/day (~1.2M/sec average, ~6M/sec peak). Where does the system break first, and what's the number that tells you?
> **Expected answer:** The **providers and the outbound rate limiters** break first, not your compute — APNs/FCM/SES/Twilio impose hard per-account throughput caps and you cannot exceed them by adding workers. The signal is rising Kafka consumer lag on the per-channel topics while worker CPU stays low (workers are blocked on the rate limiter / provider). Secondary break: the **dedup/rate Redis** — at 6M/sec you're doing >6M SETNX + several INCR/GET per send = tens of millions of Redis ops/sec, which no single cluster serves; you shard by user_id (consistent hashing) across many Redis clusters and watch p99 GET latency and per-shard ops/sec. Third: **Cassandra write throughput** on the log at 100B rows/day — watch pending compactions and write p99. Grounding numbers: average 1.2M/sec, peak 6M/sec, ~1KB/record → ~6GB/sec log write at peak, ~100TB/day pre-replication.
> **Numbers to ground the answer:** 6M sends/sec × ~4 Redis ops = 24M Redis ops/sec → need ~20–40 shards (each ~500k–1M ops/sec). Provider negotiation (raising APNs/FCM/SES quotas, multiple sender accounts/IPs) becomes a *business* bottleneck, not just engineering.

Q14. How do you shard the per-user state (dedup, rate counters, scheduled ZSETs) and the log, and how do you handle hot spots?
> **Expected sharding strategy:** **Hash by user_id with consistent hashing + virtual nodes** (Lesson 19) so adding a Redis/Cassandra node reshuffles only ~1/N of keys and per-user state (dedup, caps, tokens, log) stays colocated on one shard → single-shard lookups, no scatter-gather. Range sharding is wrong here (creates temporal/alphabetical hot spots).
> **Hot spot problem:** A celebrity/broadcast account or a system-wide "@everyone" isn't user-partitioned — 50M recipients of one campaign are fine (they shard by *recipient* user_id), but a single user receiving a viral flood (100k "liked your post") hammers one shard's rate counter + log partition. Detect via per-key ops metrics and hot-partition alerts. Fix: (1) **aggregate/coalesce** — collapse "100k likes" into one "X and 99,999 others liked your post" digest (this is both a UX and a scaling win), (2) shard the counter itself (N sub-counters summed), (3) for the log, add a hash suffix to the partition key for super-hot users to spread the write.

Q15. Design the caching layers and name the invalidation trap unique to notifications.
> **Expected layered cache design:** **L1** in-process (Caffeine/LRU, Lesson 4) in each worker for templates (rarely change, tiny) and provider config — sub-microsecond, TTL a few minutes. **L2** Redis (sharded) for prefs, device tokens, dedup, rate counters, opt-out/suppression set — sub-ms, write-through on user updates. **CDN/edge** for in-app notification assets/images and public unsubscribe pages. Warm token cache from device-registration events.
> **Cache invalidation trap:** The **opt-out / suppression** state is the dangerous one. A stale cached "user is subscribed" leads to sending after unsubscribe — a compliance violation, not just a bug. So opt-out is *not* just TTL-invalidated; it's write-through *and* checked against an authoritative suppression store at the last moment (a Redis SET or Bloom filter, backed durably). Templates have the opposite trap: caching a template version too long means a fixed typo/legal correction keeps sending the old body — key the cache by `template_id:version` and roll versions rather than mutating in place.

Q16. Cost/efficiency at 100B/day. Where's the money and how do you cut it without hurting reliability?
> **Expected answer:** The dominant cost is **per-message provider fees** (SMS especially — Twilio ~$0.0075+/SMS; 100B SMS would be absurd, so channel routing matters: prefer free push over paid SMS wherever the user has an active device; fall back to SMS only for OTP/critical). **Coalescing/digesting** cuts volume outright (batch N notifications into one digest email/push → fewer sends, fewer fees, better UX). **Dead-token pruning** stops paying to send to uninstalled apps (APNs `Unregistered`, FCM `NotRegistered`). **Frequency caps** cut marketing volume. **Storage:** the log is append-only time-series → compress (Cassandra + TTL, tier cold buckets to object storage/S3, drop raw payloads after N days keeping only metadata). **Batch provider APIs** (FCM/SES support multicast/batch) to amortize connection overhead. **Compute:** autoscale consumers on Kafka lag so you're not paying for peak capacity at 3 AM. Instrument cost-per-notification per channel as a first-class metric.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Kafka internals for this system: you partition `raw_events` by `user_id` for per-user ordering, but a broadcast fan-out produces to `push_topic` with no ordering need and huge volume. How do you choose partition counts, and what happens to ordering guarantees when you rebalance mid-broadcast? (Expect: partition count sized to peak throughput / per-partition ceiling ~10MB/s; per-user ordering only matters on `raw_events`, so key by user_id there; `push_topic` can be keyed by nothing / round-robin for max parallelism since sends are independent; rebalance can replay uncommitted offsets → why dedup, not ordering, is the correctness mechanism; committing offset after send + idempotent effect.)

**H2.** Compliance & multi-region: TCPA/GDPR/CAN-SPAM require consent proof, honored opt-outs within a deadline, data residency (EU user data stays in EU), and a `List-Unsubscribe` header on marketing email. How does this reshape your data model and topology? (Expect: consent ledger with timestamp+source per channel; suppression list checked at dispatch; regional Kafka + storage with user_id → region routing; PII minimization in the log; right-to-erasure requires deleting/anonymizing log rows → why you separate PII from event metadata.)

**H3.** Zero-downtime operations: you need to change the `order.shipped` template and roll out a new SMS provider without dropping in-flight notifications or double-sending. Walk the deploy. (Expect: template versioning — new sends use v+1, in-flight keep their pinned version; provider rollout behind a feature flag + canary % of traffic to the new adapter with the abstraction layer, watch delivery/bounce rates, ramp; blue-green for the workers with drain — stop consuming, finish in-flight, commit offsets, then swap; never in-place mutate a template.)

**H4.** Observability: what do you instrument so you know a *category* of notifications silently stopped delivering (e.g., iOS push broken by an APNs cert expiry) before users complain? (Expect: funnel metrics per channel/category — queued → sent → delivered → clicked, with delivery-rate as the SLO; alert on delivery-rate drop per provider/platform, not just error count; delivery-receipt latency; DLQ depth; Kafka lag per topic; cert/token expiry monitors; synthetic canary sends to test devices every minute; RED + USE dashboards, distributed tracing from event → provider with the notification_id as trace id.)

**H5.** 'Undo a bad decision': you originally built dedup on Kafka exactly-once semantics (EOS transactions) and it's causing throughput collapse and operational pain. Migrate to app-level idempotency without a maintenance window. (Expect: run both in parallel — keep EOS but add the SETNX/DB-unique dedup as the authoritative check; verify via shadow metrics that app-level dedup catches everything EOS did (compare duplicate counts); once confident, disable EOS transactions (switch producers to at-least-once, higher throughput); the app-level dedup was always the real safety net. Emphasize measure-before-cutover and reversibility.)

---

### Mentor's Closing Notes

**Top 3 things most candidates get wrong on this topic:**
1. **Chasing exactly-once delivery.** They design elaborate Kafka EOS + XA transactions instead of accepting at-least-once transport + idempotent effect (SETNX / unique key). Exactly-once *delivery* across a third-party provider is physically impossible; exactly-once *effect* is the achievable and correct goal.
2. **Forgetting the return path.** They design the send fan-out beautifully and never handle delivery receipts, bounces, dead tokens, or opt-out — which is where 80% of the real complexity (and the compliance risk) lives.
3. **Rate-limiting and preference-checking in the wrong place / too late.** Checking prefs per-channel four times, or applying frequency caps after fan-out, or ignoring provider-side throttling until the 429s start. Prefs/dedup/caps belong once, before fan-out; provider throttling is a token bucket on the outbound edge.

**The one insight that makes an answer truly impressive:**
Treat **coalescing/digesting** as a core architectural lever, not a UX afterthought. Collapsing "100k people liked your post" into one notification simultaneously fixes the hot-partition scaling problem, slashes provider cost, and improves user experience — one design decision that pays off in three dimensions at once. Very few candidates connect the scaling win to the product win.

**Suggested follow-up reading:**
- Uber Engineering — "Building Uber's Notification Platform (uNotify)" and their push-reliability posts.
- Meta/Instagram Engineering on notification delivery + coalescing; and the APNs/FCM provider docs on token feedback, HTTP/2 limits, and priority.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
