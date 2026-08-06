# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 53 of 63 — Phase 2: System Design Track (Design 25 of 35)
**Topic:** Google Maps — Routing + ETA
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Routing on a continental road network is a shortest-path problem on a graph with ~hundreds of millions of nodes (intersections) and edges (road segments), where a naive Dijkstra would scan the whole continent and take seconds — but users expect a route in tens of milliseconds. The industry solution is **graph preprocessing**: Contraction Hierarchies (CH) and Customizable Route Planning (CRP, what Google/Bing actually use) precompute shortcuts so a query touches a few thousand nodes instead of hundreds of millions. Layered on top is **live ETA**: fusing historical speed profiles with real-time traffic from millions of phones (GPS probes) — a problem Google now attacks with Graph Neural Networks (DeepMind's work cut ETA errors). What makes it hard: the graph is huge and mostly static, but edge *weights* (travel times) change every few minutes with traffic, so you must separate slow, expensive topology preprocessing from fast, frequent weight updates.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Graphs + Dijkstra / A* (taught in Phase 1, Lesson 4):** Dijkstra finds shortest paths from a source by expanding the closest unsettled node using a min-priority queue; A* speeds it with an admissible heuristic (straight-line distance) to bias the search toward the goal. This is the *baseline* router — everything else is preprocessing to make it fast enough on a continental graph.

**Tries / heaps / spatial structures (taught in Phase 1, Lesson 5):** A min-heap is Dijkstra's priority queue. Spatial indexing (geohash/quadtree/S2 cells) answers "what's near this lat/lng" and maps a raw GPS point to the nearest road segment (map-matching). Both are core here.

**Caching / Redis (taught in Phase 1, Lesson 24):** Popular routes and current edge-weight (traffic) tables are cached in memory; you can't re-run preprocessing per request, and hot O-D (origin-destination) pairs repeat constantly.

**Consistent hashing (taught in Phase 1, Lesson 19):** The road graph is partitioned geographically across servers; consistent hashing / geo-sharding places each region's subgraph and routes queries to the owning shard(s).

**Kafka / event-driven streaming (taught in Phase 1, Lesson 21):** Millions of GPS probes/sec stream in as events; a streaming pipeline aggregates them into per-edge speed estimates that feed the live-weight table. Classic high-volume ingest + windowed aggregation.

**MapReduce / batch (taught in Phase 1, Lesson 15):** The heavy CH/CRP preprocessing (computing shortcuts over the whole continent) is an offline batch job; historical speed profiles are also batch-aggregated from months of probe data.

**Capacity estimation (taught in Phase 1, Lesson 28):** Route QPS, probe ingest rate, graph memory footprint, preprocessing time. Sized below.

> **Recap box:**
> - Dijkstra/A* = baseline shortest path; preprocessing makes it fast.
> - Spatial index (S2/quadtree) = map-match GPS→road segment + nearest queries.
> - Separate static topology (slow preprocess) from dynamic edge weights (fast updates) — the central idea.
> - Kafka streaming = ingest millions of GPS probes → live per-edge speeds.
> - Geo-sharding = partition the continental graph across servers.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Why can't you just run plain Dijkstra (or A*) per request on the road graph?
> **What a strong answer covers:** Plain Dijkstra from source to a far destination settles a huge fraction of the continent — for a cross-country route on a ~hundreds-of-millions-of-nodes graph, that's tens of millions of node expansions and heap operations, taking seconds and gigabytes of working set per query. A* with straight-line heuristic helps (biases toward the goal) but degrades badly on long routes, detours, and when the heuristic is weak (highways vs the crow-flies distance). At Google's QPS you'd need absurd compute. The fix is **preprocessing** (Contraction Hierarchies / CRP) so a query examines a few thousand nodes and returns in ~1–10ms.
> **Common weak answer:** "Use A*, it's faster than Dijkstra" — true but insufficient; A* is still linear-ish in the relevant graph region and can't hit 10ms on continental routes.
> **Mentor follow-up if they answer well:** A* needs an admissible heuristic. Straight-line distance is admissible for distance but is it admissible for *travel time* with speed limits? What breaks?

Q2. Estimate route-request QPS and GPS probe ingest. Assume 1B users, 2% requesting a route in a peak hour, and active-navigation phones sending a probe every 5s.
> **What a strong answer covers:** Route requests: 1B × 2% = 20M requests in the peak hour = ~5,600 req/s average over that hour, but bursty → design for ~20–50K route QPS peak. Probes: say 10M phones actively navigating at peak, each sending a GPS point every 5s → 10M/5 = **2M probes/sec**. Each probe is small (~50–100 bytes: id, lat, lng, heading, speed, ts) → ~150–200 MB/sec ingest, handled by a partitioned Kafka pipeline. Graph memory: hundreds of millions of edges × tens of bytes (with CH shortcuts) → tens of GB, fits in RAM sharded across servers.
> **Mentor follow-up:** Those 2M probes/sec must map to specific road *edges* before they're useful. What's the cost of map-matching each one, and can you afford it per-probe?

Q3. Would you recompute the shortest-path preprocessing every time traffic changes? Defend the choice.
> **What a strong answer covers:** No — that's the whole reason CRP (Customizable Route Planning) exists. Full Contraction Hierarchies preprocessing takes minutes-to-hours over a continent; traffic changes every 1–5 minutes. You **separate metric-independent topology preprocessing** (done rarely, when roads change) from a **fast metric customization** (re-apply current edge weights to the precomputed structure in seconds-to-sub-second). So topology shortcuts are computed once; live traffic just updates the *weights* on those shortcuts via a cheap customization step. That layering is the key insight of the whole system.
> **Red flag answer:** "Recompute Contraction Hierarchies when traffic updates" — CH's shortcut hierarchy is expensive to rebuild; doing it per traffic tick is infeasible. That's exactly what CRP was invented to avoid.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Draw the architecture: from a route request to a returned route with live ETA, plus the traffic-ingest side.
> **Key components expected:** Client, API gateway/LB, Map-matching service (GPS→edge, S2/quadtree index), Routing service (holds preprocessed graph, runs CH/CRP query), Graph store (topology + shortcuts), Metric/weight service (current edge travel times), Traffic pipeline (Kafka probe ingest → stream aggregation → speed estimates), Historical speed-profile store, ETA/ML service, Route cache (Redis), Tile/rendering service (for the map itself, separate concern).
> **Architecture diagram (text):**
```
   Phones (navigating) ──GPS probes(2M/s)──▶ ┌──────────────┐
                                             │ Kafka ingest │ (partitioned by geo/S2 cell)
                                             └──────┬───────┘
                                                    ▼
                                          ┌───────────────────┐
                                          │ Stream aggregation│  map-match probes → edges,
                                          │ (windowed, per-edge│  windowed median speed
                                          │  speed estimate)   │
                                          └─────────┬─────────┘
                                                    ▼ live edge speeds
   ┌──────────┐                          ┌────────────────────┐    ┌──────────────────┐
   │ Historical│─────profiles──────────▶ │  Metric / Weight    │◀──│ Traffic incidents │
   │ speed DB  │  (time-of-day, dow)     │  service (edge→time)│    │ (closures/accidents)│
   └──────────┘                          └─────────┬──────────┘
                                                    │ customize (CRP)
   Client ──POST /route──▶ ┌────────────┐  route    ▼
                           │ API Gateway│──────▶ ┌────────────────────┐   ┌───────────────┐
                           └────────────┘        │  Routing Service   │──▶│ Graph store    │
                                     ▲           │  CH/CRP bidirectional│  │ topology +     │
                            route+ETA │           │  Dijkstra query     │  │ shortcuts (RAM)│
                                     └───────────│  + ETA/ML overlay   │  └───────────────┘
                                                 └─────────┬──────────┘
                                                           │ cache hot O-D routes
                                                           ▼
                                                      ┌──────────┐
                                                      │Route cache│ (Redis)
                                                      └──────────┘
```
> **What separates SDE2 from SDE3 here:** SDE2 draws "graph + Dijkstra + traffic." SDE3 makes the **topology/metric separation explicit**: the graph store holds metric-independent shortcuts (rebuilt only when roads change); the metric service holds fast-updating weights; CRP's *customization phase* stitches live weights onto shortcuts in sub-second. SDE3 also separates **map-matching** (GPS→edge, a spatial-index problem) as its own service feeding *both* the live-traffic pipeline and the per-user turn-by-turn tracking, and treats **ETA** as a distinct model layered on the path, not just "sum of edge times."

Q5. Trace a single route request from "A to B, driving, avoid tolls, depart now."
> **Expected trace:**
> 1. Client → gateway → routing service with `{origin_latlng, dest_latlng, mode:driving, prefs:{avoid_tolls}, depart:now}`.
> 2. **Snap endpoints**: map-match origin and destination lat/lng to the nearest routable edges via the S2/quadtree spatial index (nearest-segment query).
> 3. Check **route cache** (Redis) keyed by (origin-cell, dest-cell, mode, prefs, time-bucket) — hot commuter routes hit here.
> 4. On miss: run **bidirectional CH/CRP query** — search forward from origin and backward from destination over the shortcut hierarchy; they meet in the middle, touching a few thousand nodes instead of millions. Edge weights come from the **metric service** (current live travel times per edge), respecting `avoid_tolls` (edges tagged toll are excluded/penalized).
> 5. **Unpack shortcuts** into the actual road-segment path (CH shortcuts represent contracted paths; expand to real turns).
> 6. **ETA overlay**: sum edge travel times, then adjust with the ETA/ML model (traffic-light delays, historical corrections, incident penalties) → final ETA. Generate turn-by-turn instructions.
> 7. Return route + alternates + ETA; cache it with a short TTL (traffic-sensitive).
> **Tricky part:** Candidates forget **map-matching the endpoints** and **shortcut unpacking**. A CH query returns a path *through shortcuts*; you must expand them to real segments for turn-by-turn. Also: the ETA is not just Σ(edge length ÷ speed) — intersections, turn penalties, and light timing matter, which is why ML is layered on.

Q6. Design the routing and traffic-ingest APIs.
> **Expected API design:**
> - `POST /v1/routes` body `{origin, destination, mode, avoid[], departure_time, alternatives:true}` → `{routes:[{polyline, distance_m, duration_s, duration_in_traffic_s, steps[], toll_info}], ...}`.
> - `POST /v1/matrix` (distance/ETA matrix for M origins × N destinations — used by ride-hailing/logistics).
> - `POST /v1/probes` (batched) `{device_id(anon), points:[{lat,lng,speed,heading,ts}]}` — high-volume ingest, fire-and-forget.
> - `GET /v1/snap` (map-matching a raw GPS trace to roads).
> **What to push on:** (1) **Time-dependent routing** — `departure_time` (or arrival_time) changes the answer because edge weights are time-of-day dependent; the API must support future departures using historical profiles. (2) **Idempotency/anonymity** on probes — probes are privacy-sensitive; batched, anonymized/rotating IDs, aggregated not stored per-user. (3) **Matrix API** blows up as O(M×N) — must use many-to-many CH techniques, not M×N independent queries. (4) Polyline encoding to compress the geometry.

---

### Data Modeling (Medium–Hard)

Q7. Design how the road graph, shortcuts, and edge weights are stored.
> **Expected schema:**
```
# Topology (metric-INDEPENDENT): nodes = intersections, edges = road segments.
# Stored as a compact adjacency structure (CSR) in RAM, geo-sharded by S2 cell.
node:      { node_id, s2_cell, lat, lng }
edge:      { edge_id, from_node, to_node, length_m, road_class,   # highway/arterial/local
             toll:bool, oneway:bool, turn_restrictions[], speed_limit }

# CH/CRP shortcuts (metric-independent overlay) — computed offline (batch)
shortcut:  { from_node, to_node, via_nodes[], level }             # contracted path

# Metric (dynamic weights) — the fast-changing layer, keyed by edge
CREATE TABLE edge_weights (            -- current + profiled travel times
  edge_id   bigint,        -- partition/shard by geo cell
  live_speed_kph  float,   -- from real-time probe aggregation
  live_ttl  timestamp,
  profile   map<time_bucket, float>,   -- historical speed by (dow, hour) bucket
  PRIMARY KEY (edge_id)
);
```
```sql
-- Historical probe aggregates (batch, for profiles) — columnar/warehouse
-- (device-anonymized, aggregated per edge × time bucket; raw probes NOT retained long-term)
CREATE TABLE speed_profiles (
  edge_id bigint, dow smallint, hour smallint, quarter smallint,
  median_speed_kph float, sample_count int,
  PRIMARY KEY (edge_id, dow, hour, quarter)
);
```
> **Index choices and why:** The graph itself is stored as **CSR (compressed sparse row) adjacency arrays** in RAM for cache-friendly O(1) neighbor iteration — critical for Dijkstra/CH hot loops. Spatial index (S2 cells / quadtree) over node coordinates for map-matching and endpoint snapping. `edge_weights` keyed by edge_id for O(1) weight lookup during the query.
> **Partitioning key and why:** Geo-shard by **S2 cell / region** — routes are mostly local, so co-locating a region's subgraph keeps most queries on one shard; cross-region routes stitch at boundary "gateway" nodes (this is also how CRP's partition-based overlay works). Probe ingest partitioned by geo cell so aggregation for an edge lands on one worker.

