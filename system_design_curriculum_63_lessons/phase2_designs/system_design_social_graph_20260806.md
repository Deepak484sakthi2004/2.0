# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 46 of 63 — Phase 2: System Design Track (Design 18 of 35)
**Topic:** Social Graph (LinkedIn) — Degrees of Connection
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
LinkedIn's "1st / 2nd / 3rd degree" label on every profile is deceptively hard: it's a shortest-path query on a billion-node, ~30-billion-edge undirected graph, computed in single-digit milliseconds, on *every* search result and profile view — hundreds of thousands of times per second. You cannot run Dijkstra/BFS against a database per request. LinkedIn built a purpose-built in-memory graph service (the "Graph DB" / later "LIquid") that holds the entire connection graph in RAM and answers distance/path queries with a bidirectional BFS bounded to 3 hops. What makes it hard: the graph is huge, mutable, must be near-real-time, and the interesting queries (2nd-degree, path, "how you're connected") are inherently multi-hop.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Graphs, BFS/Dijkstra (taught in Phase 1, Lesson 4):** BFS explores level by level and gives shortest path in unweighted graphs; bidirectional BFS searches from both endpoints and meets in the middle, cutting explored nodes from O(b^d) to O(b^(d/2)). This is the core of degree-of-connection: distance between two members is a bounded (≤3) BFS. Adjacency lists are the representation; we keep them in memory.

**Bloom filters / skip lists / tries (taught in Phase 1, Lesson 5):** A Bloom filter answers "is X possibly in this set" in O(k) with no false negatives. Used to cheaply test set membership during BFS frontier intersection and to prune "definitely not a connection" without touching the full adjacency list.

**Consistent hashing (taught in Phase 1, Lesson 19):** Partition members across graph-service nodes by member_id so each node owns a slice of adjacency lists; add capacity with minimal remap. The graph is too big for one machine's RAM, so it's sharded.

