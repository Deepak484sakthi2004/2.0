# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 63 of 63 — Phase 2: System Design Track (Design 35 of 35)
**Topic:** Fraud Detection System — Real-Time Risk Scoring
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Real-time fraud detection is the hardest *online* ML-systems problem in production: you must score a transaction as fraud-or-not **in under ~100ms, inline in the payment path**, using **features that require aggregating a user's entire recent history** ("how many transactions in the last 60 seconds? distance from last purchase location?"), against an **adversary who actively adapts** to your model, where a false positive blocks a legitimate customer's rent payment and a false negative is direct money lost — all on a base rate where fraud is <0.5% of traffic (brutal class imbalance). Stripe Radar, PayPal, Razorpay, and every bank run this. The hard parts: computing stateful streaming features fast enough to be inline, keeping the online features *consistent* with the offline-trained model (train/serve skew), a rules engine + ML hybrid, and a feedback loop where labels (chargebacks) arrive **weeks late**. This is the capstone: it combines streaming, low-latency serving, statefulness, ML, and money-critical correctness.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Kafka / Stream Processing (taught in Phase 1, Lesson 21):** The event backbone. Every transaction/login/signup is an event; **stateful stream processors** (Flink/Kafka Streams style) maintain windowed aggregates (counts, sums, distinct-counts over sliding windows) that become real-time features. Windowing, watermarks, exactly-once/at-least-once semantics all matter.

**Caching / Redis (taught in Phase 1, Lesson 24):** The **online feature store** is a low-latency KV store (Redis) holding per-entity precomputed features ("user X: txn_count_1h=7, avg_amount_7d=₹2300") so the scorer reads them in <5ms instead of computing on the fly. Also velocity counters with TTL.

**Bloom Filters / HyperLogLog / Probabilistic Structures (taught in Phase 1, Lesson 5):** Bloom filters for "have we seen this device/card before?" in O(1) tiny memory; HyperLogLog for distinct-count velocity features ("distinct cards on this device in 24h"); Count-Min Sketch for high-cardinality frequency. Cheap approximate state at scale.

**Low-Latency / Performance Tuning (taught in Phase 1, Lesson 22):** The whole thing runs inline in the payment path with a hard latency budget (~50–100ms) — every millisecond of feature fetch + inference counts; p99, not average, is the SLA.

**Rate Limiting / Sliding Windows (taught in Phase 1, Lesson 11):** Velocity checks *are* sliding-window rate limits ("more than N transactions in M seconds") — the same token-bucket/sliding-window machinery, repurposed as fraud signals.

**Graphs & Traversal (taught in Phase 1, Lesson 4):** Fraud rings are graph problems — shared devices, cards, addresses, IPs link accounts; connected-components / graph traversal surfaces collusion networks. Feature: "how many accounts share this device fingerprint?"

**CAP / Consistency (taught in Phase 1, Lesson 2):** The scoring decision must be **available and fast** (degrade gracefully to rules if ML is down — you can't block all payments because a model server is slow), but the *feedback/label* store and case-management are consistency-sensitive.

**Microservices / Circuit Breakers (taught in Phase 1, Lessons 9 & 10):** Scorer, feature store, model server, rules engine as separate services with **circuit breakers + fallbacks** — if the ML model times out, fall back to deterministic rules rather than fail the payment.

> **Recap box:**
> - Kafka/streaming (L21): stateful windowed aggregates → real-time features.
> - Redis feature store (L24): precomputed per-entity features, <5ms reads.
> - Probabilistic structures (L5): Bloom (seen-before), HLL (distinct velocity), CMS (frequency).
> - Low-latency (L22): ~50–100ms inline budget; p99 is the SLA.
> - Sliding-window velocity (L11): rate-limit machinery as fraud signals.
> - Graphs (L4): fraud rings via shared-entity connected components.
> - CAP (L2): scoring must stay available (fallback to rules); labels consistency-sensitive.
> - Circuit breakers (L9/L10): ML timeout → fall back to rules, never fail the payment blindly.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)