Q8. A route request must fetch current travel times for the ~thousands of edges its search touches. How do you do this efficiently?
> **Expected answer:** Don't fetch per-edge over the network mid-query — that's thousands of round-trips. The **metric/weight table lives in RAM co-located with the routing graph** (same shard), so weight lookup during the CH search is an array index, not an RPC. The traffic pipeline **pushes** updated weights into the routing servers' in-memory metric arrays every 1–5 minutes (a bulk swap of the metric layer — exactly the CRP "customization" that re-decorates the fixed topology). Historical profiles for future/`departure_time` queries are also memory-resident per region. So a query does zero external weight fetches; it reads local arrays.
> **Trap:** Storing edge weights in a remote DB and querying them one-by-one during search — thousands of network calls per route, destroying the 10ms budget. Weights must be memory-local and bulk-updated.

Q9. Where do you accept eventual consistency, and where must it be strong?
> **Expected answer:** Eventual/approximate is fine (and necessary) for: **live traffic weights** (a 1–5 min lag is inherent — you're averaging probes over a window), historical profiles, and ETA (it's a prediction, inherently uncertain). Route results are *best-effort optimal* — a route computed against weights 2 minutes stale is acceptable. Stronger requirements: **road-network topology consistency** (a permanently closed road or wrong one-way is a *safety* issue — routing someone the wrong way down a one-way street is unacceptable), and **incident data** (a live road closure must propagate quickly to avoid routing into a blocked road). It's fundamentally an AP system — availability and low latency of routing beat perfectly-fresh traffic — with strong guarantees reserved for correctness/safety of the topology.
> **Mentor pushback:** A major highway just closed (accident). Traffic weights lag 3 minutes, so routes still send cars into the closure. → Incidents are a *separate, low-latency override channel* (not the windowed probe average): a closure event immediately sets the affected edges' weight to ∞ / removes them from routing, propagated to routing servers in seconds, ahead of the slower statistical traffic update.

