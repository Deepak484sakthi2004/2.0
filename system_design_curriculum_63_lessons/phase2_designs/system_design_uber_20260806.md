# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 54 of 63 — Phase 2: System Design Track (Design 26 of 35)
**Topic:** Uber — Real-Time Ride Matching + Location Tracking
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Uber is a two-sided real-time marketplace: millions of drivers emit GPS pings every 4 seconds, and riders expect a matched driver in under 5 seconds with an ETA accurate to the minute. The hard parts are (a) geospatial indexing that can answer "which drivers are within 2km of this point?" at 1M+ QPS, (b) a matching engine that avoids double-dispatching one driver to two riders under massive concurrency, and (c) supply-demand pricing (surge) computed on live, moving data. Uber (H3), Lyft (S2), and DoorDash solved variations of this; the recurring theme is that a moving-object index plus a dispatch-time lock beats any "clever ML" if you can't keep the index fresh.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Consistent Hashing (taught in Phase 1, Lesson 19):** Keys map onto a hash ring; each node owns an arc, and virtual nodes smooth the distribution so adding/removing a server only remaps ~1/N of keys. The critical property for Uber is that we shard the *location index* and the *driver-state store* by geographic cell so a city's traffic lands on a bounded set of shards. We use it today to shard the driver-location Redis cluster by H3 cell prefix and to keep a hot city (say Bengaluru at rush hour) from spilling onto one node.

**Geospatial Data Structures — grids, quadtrees, geohash (taught in Phase 1, Lessons 4 & 5):** A quadtree recursively subdivides space; a geohash interleaves lat/long bits into a base-32 string where prefix length = precision; Uber's H3 uses hexagonal cells (7 neighbors, uniform distance, no corner ambiguity). These give O(1) "which cell am I in" and O(neighbors) radius queries. Today this is the backbone of the "find nearby drivers" query.

**Kafka / Event-Driven Architecture (taught in Phase 1, Lesson 21):** Append-only partitioned log, consumer groups, offsets, at-least-once delivery, ordering guaranteed only within a partition. We ride this for the firehose of location pings, trip state-change events, and the surge-pricing stream processor. Partition key choice (by driver_id vs city) decides ordering and hot spots.

**Redis / Caching (taught in Phase 1, Lesson 24):** In-memory store with GEO commands (GEOADD/GEOSEARCH built on sorted sets + geohash scores), TTL, Lua for atomic multi-key ops. It holds the live driver-location index and the dispatch locks. Sub-millisecond reads are why we can hit 5-second matching.

**CAP / PACELC (taught in Phase 1, Lessons 2):** During a partition you choose consistency or availability; else you trade latency vs consistency. Location data is AP (stale-by-4-seconds is fine); the trip/payment state machine is CP (never double-charge, never double-assign). Recognizing which subsystem is which is the whole game.

**Rate Limiting (taught in Phase 1, Lesson 11):** Token bucket / sliding window to bound per-driver ping rate and per-rider request-a-ride spam. Protects the ingestion tier from a buggy client firmware that pings at 100Hz.

**Microservices / API Gateway / DDD (taught in Phase 1, Lesson 9):** Bounded contexts — Supply (drivers), Demand (riders), Dispatch (matching), Pricing, Trip lifecycle, Payments. The gateway terminates the mobile persistent connection and fans out. Clean context boundaries stop the trip-state machine from leaking into the location firehose.

**Load Balancers (taught in Phase 1, Lesson 6):** L4 for the raw ping ingestion (connection-oriented, sticky by driver), L7 for REST. Geo-DNS routes a rider to the nearest region. Matters because a driver's connection must stay pinned to a gateway that owns their session.

