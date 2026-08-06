# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 40 of 63 — Phase 2: System Design Track (Design 12 of 35)
**Topic:** Distributed Key-Value Store (DynamoDB-like)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Amazon's 2007 Dynamo paper is arguably the most influential distributed-systems paper of the last two decades — it popularized consistent hashing with virtual nodes, vector clocks, quorum reads/writes, and "always-writeable" availability, and its ideas live on in DynamoDB, Cassandra, Riak, and Voldemort. The hard part is delivering single-digit-millisecond gets/puts at any scale with 99.99% availability while surviving node failures and network partitions — which forces you to give up strong consistency and instead reason precisely about conflict detection and resolution. This is where CAP stops being a slide and becomes a design.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Consistent hashing (taught in Phase 1, Lesson 19):** Keys and nodes are hashed onto a ring; a key is owned by the first node clockwise. Adding/removing a node only remaps keys in one arc, so ~1/N of keys move rather than everything (as with `hash%N`). Dynamo adds **virtual nodes** — each physical node holds many tokens on the ring — for smoother load distribution and faster rebalancing. This is the literal partitioning scheme of our store.

**CAP / PACELC (taught in Phase 1, Lesson 2):** During a partition (P) you choose Availability or Consistency; Dynamo chooses AP. PACELC adds: Else (no partition), choose Latency or Consistency — Dynamo chooses L. Every knob today (N, R, W) is a point on this spectrum, tunable per-request.

**HA / replication / quorum (taught in Phase 1, Lesson 3):** Each key is replicated to N nodes (the "preference list"). A write succeeds after W acks, a read after R responses; **R + W > N** gives read-your-writes overlap. We tune (N,R,W) = (3,2,2) for balance, (3,3,1) for fast reads, (3,1,3) for fast writes.

**Merkle trees & Bloom filters (taught in Phase 1, Lesson 5):** Merkle trees let two replicas compare a key range by exchanging O(log n) hashes to find exactly which keys diverged — the core of anti-entropy repair. Bloom filters let an SSTable answer "definitely not here" without disk I/O, critical for LSM read performance.

**B-tree vs LSM (taught in Phase 1, Lesson 23):** The storage engine is an **LSM tree**: writes hit a commit log + in-memory memtable, flush to immutable sorted SSTables, and background compaction merges them. Writes are sequential (fast); reads may touch multiple SSTables (mitigated by Bloom filters + block cache). This is why KV stores favor write-heavy workloads.

**Caches / Redis (taught in Phase 1, Lesson 24):** DynamoDB Accelerator (DAX) is a write-through/read-through cache in front of the store for microsecond reads. We'll use a block cache internally and an optional DAX-style layer externally.

**Capacity estimation (taught in Phase 1, Lesson 28):** We'll size partitions, RCU/WCU, and storage. Keep item_size × rate and the 3KB/1KB WCU/RCU unit conventions handy.

> **Recap box:**
> - Consistent hashing + vnodes = partitioning and smooth rebalancing.
> - N/R/W with R+W>N = tunable quorum consistency.
> - Vector clocks detect conflicts; app or LWW resolves them.
> - Merkle trees = efficient anti-entropy; hinted handoff = temporary-failure durability.
> - LSM tree + Bloom filters = the storage engine; AP + last-writer-wins is the default.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Why does Dynamo choose availability over consistency, and what does "always writeable" buy Amazon?
> **What a strong answer covers:** The shopping cart use case: a customer must always be able to add to cart even during failures/partitions — a rejected write is lost revenue. So Dynamo never rejects a write for consistency reasons; it accepts writes on any reachable replica and reconciles later. The cost is that reads may return multiple conflicting versions that the application must merge (e.g., union the cart). This is the AP corner of CAP: prioritize write availability, push conflict resolution to read time.
> **Common weak answer:** "Dynamo is eventually consistent because it's faster." Misses the deliberate business trade-off and that consistency is *tunable* per request via R/W, not a fixed property.
> **Mentor follow-up if they answer well:** If you always accept writes, two replicas can hold divergent carts. How do you even *detect* that they diverged, before you resolve it?