---

### Low-Level Design (Hard)

Q10. Design the fast shortest-path query. Explain the preprocessing that makes 10ms continental routing possible.
> **Problem statement:** Answer point-to-point shortest (fastest) path on a graph with ~hundreds of millions of nodes in ~1–10ms, while supporting frequently-changing edge weights.
> **Naive solution:** Bidirectional Dijkstra/A* per request over the raw graph.
> **Why naive fails at scale:** Even bidirectional A*, a long route settles millions of nodes; heap ops dominate; can't hit 10ms and can't sustain tens of thousands of QPS without enormous fleets.
> **Expected optimal approach — Contraction Hierarchies (CH) / Customizable Route Planning (CRP):**
> - **CH (preprocess):** Order nodes by "importance"; **contract** them one by one (remove a node, add **shortcut** edges preserving shortest paths among its neighbors). Result: a hierarchy where a query only ever moves "upward" in importance. A **bidirectional Dijkstra restricted to upward edges** meets in the middle touching a few thousand nodes → milliseconds. Downside: shortcuts encode the metric, so a weight change means re-preprocessing → bad for live traffic.
> - **CRP (what production systems use for live traffic):** Split into **metric-independent** phase (partition the graph into cells, build a multi-level **overlay** of boundary/shortcut edges — depends only on topology) and a fast **metric customization** phase (fill in the overlay edge weights from current traffic in *seconds*). Queries run bidirectional search over the overlay. This is the layering that lets traffic update every few minutes without touching the expensive topology preprocessing.
> **Pseudo-code or class diagram:**
```
# CH query (bidirectional, upward-only)
def ch_query(s, t, weights):
    fwd = Dijkstra(s, direction=up, weights)       # only relax edges to higher-rank nodes
    bwd = Dijkstra(t, direction=up_reverse, weights)
    meet, best = None, INF
    # alternate settling fwd/bwd; when a node is settled in both, candidate = d_fwd+d_bwd
    for node settled in both:
        best = min(best, fwd.dist[node] + bwd.dist[node])
    path = unpack_shortcuts(reconstruct(meet))     # expand shortcuts → real segments
    return path, best

# CRP two-phase
preprocess_topology(graph):        # SLOW, only when roads change
    cells = partition_multilevel(graph)            # e.g., PUNCH/KaHIP partitioning
    overlay = build_boundary_shortcut_edges(cells) # metric-independent
    return overlay

customize(overlay, current_weights):   # FAST, every few minutes
    for level in overlay.levels:
        recompute shortcut weights within each cell from current_weights   # seconds
```