> **Recap box:**
> - Consistent hashing (L19): shard location index by H3 cell, no hot city meltdown.
> - Geospatial (L4/L5): H3 hex cells → O(neighbors) radius search.
> - Kafka (L21): location + trip-event firehose; partition key = ordering.
> - Redis (L24): live location index + dispatch locks, sub-ms.
> - CAP/PACELC (L2): location = AP, trip/payment = CP.
> - Rate limiting (L11): bound ping rate per driver.
> - Microservices/DDD (L9): Supply/Demand/Dispatch/Pricing/Trip/Payments contexts.
> - LB (L6): L4 sticky for pings, geo-DNS to nearest region.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. A driver's phone sends a GPS ping every 4 seconds. Walk me through what happens to that single ping from the moment it leaves the phone to when it's queryable by the dispatch system. Why 4 seconds and not real-time streaming?
> **What a strong answer covers:** Ping goes over a persistent connection (WebSocket/gRPC stream, not a fresh HTTPS per ping — TCP+TLS handshake per 4s would be wasteful) to an L4-balanced ingestion gateway pinned to that driver's session. Gateway validates + rate-limits, writes to Redis GEO index (GEOADD with current position) and appends to a Kafka topic partitioned by driver_id for downstream consumers (analytics, ETA, surge). 4 seconds is a deliberate battery-vs-freshness trade: at 30–60 km/h a car moves ~35–70m in 4s, well within matching tolerance, and it cuts radio wakeups ~vs 1s by 4x. Adaptive: idle/parked drivers back off to 10–30s.
> **Common weak answer:** "The phone calls a REST API and we save it to Postgres." Misses persistent connection, misses that a relational write per ping = ~250K writes/sec globally that Postgres will die under, misses the battery cost.
> **Mentor follow-up if they answer well:** If the driver is stationary at a red light for 90 seconds, do you keep writing the same coordinate 22 times? How do you avoid it?

Q2. Estimate the write QPS to the location index. Assume 5 million drivers online globally at peak, pinging every 4 seconds. Then estimate the read QPS from the matching system.
> **What a strong answer covers:** Writes = 5,000,000 / 4 = **1.25M writes/sec** to the location index. Reads: say 1M ride requests/hour at peak = ~280 req/s of *ride requests*, but each match scans neighbors and may re-query as the candidate set changes, plus every rider with the app open watching nearby-car animation polls ~every 5s. If 3M riders have the app foregrounded, that's ~600K read/s just for the "cars near me" map. So reads (~600K–1M/s) and writes (~1.25M/s) are the same order of magnitude — this is a write-heavy, read-heavy in-memory workload, which is exactly why it lives in Redis/sharded memory, not disk. Storage per driver record: driver_id(8B)+lat/long(16B)+timestamp(8B)+status ≈ 50–100B → 5M × 100B = **500MB**, trivially fits in RAM; the cost is throughput, not size.
> **Mentor follow-up:** Now shard it. If one Redis node does ~100K ops/s comfortably, how many shards and how do you route a ping to the right one?