**Caches / Redis (taught in Phase 1, Lesson 24):** Hot query results (a member's 2nd-degree network size, frequent pair distances) are cached with TTLs. But the primary "cache" here is the entire graph held in RAM in the graph service.

**Kafka / event-driven (taught in Phase 1, Lesson 21):** Connection accept/remove events stream to the graph service to update in-memory adjacency lists in near-real-time, keeping the RAM graph consistent with the source-of-truth DB.

**CAP / eventual consistency (taught in Phase 1, Lesson 2):** Degree labels are AP — a brand-new connection showing as "2nd" for a few seconds is acceptable; the graph service being *down* (no degree labels, no network filtering) breaks search. Source of truth (the edge exists) is strongly consistent; the derived in-memory index is eventually consistent.

**SQL/NoSQL (taught in Phase 1, Lesson 23):** The durable edge list lives in a sharded store (Espresso/document store); the graph service is a *derived, in-memory* index built from it — not the system of record.

> **Recap box:**
> - Degree of connection = bounded bidirectional BFS (≤3 hops).
> - Hold the whole graph in RAM, sharded by member_id; DB is only the durable edge log.
> - Bidirectional BFS + Bloom-filter frontier intersection = the algorithm.
> - Kafka streams edge changes → keep RAM graph near-real-time.
> - Degree labels are eventually consistent; the edge itself is strongly consistent.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. You learned BFS in Lesson 4. Why is *bidirectional* BFS the right algorithm for "are A and B 1st/2nd/3rd degree," and how much does it save?
> **What a strong answer covers:** Distance in an unweighted, undirected connection graph is a shortest-path problem → BFS. But a single-source BFS to depth 3 with average degree ~500 explores ~500^3 = 125M nodes worst case. Bidirectional BFS runs BFS from A and from B simultaneously and stops when frontiers intersect; each side only goes ~1.5 hops, so it explores ~500^1.5 ≈ ~11K per side instead of 125M — orders of magnitude fewer. The meeting point also gives the connecting path ("you're connected through X").
> **Common weak answer:** "Run BFS from A until you find B" — explores the full 3-hop neighborhood, ~10,000x more work.
> **Mentor follow-up if they answer well:** How do you detect the frontier intersection cheaply without an O(n·m) set comparison each level?

Q2. Estimate the RAM to hold the connection graph. 1B members, avg 500 connections each, undirected.
> **What a strong answer covers:** Edges (directed representation) = 1B × 500 = 500B adjacency entries. At ~8 bytes per neighbor id (or ~4–5 with compression), that's ~4TB raw, ~2–2.5TB compressed. Plus overhead for offsets/CSR structure. This obviously doesn't fit one box → shard across, say, ~50–200 nodes with replication. The point of the estimate: it fits in *distributed* RAM but not a single machine, and it's small enough that a DB round-trip per hop would be the bottleneck, justifying the in-memory service.
> **Mentor follow-up:** Now add the query load — if every profile view and every search result needs a degree label at ~300K QPS, how many BFS ops/sec is that and why can't a database serve it?

Q3. Should degree-of-connection be computed at write time (materialized) or read time (on demand)? Defend it.
> **What a strong answer covers:** **Read time.** Materializing 2nd/3rd degree is infeasible: a member's 2nd-degree network is ~500×500 = 250K members, 3rd is ~125M — you can't store and maintain, per member, a set that large that changes every time any friend-of-a-friend adds a connection (write amplification is astronomical). Instead compute distance on demand with bounded bidirectional BFS from the in-memory graph (~ms), and cache hot results briefly. 1st-degree *can* be materialized (it's just the adjacency list).
> **Red flag answer:** "Precompute and store everyone's 2nd/3rd-degree set" — combinatorial explosion; a single new edge would invalidate millions of precomputed sets.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the graph service that answers distance and path queries at scale.
> **Key components expected:** Client/API gateway, Edge write service, durable edge store (sharded DB), Kafka edge-change stream, in-memory Graph Service (sharded by member_id, replicated), distance/path query engine (bidirectional BFS), result cache, and a graph-build/bootstrap pipeline.
> **Architecture diagram (text):**
```
  Client ─▶ API Gateway ─▶ Distance/Path Query Router
                               │  distanceTo(A,B) / path(A,B) / networkOfDegree(A,2)
                               ▼
     ┌───────────────────────────────────────────────┐
     │           In-Memory Graph Service              │
     │   sharded by member_id (consistent hashing)    │
     │  ┌──────────┐ ┌──────────┐ ┌──────────┐        │
     │  │ Shard 0  │ │ Shard 1  │ │ Shard N  │  ...   │  each: CSR adjacency
     │  │ adj[m]=  │ │          │ │          │        │  lists in RAM + replicas
     │  │ [n1..nk] │ │          │ │          │        │
     │  └──────────┘ └──────────┘ └──────────┘        │
     │   bidirectional BFS spans shards via RPC        │
     └───────────────▲───────────────────────────────┘
                     │ apply edge changes (near-real-time)
              ┌──────┴──────┐
              │    Kafka     │  edge-events (connect/accept/remove/block)
              └──────▲──────┘
                     │ CDC / emit
        ┌────────────┴───────────┐        ┌─────────────┐
        │ Edge Write Service      │──────▶ │ Edge Store  │  (durable source of truth,
        │ (invite/accept/remove)  │        │ sharded DB  │   sharded by member_id)
        └─────────────────────────┘        └─────────────┘
                                    Result cache (Redis): (A,B)->dist, TTL
```
> **What separates SDE2 from SDE3 here:** SDE3 recognizes that a bidirectional BFS *spans shards* — A's neighbors may live on shard 3, B's on shard 17 — so the query engine must fan out RPCs across shards per BFS level and intersect frontiers, and they minimize cross-shard hops by expanding the *smaller* frontier first (degree-aware). They also treat the in-memory graph as a rebuildable index, not the DB.

Q5. Trace a distance query: user viewing a search results page with 25 people, each needing a degree label.
> **Expected trace:**
> 1. Search service returns 25 candidate member_ids.
> 2. Query router issues a **batched** `distance(viewer, [25 ids])` to the graph service (not 25 separate calls).
> 3. Graph service checks: which candidates are in viewer's 1st-degree set (direct adjacency lookup — O(1) hash/Bloom check)? Those are "1st."
> 4. For the rest, expand viewer's 1st-degree frontier one hop (their connections) and test intersection with each candidate → "2nd."
> 5. Remaining go one more hop (bounded at 3) → "3rd"; beyond that → "3rd+/out of network."
> 6. Cache the (viewer, candidate) results with a short TTL; return labels.
> **Tricky part:** Candidates issue per-result BFS instead of *one* expansion of the viewer's neighborhood reused across all 25 candidates. Expand the viewer's frontier once, test all candidates against it — that's the key optimization.

Q6. Design the query API.
> **Expected API design:**
> - `GET /v1/graph/distance?from=A&to=B` → `{degree: 1|2|3|"out"}`.
> - `POST /v1/graph/distance/batch` body `{from, to:[...ids]}` → `{id: degree}` (the important one — batched).
> - `GET /v1/graph/path?from=A&to=B` → `{path:[A, X, B], degree:2}` ("how you're connected through X").
> - `GET /v1/graph/network?member=A&degree=2&limit=...` → count/sample of 2nd-degree network (for "People You May Know" candidate generation).
> - `POST /v1/graph/edges` (connect/accept) / `DELETE` (remove).
> **What to push on:** (1) Batch endpoint is mandatory — degree labels are always needed in bulk (search page, feed). (2) Path queries must return an actual connecting path, so BFS must record predecessors. (3) `network(degree=2)` returns *counts or samples*, never the full 250K set inline — pagination/sampling. (4) Idempotent edge writes (accepting an invite twice is a no-op).

---

### Data Modeling (Medium–Hard)

Q7. Design the durable edge store and the in-memory adjacency representation.
> **Expected schema:**
```sql
-- Durable edge store (source of truth). Undirected connection stored as TWO rows
-- for O(1) neighbor lookup from either side. Sharded by member_id.
CREATE TABLE connections (
  member_id     bigint,        -- shard/partition key
  neighbor_id   bigint,
  status        smallint,      -- 1=connected 2=pending_out 3=pending_in 4=blocked
  connected_at  timestamptz,
  PRIMARY KEY (member_id, neighbor_id)
);
```
```
# In-memory graph (per shard): Compressed Sparse Row (CSR) adjacency
#   offsets[]  : length = num_local_members + 1
#   neighbors[]: flat array of neighbor ids, sorted, delta+varint compressed
#   member m's neighbors = neighbors[ offsets[m] : offsets[m+1] ]
# Plus a per-member Bloom filter of its neighbor set for O(1) membership tests.
```
> **Index choices and why:** In the DB, PK (member_id, neighbor_id) gives a single-partition read of "all of A's connections" and an O(log) point check "is A–B connected." In memory, CSR is cache-friendly and compact (sequential neighbor scan), far better than pointer-chasing hash maps of maps for BFS frontier expansion. Per-node Bloom filters make frontier-intersection tests cheap.
> **Partitioning key and why:** member_id in both DB and graph service. Storing the edge twice (both directions) is deliberate denormalization: BFS from either endpoint needs local neighbor access without a cross-shard hop to "find edges where neighbor=A."

Q8. How do you answer "size/sample of my 2nd-degree network" for People-You-May-Know candidate generation, efficiently?
> **Expected answer:** Expand 1st-degree (adjacency of A), union their adjacency lists, subtract A's own 1st-degree and A itself → 2nd-degree set. For *count*, use a HyperLogLog-style cardinality estimate (exact count of a 250K-element multi-hop union per request is wasteful and rarely needs to be exact). For *candidates*, sample: prioritize members reachable through many mutual connections (a member appearing via 20 of your connections is a strong PYMK signal) — that's a count-of-paths, computed by counting frontier multiplicity, not just membership. Cache per-member with a longer TTL since 2nd-degree changes slowly relative to reads.
> **Trap:** Materializing and storing the full 2nd-degree set per member. It's ~250K entries that churn constantly — compute on demand, estimate cardinality, sample smartly.

Q9. Consistency: a member accepts a connection. When must the degree label flip, and where is eventual consistency acceptable?
> **Expected answer:** The **edge in the durable store is strongly consistent** (accept is a committed transaction; you must not lose an accepted connection). The **in-memory graph index is eventually consistent** — it's updated via the Kafka edge-event stream within ~seconds. So immediately after accepting, a query might still label the pair "2nd" for a moment. That's acceptable (AP for labels). Read-your-writes exception: on *your own* connections page you should see the new 1st-degree connection immediately — serve that from the durable store, not the lagging index.
> **Mentor pushback:** Member A *blocks* B. Blocking must take effect immediately for safety — B must not see A's content or appear in-network. But the in-memory index lags. → Blocks are checked against the strongly-consistent store (or a fast-propagated block cache), never left solely to the eventually-consistent graph index; degree/visibility applies the block at query time.

---

### Low-Level Design (Hard)

Q10. Implement bounded bidirectional BFS across a sharded, in-memory graph.
> **Problem statement:** Given A and B on possibly different shards, return their distance (≤3) and a connecting path, minimizing cross-shard RPCs and total nodes explored, at ~ms latency.
> **Naive solution:** Single-source BFS from A across shards until you hit B, expanding full frontiers each level.
> **Why naive fails at scale:** With avg degree ~500, level-3 frontier is ~125M nodes and spans every shard — you'd RPC-storm the whole cluster and blow the latency budget. Even one 3-hop query could touch billions of adjacency entries.
> **Expected optimal approach:** Bidirectional BFS, at most ~1.5 levels each side, **always expand the smaller frontier next** (a member with 50 connections vs one with 30,000 — expand the small one). Intersect frontiers using **Bloom filters / hash sets** per level. Cap total depth at 3; if frontiers haven't met, return "out of network." Record predecessors to reconstruct the path. Batch cross-shard neighbor fetches (one RPC per shard per level, not per node).
> **Pseudo-code or class diagram:**
```
def distance(A, B, max_depth=3):
    if A == B: return 0
    fwd = {A: None}; bwd = {B: None}      # node -> predecessor
    fwd_frontier = {A}; bwd_frontier = {B}
    depth = 0
    while depth < max_depth:
        # expand the SMALLER frontier this round (balances work)
        if total_degree(fwd_frontier) <= total_degree(bwd_frontier):
            fwd_frontier = expand(fwd_frontier, fwd)     # batched per-shard RPCs
            hit = fwd_frontier & bwd.keys()              # set/Bloom intersection
        else:
            bwd_frontier = expand(bwd_frontier, bwd)
            hit = bwd_frontier & fwd.keys()
        depth += 1
        if hit:
            meet = pick(hit)
            return len(path(fwd, meet)) + len(path(bwd, meet))  # + reconstruct
    return "out_of_network"   # >3 degrees

def expand(frontier, visited):
    # group frontier by owning shard, one RPC per shard, merge neighbors
    next = set()
    for shard, ids in group_by_shard(frontier):
        for m, neighbors in shard.rpc_get_adj(ids):
            for n in neighbors:
                if n not in visited:
                    visited[n] = m           # predecessor for path reconstruction
                    next.add(n)
    return next
```

Q11. Concurrency: an edge is removed (A–B unfriend) while a BFS is mid-flight using that edge, or two edge-events for the same pair arrive out of order.
> **Scenario:** BFS reads A's neighbor list including B; concurrently A removes B; the query returns a path through a now-deleted edge. Or Kafka delivers "remove A–B" before "add A–B" due to partition reordering.
> **Expected fix:** (1) Snapshot/versioned reads of adjacency (copy-on-write CSR segments or MVCC) so a query sees a consistent snapshot; a slightly-stale path is acceptable (eventual consistency) and the durable store is authoritative if it matters. (2) Ordering: key edge-events by the **ordered pair (min(A,B),max(A,B))** so all events for a pair land on the *same Kafka partition* → in-order delivery; apply with a monotonic version/timestamp and ignore stale events (last-write-wins on the edge state). This makes reordered add/remove impossible for a given pair.
> **Follow-up:** What if a graph shard node dies mid-query? The query engine retries the affected shard's sub-request against a **replica** (graph shards are replicated); partial results from other shards are still valid because BFS levels are recomputable and idempotent.

Q12. Failure deep-dive: a graph shard's process restarts and loses its in-memory adjacency; or the Kafka edge-stream lags badly.
> **Scenario:** RAM graph is volatile — a restart empties a shard. Or the edge-change consumer falls behind and the index is minutes stale.
> **Expected handling:** (1) **Rebuild from source of truth**: the shard bootstraps its CSR from the durable edge store (bulk scan of its member_id range) + replays recent Kafka edge-events from the last checkpoint offset to catch up to now. Serve from a warm **replica** while the cold shard rebuilds (never serve an empty shard). (2) Edge-stream lag → autoscale consumers, and because labels are AP, a few seconds of staleness is tolerable; critical operations (blocks) bypass the index. (3) A poison edge-event that crashes the applier goes to a DLQ after N retries; the shard skips it rather than wedging the whole stream.

---

### Scaling to 10x / 100x (Hard)

Q13. At 10x, where does this break first?
> **Expected answer:** **Cross-shard fan-out during BFS.** As the graph grows, high-degree "super-connectors" (30K-connection accounts, recruiters, influencers) make frontier expansion touch every shard, and query latency is dominated by the slowest shard RPC (tail latency amplification — a 3-hop query fanning to 100 shards is as slow as the slowest of 100). RAM capacity is second (more members → more shards), but the fan-out tail is what blows the p99 first.
> **Numbers to ground the answer:** At ~300K degree-QPS × avg (say) 3 cross-shard RPCs = ~1M internal RPCs/sec; a p99 of 1ms per shard RPC but fanning to 100 shards means the *query* p99 tracks the 99th-percentile-of-100 shards ≈ much worse than 1ms. This is why frontier size must be bounded and super-nodes handled specially.

Q14. How do you shard the graph, and how do you handle super-connector hotspots?
> **Expected sharding strategy:** Consistent hashing by member_id — evenly distributes members and their adjacency lists; the ID embeds/maps to the shard so you route neighbor fetches directly. Undirected edges stored both directions so each side's neighbors are local.
> **Hot spot problem:** Super-connectors: their adjacency list is huge (30K+) and their member row is read constantly. Detect via per-member access counters + degree stats. Fixes: (1) **Never expand a super-node's full frontier** in bidirectional BFS — expand the *other, smaller* side and test membership against the super-node instead (the "expand smaller frontier" rule handles this automatically). (2) Replicate hot super-node adjacency lists across replicas to spread read load. (3) Cache frequent (viewer, super-node) distances. (4) Optionally partition a giant adjacency list across sub-shards.

Q15. Design caching and name the hardest invalidation.
> **Expected layered cache design:** L1 = the in-memory graph itself (the primary "cache" — the whole graph in RAM). L2 = Redis result cache for hot (viewer, target) distance pairs and per-member 2nd-degree cardinality, with short TTLs. L3 = client/edge cache of the degree labels on a rendered page (they rarely change within a session).
> **Cache invalidation trap:** A **single new edge invalidates a huge, non-local set of cached results.** If A connects to B, every cached "distance(X, B)" and "distance(A, Y)" that passed through this new edge could change (someone who was 3rd-degree via A–B is now 2nd). You cannot enumerate all affected pairs. Solution: don't try to precisely invalidate — use **short TTLs** on distance results (seconds to low minutes) so staleness self-heals, and rely on the fact that degree labels are AP. For 1st-degree (the one that *must* be right immediately — it drives messaging permissions), bypass the cache and check the durable store. This is the crux: precise invalidation is impossible, so you bound staleness with TTL and treat only 1st-degree as strongly consistent.

Q16. Cost/efficiency levers at scale?
> **Expected answer:** (1) CSR + delta/varint compression of adjacency lists — cuts the ~4TB graph to ~2–2.5TB RAM, halving the fleet. (2) Bound every query to 3 hops and expand-smaller-first — caps compute per query. (3) Batch degree lookups (one neighborhood expansion serves 25 search results). (4) Cache 2nd-degree cardinality (slow-changing) with long TTLs; only recompute distance (fast-changing) frequently. (5) Tier by activity — dormant members' adjacency can be compressed harder / colder, hydrated on access. (6) Estimate (HyperLogLog) instead of exact-counting large network sizes.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Deep internals: why CSR (Compressed Sparse Row) over a hash-map-of-sets or a native graph DB (Neo4j) for this workload? Discuss cache locality during frontier scans, memory overhead per edge (~4–5 bytes CSR vs ~40+ bytes pointer-based), and why a general graph DB's ACID traversal engine is the wrong tool when you need one bounded query type at 300K QPS. Mention LinkedIn's "LIquid" graph engine as the real-world instance.

**H2.** Privacy/visibility: the graph must respect that some members hide their connection list, blocks must be enforced, and 2nd-degree paths shouldn't leak a hidden connection ("you're connected through X" where X hid the connection). How do you filter paths for visibility *without* destroying BFS performance?

**H3.** Operational: roll out a new graph-service version holding 2.5TB across replicas without a cold-start latency cliff. Discuss replica-by-replica bootstrap from the durable store + Kafka catch-up, keeping a warm replica serving throughout, and shadow-comparing distances between old and new before cutover.

**H4.** Observability: BFS fan-out width per query, cross-shard RPC count, per-shard p99 (tail-latency amplification is the killer metric), edge-stream consumer lag, in-memory graph vs durable-store edge-count drift (consistency drift alarm), cache hit rate on distance results. The single most important SLO: **degree-label p99 for batched search-page queries**.

**H5.** "Undo a bad decision": v1 computed degree with an on-the-fly SQL recursive CTE against the edge DB (works at 10M members, dies at 1B). Migrate to the in-memory graph service without downtime: build the graph service in shadow, dual-read and compare labels for correctness, route a small % of degree queries to it behind a flag, ramp to 100%, then retire the SQL path.

---

### Mentor's Closing Notes

**Top 3 things most candidates get wrong on this topic:**
1. Trying to *materialize* 2nd/3rd-degree sets — combinatorial explosion and impossible invalidation.
2. Running plain single-source BFS instead of bidirectional-with-smaller-frontier-first — 1000x+ more work and unbounded fan-out.
3. Treating the in-memory graph as the system of record instead of a rebuildable index over a durable edge store.

**The one insight that makes an answer truly impressive:**
The whole design turns on one asymmetry: *edges are cheap to store durably but expensive to traverse, and multi-hop reachability is impossible to precompute.* So you invert the usual "cache the answer" instinct — you cache the *graph* (in RAM) and compute the answer per request with a bounded, direction-aware BFS, bounding staleness with TTLs rather than trying to invalidate an unenumerable set of derived results. Expand-the-smaller-frontier is the one line that makes super-connectors tractable.

**Suggested follow-up reading:**
- LinkedIn Engineering — "LIquid: A Distributed Graph Database" and the earlier in-memory graph service posts.
- Facebook TAO paper (association store contrast); "Bidirectional Search" and CSR graph representation references.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