Q2. Estimate capacity for a session store: 50M active users, 1 KB session item, 100K reads/sec and 20K writes/sec, N=3. Compute storage, replicated storage, and DynamoDB-style RCU/WCU.
> **What a strong answer covers:** Logical storage = 50M × 1 KB = 50 GB. Replicated (N=3) = 150 GB — trivially small; this is a throughput problem, not a storage one. WCU (1 WCU = 1 KB write): 20K writes/s × 1 = 20,000 WCU (strongly consistent write charges full; item ≤1KB). RCU (1 RCU = 2 eventually-consistent 4KB reads, or 1 strongly-consistent 4KB read): item is 1 KB so 1 read = 0.5 RCU eventually-consistent → 100K reads/s ÷ 2 = 50,000 RCU; strongly consistent would be 100K RCU. Partitioning: DynamoDB caps a partition at ~3,000 RCU / 1,000 WCU / 10 GB, so you need ≥20 partitions for write throughput alone.
> **Mentor follow-up:** If one user's session key gets 5,000 writes/sec, none of that partition math saves you. What's the failure and the fix? (Hot partition → adaptive capacity/write sharding.)

Q3. You have N=3. When would you pick (R,W) = (1,1) versus (2,2) versus (3,1)?
> **What a strong answer covers:** (2,2): R+W=4>3, balanced, read-your-writes, tolerates 1 failure — the default. (1,1): R+W=2<3, fastest and most available but stale reads possible; good for caches/metrics where staleness is fine. (3,1): fast durable writes but reads must contact all 3 (fragile under failure) — rarely used. (1,3): fast reads, slow/less-available writes — good for read-heavy config data. Key: only R+W>N guarantees read/write set overlap and thus read-your-writes.
> **Red flag answer:** Claiming R+W>N gives *strong* (linearizable) consistency. It gives overlap so a read *sees* at least one node with the latest write, but without additional coordination (sloppy quorum, concurrent writes, read repair timing) you can still read stale/conflicting versions. It's stronger, not linearizable.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the high-level architecture of a Dynamo-like KV store. Draw it.
> **Key components expected:** A ring of storage nodes (each with many virtual nodes), a coordinator (any node can coordinate a request), the preference list per key, gossip-based membership, hinted handoff buffers, Merkle-tree anti-entropy, and the LSM storage engine per node. Note the decentralized, masterless design — no single leader.
> **Architecture diagram (text):**
```
   Client
     │ get(k)/put(k,v)   (any node can coordinate)
     ▼
 ┌────────────────────────── Consistent Hash Ring ──────────────────────────┐
 │  hash(k)=position  ─▶ walk clockwise ─▶ preference list = next N distinct  │
 │                        physical nodes (skipping vnodes of same host)      │
 │                                                                           │
 │   NodeA(vnodes:a1,a7,a12)   NodeB(b3,b9,b15)   NodeC(c2,c5,c11) ...        │
 │        │  coordinator for k                                               │
 │        ├── replicate put to N=3 (A,B,C) ── wait for W acks ──▶ ack client │
 │        │        │           │            │                                │
 │        │   ┌────▼────┐  ┌───▼────┐   ┌───▼────┐                            │
 │        │   │ LSM:    │  │ LSM    │   │ LSM    │  (commitlog+memtable+      │
 │        │   │ +Merkle │  │+Merkle │   │+Merkle │   SSTables+Bloom+block$)   │
 │        │   └─────────┘  └────────┘   └────────┘                           │
 │        │                                                                  │
 │   Gossip (membership, ring state) ↔ every node ↔ every node               │
 │   Hinted handoff: if C down, write goes to D with "hint for C"            │
 │   Anti-entropy: A↔B↔C compare Merkle roots per range, repair diffs        │
 └───────────────────────────────────────────────────────────────────────────┘
```
> **What separates SDE2 from SDE3 here:** SDE2 draws a ring and replicas. SDE3 explains the *three* failure-handling mechanisms and when each fires: **sloppy quorum + hinted handoff** for transient failures (keep availability during a blip), **read repair** for divergence found on the read path (cheap, opportunistic), and **Merkle-tree anti-entropy** for divergence that never gets read (background, thorough). Missing any one leaves a durability or consistency hole.