Q3. Would you store the live driver location in PostgreSQL, MongoDB with a 2dsphere index, or Redis GEO? Defend the choice.
> **What a strong answer covers:** Redis GEO for the *hot live index* — sub-ms, in-memory, GEOSEARCH does radius queries natively, TTL auto-expires stale drivers. Postgres/PostGIS is great for *static geospatial* (geofences, city polygons, historical) but 1.25M writes/s of updates with index maintenance will thrash it. Mongo 2dsphere sits in between — fine for moderate scale, but B-tree updates on every ping are the same problem at 1M/s. The nuance: use Redis for the live moving-object index, and a durable store (Cassandra/Postgres) for the trip record and location *history* (async from Kafka), not on the hot path.
> **Red flag answer:** "Postgres with a GIST index handles everything." At 1.25M writes/sec, index bloat and autovacuum will fall over within minutes.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the end-to-end architecture for ride matching + location tracking. Draw it.
> **Key components expected:** Mobile clients (driver + rider), geo-DNS, L4 connection gateway (persistent streams), Location Ingestion service, Redis GEO cluster (sharded by H3), Kafka (location + trip topics), Dispatch/Matching service, Supply & Demand services, ETA service (routing/OSRM), Surge/Pricing service (stream processor), Trip lifecycle service (CP store), Payments, and a durable location-history sink (Cassandra).
> **Architecture diagram (text):**
```
 Driver App --gRPC stream-->  [L4 GW]--+
 Rider App  --gRPC stream-->  [L4 GW]--+--> [Location Ingest]
                                          |         |
                                          |    GEOADD (Redis GEO cluster,
                                          |         sharded by H3 cell)
                                          |         |
                                          +--> [Kafka: location-pings]---+
                                                    |                    |
                                          [ETA svc] [Surge stream proc]  [Cassandra
                                          (OSRM)    (windowed sup/dem)    loc-history]
                                                    |
  Rider "request ride" --REST--> [API GW] --> [Dispatch/Matching svc]
                                                    |  1) GEOSEARCH nearby
                                                    |  2) rank by ETA+accept-rate
                                                    |  3) SETNX dispatch lock
                                                    |  4) offer -> driver (push)
                                                    v
                                          [Trip Lifecycle svc] --(state machine)--> [Postgres/Spanner CP]
                                                    |                                     |
                                          [Kafka: trip-events] ------------------> [Payments]
```
> **What separates SDE2 from SDE3 here:** SDE2 draws the boxes. SDE3 says: "The location index (AP, Redis) and the trip state machine (CP, Spanner/Postgres) are *two different consistency worlds joined by Kafka*. I never let the matching read the trip DB on the hot path, and I never let a location ping touch the CP store. The dispatch lock is the bridge — a short-lived Redis lock (SETNX + TTL) that reserves a driver *before* the durable trip row is written, so a slow trip-DB write can't cause a double-dispatch."

Q5. Trace a rider tapping "Request Ride" until a driver is assigned. Step by step.
> **Expected trace:**
> 1. Rider request hits API GW → Demand service; geohash/H3-encode pickup point.
> 2. Pricing service returns fare estimate incl. current surge multiplier for that cell.
> 3. Dispatch does GEOSEARCH on Redis for drivers within an expanding radius (2km → 5km) in the pickup H3 cell + 6 neighbor cells, filtered to status=AVAILABLE, correct vehicle type.
> 4. Rank candidates by *road-network ETA* (not straight-line) from ETA/OSRM service, tie-broken by acceptance rate and idle time (fairness).
> 5. Attempt atomic reservation: Redis `SET driver:{id}:lock {tripId} NX EX 15`. If it fails, driver was grabbed by another dispatch — skip to next candidate.
> 6. Send ride offer push to the top locked driver; start a 15s accept timer.
> 7. On accept → write Trip row (state=ACCEPTED) to CP store, emit trip-event to Kafka, mark driver status=ON_TRIP, release nothing (lock now backed by trip). On timeout/decline → release lock, offer next candidate, optionally penalize decline.
> **Tricky part:** Between step 3 (read) and step 5 (lock) the world moves — the driver may accept another trip or go offline. Candidates who say "just pick the nearest" and skip the reserve-then-confirm handshake are the ones who cause double-dispatch. The read is advisory; the lock is authoritative.

Q6. Design the key APIs: driver location update, request ride, and driver accept.
> **Expected API design:**
> - Driver stream (gRPC bidi): `LocationUpdate{driver_id, lat, lng, heading, speed, ts, seq}` → server acks `seq`. Bidi so server can push offers down the same stream.
> - `POST /v1/trips` (rider request): body `{rider_id, pickup{lat,lng}, dropoff, vehicle_type}`, header `Idempotency-Key`. Returns `{trip_id, status: SEARCHING}`. 202-style; final assignment arrives via rider's push channel.
> - `POST /v1/trips/{trip_id}/accept` (driver): header `Idempotency-Key`, body `{driver_id}`. Server validates the offer is still open + lock still held by this driver.
> **What to push on:** Idempotency is mandatory — mobile networks retry; a double-tap on "Request" must not create two trips (same Idempotency-Key → same trip_id). Location updates carry a monotonic `seq` so out-of-order UDP-ish delivery doesn't rewind a driver's position (drop pings with seq older than last-seen). Versioning via `/v1/`. Pagination is irrelevant here; ordering and idempotency are everything.