Q11. Concurrency: a live-traffic metric update is being swapped in while thousands of route queries are mid-flight reading the weights.
> **Scenario:** The customization step produces a new metric array (updated edge speeds); if a query reads a half-updated array, it could mix old and new weights → an inconsistent/incorrect route or a crash.
> **Expected fix:** **Immutable, atomically-swapped metric snapshots.** The customization produces a *new* complete metric array off to the side; when ready, flip an atomic pointer so new queries read the new snapshot while in-flight queries keep reading the old one (read-copy-update / double-buffering). No locks on the hot query path — queries never block on updates and never see a torn read. Old snapshot is freed once no query references it. This is the same reason the topology and metric are separate structures: you swap the small metric layer atomically without touching the huge topology.
> **Follow-up:** What if a customization run crashes/produces bad weights (e.g., a bug sets speeds to 0 → infinite ETA)? Validate the new metric (sanity bounds on speeds) before the atomic swap; keep the previous good snapshot as fallback; roll back the pointer. Never swap in an unvalidated metric.

Q12. Failure deep-dive: the GPS-probe ingest pipeline backs up, or a region's probe volume drops to near-zero (bad cell coverage).
> **Scenario:** (a) Kafka consumer lag on the probe pipeline → live traffic weights go stale by 15+ minutes. (b) A rural region gets almost no probes, so there's insufficient data to estimate current speeds.
> **Expected handling:** (a) **Graceful fallback to historical profiles** — if live weights are older than a freshness threshold, the metric service uses the time-of-day/day-of-week **historical speed profile** for that edge instead of stale live data (better a good average than a wrong 15-min-old snapshot). Autoscale consumers; partition by geo so a hot region's lag doesn't block others; the topology/query path is unaffected (it just reads whatever weights are current). (b) **Sparse-data fallback**: with too few probes for a confidence threshold, fall back to speed-limit-based or historical-profile estimates and widen the ETA uncertainty band. Backfill via road-class priors (a highway defaults to highway speed). Incidents remain a separate authoritative override regardless of probe volume.

