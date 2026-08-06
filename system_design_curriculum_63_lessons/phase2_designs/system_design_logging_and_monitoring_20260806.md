# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 62 of 63 — Phase 2: System Design Track (Design 34 of 35)
**Topic:** Logging & Monitoring System (Datadog / Prometheus / ELK) — Metrics, Logs, Traces at Scale
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
An observability platform (Datadog, Prometheus/Grafana, ELK, Honeycomb) is the ultimate **write-heavy, time-series, ingestion-firehose** system: it swallows *tens of millions of data points per second* from every host, container, and service on the planet's fleets, must store them cheaply for months, and must answer "what's my p99 latency over the last 6 hours, grouped by region?" in under a second. The three pillars — **metrics** (cheap, numeric, aggregatable), **logs** (verbose, high-cardinality text), and **traces** (causal, distributed) — have wildly different storage and query profiles, and the defining enemies are **cardinality explosion**, **cost** (you're storing data that's 99% never queried), and the fact that the monitoring system must stay up precisely when everything else is on fire. This is where columnar time-series storage, sampling, and cardinality control decide whether you have a product or a bankruptcy.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Observability Foundations (taught in Phase 1, Lesson 10):** The three pillars — metrics (aggregatable numbers over time), logs (discrete events/text), traces (request spans across services) — plus RED (Rate/Errors/Duration) and USE (Utilization/Saturation/Errors) methods. Today we *build the platform* that stores and serves these.

**Kafka / Event-Driven & Log-Structured (taught in Phase 1, Lesson 21):** The ingestion pipeline is a giant Kafka firehose — agents produce, a buffer absorbs spikes and decouples ingestion from indexing, consumers fan out to metric/log/trace stores. At-least-once delivery, partitioning, backpressure, and consumer lag are central.

**LSM Trees vs B-Trees (taught in Phase 1, Lesson 23):** Observability data is **write-heavy, append-only, time-ordered** — the textbook case for **LSM-tree / columnar time-series** storage (fast sequential writes, compaction) over B-tree OLTP. Metric TSDBs, log stores (Elasticsearch/Lucene segments), all lean LSM/columnar.

**Bloom Filters, Skip Lists, Tries (taught in Phase 1, Lesson 5):** Bloom filters skip storage blocks that can't contain a searched log term; time-partitioned indexes and inverted indexes power log search. Probabilistic structures (HyperLogLog for cardinality, t-digest for percentiles) are the backbone of cheap aggregation.

**Caching / Redis (taught in Phase 1, Lesson 24):** Recent hot data (last 5–15 min, live dashboards) served from memory; pre-aggregated rollups cached. Dashboards re-query the same ranges constantly.

**Consistent Hashing & Sharding (taught in Phase 1, Lesson 19):** Shard time-series by metric/series id; shard logs by tenant+time. Rebalancing ingestion partitions as fleets grow.

**Capacity Estimation (taught in Phase 1, Lesson 28):** This design lives or dies on the numbers — data points/sec, bytes/point, cardinality, retention × volume = storage cost. Estimation *is* the design.