---

### Data Modeling (Medium–Hard)
Q7. Design the core data model: the live location index, the trip record, and location history.
> **Expected schema:**
```sql
-- TRIP (CP store: Postgres/Spanner). One row, mutated through a state machine.
CREATE TABLE trips (
  trip_id        UUID PRIMARY KEY,
  rider_id       BIGINT NOT NULL,
  driver_id      BIGINT,                 -- NULL while SEARCHING
  status         VARCHAR NOT NULL,       -- SEARCHING/ACCEPTED/ARRIVED/ON_TRIP/COMPLETED/CANCELLED
  pickup_h3      CHAR(15), pickup_lat DOUBLE, pickup_lng DOUBLE,
  dropoff_lat    DOUBLE, dropoff_lng DOUBLE,
  fare_estimate  NUMERIC, surge_mult NUMERIC,
  requested_at   TIMESTAMPTZ, accepted_at TIMESTAMPTZ, completed_at TIMESTAMPTZ,
  idempotency_key TEXT UNIQUE,           -- dedupe request retries
  version        INT NOT NULL DEFAULT 0  -- optimistic concurrency
);
CREATE INDEX ON trips (driver_id, status);
CREATE INDEX ON trips (rider_id, requested_at DESC);
```
```
-- LIVE LOCATION (Redis). Per shard = per H3-cell-prefix.
GEOADD  geo:{cellPrefix}  <lng> <lat>  driver:{id}     # sorted-set, geohash score
SET     drv:{id}:meta     {status,vehicle,updated} EX 30   # TTL auto-reaps dead drivers
SET     driver:{id}:lock  {tripId} NX EX 15                 # dispatch reservation
```
```sql
-- LOCATION HISTORY (Cassandra, from Kafka, append-only, time-series).
-- partition key groups a driver's day; clustering orders by time.
CREATE TABLE loc_history (
  driver_id BIGINT, day DATE, ts TIMESTAMP,
  lat DOUBLE, lng DOUBLE, trip_id UUID,
  PRIMARY KEY ((driver_id, day), ts)
) WITH CLUSTERING ORDER BY (ts DESC);
```
> **Index choices and why:** trips (driver_id,status) to find a driver's active trip in O(1); (rider_id, requested_at) for rider history. Cassandra clusters by ts DESC so "last known N positions" is a partition scan, not a global sort. Redis needs no secondary index — the sorted-set score *is* the geohash.
> **Partitioning key and why:** Redis: **H3 cell prefix** so a city's drivers colocate and GEOSEARCH stays single-shard for the common case (only cross-shard when the radius spills into a neighbor cell on another node). Cassandra: **(driver_id, day)** to bound partition size (~21K pings/day/driver, healthy) and avoid the classic unbounded-partition anti-pattern. Never partition location history by driver_id alone — a year of a full-time driver's pings = millions of rows in one partition.

Q8. A rider's map shows "cars near me." How do you serve "give me the 20 nearest available drivers within 3km" efficiently at 600K QPS?
> **Expected answer:** `GEOSEARCH geo:{cell} FROMLONLAT <lng> <lat> BYRADIUS 3 km ASC COUNT 20` — Redis walks the sorted set by geohash score, sub-ms. Route the query to the shard owning the rider's H3 cell (client-side or via a routing proxy that knows the H3→shard map). For the map animation you don't need exactness or the *matchable* set — cache a coarse "cars in this cell" list per H3 cell with a 2–5s TTL and let *all* riders in that cell share it (fan-out read collapse), turning 600K distinct queries into ~#cells queries. Cross-cell edge: query the pickup cell + its 6 hex neighbors and merge.
> **Trap:** Computing Haversine distance in application code over *every* online driver (5M) per request — O(N) per query at 600K QPS is 3×10^12 distance calcs/sec. Any candidate who forgets the spatial index and "just filters by lat/long range in a WHERE clause" fails here.