---

### Scaling to 10x / 100x (Hard)

Q13. At 10x, where does this break first?
> **Expected answer:** Two fronts: (1) **Probe ingest + map-matching throughput** — 2M→20M probes/sec, and each probe needs a spatial map-match to an edge, which is CPU-heavy; the aggregation pipeline saturates before the query side. (2) **Route query CPU on hot regions** (dense metros) and the **matrix API** (O(M×N) for logistics customers). The static graph memory grows only when roads change, not with load, so storage isn't the first wall — it's ingest CPU and query CPU in dense cells.
> **Numbers to ground the answer:** 20M probes/sec × a map-match costing (say) tens of microseconds → needs thousands of cores just for matching; partition by S2 cell and scale horizontally. Route QPS 20–50K → 200–500K at 10x; each CH/CRP query touches a few thousand nodes in ~1–10ms, so ~a few thousand routing cores, geo-sharded. A Distance Matrix request for 1000×1000 = 1M pairs must use many-to-many CH (bucket-based), not 1M single queries.

Q14. How do you shard the graph, and what's the hotspot?
> **Expected sharding strategy:** **Geo-sharding by S2 cell / region** — each routing server owns a region's subgraph (topology + metric) in RAM. Local routes (the majority) stay on one shard. Long cross-region routes use the **CRP overlay**: route within origin cell to its boundary nodes, hop across the higher-level overlay between regions, descend into the destination cell — so a cross-country route never loads the whole continent, just the two endpoint regions + the sparse top-level overlay. Probe ingest sharded by the same cells so an edge's probes aggregate on one worker.
> **Hot spot problem:** **Dense metros at rush hour** (a Manhattan cell) — disproportionate query load *and* probe volume *and* the most volatile traffic. Detect via per-cell QPS/probe metrics. Fix: **split hot cells finer** (S2 supports variable-resolution cells — subdivide dense areas), replicate hot-region routing servers behind a load balancer, and cache the flood of near-identical commuter O-D routes aggressively. Another hotspot: a **popular event** (stadium letting out) spikes one area's probes and route demand — handled by the same finer cells + caching + incident overrides for road closures.