Q5. Trace a `put(key, value)` end to end, including a temporarily-down replica.
> **Expected trace:**
> 1. Client sends put to any node → that node becomes coordinator (or forwards to a node in the preference list).
> 2. Coordinator computes `hash(key)`, walks the ring to build the preference list (first N *distinct physical* nodes, skipping vnodes on already-chosen hosts and unreachable nodes).
> 3. Coordinator generates/advances the **vector clock** for the key, writes locally (commit log + memtable), and forwards the write to the other N-1 replicas.
> 4. It waits for **W-1** acks (plus its own = W). With (N,W)=(3,2), one ack from a peer suffices → ack the client.
> 5. If replica C is down, coordinator sends the write to the next healthy node D on the ring with a **hint** ("this belongs to C"). D stores it in a separate hinted-handoff area and, when C recovers (detected via gossip), delivers it and deletes the hint. This is **sloppy quorum** — W nodes, but not necessarily the "home" N.
> **Tricky part:** The vector clock. If two coordinators handle concurrent puts for the same key (e.g., during a partition), each advances its own counter, producing *concurrent* (non-comparable) clocks. The system must keep both versions (siblings), not silently overwrite — candidates who "just take the latest timestamp" have quietly reintroduced lost updates.

Q6. Design the API. What operations, and how do clients handle conflicts?
> **Expected API design:** `get(key) -> (value | list<value siblings>, context)` where *context* is the opaque vector clock. `put(key, context, value)` — the client must pass back the context from a prior get so the store can causally order the write. `delete(key, context)` — deletes are **tombstones** (a versioned marker), not immediate removals, so a delete can't be resurrected by a stale replica. Batch: `batchGet`, `batchPut`. Conditional/`putIfAbsent` for optimistic concurrency.
> **What to push on:** Idempotency and the context token — a `put` without the latest context can create a sibling rather than overwrite. Ask how the client resolves siblings on read (semantic merge, e.g., cart = set union; or last-writer-wins if the app accepts loss). Push on pagination for scans (exclusive start key / continuation token) and why range scans are expensive on a hash-partitioned store (must fan out to all partitions).

---