Q9. The location index (Redis, AP) and the trip state (CP) can disagree — Redis says a driver is AVAILABLE but the trip DB just marked them ON_TRIP. How do you reason about this and prevent a double-dispatch?
> **Expected answer:** Location freshness is intentionally eventual (AP, PACELC "else latency") — a driver being shown at a 4s-old position is fine. But *assignment* must be linearizable (CP). We resolve the split by making the **Redis dispatch lock the single source of truth for reservation**, not the location index: dispatch reads the AP index to *find* candidates, then does `SET lock NX` to *claim*. The lock, not the AVAILABLE flag, decides. The trip DB write happens after the lock succeeds, so the CP store can never be the thing two dispatches race on. If a driver goes offline mid-offer, the lock's 15s TTL frees them.
> **Mentor pushback:** Two dispatch nodes both GEOSEARCH and both see driver D. Both try `SET lock:D NX`. Redis guarantees only one wins (single-threaded, atomic NX) — good. But what if the two nodes hit *different Redis replicas* and the lock hasn't replicated yet? Then you have a split-brain reservation. Fix: locks live on a single primary per cell (not a read replica), or use Redlock across an odd number of masters, or better — since the trip row has a UNIQUE partial index on (driver_id) WHERE status IN active, the CP store rejects the second insert as the final backstop. Belt and suspenders.

---