Q15. Design the caching layers and the hardest invalidation problem.
> **Expected layered cache design:** L1 = in-process caches on routing servers: the resident graph (CSR) + current metric snapshot + a small LRU of recently unpacked shortcut paths. L2 = Redis **route cache** keyed by (origin-cell, dest-cell, mode, prefs, coarse time-bucket) for hot commuter/popular routes. L3 = CDN for *map tiles* (a separate, highly-cacheable static/immutable concern — vector/raster tiles by z/x/y). Historical profiles are memory-resident per region.
> **Cache invalidation trap:** **The route cache vs live traffic.** A cached route from 5 minutes ago may now be wrong because an accident changed the fastest path — but you can't invalidate every cached route on every traffic tick (that's most routes, constantly). Solution: **short, traffic-aware TTLs** (e.g., 1–3 min, shorter in volatile cells) and **event-driven invalidation for incidents** — a road closure/major-incident event explicitly evicts cached routes passing through the affected edges (reverse-index cached routes by the edges they traverse, or bucket by cell so an incident invalidates that cell's route cache). The static map-tile cache, by contrast, is near-permanently cacheable (roads rarely move) — versioned by map-data release, so its invalidation is trivial (new tile version = new URL).

Q16. How do you keep this efficient at scale?
> **Expected answer:** (1) **Topology/metric separation (CRP)** is the master efficiency lever — never redo expensive preprocessing for a traffic change; only re-customize the cheap metric layer. (2) **Compact in-memory graph (CSR + bit-packed edges)** so hundreds of millions of edges fit in tens of GB and stay cache-friendly. (3) **Aggregate probes at the edge** — don't store raw 2M/sec probes; compute windowed per-edge speeds and discard raw points (also a privacy win). (4) **Cache hot O-D routes** — commuter traffic is enormously repetitive; a high route-cache hit rate slashes query CPU. (5) **Precompute distance-matrix building blocks** (boundary-to-boundary distances) so logistics matrix queries reuse them. (6) **Tile CDN** offloads all map-rendering bandwidth to the edge. (7) **Historical profiles** avoid needing live data everywhere — cheap, dense coverage for free.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Explain **Contraction Hierarchies node contraction** precisely: how you order nodes by importance (edge-difference / simulated contraction heuristics), what a **witness search** is (checking whether a shortcut is actually necessary — is there already a shorter path bypassing the contracted node?), and why the query is a *bidirectional upward* search. Then explain why CH alone is unsuitable for live traffic and how **CRP's separation of metric-independent overlay from metric customization** fixes it.

**H2.** ETA is not Σ(edge length ÷ speed). How does Google use **Graph Neural Networks** (DeepMind's routing work) over "supersegments" to predict ETA, what features feed it (historical profiles, live speeds, road class, turn/light penalties, weather), and why does a learned model beat additive edge times? (Answer: it captures correlations edges don't — light-timing, congestion propagation between adjacent segments, systematic biases — reducing ETA error meaningfully vs naive summation.)

**H3.** Privacy/GDPR for location data: 2M GPS probes/sec are extraordinarily sensitive. How do you use them for traffic without building a surveillance history of individuals? (Answer: anonymize/rotate device IDs, aggregate to per-edge windowed statistics and **discard raw traces**, differential-privacy noise on aggregates, minimum-sample thresholds so a single car can't be re-identified from an edge estimate, and don't retain per-user paths.)

**H4.** What do you instrument? Route-query latency p50/p99, cache hit rate, **ETA accuracy** (predicted vs actual arrival — the core quality metric, tracked as error distribution), probe ingest lag & per-cell probe density (alert on stale/sparse cells), metric-customization duration & freshness, incident-propagation latency (closure event → routing servers), per-cell QPS for hotspot detection. The one SLO that matters: **ETA error (p50/p90) and route latency p99**.

**H5.** "Undo a bad decision": you launched with plain Contraction Hierarchies and now live traffic requires re-running the full CH preprocessing (minutes) on every traffic update, so traffic is always 20+ minutes stale. Migrate to CRP without downtime. (Answer: build the CRP metric-independent overlay offline in parallel; validate its routes match CH on a shadow traffic slice; cut regions over one at a time behind a flag so you can compare route quality and latency; once the fast customization path is proven, retire the CH-rebuild pipeline. Keep historical profiles as the fallback throughout the migration.)

---

### Mentor's Closing Notes

**Top 3 things most candidates get wrong on this topic:**
1. Answering "just run Dijkstra/A*" — missing that the entire engineering problem is **preprocessing** (CH/CRP) to hit the latency budget on a continental graph.
2. Not separating **static topology from dynamic edge weights** — proposing to re-preprocess on every traffic change, which is exactly what CRP was invented to avoid.
3. Treating ETA as a simple sum of edge times and ignoring map-matching, shortcut unpacking, turn/light penalties, incidents-as-override, and the historical-vs-live fallback.

**The one insight that makes an answer truly impressive:**
The whole system is built on the axis of **change frequency**: the graph *topology* changes rarely (do the expensive work — contraction/overlay — offline and once), edge *weights* change every few minutes (a cheap customization + atomic snapshot swap), and *incidents* change in seconds (a separate authoritative override channel). Map every component to how often its input changes and the architecture — CRP's two phases, RCU metric swaps, incident overrides, historical fallbacks — falls out naturally. That framing is what separates a memorized "use CH" from someone who understands *why*.

**Suggested follow-up reading:**
- "Customizable Route Planning" (Delling, Goldberg, Pajor, Werneck — Microsoft Research) and the "Contraction Hierarchies" paper (Geisberger et al.) — the two foundational algorithms.
- DeepMind/Google "Traffic prediction with advanced Graph Neural Networks" (ETA in Google Maps) and Google's S2 Geometry library docs for the spatial-indexing side.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