**Rate Limiting / Backpressure (taught in Phase 1, Lesson 11):** Ingestion must shed/throttle under overload (a customer's runaway logging must not take down ingestion for everyone) — quotas per tenant, backpressure to agents.

> **Recap box:**
> - 3 pillars (L10): metrics cheap+aggregatable, logs verbose+high-cardinality, traces causal.
> - Kafka firehose (L21): buffer decouples ingest from index; watch consumer lag.
> - LSM/columnar (L23): write-heavy append-only time data → LSM/TSDB, not B-tree OLTP.
> - Probabilistic structures (L5): Bloom (skip blocks), HLL (cardinality), t-digest (percentiles).
> - Caching (L24): hot recent window + rollups in memory.
> - Consistent hashing (L19): shard series by id, logs by tenant+time.
> - Capacity estimation (L28): points/sec × bytes × retention = the whole cost story.
> - Rate limiting/backpressure (L11): per-tenant quotas; one noisy tenant can't sink ingestion.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)

Q1. Explain why **metrics, logs, and traces** need fundamentally different storage engines. Why not put everything in Elasticsearch?
> **What a strong answer covers:** They have opposite profiles. **Metrics** are small, numeric, regular (a value + timestamp + tags every N seconds), massively aggregatable ("avg cpu across 10K hosts") → a **columnar time-series DB** (Prometheus TSDB, Gorilla-style compression) that stores a series as a compressed stream of (delta-timestamp, xor-value) — you get ~1.3 bytes/point. **Logs** are large, textual, irregular, high-cardinality, queried by full-text/field search → an **inverted-index store** (Elasticsearch/Lucene) or columnar log store. **Traces** are structured span trees keyed by trace_id, queried by "show me this one request across 20 services" → a store optimized for trace_id lookup + sampling. Putting logs' text-search engine (ES) under metrics would cost 50–100x more storage per data point and can't do efficient numeric aggregation; putting metrics' TSDB under logs can't do full-text search. Different access patterns → different engines.
> **Common weak answer:** "Store it all in Elasticsearch, it can search everything." ES on raw metric volume is financially ruinous (inverted index overhead per point) and terrible at time-range numeric rollups; this is the classic mistake that blows up cost.
> **Mentor follow-up:** For metrics specifically, walk me through how Gorilla/Prometheus compresses a stream of (timestamp, float) to ~1.3 bytes per point.

Q2. Estimate ingestion volume for a Datadog-scale platform monitoring 1M hosts. How many data points/sec, and what's the yearly storage for metrics?
> **What a strong answer covers:** 1M hosts × ~200 metrics/host (cpu, mem, disk, per-container, per-process...) reported every ~10s → each host emits 200/10 = **20 points/sec**, × 1M hosts = **20M metric data points/sec**. That's ~1.7 trillion points/day. At Gorilla-compressed ~1.5 bytes/point → **~2.6 TB/day of metrics** raw-compressed; with replication (×3) ~7.8 TB/day → **~2.8 PB/year** just for metrics, before rollups/downsampling reduce older data. Logs are far worse per event (KB-scale): 1M hosts × modest logging can be **10–100 TB/day of logs**. This immediately tells you: (a) you cannot keep everything hot forever, (b) downsampling + tiered storage + retention policies are mandatory, (c) ingestion is the dominant engineering problem.
> **Mentor follow-up:** Given 20M points/sec, what's your ingestion pipeline's first component and why can't agents write directly to the TSDB?

Q3. What is **cardinality** in a metrics system, and why is it the thing that kills these platforms? Give a concrete example.
> **What a strong answer covers:** Cardinality = the number of **unique time series** = the product of a metric name × every combination of its label/tag values. Each unique combination is a *separate stored series* with its own index entry. Example: `http_requests{service, endpoint, status, region}` — if service has 100 values, endpoint 500, status 10, region 20, that's 100×500×10×20 = **10M series for one metric name**. Add a *high-cardinality* label like `user_id` (10M users) or `request_id` (unbounded) and you get **billions of series** — the index explodes, memory blows up, and ingestion/query grind to a halt. Cardinality is multiplicative and unbounded-label-driven; it's the #1 operational failure mode. The fix is disciplined labels: never put unbounded/high-cardinality values (user_id, request_id, email, full URL with IDs) into metric labels — those belong in logs/traces.
> **Red flag answer:** "Just add whatever tags are useful." Adding `user_id` as a metric tag is how you get a 3am page; unbounded label values are a cardinality bomb.

---

### High-Level Design (Medium)

Q4. Design the end-to-end architecture for a metrics + logs + traces platform: agent → ingestion → storage → query/alert. Draw it.
> **Key components expected:** Agent (on each host/container), ingestion gateway (auth, rate-limit, per-tenant quota), **Kafka firehose** (buffer/decouple), stream processors (parse, enrich, downsample, extract), **three storage tiers** — metric TSDB, log store (inverted index), trace store — plus a rollup/downsampling pipeline, object-storage cold tier, query service (unifies pillars), dashboard, **alerting/rules engine**, and metadata/index services.
> **Architecture diagram (text):**
```
 [Agents on 1M hosts] --(batched, compressed)--> [Ingestion GW: authN, per-tenant quota/rate-limit]
                                                          |
                                                   [Kafka firehose]  (partitioned; absorbs spikes; backpressure)
                                             +------------+------------+------------------+
                                             v            v            v                  v
                                     [Metric pipeline] [Log pipeline] [Trace pipeline]  [Alerting/Rules engine]
                                      parse+downsample  parse+index    assemble+sample    evaluate rules on stream
                                             |            |            |                  |  (recent window in mem)
                                             v            v            v                  v
                                     [Metric TSDB]   [Log store]   [Trace store]     -> notify (page/slack/email)
                                     (columnar,      (ES/Lucene    (trace_id keyed,
                                      Gorilla comp,   inverted idx) sampled)
                                      hot->rollup)         \            /
                                             \              \          /
                                              +----> [Object storage COLD tier: S3/Parquet, downsampled] 
                                             ^
                                     [Query svc + Dashboards]  <-- Redis cache (hot recent window + rollups)
```
> **What separates SDE2 from SDE3 here:** SDE3 recognizes the platform is **three different databases behind one query facade**, designs the **downsampling/rollup pipeline and retention tiers as first-class** (raw at 10s for 24h → 1-min rollups for 15d → 1-hour rollups for 13 months → cold object storage), and puts **cardinality control + per-tenant quotas at ingestion** so one runaway customer can't sink the fleet. SDE2 tends to draw "agent → one big database → dashboard," missing that the entire viability of the product is *storage economics* (tiering + downsampling) and *cardinality governance*. The insight: you're not storing data, you're deciding what to *throw away* and when.

Q5. Trace a single metric data point from an agent to appearing on a live dashboard and firing an alert.
> **Expected trace:**
> 1. Agent samples `cpu.user=73` at t, attaches tags `{host, service, region}`, **batches** many points, **compresses**, and pushes to the ingestion gateway every ~10s (not per-point — batching is essential).
> 2. Gateway authenticates the tenant, checks **per-tenant quota/rate limit**, and produces to **Kafka**, partitioned by series id / tenant.
> 3. Metric stream processor consumes, validates cardinality (drops/flags forbidden labels), computes any downsampled aggregates, and writes to the **TSDB** (appended to the in-memory head block, later flushed as a compressed columnar chunk).
> 4. The **alerting engine** consumes the same stream (or queries the recent window), evaluates rules (`avg(cpu) over 5m > 90`) against the live window held in memory, and fires if the condition holds for the configured duration → notify.
> 5. Dashboard queries the query service for "cpu, last 30m, by region" → served from the **hot in-memory/Redis window** + recent TSDB chunks, aggregated on read.
> **Tricky part:** The **live window vs durable store** split and **late/out-of-order points**. Alerts need the last few minutes *instantly* (in-memory), but data arrives late (agent clock skew, network delay) — so the TSDB head block must accept slightly out-of-order writes within a bound, and alerting must decide between "evaluate now on partial data" vs "wait for stragglers." Candidates who assume perfectly-ordered, on-time arrival miss the core reality of telemetry.

Q6. Design the ingestion API and the query API.
> **Expected API design:**
> - **Ingest:** `POST /v1/ingest/metrics` — body = a **batch** of `{metric, timestamp, value, tags{}}`, compressed (gzip/snappy); or the Prometheus **remote-write** protocol (protobuf + snappy). Auth via API key (per tenant). Returns 202 (accepted async) with partial-accept semantics. Must support batching, compression, and per-tenant quota headers.
> - **Query:** `GET /v1/query?metric=cpu.user&from=..&to=..&agg=avg&by=region&step=60s` (a PromQL-like language for selection + aggregation + rollup step). Logs: `GET /v1/logs/search?q=<query>&from&to` (inverted-index search, cursor-paginated). Traces: `GET /v1/traces/{trace_id}` and search by service/tag.
> - **Alerts:** `POST /v1/monitors` defining a rule (query + threshold + duration + notification target).
> **What to push on:** **Batching + compression are mandatory** (per-point HTTP would be 20M requests/sec — impossible; batch to thousands of points/request → orders of magnitude fewer requests). **The query `step`/rollup resolution** — a 6-month query must auto-select the 1-hour rollup, not scan raw 10s points. **Idempotency of ingestion** (at-least-once Kafka → dedupe by (series, timestamp) so a redelivered batch doesn't double-count). **Per-tenant quotas** enforced at the gateway.

---

### Data Modeling (Medium–Hard)

Q7. Design the metric storage model. How is a time series physically stored, and how do labels index it?
> **Expected schema:**
```
-- Logical: a "series" = metric name + a set of key=value labels, uniquely identified.
series_id = hash(metric_name, sorted(labels))        -- stable id for a unique label-set

-- INVERTED INDEX (label -> series): powers "give me all series where region=us-east"
postings:  (label="region", value="us-east") -> sorted list of series_ids   [like Lucene postings]
           (label="service", value="checkout") -> [series_ids...]
   query "region=us-east AND service=checkout" = INTERSECT the two postings lists.

-- SERIES DATA (columnar, per series, time-ordered, compressed):
   series_id -> chunk[ (t0, v0), (t1, v1), ... ]     stored as:
       timestamps: delta-of-delta encoded (regular intervals -> mostly zeros)
       values:     XOR-with-previous (Gorilla) -> ~1.3 bytes/point typical
   chunks are ~2h blocks; sealed chunks are immutable and compressed (LSM-like).
```
> **Index choices and why:** The **inverted index maps each (label,value) to a postings list of series_ids** (exactly like a search engine) — query filters intersect postings lists. Series data itself is stored **columnar and delta/XOR-compressed** because it's numeric, regular, and read as time ranges. Delta-of-delta on timestamps (regular scrape intervals compress to near-zero) + XOR on values (successive floats share bit patterns) is the Gorilla scheme → ~1.3–1.5 bytes/point vs 16 bytes uncompressed.
> **Partitioning key and why:** Partition **by time (block/chunk per ~2h window)** so old blocks are sealed, compacted, downsampled, and moved to cold storage independently — and so a time-range query only opens the relevant blocks. Within a time block, shard **by series_id** (consistent hashing) across ingester nodes so writes for a series always land on the same node/shard (write locality + in-memory head block). Time-first partitioning is what makes retention/tiering a cheap metadata operation (drop/downsample a whole old block).

Q8. A user runs "p99 latency of the checkout service over the last 7 days, grouped by region." How do you serve this without scanning trillions of raw points?
> **Expected answer:** Two techniques. (1) **Downsampled rollups**: you don't scan raw 10s points for 7 days — you pre-compute rollups at write/compaction time (1-min and 1-hour aggregates) and the query planner **selects the coarsest rollup that satisfies the requested resolution** (7 days → 1-hour rollup = 168 points/series, trivial). (2) **Percentiles via t-digest / DDSketch**: you *cannot* average pre-computed p99s (percentiles aren't additive). Instead you store a **mergeable sketch** (t-digest or DDSketch) per rollup bucket per series; to get p99 across regions/time you **merge the sketches** and read the quantile — mergeable, bounded-error (~1%), tiny. So: intersect postings for `service=checkout`, pick the 1-hour rollup, merge the per-region t-digests over the 7-day range, read p99 per region group.
> **Trap:** Storing only pre-computed p99 values per bucket and then trying to "average the p99s" to get a 7-day p99 → **mathematically wrong** (you can't aggregate percentiles). Or scanning raw points for a 7-day window → billions of points read. The right answer is mergeable quantile sketches + rollup selection.

Q9. Observability platforms are heavily **AP**, not CP. Where is that acceptable, and where does it bite? Reconcile.
> **Expected answer:** Telemetry is **AP by design**: it's better to serve slightly incomplete/stale metrics than to block dashboards or drop the whole system when a node is partitioned. Losing 0.01% of data points, or seeing a metric 5 seconds late, is acceptable — you're looking at trends and aggregates, not exact financial counts. So ingestion favors availability (accept and buffer, at-least-once, best-effort), and queries tolerate slightly-incomplete recent windows. **Where it bites:** (1) **Alerting on gaps** — if a partition causes missing data, is "no data" an alert (host down!) or noise (just a blip)? You need explicit "no data" alert semantics. (2) **Billing/usage metering** that's derived from ingestion volume must be more careful (that leans toward exactness). (3) During the incident the monitoring system is *most* needed and *most* stressed — availability must not degrade exactly when everything else fails.
> **Mentor pushback:** "If it's AP and lossy, how do you trust an alert that says everything's fine?" You distinguish **absence of data from presence of good data** — a heartbeat/`up` metric per target lets you alert on *missing* data (target not reporting) separately from *bad* data (target reporting bad numbers). Silent data loss masquerading as health is the dangerous failure; explicit staleness/heartbeat detection is the reconciliation.

---

### Low-Level Design (Hard)

Q10. The core sub-problem: **cardinality explosion**. A customer deploys code that puts `request_id` (unbounded) into a metric label, generating billions of new series. Detect, contain, and prevent it — without dropping the customer's good data.
> **Problem statement:** Bound the number of unique series so one label mistake can't exhaust ingester memory / index and degrade every tenant, while preserving legitimate metrics.
> **Naive solution:** Accept all series; rely on big memory; or hard-cap total series and drop everything past the cap.
> **Why naive fails at scale:** Each new series consumes index + head-chunk memory; billions of one-shot series (each `request_id` appears once) means the index grows unbounded, ingesters OOM, and *all* tenants on that shard suffer (noisy-neighbor). A blunt global cap that drops "everything past N" might drop *good* series and is unfair across tenants.
> **Expected optimal approach:** Multi-layer cardinality governance: (1) **Per-tenant series budget/quota** enforced at ingestion — track active series count per tenant (via a **HyperLogLog** estimator per (tenant, metric) for cheap cardinality counting) and reject *new* series past the budget while still accepting existing ones (so good series keep flowing; only the runaway new ones are shed). (2) **Per-label-value limits** — detect a label whose distinct-value count is exploding (HLL per (metric, label)); when a label's cardinality crosses a threshold, **flag/drop that label** or roll its values into an `overflow` bucket rather than minting infinite series. (3) **Ingestion-time alerts** back to the customer ("metric X exceeded 100K series") so they fix the instrumentation. (4) Optionally **auto-detect unbounded labels** (values that are almost all unique, like request_id) and refuse them.
> **Pseudo-code or class diagram:**
```
def on_metric(tenant, metric, labels, value, ts):
    series_key = hash(metric, sorted(labels))
    hll_tenant[tenant].add(series_key)                     # est. active series for tenant
    for k,v in labels:
        hll_label[(metric,k)].add(v)                       # est. distinct values per label
        if hll_label[(metric,k)].estimate() > LABEL_LIMIT: # unbounded label detected
            labels[k] = "__overflow__"                     # collapse; stop minting new series
            emit_cardinality_alert(tenant, metric, k)
    if is_new_series(series_key) and hll_tenant[tenant].estimate() > tenant_budget(tenant):
        drop_and_alert(tenant, metric)                     # shed only NEW series past budget
        return
    tsdb.append(series_key, ts, value)
```

Q11. Concurrency: out-of-order and duplicate data points for the same series arrive from Kafka (at-least-once + agent retries). Ensure correctness in the TSDB.
> **Scenario:** For series S at timestamp t, you receive value v twice (Kafka redelivery) and also a point at t-5s *after* you already wrote t (out of order, agent retried an old batch).
> **Expected fix:** (1) **Idempotency / dedupe by (series_id, timestamp)** — a metric point is uniquely keyed by its series and timestamp; a second write for the same (series, t) is either ignored (idempotent, same value) or last-write-wins by ingestion time (metrics are not money — a dup must not double-count in a *counter* aggregate, so counters are stored as monotonic cumulative values and rate() is computed on read, which is naturally idempotent to dup absolute readings). (2) **Out-of-order within a bounded window**: the head block accepts writes older than the latest by up to a configured bound (e.g., 1h); points older than the bound are rejected (they'd require rewriting sealed, immutable compressed chunks — too expensive). (3) The delta-of-delta encoding assumes append-mostly-ordered; out-of-order within the head is handled before the chunk seals.
> **Follow-up (what if the writer/ingester dies?):** State lives in **Kafka + the sealed chunks on durable storage**, not the ingester's memory. If an ingester dies with an unflushed head block, its Kafka **consumer offset wasn't advanced past unprocessed points**, so on restart/failover another ingester **replays from the last committed offset** and rebuilds the head block — at-least-once replay is safe *because* writes are idempotent by (series, timestamp). You may briefly re-ingest points, but dedupe makes it correct. A **WAL** on the head block also lets a restarted ingester recover in-flight points without full replay.

Q12. Edge case: a downstream storage tier (the log store / ES cluster) is overwhelmed and slows to a crawl during an incident (when logging spikes hardest). Prevent it from taking down ingestion for everyone.
> **Scenario:** Log volume spikes 10x during an outage (everyone's error-logging); the log index can't keep up; consumer lag balloons.
> **Expected handling:** **Kafka is the shock absorber** — ingestion keeps accepting into Kafka even when the log store lags, decoupling producers from consumers (this is *the* reason Kafka sits in the middle). Then: (1) **Backpressure + per-tenant rate limiting/quotas** — the runaway tenant is throttled/shed at ingestion so it can't consume everyone's capacity (noisy-neighbor isolation via **bulkheads/quotas**, Lesson 11). (2) **Prioritized/tiered ingestion** — drop or sample low-value logs (DEBUG) before high-value (ERROR) under pressure; **adaptive sampling** raises drop rate as lag grows. (3) **Circuit breaker** on the log-store writer with a **DLQ / spillover to object storage** for logs that can't be indexed now (index them later, or leave them searchable only in cold storage). (4) Alert on **consumer lag** as the leading indicator. The principle: bounded buffers + backpressure + per-tenant fairness mean a spike degrades *gracefully and locally* (that tenant, that pillar) instead of collapsing the platform.

---

### Scaling to 10x / 100x (Hard)

Q13. At 100x (200M+ metric points/sec, PB/day of logs), where does it break first?
> **Expected answer:** Two things: (1) **Ingestion + the metric index (cardinality)** — write throughput scales horizontally by sharding, but the **inverted-label-index memory** for total active series is the real wall; hundreds of millions of active series exhaust index memory before raw write bandwidth does. (2) **Storage cost / IO for logs** — logs at PB/day dwarf metrics; the ES/inverted-index tier's storage and indexing CPU is the first *financial* and operational break. Query-side, **high-cardinality or wide-time-range queries** (scanning many series/blocks) saturate the query tier.
> **Numbers to ground the answer:** 200M points/sec at ~1.5 bytes = ~300 MB/sec = ~26 TB/day compressed metrics; logs commonly 5–20x that → **100s of TB to PB/day**. Active series in the tens/hundreds of millions each needing an index entry (~tens of bytes) = tens of GB of index RAM per ingester shard — this is why cardinality, not byte-throughput, is the metrics wall. Retention math: 1 PB/day × 30 days hot = 30 PB hot — infeasible to keep hot, forcing aggressive downsampling + cold tiering.

Q14. Apply consistent hashing (Lesson 19): shard the ingestion and storage tiers. Handle the hot series / hot tenant.
> **Expected sharding strategy:** Shard **metrics by series_id** (consistent hashing with virtual nodes) so all points for a series land on the same ingester (write locality + coherent head block + coherent index shard). Shard **logs by (tenant, time)**. Shard **traces by trace_id** (so all spans of one request co-locate for assembly). Time-block partitioning layers on top so retention/tiering is per-block.
> **Hot spot problem:** (1) A **hot series** (one extremely high-frequency metric) overloads its shard — rare for metrics (fixed scrape interval caps per-series rate), but a hot *tenant* (huge customer / runaway cardinality) concentrates load. **Detect:** per-shard ingest rate, per-tenant series count, consumer lag per partition. **Fix:** **per-tenant sharding / dedicated shards** for the largest tenants (bulkhead so a whale can't starve small tenants); **repartition** Kafka topics and use consistent hashing so adding ingester nodes moves only 1/N of series. For a hot *label query* (everyone querying the same dashboard), cache the result. Never shard metrics by tenant alone (a single huge tenant becomes one hot shard) — shard by series_id within tenant to spread.

Q15. Design the caching + downsampling/retention strategy. What's the hardest part?
> **Expected layered cache/tiering design:**
> - **Hot in-memory (last ~2h head block + Redis):** live dashboards and alerting read the most-recent window from memory — the vast majority of queries are "last 1h/last 24h."
> - **Warm TSDB (SSD, raw 10s → 1-min rollups, days):** recent history at fine resolution.
> - **Cold object storage (S3/Parquet, 1-hour rollups, months–years):** old data, heavily downsampled, cheap, slower to query.
> - **Query result cache:** dashboards re-run identical queries every refresh → cache aggregated results with short TTL.
> - **Downsampling pipeline:** at compaction, roll raw → 1-min → 1-hour, storing **mergeable sketches** (t-digest) so percentiles survive downsampling.
> **Cache/tiering hardest part:** **Retention + downsampling policy is the real "invalidation" problem** — deciding *what resolution to keep for how long* is a cost/fidelity tradeoff with no undo (once you downsample away raw points, they're gone; you can't retroactively compute a p99.9 you didn't keep a sketch for). The subtle trap: percentiles and unique-counts can't be recovered from naive averages after downsampling, so you must **decide up front which aggregations (sketches) to retain** in the rollup. Choosing the rollup schema wrong = permanently unanswerable queries later. That irreversibility is what makes it hard.

Q16. Cost/efficiency: you're storing petabytes that are 99% never queried. Optimize aggressively.
> **Expected answer:** The defining fact: **most stored telemetry is never read** — 90%+ of queries hit the last 24h, yet you retain months for the rare incident/audit. Strategies: (1) **Tiered storage** — hot (memory/SSD) only for recent; **cold object storage (S3) with columnar Parquet** for old data at ~10–50x lower cost/GB. (2) **Downsampling/rollups** — drop raw resolution as data ages (10s→1min→1hr); a year-old metric doesn't need 10s granularity. (3) **Retention policies per data type/tenant** — logs 7–30d, metrics rolled up 13 months, traces sampled heavily. (4) **Sampling** — traces are sampled at ingest (keep 1–10%, or tail-based sampling that keeps errors/slow traces); logs sampled by level. (5) **Compression** — Gorilla for metrics (~1.3 B/point), columnar+zstd for logs. (6) **Per-tenant quotas + usage-based billing** align cost with value. The discipline: never store at full fidelity forever; the art is keeping *just enough* (sketches, rollups, sampled exemplars) to answer the queries that matter while discarding the 99% no one reads. Datadog/observability bills are famously the #2 cloud cost — this *is* the product's margin.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Explain **tail-based sampling** for distributed traces and why it's harder than head-based sampling. How do you keep 100% of error/slow traces while sampling 1% of the rest, at scale? *(Expected: head-based sampling decides at the trace's start (cheap, but you don't yet know if it'll error/be slow). Tail-based decides after the full trace completes — so you must buffer all spans of a trace (across many services, arriving out of order) keyed by trace_id until the trace is "done," then apply a keep/drop policy (keep if any span errored or duration > p99). Hard because: spans arrive at different collectors, you must route all spans of a trace_id to the same sampling decider (consistent hashing on trace_id), hold them in a bounded time-window buffer, and handle traces that never "complete." Worth it because errors/slow traces are exactly the ones you must never sample away.)*

**H2.** Cross-cutting: multi-tenancy + data isolation. How do you guarantee tenant A can never query tenant B's logs, and that A's runaway volume can't degrade B? *(Expected: tenant_id embedded in every series/log/trace and enforced at the query layer (every query scoped to the authenticated tenant — never trust client-supplied tenant); per-tenant quotas + rate limits + optionally dedicated shards/clusters for large tenants (bulkhead) so noisy-neighbor is contained; encryption + access control per tenant; separate retention/billing. The isolation is both a security boundary (authz on every read) and a resource boundary (quotas + bulkheads).)*

**H3.** Operational: the monitoring system must not go down when everything else does (correlated failure) — and you can't monitor the monitor with itself. How? *(Expected: the observability platform needs its own independent, minimal **meta-monitoring** (a separate, simpler system / dead-man's-switch that alerts if the main platform stops reporting), multi-region/multi-AZ redundancy so a regional outage doesn't blind you, and graceful degradation (shed load, keep alerting on critical signals even if dashboards/log-search degrade). Alerting path must be the most resilient component — decouple it from the heavy query/storage tiers so alerts fire even when log search is down. Dead-man's-switch / heartbeat: if the platform stops sending "I'm alive," an external pager fires.)*

**H4.** Observability of the observability system: what do *you* instrument, and what's the leading indicator of ingestion trouble? *(Expected: ingestion rate + reject/drop rate per tenant, **Kafka consumer lag per partition/pillar** (the #1 leading indicator — rising lag means storage can't keep up before data goes stale/lost), per-tenant active-series count (cardinality leading indicator), query latency p99 by query type, storage tier fill rate + downsampling backlog, alert-evaluation latency and missed-evaluation count. Leading indicator: consumer lag climbing or active-series growth spiking predicts an outage before dashboards go stale.)*

**H5.** 'Undo a bad decision': you launched storing all metrics in Elasticsearch (treating them like logs) and the bill/latency is unsustainable. Migrate to a purpose-built TSDB with no gap in monitoring. *(Expected: stand up the TSDB alongside; dual-write metrics to both ES and the TSDB via the ingestion pipeline (a new Kafka consumer), backfill recent history, and dual-read/compare query results for correctness; cut dashboards + alerts over to the TSDB pillar-by-pillar behind flags, verify parity (same series, same aggregates, correct percentiles via sketches), then stop writing metrics to ES and reclaim its capacity for logs only. The lesson being corrected: metrics and logs are different databases — never conflate the numeric-aggregatable pillar with the text-search pillar.)*

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Underestimating **cardinality** — putting unbounded labels (user_id, request_id) into metrics, or not designing per-tenant/per-label cardinality governance. Cardinality, not byte-throughput, is the metrics wall.
2. Using **one storage engine** (usually Elasticsearch) for all three pillars. Metrics, logs, and traces have opposite access patterns and need columnar-TSDB, inverted-index, and trace-id-keyed stores respectively; conflating them blows up cost 50–100x.
3. Ignoring **storage economics** — no downsampling/rollups, no tiered storage, no retention/sampling. You're storing PB that's 99% unread; the design's viability *is* deciding what to throw away and when (and the irreversibility of downsampling — you can't average percentiles or recover discarded resolution).

**The one insight that makes an answer truly impressive:**
Framing the platform as **"a system whose primary job is deciding what data to discard, and when, at every layer"** — sampling at ingest (traces), sketches instead of raw for aggregations (t-digest/HLL so percentiles and unique-counts survive downsampling), rollups + tiered retention as data ages, and per-tenant cardinality budgets — all while keeping the *alerting path* the most resilient component (decoupled, meta-monitored by an independent dead-man's-switch) because the monitoring system must survive precisely the correlated outage that takes everything else down. Naming that percentiles aren't additive (hence mergeable sketches) is the tell of someone who's actually built this.

**Suggested follow-up reading:**
- Facebook's **Gorilla** paper ("Gorilla: A Fast, Scalable, In-Memory Time Series Database") — the delta-of-delta + XOR compression that underlies modern TSDBs.
- Prometheus TSDB design docs; Ted Dunning's **t-digest** paper (mergeable percentiles); Google's **Dapper** paper (distributed tracing) and Honeycomb/Datadog blogs on tail-based sampling and cardinality.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