### Low-Level Design (Hard)
Q10. The core problem: matching. Given a ride request, select the best driver from thousands of moving candidates, at 1M requests/hour, without double-assigning. Design it.
> **Problem statement:** Input: pickup point, vehicle type, time. Output: exactly one driver, reserved atomically, chosen to minimize rider ETA while keeping global efficiency (don't strand far-away requests). Constraint: candidate set is stale-by-seconds and mutating.
> **Naive solution:** Greedy nearest-by-straight-line, assign immediately on read.
> **Why naive fails at scale:** (1) Straight-line ignores rivers/one-ways — the "nearest" car may be 12 min away by road. (2) Instant greedy assignment causes double-dispatch (read-then-assign without lock). (3) Pure per-request greedy is globally suboptimal: assigning the only nearby car to request A can leave request B (who had two options) with nothing — this is the classic online bipartite matching regret. (4) At high demand, thousands of dispatches contend for the same few cars → lock storms.
> **Expected optimal approach:** Two ideas. **(a) Reserve-then-confirm:** GEOSEARCH (advisory) → rank by *road ETA* from a contraction-hierarchies router (OSRM/Valhalla) → `SET lock NX EX 15` to claim → offer → on accept commit trip; on decline/timeout release + next. **(b) Batched matching under high load:** instead of matching each request instantly, accumulate requests in a short window (e.g., 1–2s per cell) and solve a **min-cost bipartite assignment** (Hungarian algorithm / min-cost max-flow) over the request×driver ETA matrix. Batching turns "first-come grabs the only car" into a globally near-optimal assignment and cuts total wait time measurably at peak. Uber's "batched matching" does exactly this. Fall back to instant greedy in low-demand cells where batching just adds latency.
> **Pseudo-code or class diagram:**
```
def dispatch(req):
    cell = h3(req.pickup, res=8)
    cands = redis.geosearch(cell.neighbors_incl_self(),
                            req.pickup, radius=2km, filter=AVAILABLE & req.vtype)
    cands = expand_radius_until(cands, min_k=5, max_radius=5km)
    ranked = sort(cands, key=lambda d: road_eta(d.pos, req.pickup))   # CH router
    for d in ranked:
        if redis.set(f"lock:{d.id}", req.trip_id, NX=True, EX=15):    # atomic claim
            if offer_and_await_accept(d, req, timeout=15s):
                trip_db.commit(trip_id=req.trip_id, driver=d.id, status=ACCEPTED)  # v++ optimistic
                redis.set(f"drv:{d.id}:meta", ON_TRIP)
                return d
            else:
                redis.delete(f"lock:{d.id}")   # declined/timeout -> free
    return NO_DRIVER_FOUND        # widen radius / raise surge / queue

# High-load path: batch window solver
def batch_match(requests, drivers):        # per cell, per 1-2s window
    cost = [[road_eta(dr.pos, rq.pickup) for dr in drivers] for rq in requests]
    assignment = hungarian_min_cost(cost)  # or min-cost-max-flow
    for (rq, dr) in assignment: reserve_and_offer(rq, dr)
```

Q11. Two riders request at the same instant and the system considers the same single available driver. Walk the exact race and fix it.
> **Scenario:** Dispatch node N1 (for rider R1) and node N2 (for rider R2) both GEOSEARCH the same cell at t=0 and both rank driver D first. Both proceed to reserve.
> **Expected fix:** The reservation is a single atomic `SET lock:D <trip> NX EX 15` against the cell's Redis primary. Redis is single-threaded per shard, so exactly one SET-NX succeeds; the loser gets nil and moves to its next-best candidate. This is optimistic-concurrency at the reservation layer. The trip DB has a partial unique index `UNIQUE(driver_id) WHERE status IN ('ACCEPTED','ON_TRIP')` as the durable backstop so even a lock-layer bug can't produce two active trips for D. Idempotency keys on accept stop a driver's retried accept from committing twice.
> **Follow-up (what if the lock holder dies?):** N1 wins the lock, sends the offer, then N1 crashes before the driver responds. The `EX 15` TTL auto-expires the lock, so D becomes claimable again — no permanent leak. But we must ensure D didn't *actually* accept in a now-orphaned flow: the accept path is idempotent and writes the trip row transactionally, so on recovery we reconcile from the CP store (the trip row, if committed, wins; if absent, the lock expiry correctly frees D). Never use a lock without a TTL — a crashed holder would strand the driver forever.

Q12. A network partition splits a region: the location Redis and dispatch nodes can still talk, but the CP trip store is unreachable for 30 seconds. What happens to in-flight and new rides?
> **Scenario:** Partition between dispatch tier and the CP trip DB.
> **Expected handling:** New matches can still *find and lock* drivers (AP path healthy) but **cannot commit a trip** (CP path down). Choose consistency: block the final commit rather than assign without a durable record — buffer accepted offers in a Kafka `pending-commits` topic with the idempotency key, and have the Trip service drain it when the DB returns (at-least-once + idempotent upsert = exactly-once effect). Riders see "still finding your driver" (degraded UX) rather than a phantom trip. In-flight trips already ON_TRIP keep going purely on the driver/rider apps + location stream; their state changes queue to Kafka and reconcile on heal. Circuit-breaker the trip-DB calls so dispatch fast-fails instead of piling up threads. DLQ for events that fail idempotent replay for manual review. The rule: it is acceptable to *delay* a match; it is never acceptable to double-charge or double-assign, so we sacrifice availability of *commit*, not integrity.

---

### Scaling to 10x / 100x (Hard)
Q13. At 100x (50M drivers, ~12.5M location writes/sec, ~10M reads/sec), where does this break first?
> **Expected answer:** The **location ingestion + Redis write throughput** breaks first, not storage. A single Redis shard sustains ~100K GEOADD/s; 12.5M/s needs ~150+ shards *just for writes*, and the connection gateway tier must terminate ~50M persistent streams (at ~50K conns/box that's ~1000 gateway boxes). Second bottleneck: the ETA/router service — road-ETA for every candidate on every request is CPU-heavy; at 10M matches it dominates. Third: Kafka partitions for the location firehose — 12.5M msg/s needs careful partition count (by cell, not driver) and consumer parallelism.
> **Numbers to ground the answer:** 12.5M writes/s ÷ 100K/shard ≈ 150 Redis shards; reads similar → ~300 shards total with replicas. Location payload ~100B × 50M = 5GB live set, trivial in RAM — again, throughput not size. ETA: if road-ETA is 200µs and we rank 10 candidates/request × 10M req/s = 2×10^7 lookups/s → precompute/cache ETAs per cell-pair.

Q14. Apply consistent hashing (Lesson 19): shard the location index. What's the key, and how do you handle a hot city like Bengaluru at 6pm?
> **Expected sharding strategy:** Shard by **H3 cell prefix** (coarse resolution, e.g. res-6 ≈ ~36km² cells) via consistent hashing with virtual nodes, so geographic locality is preserved (radius queries stay mostly single-shard) while the ring smooths distribution. Key = `hash(h3_res6_cell)`. Trip data shards by trip_id (uniform). This keeps a GEOSEARCH from fanning across all shards.
> **Hot spot problem:** A megacity cell at rush hour concentrates millions of pings on one shard → hot node. **Detect:** per-shard ops/s and CPU dashboards; a cell exceeding a threshold. **Fix:** *split the hot cell* — dynamically subdivide the res-6 cell into res-7 children (H3 gives 7 children per cell) and remap them to additional shards, so one dense city spreads across N nodes. This is adaptive/dynamic partitioning, analogous to region-splitting in HBase/Bigtable. Virtual nodes let us add capacity to just the hot arc without a full reshuffle. Never shard by raw driver_id hash — you lose locality and every radius query becomes a scatter-gather across all shards.

Q15. Design the caching for "nearby cars" and ETA. What do you cache, where, and how do you invalidate?
> **Expected layered cache design:**
> - **L1 (client):** rider app interpolates/animates car positions locally between 4–5s server updates — zero server load for smooth motion.
> - **L2 (per-cell shared cache):** for the map view, one cached "cars in cell X" list per H3 cell, TTL 2–5s, shared by all riders in that cell → collapses 600K individual reads into ~#cells reads (read fan-in). This is *not* used for matching (which reads the live index).
> - **L3 (ETA cache):** precomputed cell-pair travel times refreshed from live traffic every 1–5 min, so dispatch ranking is a lookup not a route solve. Cache surge multiplier per cell (TTL 30–60s).
> - **CDN:** static map tiles, not dynamic data.
> **Cache invalidation trap:** The nearby-cars cache is *time-based*, never event-based — trying to invalidate on every driver move at 12.5M moves/s would be more expensive than the queries. Accept 2–5s staleness for the map (it's AP). The genuinely hard invalidation is **surge**: a stale surge multiplier means you quote a fare you can't honor. Solution: version the surge value and pin the multiplier to the trip at request time (the rider pays the multiplier quoted, computed once), so cache staleness affects *future* quotes only, never an accepted fare. Don't let the map cache and the matching index share a code path — they have opposite consistency needs.

Q16. This runs on huge infra. Where's the cost, and how do you cut it without hurting the 5-second match SLA?
> **Expected answer:** Biggest costs: (1) 50M persistent connections + 12.5M writes/s of Redis RAM & CPU, (2) the routing/ETA compute, (3) cross-region Kafka replication bandwidth. Cuts: **adaptive ping rate** — parked/idle drivers ping every 15–30s not 4s, cutting a large fraction of the write volume with zero matching impact (most online drivers aren't on a trip at any instant). **Delta encoding** location pings (send heading+speed, dead-reckon position) shrinks bandwidth. **Tiered storage** for location history — hot 24h in Cassandra, older to S3/Parquet, aggregated for analytics (you don't need per-4s pings from 2 years ago at full fidelity; downsample). **Batch** low-priority consumers off Kafka. **Precompute** ETAs per cell-pair (amortize the router). **Spot/regional** capacity for the stateless dispatch and stream-processing tiers. The discipline: cut the *write firehose* (adaptive rate) because it's the dominant recurring cost, and never cut the reserve-then-confirm path because that protects correctness.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Why did Uber build H3 (hexagons) instead of using geohash or a quadtree? Give the concrete geometric reasons and one place where hex indexing still hurts you. *(Expected: hexagons have 6 equidistant neighbors — uniform adjacency, no corner/edge distance ambiguity that squares/geohash have; better for smoothing supply/demand and radius queries. Pain: hexagons don't tile hierarchically perfectly — a res-7 cell isn't cleanly contained in one res-6 parent (children overlap parent edges), so aggregation across resolutions is approximate.)*

**H2.** A rider requests, sees a driver's live location, cancels — you now hold that rider's and driver's precise movement traces. Walk the GDPR/privacy design: retention, the "delete my data" flow, and how location history in an append-only Cassandra store complicates deletion. *(Expected: pseudonymize driver_id in analytics, TTL raw pings (e.g., purge after N days), keep only aggregated/anonymized long-term; deletion in append-only/immutable stores via crypto-shredding — encrypt each user's data with a per-user key and delete the key to render it unrecoverable, since row-level deletes in time-series Cassandra are expensive tombstones.)*

**H3.** How do you deploy a new matching algorithm to production without a bad version stranding riders in a city? *(Expected: feature-flag + per-city/per-cell canary — route a small % of a single city's dispatches to the new matcher, compare match-time/cancel-rate/driver-idle metrics against control, auto-rollback on regression; shadow mode first (run new matcher, log its choice, don't act) to validate before it touches a real rider. Blue-green the stateless dispatch tier; the Redis/CP stores stay put.)*

**H4.** What do you instrument to know matching is healthy *before* riders complain? Name the specific metrics and one leading indicator. *(Expected: match-time p50/p95/p99, dispatch-to-accept rate, driver-decline rate, "no driver found" rate per cell, surge multiplier distribution, location-ping staleness (age of last ping), lock-contention rate. Leading indicator: rising "expanding radius exhausted" or lock-contention in a cell signals supply collapse minutes before match-time SLA breaks. Distributed traces across dispatch→ETA→trip-commit to find the slow hop.)*

**H5.** You launched with straight-line distance matching and it's causing bad assignments (car across the river). Tell the migration story to road-network ETA without a big-bang cutover. *(Expected: introduce ETA service behind a flag, run it in shadow (log road-ETA vs straight-line choice), measure divergence and downstream cancel-rate on the shadow set, then canary road-ETA in one city, watch cancel-rate and match-time, expand city by city; keep straight-line as the fallback when the router times out (SLA-bound: if road-ETA doesn't return in 50ms, degrade to straight-line rather than delay the match). Never remove the fallback.)*

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Treating the whole system as one consistency model — they either make location strongly consistent (dies under 1.25M writes/s) or make assignment eventually consistent (double-dispatch). The insight is *two worlds joined by a lock and Kafka*.
2. Skipping the reserve-then-confirm handshake — they read nearby drivers and assign in one step, which races. The read is advisory; the atomic lock is authoritative.
3. Using straight-line distance for ranking and forgetting the road network / the router-timeout fallback.

**The one insight that makes an answer truly impressive:**
Batched matching under high demand: instead of greedily assigning each request as it arrives, buffer requests per cell for 1–2 seconds and solve a global min-cost bipartite assignment (Hungarian / min-cost-max-flow) over the ETA matrix. It provably reduces total wait time and driver deadhead versus greedy, and almost no candidate reaches for it because they're stuck in per-request thinking.

**Suggested follow-up reading:**
- Uber Engineering: "H3: Uber's Hexagonal Hierarchical Spatial Index" and the "Marketplace / DISCO dispatch" posts.
- "Efficient Large-Scale Fleet Management via Multi-Agent Deep Reinforcement Learning" and Uber's batched-matching blog on ride-matching optimization.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