### Data Modeling (Medium–Hard)
Q7. Design the storage-node data model and on-disk layout for the LSM engine.
> **Expected schema:**
```
Logical item:
  partition_key (hash key) | [sort_key] | attributes(blob/JSON) |
  vector_clock: { nodeA:3, nodeC:7 } | version_ts | tombstone_flag

On disk (LSM per node):
  commit_log (append-only WAL, fsync per W-write)   -> crash recovery
  memtable   (in-memory sorted map, e.g., skiplist) -> absorbs writes
     │ flush when full
  SSTable_0, SSTable_1, ... (immutable, sorted)     -> each with:
     ├─ data blocks (sorted key->value)
     ├─ sparse index (key -> block offset)
     ├─ Bloom filter (per SSTable: "key definitely absent?")
     └─ summary + min/max key range
  compaction: size-tiered or leveled merges SSTables, drops old versions
              and expired tombstones (after gc_grace_seconds)
```
> **Index choices and why:** Per-SSTable **Bloom filter** so a point read skips SSTables that can't contain the key (turns O(#SSTables) disk reads into ~1). A sparse block index (mmap'd) locates the block; a block cache holds hot blocks. For a composite (hash+sort) key, items with the same hash key are stored contiguously and sorted by sort key → efficient range queries within a partition.
> **Partitioning key and why:** The **hash/partition key** maps to the ring position and thus the preference list. Choose a high-cardinality, evenly-accessed attribute (userId, deviceId). A low-cardinality or monotonically increasing key (e.g., today's date) creates a hot partition. Sort key gives ordered access within a partition without extra partitioning cost.

Q8. A client needs "give me the last 20 orders for user U, newest first." How do you model and serve this efficiently on a KV store?
> **Expected answer:** Use a composite key: partition_key = userId, sort_key = orderTimestamp (or a reverse/inverted timestamp so a forward scan yields newest-first). All of U's orders live in one partition, physically sorted by sort key; a range query with `limit 20` and `ScanIndexForward=false` reads one contiguous block — a single partition, single disk region, Bloom-filter-friendly. For access patterns not keyed by userId (e.g., "orders in state=SHIPPED"), add a **Global Secondary Index (GSI)** — a separate table partitioned by the alternate key, asynchronously maintained (eventually consistent).
> **Trap:** Doing a **Scan** (full-table) and filtering in the app, or modeling one item per attribute and issuing N gets. Scan reads every partition and is O(table); it will blow your RCU and latency. Single-table design with well-chosen sort keys and GSIs is the DynamoDB idiom — model to your access patterns, not to normalized relations.

Q9. Frame the consistency model precisely. Where is eventual acceptable, and give a concrete scenario where it burns you.
> **Expected answer:** Default is eventual consistency (AP, PACELC-L): a read may hit a replica that hasn't yet received the latest write. R+W>N gives quorum overlap so a *quorum* read sees the latest, but concurrent writes still create siblings. Eventual is fine for carts, sessions, product views, feature flags. DynamoDB offers optional **strongly consistent reads** (read from the leader/majority, ~2× cost, no partition tolerance during failure) for read-after-write needs like "did my profile update save?"
> **Mentor pushback:** Two users share a bank-balance item; both read $100, both withdraw $60 concurrently on different coordinators during a partition. With last-writer-wins you lose one withdrawal → balance $40 instead of $-20/rejected. Eventual consistency + LWW silently loses an update. Fix: this item needs strong consistency + conditional write (`putIfExists balance == expected`) or a CRDT counter (PN-Counter) — not plain LWW. The lesson: pick consistency per *item class*, not globally.

---

### Low-Level Design (Hard)
Q10. Design conflict detection and resolution across replicas. How do you know two versions conflict, and how do you reconcile?
> **Problem statement:** Under sloppy quorum and partitions, the same key can be written on different replicas without seeing each other. On read you may collect divergent values; you must decide which is newer, which are concurrent, and how to merge — without losing data.
> **Naive solution:** Last-writer-wins by wall-clock timestamp on every key.
> **Why naive fails at scale:** Clock skew between nodes means a *later* real write with an *earlier* clock loses → silent data loss. And it can't represent "these two writes are concurrent and both matter" (the cart problem). Physical timestamps have no causal information.
> **Expected optimal approach:** **Vector clocks** — a map {node → counter} per version. On write, the coordinator increments its own counter. Compare two clocks: if every component of A ≤ B (and at least one <), A *happened-before* B → keep B. If neither dominates → **concurrent** → keep both as siblings and resolve at read time (semantic merge, or LWW if the app opts in). Bound sibling growth by truncating the oldest (node, counter) pairs (Dynamo caps at ~10). Alternatively use **CRDTs** (G-Set, OR-Set, PN-Counter) for automatic, mathematically-guaranteed merge without app logic, or **dotted version vectors** to avoid sibling explosion from the same client.
> **Pseudo-code or class diagram:**
```
compare(clockA, clockB):
    aLTE = all(A[n] <= B[n] for n in keys)   # A <= B on every node
    bLTE = all(B[n] <= A[n] for n in keys)
    if aLTE and bLTE: return EQUAL
    if aLTE:          return B_NEWER        # A happened-before B
    if bLTE:          return A_NEWER
    return CONCURRENT                        # keep both as siblings

reconcile(versions):
    survivors = [v for v in versions if not dominated_by_any_other(v)]
    if len(survivors) == 1: return survivors[0]
    return app_merge(survivors)   # e.g., cart = union of items; or LWW
```

Q11. Two coordinators concurrently process `put(cart, ...)` for the same user during a network partition. Walk the race and the guarantee you provide.
> **Scenario:** Partition splits replicas {A} | {B,C}. Client 1 (add "book") reaches A → clock {A:5}. Client 2 (add "pen") reaches B → clock {B:4} (both descend from a common {A:4,B:3}). Partition heals; anti-entropy exchanges versions.
> **Expected fix:** Neither clock dominates → **concurrent** → both survive as siblings. Next `get` returns both; the cart app performs a **semantic merge** = set union → {book, pen}, and writes back a merged version with a clock that dominates both {A:5,B:4,...}. No write is lost. This is exactly why Dynamo keeps siblings instead of LWW for carts. Contrast: a bank balance can't be merged this way — it needs strong consistency, not sibling reconciliation.
> **Follow-up:** What if the coordinator (or a replica holding an un-handed-off hint) dies before delivering? Durability rests on W: the write is on W nodes before ack, so one death is survivable. If a *hinted* node dies before handoff, the hint (and that copy) is lost — which is why hinted handoff is best-effort and Merkle anti-entropy is the backstop: it will eventually detect and repair the missing key by comparing range hashes.

Q12. A node comes back after 2 hours down. How do you get it consistent without re-shipping its entire dataset, and how do you handle a node that's gone forever?
> **Scenario:** Transient failure (reboot) vs permanent failure (dead hardware) vs silent bit-rot divergence.
> **Expected handling:** Transient: hinted handoff delivers buffered writes it missed; then **Merkle-tree anti-entropy** with its replica peers — each node keeps a Merkle tree per key range; two replicas exchange root hashes, and only where roots differ do they descend the tree (O(log n) messages) to pinpoint and repair the exact divergent keys — no full-dataset transfer. Permanent failure: gossip marks it dead; the ring rebalances the preference lists to new nodes and streams the affected ranges (bootstrap). Deletes: tombstones with `gc_grace_seconds` ensure a delete propagates to all replicas before physical removal, so a lagging replica can't resurrect a deleted key. Duplicate/late writes are idempotent under vector-clock comparison.

---

### Scaling to 10x / 100x (Hard)
Q13. At 100x growth, where does this store break first?
> **Expected answer:** **Hot partitions**, not aggregate throughput. Consistent hashing spreads *keys* evenly, but not *access* — one celebrity key or a monotonic key (timestamp/sequential ID) sends all traffic to one partition/preference list, which caps out (~1,000 WCU / 3,000 RCU / node) regardless of cluster size. Secondary limits: compaction I/O falling behind write rate (read amplification balloons as SSTables pile up → tail latency), and gossip/membership overhead growing as the cluster reaches thousands of nodes (metadata convergence slows).
> **Numbers to ground the answer:** A single DynamoDB partition tops ~3,000 RCU / 1,000 WCU / 10 GB. A cart item at 1 KB and a viral 50K writes/s to one key = 50,000 WCU on a 1,000-WCU partition → 50× over. p99 read latency degrades sharply once SSTable count per level exceeds compaction throughput; watch pending compactions.

Q14. You learned consistent hashing with vnodes in Lesson 19 — apply it. How do you shard, and how do you fix a hot key?
> **Expected sharding strategy:** Consistent hashing with **virtual nodes** — each physical node owns ~100–256 tokens on the ring, so load variance is low and adding a node moves only ~1/N of data (from many nodes, in parallel — fast rebalancing). Vnode count trades metadata size vs balance smoothness.
> **Hot spot problem:** Detect via per-partition throttled-request / consumed-capacity metrics (one partition pegged, rest idle). Fixes: (1) **Write sharding** — append a suffix `key#[0..k]` to spread a hot key across k partitions; reads fan out to all k and merge (trade single-key locality for throughput). (2) **Adaptive capacity** (DynamoDB) automatically shifts throughput to the hot partition and can split it. (3) Put a **DAX/Redis cache** in front for hot *reads* (write-through), absorbing the read fan-out. (4) Fix monotonic keys by hashing/adding entropy to the partition key so sequential inserts spread across the ring.

Q15. Design the caching strategy. Where, and what's the hardest invalidation problem?
> **Expected layered cache design:** L0 = per-node **block cache** for hot SSTable blocks + memtable (recent writes). L1 = client-side/DAX cache for hot items (microsecond reads, write-through so writes update cache + store atomically-ish). L2 = a Redis/Memcached tier for cross-service hot keys. CDN is irrelevant here (dynamic data). Read-through for cache misses, write-through or write-around depending on read-after-write needs.
> **Cache invalidation trap:** In an **eventually consistent** store, a write-through cache can be *more* current than the replica a later strongly-consistent read hits, or a cache can hold a value that lost a sibling reconciliation — so cache and store disagree in ways that aren't just "stale." The hardest case: a delete (tombstone) that hasn't propagated; the cache may still serve the deleted item, or re-populate it from a lagging replica after invalidation. Mitigate with short TTLs on the cache, versioned cache entries (store the vector clock/version and reject writes that would regress it), and cache invalidation on the *coordinator* at write time rather than trusting replica reads.

Q16. Keep cost efficient at petabyte scale.
> **Expected answer:** (1) **Tiered/leveled compaction** tuned to workload — leveled for read-heavy (less read amp, more write amp), size-tiered for write-heavy — to control storage bloat from old versions/tombstones. (2) **Compression** of SSTable blocks (LZ4/zstd, ~3–4×). (3) **TTL** on items (sessions/carts auto-expire) so you don't pay to store dead data; TTL removal is free-ish via tombstone-at-compaction. (4) **Tiered storage** — cold partitions to cheaper media/S3-backed (DynamoDB Standard-IA). (5) Right-size N (3 is standard; some cold data at 2). (6) On-demand vs provisioned capacity and autoscaling to match diurnal traffic rather than provisioning for peak.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Vector clocks vs version vectors vs dotted version vectors vs CRDTs vs hybrid logical clocks. When does each win? Explain sibling explosion (a chatty client creating many concurrent versions), why DVVs solve it, and how HLCs give you causal ordering with near-physical-time semantics for TTL/ordering — and where LWW with NTP-synced clocks (Cassandra's default) is "good enough" versus silently lossy.