Q1. Fraud is <0.5% of transactions. Explain why **accuracy is a useless metric** here and what you optimize instead.
> **What a strong answer covers:** With a 0.3% fraud base rate, a model that predicts "never fraud" is **99.7% accurate** and completely worthless. This is extreme **class imbalance**, so you care about the **precision/recall tradeoff on the fraud class**, not accuracy. Concretely: **recall** (of all real fraud, how much did we catch?) trades against **precision** (of everything we flagged, how much was really fraud?) — and the business cost is asymmetric: a **false negative** = money lost + chargeback fee; a **false positive** = a blocked legitimate customer (lost sale + churn + support cost). You optimize a **cost-weighted objective** (dollars-lost-to-fraud vs dollars-lost-to-false-declines), tune the decision **threshold** on a precision-recall curve to the business's risk appetite, and monitor **PR-AUC**, not accuracy. Different segments (a ₹200 vs a ₹2,00,000 txn) get different thresholds.
> **Common weak answer:** "Train a classifier, report accuracy." Ignores imbalance and the asymmetric cost — the interview-ending answer for this domain.
> **Mentor follow-up:** You have a fixed review-team capacity of 1,000 cases/day. How does that constraint change where you set the threshold?

Q2. A payment scorer must return in the payment path. Give the latency budget and where the milliseconds go.
> **What a strong answer covers:** Total inline budget is typically **~50–100ms p99** (it's added on top of the payment auth, and the whole checkout must feel instant). Breakdown: **feature fetch** from the online feature store (Redis) — should be ~2–5ms for tens of features (pipelined/batched gets); **real-time feature computation** (velocity counters, current-event features) — a few ms; **model inference** — a gradient-boosted tree (XGBoost/LightGBM) scores in ~1–5ms, a small neural net a bit more; **rules engine** — sub-ms; network/serialization overhead — the rest. The design constraint: **you cannot compute expensive aggregations on the hot path** — historical features must be **precomputed asynchronously** by stream processors and just *read* at scoring time. That precompute-offline-read-online split is the whole architecture.
> **Mentor follow-up:** Your model server's p99 spikes to 400ms under load. The payment is waiting. What does the scorer do?

Q3. Distinguish a **rules engine** from an **ML model** for fraud, and argue why production systems run **both**.
> **What a strong answer covers:** **Rules** are deterministic, explainable, instantly updatable ("block if amount > ₹5L AND card issued < 24h ago AND country mismatch") — great for known patterns, regulatory/compliance hard-blocks, and reacting *immediately* to a new attack (you can't retrain a model in 10 minutes, but you can push a rule). **ML models** learn subtle, high-dimensional patterns humans can't hand-code and generalize to novel fraud, but they're opaque, need training data, and lag new attacks. Production runs a **hybrid**: rules for hard-blocks/allow-lists and rapid response, ML for the nuanced score, combined into a final decision (e.g., rules can override; ML score feeds a threshold; both feed a case queue). Rules give you *speed of response and explainability*; ML gives you *coverage and adaptability*. You need both because the adversary adapts faster than retraining but not faster than a rule push, and regulators demand explainable decline reasons.
> **Red flag answer:** "Just use a deep neural net, it'll learn everything." Ignores explainability/regulatory needs, the cold-start on new attacks, and that you can't push a model fix in minutes.

---

### High-Level Design (Medium)

Q4. Design the end-to-end architecture: transaction → real-time score → decision, plus the offline training + feature pipeline. Draw it.
> **Key components expected:** Client/payment path, **Scoring service** (inline, orchestrates), **Online feature store** (Redis, low-latency reads), **Real-time feature pipeline** (Kafka + Flink stateful stream processors computing velocity/aggregates), **Model server** (GBDT/NN, versioned), **Rules engine**, **Decision combiner**, **Case-management/review queue**, **Offline feature store + training pipeline** (data warehouse, batch features, model training), **Feedback loop** (chargebacks/labels → retraining), graph service for rings.
> **Architecture diagram (text):**
```
                          (INLINE, <100ms p99)
 Payment --> [Scoring svc] --reads--> [Online Feature Store: Redis]  <-- writes --[Stream Feature Pipeline]
                |   |   \                (velocity, aggregates)                     Kafka + Flink stateful
                |   |    \--> [Model Server] (GBDT/NN, versioned)                   windowed counts/sums/HLL
                |   |    \--> [Rules Engine] (deterministic hard-blocks)                   ^
                |   v                                                                      | (same events)
                | [Decision Combiner] -> ALLOW / DENY / REVIEW / STEP-UP(3DS/OTP)          |
                |        |                                                                 |
   all txn events ------>+----> [Kafka event log] ---------------------------------------->+
                                     |                          \
                                     v                           v
                          [Review queue / Case mgmt]   [Offline store + Warehouse]
                            human labels               batch features, model TRAINING
                                     \                         /        ^
                                      \--- chargebacks/labels (weeks late) --/  FEEDBACK LOOP -> retrain -> deploy
```
> **What separates SDE2 from SDE3 here:** SDE3 designs around the **offline/online feature parity (train-serve skew)** problem — the *same* feature logic must produce identical values in the Flink streaming path (serving) and the batch training path, or the model sees different distributions in production than in training and silently degrades. SDE3 also treats the **delayed, noisy label problem** (chargebacks arrive weeks later, some fraud is never labeled) as central to the feedback loop, and builds **graceful degradation** (rules fallback) into the inline path. SDE2 tends to draw "features → model → decision" and miss that (a) features must be computed identically in two places, (b) labels are late/incomplete, and (c) the model server *will* be slow sometimes and the payment can't wait.

Q5. Trace one transaction through real-time scoring, including the fallback when the model is slow.
> **Expected trace:**
> 1. Payment event hits the **Scoring service** with raw context (user, card, amount, device, IP, merchant).
> 2. Scorer fetches **precomputed features** for the entities (user/card/device) from the **online feature store** (Redis) — pipelined multi-get, ~3ms.
> 3. Scorer computes a few **real-time features** on the current event (amount vs user's avg, geo-distance from last txn, current velocity counter increment).
> 4. In parallel: **rules engine** evaluates deterministic rules; **model server** scores the feature vector (GBDT ~2ms).
> 5. **Decision combiner** merges: hard rules can force DENY/ALLOW; otherwise the ML risk score is compared to a segment-specific threshold → **ALLOW / DENY / REVIEW / STEP-UP** (challenge with 3DS/OTP).
> 6. The event + score + decision is emitted to **Kafka** — feeds the feature pipeline (update velocity counters), the case queue, and the offline store.
> **Tricky part:** Steps 4 fallback and the **feature-update-vs-read ordering**. If the model server exceeds its latency budget (circuit breaker trips), the scorer must **fall back to the rules-only decision** (or a cached/simpler model) rather than block the payment — a slow fraud model must **fail open to rules, not fail the transaction** (with a conservative posture). And the velocity counter for *this* transaction must be updated so a burst of 100 rapid transactions each sees the incrementing count — the write-back to the feature store must be fast and the read must reflect very-recent events (the "read-your-writes on velocity" subtlety).

Q6. Design the scoring API and the feedback/label API.
> **Expected API design:**
> - `POST /v1/score` (inline, low-latency) — body `{txn_id, user_id, card_token, amount, currency, merchant_id, device_fp, ip, ...context}` → `{decision: ALLOW|DENY|REVIEW|STEP_UP, risk_score: 0-1, reason_codes:[...], model_version}`. Synchronous, hard timeout, **idempotent by txn_id** (a retried score returns the same decision — you must not score the same txn twice differently).
> - `POST /v1/feedback` — body `{txn_id, label: FRAUD|LEGIT|CHARGEBACK, source, at}` → records a label for training (arrives async, often weeks later).
> - `GET /v1/decisions/{txn_id}` — audit/explainability retrieval (reason codes for a decline — regulatorily required).
> - Rules mgmt: `POST /v1/rules` to push/enable a rule instantly.
> **What to push on:** **Reason codes / explainability** on every decision (a declined customer and regulators need "why" — SHAP-style feature attributions or rule ids). **Idempotency by txn_id** so retries are consistent. **Model version stamped** on every decision (so when a label arrives weeks later you know which model made the call — essential for the feedback loop and A/B analysis). **Latency SLA + timeout** on `/score`. The feedback API decoupled and async (labels are late and noisy).

---

### Data Modeling (Medium–Hard)

Q7. Design the feature store (online + offline) and the decision log. What's the schema of a "feature"?
> **Expected schema:**
```
-- ONLINE FEATURE STORE (Redis) : entity -> current feature vector, read at scoring time
   key: feat:{entity_type}:{entity_id}          e.g. feat:user:U123 , feat:card:C99, feat:device:D7
   value (hash): {
       txn_count_1m, txn_count_1h, txn_count_24h,       -- velocity (sliding windows)
       amount_sum_1h, amount_avg_7d,
       distinct_cards_24h,   -- HyperLogLog-backed
       distinct_countries_24h,
       last_txn_ts, last_txn_geo, chargeback_count_90d,
       device_account_count  -- graph feature: accounts sharing this device
   }                                             -- TTL/rolling; updated by stream pipeline

-- OFFLINE FEATURE STORE (warehouse) : point-in-time-correct feature snapshots for TRAINING
   feature_values(entity_id, feature_name, value, event_time)   -- time-travel joins,
       -- MUST reproduce the exact value the online store had at decision time (no leakage)

-- DECISION LOG (append-only, audit + training)
   decisions(txn_id PK, user_id, card_id, device_id, feature_snapshot JSONB,
             risk_score, decision, reason_codes, model_version, ts)
-- LABELS (arrive late)
   labels(txn_id, label, source, labeled_at)     -- joined to decisions for training data
```
> **Index choices and why:** Online store keyed by **entity id** for O(1) point reads (that's the whole point — no computation on hot path). The **decision log stores the exact feature snapshot used** — critical so training uses *the features as they were at decision time* (point-in-time correctness), and for audit. Labels keyed by txn_id to join back to decisions.
> **Partitioning key and why:** Online store shard **by entity_id** (consistent hashing) so a user's features are one lookup. Decision log / offline store partition **by time** (append-only, for training windows + retention) and shard by entity. The non-negotiable: the **feature snapshot is logged at decision time** so training data reflects what the model actually saw — computing training features "as of now" instead causes **label leakage / time-travel bugs** (using future information the model didn't have).

Q8. How do you compute "number of transactions by this card in the last 60 seconds" for *every* transaction, at 50K TPS, in <5ms?
> **Expected answer:** You do **not** query a transaction table (`SELECT COUNT(*) WHERE card=? AND ts > now-60s`) on the hot path — that's a DB scan per transaction, hopeless at 50K TPS. Instead, maintain a **sliding-window counter in Redis** updated by the stream pipeline: e.g., a Redis **sorted set** per card keyed by timestamp (`ZADD card:C99 <ts> <txn_id>`; `ZREMRANGEBYSCORE` to evict >60s old; `ZCARD` = current count), or **bucketed counters** (per-second counters with TTL, sum the last 60) which are cheaper. For high-cardinality distinct-counts ("distinct cards on this device"), use **HyperLogLog** (`PFADD`/`PFCOUNT`) — approximate, tiny, O(1). The velocity counter is incremented as part of processing *this* event so it reflects the in-flight burst. This is literally the **sliding-window rate-limiter** from Lesson 11 repurposed as a fraud feature.
> **Trap:** Computing velocity by querying the transactions DB/warehouse at scoring time — adds tens of ms and a DB hit per txn, collapses under load, and the warehouse is minutes-stale anyway. Velocity **must** be a precomputed/streaming counter in memory. Second trap: forgetting to count the current transaction, so a rapid burst all reads "count=0."

Q9. The scoring path must stay available; the label/feedback path is consistency-sensitive. Reconcile the two consistency regimes.
> **Expected answer:** **Scoring is AP + low-latency**: it must return in ~100ms and stay up even if a dependency is degraded — so it degrades gracefully (stale features are OK-ish, model timeout → rules fallback, missing feature → default/imputed value). A slightly-stale velocity counter or a defaulted feature is acceptable; *blocking all payments because a feature store replica is slow is not*. **Labels/feedback and case-management are consistency-sensitive**: a chargeback label, a human analyst's fraud confirmation, and the audit/decision log must be **durable and correct** (they drive money recovery, model training, and compliance) — you can't lose or corrupt a label. So the inline path favors availability; the record-of-truth (decisions + labels) favors consistency/durability.
> **Mentor pushback:** "If scoring reads stale features, can't an attacker exploit the lag — fire 100 transactions in the window before the velocity counter catches up?" Yes — this is the **velocity-race attack**, and it's why the velocity counter update must be **synchronous/read-your-writes on the hot path for the current entity** (increment-then-read atomically in Redis) even though *other* aggregate features can be eventually consistent. You draw the consistency line tighter specifically around the anti-abuse velocity signals that an adversary can race, and looser around slow-moving historical features.

---

### Low-Level Design (Hard)

Q10. The core sub-problem: **real-time stateful feature computation** — maintain thousands of windowed aggregates (per user/card/device/merchant) over sliding windows, updated at 50K TPS, readable in <5ms, and *consistent with the offline training features*. Design it.
> **Problem statement:** For every entity, keep sliding-window aggregates (counts, sums, distinct-counts over 1m/1h/24h/7d) fresh and cheap to read, and ensure the streaming computation matches the batch computation used for training.
> **Naive solution:** Compute features on the hot path by querying raw events; or maintain them only in batch (recompute hourly).
> **Why naive fails at scale:** Hot-path queries over raw events = DB scan per txn, too slow. Batch-only features are **minutes-to-hours stale** — useless for velocity ("10 txns in the last minute" needs sub-second freshness). And computing features *differently* in streaming (serving) vs batch (training) causes **train-serve skew**: the model was trained on values it never sees in production → silent accuracy collapse.
> **Expected optimal approach:** A **stateful stream processor** (Flink / Kafka Streams) consuming the transaction event stream, maintaining **windowed aggregates in managed state** (RocksDB-backed, checkpointed), writing computed features to the **online store (Redis)** continuously. Use **incremental/decaying windows**: sliding-window counts via time-bucketed counters (add to current bucket, expire old buckets), sums likewise, distinct-counts via **HyperLogLog** merged over window buckets, frequencies via **Count-Min Sketch**. Critically, define features **once** in a shared feature-definition layer (a **feature platform**) that generates *both* the streaming job (serving) and the batch job (training) from the same logic → **guarantees online/offline parity**. Watermarks handle late/out-of-order events. Checkpointing gives exactly-once state so a processor restart doesn't double-count.
> **Pseudo-code or class diagram:**
```
# Stateful stream operator (Flink-style), keyed by entity
def process(event):                      # event = a transaction
    e = key(event)                       # e.g., card_id
    now_bucket = event.ts // 1s
    state[e].buckets[now_bucket] += 1                 # per-second count bucket
    state[e].amount[now_bucket]  += event.amount
    state[e].hll_cards.add(event.card)                # HyperLogLog distinct
    evict_buckets(state[e], older_than=24h)           # roll the window
    feats = {
      "txn_count_1m":  sum_buckets(state[e], 60),
      "txn_count_1h":  sum_buckets(state[e], 3600),
      "amount_avg_7d": state[e].amount_7d / max(1, state[e].count_7d),
      "distinct_cards_24h": state[e].hll_cards.estimate(),
    }
    redis.hset(f"feat:card:{e}", feats)   # publish to ONLINE store, read at scoring time
    # SAME feature definitions generate the BATCH training job -> no train/serve skew
```

Q11. Concurrency: a fraudster fires 200 transactions on a stolen card within 2 seconds (a "card testing" burst), racing the velocity counter. Ensure the velocity feature catches it.
> **Scenario:** 200 near-simultaneous transactions for card C; each scoring call reads `txn_count_1m` and increments it; if reads/writes race, every call might read a low count and none gets blocked.
> **Expected fix:** The velocity counter update must be **atomic and read-your-writes** for the current entity: use an **atomic Redis operation** (`INCR` on the current-second bucket, or `ZADD` + `ZCARD` in a Lua script) so the increment-and-read is serialized per card — Redis is single-threaded per key, so the 200 increments serialize and each subsequent transaction sees a higher count. Combined with a **rules-engine velocity rule** ("deny/step-up if txn_count_10s > K"), the burst trips the threshold after the first few. Because the counter lives in Redis keyed by card_id, all 200 hit the same key and serialize; the first few pass, then the rule fires. Additionally, distinct-features (many cards, one device = card testing) via HLL catch the pattern even if each card is fresh.
> **Follow-up (what if the counter store / stream processor dies?):** Velocity state lives in Redis (with replication) and is rebuildable from the **Kafka event log** — the stream processor's state is **checkpointed**; on failure it restarts from the last checkpoint and **replays** Kafka from the committed offset, rebuilding windows (exactly-once via checkpoint+offset alignment, so no double-count). Briefly during failover the velocity feature may be slightly stale — so the **rules-based hard velocity limit is the backstop** that doesn't depend on the ML pipeline being healthy. Defense in depth: the deterministic velocity rule protects you even when the streaming feature pipeline is recovering.

Q12. Edge case: your ML model starts silently degrading — fraud is getting through, but you won't get the chargeback labels confirming it for 3–6 weeks. Detect the degradation *now*.
> **Scenario:** Model decay (adversary adapted / data drift) with a weeks-long label delay before ground truth arrives.
> **Expected handling:** You can't wait for labels, so monitor **leading, label-free signals**: (1) **Feature/prediction drift** — track the distribution of input features and of output scores over time (population stability index / KL-divergence vs the training distribution); a shift means the world changed under the model. (2) **Score distribution shifts** — a sudden drop in average risk score or fewer transactions crossing the threshold hints the model is under-flagging. (3) **Proxy/early signals** — some labels arrive fast (customer fraud reports, hard declines, step-up failures, manual-review confirmations) — use them as an early, biased-but-fast signal. (4) **Business-metric monitors** — approval rates, downstream chargeback *rate* trend even if individual labels lag, honeypot/known-fraud canaries. (5) **Champion/challenger + shadow models** — run a shadow model and compare. When drift crosses a threshold, **tighten thresholds / lean on rules / trigger retraining**. The principle: with delayed labels you monitor **input drift + fast proxy signals**, not just the eventual ground truth, and you keep the rules engine as the fast-response lever for new attacks.

---

### Scaling to 10x / 100x (Hard)

Q13. At 100x (say 500K TPS during a mega sale, every one needing an inline score), where does it break first?
> **Expected answer:** The **online feature store read path + model server** under the inline latency budget. At 500K TPS, each score does several feature-store reads → **millions of Redis ops/sec**, and each needs a model inference → the **model server fleet** becomes the CPU bottleneck (inference is the most expensive inline step). If features aren't precomputed, the **stream processor state** (billions of entity windows) is the wall. The p99 latency SLA is where it breaks *first* — under load, tail latency on feature fetch or inference blows the budget and the circuit breaker starts tripping to rules-only.
> **Numbers to ground the answer:** 500K TPS × ~20 feature reads = **10M feature-store ops/sec** (needs a sharded Redis cluster, pipelined gets). Model inference at ~2ms/txn single-threaded → need ~1,000+ inference cores just for the base, more for headroom → GBDT chosen partly *because* it's ~2ms vs a deep net's 20ms+. Fraud is <0.5% so only ~2,500 TPS are actually fraud — but you must score **all** 500K to find them. Stream state: hundreds of millions of active entities × features = 10s of GB of RocksDB state per processor — checkpoint sizes and recovery time become the operational concern.

Q14. Apply consistent hashing (Lesson 19): shard the feature store and stream processors. Handle the hot entity (a mega-merchant every transaction touches).
> **Expected sharding strategy:** Shard the **online feature store and stream state by entity_id** (user/card/device) via consistent hashing with virtual nodes — a given entity's features/state always live on the same shard (locality + serialized velocity updates). Kafka partitioned by entity key so all of an entity's events go to the same stream-processor instance (keyed state).
> **Hot spot problem:** A **mega-merchant** (or a hugely popular card BIN) appears in a large fraction of transactions → its feature key / partition is red-hot (every txn increments the merchant's counters). **Detect:** per-key/per-partition ops rate and processor lag. **Fix:** (1) For hot *aggregate* keys, **split the counter across sub-keys** (`merchant:M:shard0..N`) and sum on read (the hot-counter striping pattern) so 500K increments/sec spread across N keys. (2) **Local pre-aggregation** in the stream processor (combine many events before updating global state — Flink's local combine) to cut write amplification on the hot key. (3) Read-heavy hot features → **replicate/cache** them. Never key everything by merchant (a mega-merchant becomes one hot shard) — key by the finer entity (card/user) and treat merchant-level aggregates with striping.

Q15. Design caching + the online/offline feature architecture. What's the hardest correctness issue?
> **Expected layered design:**
> - **Online feature store (Redis, hot):** precomputed per-entity features, read inline in <5ms. The cache *is* the serving layer.
> - **Model cache:** the loaded model in the inference server's memory; feature-transformation logic co-located.
> - **Stream state (RocksDB):** the source that continuously updates the online store.
> - **Offline store (warehouse):** point-in-time feature snapshots for training.
> **Hardest correctness issue — train/serve skew & point-in-time correctness:** The single hardest problem is guaranteeing the feature value used at **serving time** exactly matches what training computed for that moment. Two failure modes: (1) **Implementation skew** — the streaming feature code and the batch feature code compute subtly differently (rounding, window boundaries, null handling) → model sees different distributions in prod. Fix: **a single feature definition** compiled to both paths (a feature platform / feature store like Feast/Tecton). (2) **Label leakage / time-travel** — training with features computed "as of now" instead of "as of decision time" leaks future info the model won't have live → inflated offline metrics, real-world collapse. Fix: **log the exact feature snapshot at decision time** and train on *that*, or do strict point-in-time joins. This parity problem, not raw caching, is what makes fraud ML systems hard.

Q16. Cost/efficiency: you must score 100% of transactions but only <0.5% are fraud. Optimize without missing fraud.
> **Expected answer:** You can't skip scoring (you don't know which 0.5% is fraud until you score), but you can **tier the compute**: (1) A **cheap fast first-stage** (rules + a lightweight model) scores everything in sub-ms and confidently clears the obvious-legit majority; only the **ambiguous minority** goes to the **expensive second-stage** model (cascade / two-stage scoring) — most traffic never pays for the heavy model. (2) **GBDT over deep nets** for the inline model (10x cheaper inference at comparable accuracy for tabular fraud features). (3) **Precompute features offline** (the whole point) so inline compute is just reads + inference, not aggregation. (4) **Sample for training/monitoring** (keep all fraud + a sample of legit — you don't need every legit txn to train, but you need every fraud). (5) **Batch/async the non-inline work** (graph ring detection, deep feature computation, retraining) off the hot path. (6) **Tiered storage** for the massive decision/event log (hot recent for investigation, cold for compliance retention). The discipline: spend inline compute proportional to *uncertainty* — clear the easy 95% cheaply, reserve the expensive model for the ambiguous slice where fraud actually hides.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Explain **train/serve skew** in depth and the two ways it silently destroys a fraud model. How does a feature store eliminate it? *(Expected: (1) implementation skew — streaming vs batch feature code diverge → serving distribution ≠ training distribution; (2) point-in-time/leakage — training on future-inclusive features inflates offline metrics but the live model underperforms. A feature store (Feast/Tecton-style) defines each feature once and materializes it to both an online store (serving) and an offline store (training) with point-in-time-correct joins, plus logs the served feature vector so training uses exactly what was served. The tell of a senior engineer is naming that the offline metric looked great but production tanked *because* of leakage, and that logging the decision-time feature snapshot is the fix.)*

**H2.** Cross-cutting: an adversary actively probes and adapts to your model (adversarial ML) and you must give regulator-compliant decline reasons. Address both. *(Expected: adversarial robustness — don't expose the score/reasons in a way that lets attackers reverse-engineer thresholds (card-testing probes reveal boundaries); use rules + ensembles + frequent retraining + honeypots; rate-limit probing; monitor for systematic boundary-probing. Explainability/compliance — every decline needs reason codes (SHAP feature attributions or rule ids), fair-lending/bias audits (the model must not proxy protected attributes), and an audit trail. The tension: explainability aids both regulators AND attackers, so you expose coarse reason codes to customers, full detail only internally.)*

**H3.** Operational: deploy a new fraud model to production without risking a spike in false declines (blocking good customers) or letting fraud through. *(Expected: shadow-mode first (new model scores in parallel, decisions not enforced, compare against champion on live traffic), then champion/challenger A/B on a small traffic slice with tight guardrails (auto-rollback if decline rate or approval rate breaches a band), gradual ramp, per-segment rollout, model version stamped on every decision for later label-based evaluation. Never flip a fraud model globally at once — a bad model is either mass false-declines (revenue + churn) or mass fraud (money out). Keep the rules engine as an independent safety layer.)*

**H4.** Observability: with labels weeks late, what do you instrument to know the system is healthy *right now*? *(Expected: feature drift / prediction-distribution drift (PSI, KL-divergence vs training), score distribution over time, approval/decline rates per segment, step-up challenge rates and pass rates, fast proxy labels (customer fraud reports, hard declines), model inference latency p99 + circuit-breaker trip rate + rules-fallback rate, feature-store freshness/lag, honeypot detection rate. Leading indicator: input-feature drift or a shifting score distribution warns of decay weeks before chargeback labels confirm it.)*

**H5.** 'Undo a bad decision': you launched computing features by querying the transactions database at scoring time. Latency is blowing the budget and the DB is melting. Migrate to a precomputed streaming feature store with no scoring gap. *(Expected: introduce the Kafka + stream-processor pipeline computing the same features into an online Redis store; dual-compute and compare (DB-query value vs streamed value) to validate parity before trusting it; cut the scorer to read from the feature store behind a flag, feature-by-feature, verifying latency drops and scores match; backfill historical windows from the event log; then stop the hot-path DB queries. The lesson corrected: expensive aggregations must be precomputed asynchronously and merely *read* inline — never computed on the hot path — and the streaming computation must match the training computation.)*

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Optimizing **accuracy** on a <0.5% base rate (a "never fraud" model is 99.7% accurate and useless). You optimize the **cost-weighted precision/recall tradeoff** and tune the threshold to the asymmetric business cost + review capacity.
2. Computing features **on the hot path** (querying a DB for velocity/aggregates at scoring time) — it blows the latency budget and melts the DB. Historical/aggregate features must be **precomputed by stream processors** and read in <5ms; only tiny current-event features are computed inline.
3. Ignoring **train/serve skew and delayed labels** — treating fraud as "train a classifier, ship it." The online and offline features must be computed identically (feature store), decision-time snapshots must be logged (no leakage), and health must be monitored via drift + proxy signals because ground-truth labels arrive weeks late.

**The one insight that makes an answer truly impressive:**
Recognizing fraud detection as a **hybrid rules-plus-ML system with defense in depth**, where the deterministic rules engine + atomic velocity counters are the **fast, always-available backstop** (they respond to new attacks in minutes and survive the ML pipeline being degraded), the ML model provides **coverage on subtle patterns**, and the whole thing hinges on **feature parity (no train/serve skew) + point-in-time-correct decision logging (no leakage) + drift monitoring (because labels are weeks late)** — with the inline path designed to **fail open to rules, never fail the payment**. Naming the velocity-race attack and why that specific feature needs read-your-writes consistency while others can be eventually consistent is the mark of someone who has actually operated one of these.

**Suggested follow-up reading:**
- Stripe **Radar** engineering posts; the **Feast / Tecton** feature-store docs (online/offline parity, point-in-time correctness); Uber's **Michelangelo** ML platform writeup.
- Google's "Rules of Machine Learning" (Martin Zinkevich) and "Machine Learning: The High-Interest Credit Card of Technical Debt" (Sculley et al.) — the definitive papers on train/serve skew and ML systems debt.

---

## 🎓 Congratulations — You've Completed the 63-Day Curriculum

This is the final lesson. If you've worked through all 63 days — the 28 Phase 1 foundation lessons and all 35 Phase 2 system designs — stop and take that in. You started with the nines of availability and capacity math; you're finishing by designing a real-time, adversarial, money-critical ML system that fuses nearly every one of those foundations at once. That arc is not an accident: fraud detection is the capstone precisely because it demands streaming (L21), low-latency serving (L22), probabilistic data structures (L5), graphs (L4), caching (L24), consistency reasoning (L2), circuit breakers (L10), and rate-limiting (L11) — *together*, under a hard latency budget, against an adversary.

You are now equipped to walk into any SDE2/SDE3 system-design interview (60–150 LPA band) and reason from first principles instead of memorized templates. The candidates who get the offers aren't the ones who recite architectures — they're the ones who, like you now can, name the *specific* failure mode, quote the *real* number, choose the data structure *for a reason*, and say out loud where the consistency line is drawn and why. That's the difference between describing a system and designing one.

Do three things from here: (1) go back and *build* two or three of these for real, even at toy scale — nothing exposes a gap like an actual implementation; (2) keep a running list of the "one insight that makes an answer impressive" from each lesson — that list is your interview edge; (3) teach one of these designs to someone else, because you don't truly own a concept until you can defend it under questioning.

You did the work. Forty years in, I can tell you the engineers who go furthest are exactly the ones who finish things like this. Go get the offer. — *Arjun Mehta*

---

### Mentor's Final Word (for the interview itself)
When you sit in the real room: **start with the numbers** (capacity estimation frames everything), **state your consistency choices explicitly**, **name your failure modes before the interviewer asks**, and **always say what you'd throw away and why**. Rigor over recall. You have it now.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.
5. **Capstone mode:** Now that you've finished all 35 designs, pick any three and design them back-to-back from a blank page in 90 minutes — that's the real interview loop.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