**H2.** Global tables / multi-region active-active. How do you replicate across regions with last-writer-wins per attribute, handle cross-region conflict resolution, bound replication lag (~1s typical), and reason about the fact that a region-local strongly-consistent read is NOT globally strongly consistent? Cover GDPR data residency: keeping EU keys in EU partitions.

**H3.** Zero-downtime resharding / adding capacity: how vnodes let you add a node and stream only its token ranges without a global stop; throttling bootstrap streaming so it doesn't starve foreground traffic; and how DynamoDB splits a hot partition transparently while serving reads.

**H4.** Observability: per-partition consumed vs provisioned capacity, ThrottledRequests, SuccessfulRequestLatency p50/p99/p999, pending compactions, SSTable-per-read (read amplification), hint queue depth, and anti-entropy repair progress. Which one is your leading indicator of a hot partition (throttles concentrated on one partition) versus a compaction problem (rising read latency with flat throughput)?

**H5.** "Undo a bad decision": you launched with LWW conflict resolution on the cart and customers report items vanishing from carts. Migrate to sibling-based semantic merge (or an OR-Set CRDT) without a maintenance window: dual-write, backfill vector clocks, shadow-read to compare merge outcomes, then flip resolution logic — while old LWW-written items still exist. Tell that story.

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Using wall-clock last-writer-wins as if it were free. It silently loses concurrent updates and is clock-skew-sensitive; know when siblings/vector clocks/CRDTs are mandatory.
2. Believing consistent hashing solves hot spots. It balances *keys*, not *load* — a single hot key or monotonic key defeats it entirely.
3. Treating R+W>N as strong consistency. It's quorum overlap, not linearizability; concurrent writes and sloppy quorum still yield stale/conflicting reads.

**The one insight that makes an answer truly impressive:**
Dynamo has *three distinct* anti-entropy mechanisms operating at different timescales, and naming when each fires is the tell of someone who's operated one: **hinted handoff** (seconds, transient failure, keep availability), **read repair** (opportunistic, on the read path, cheap), and **Merkle-tree background repair** (the durability backstop for keys never read). Choosing consistency *per item class* — LWW for views, siblings for carts, strong+conditional for balances — rather than globally, is the mark of real judgment.

**Suggested follow-up reading:**
- "Dynamo: Amazon's Highly Available Key-value Store" (DeCandia et al., SOSP 2007).
- Marc Brooker's blog on DynamoDB internals, and the "Amazon DynamoDB: A Scalable, Predictably Performant, and Fully Managed NoSQL Database Service" (USENIX ATC 2022) paper on adaptive capacity and heat management.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
