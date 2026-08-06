# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 3 of 63 — Phase 1: Foundations (Module 3 of 28)
**Module:** High Availability & Replication
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Lesson 1 told you availability is measured in nines; Lesson 2 told you consistency and availability trade off. This lesson is the *machinery* that actually buys you those nines: copies of your data on multiple machines and a plan for when one dies. Every "how do you make the database not a single point of failure?" interview question is answered here. GitHub's 2018 24-hour outage, and every clean 30-second failover you never noticed, come down to the choices in this lesson — sync vs async replication, the topology, the quorum math, and how fast your standby takes over. This is the backbone of Lesson 27 (Databases) and Lesson 41 (Fault Tolerance) in Phase 2.

## Learning Objectives
By the end of this lesson you can:
- Distinguish synchronous, asynchronous, and semi-synchronous replication by their latency and data-loss trade-offs.
- Explain master-slave replication and why reads scale but writes don't.
- Explain master-master replication and the write-conflict problem it creates.
- Compute whether a quorum configuration guarantees strong consistency using R + W > N.
- Describe failover and pick between hot and warm standby using RTO/RPO numbers.

## The Lesson

### Replication Types (synchronous, asynchronous, semi-synchronous)
**What it is (plain English):** Replication means keeping copies of your data on more than one machine. The three types differ in *when* the primary tells the client "done": after the copies are updated (sync), before (async), or after at least one copy confirms (semi-sync).

**The problem it solves:** One copy = one failure domain = data loss when that disk dies. Replication is the raw ingredient of both durability and availability. The type you choose decides your **RPO** (how much data you can lose) vs your write latency.

**How it works (mechanics):** The primary ships its write-ahead log (WAL) to replicas.
```
 Synchronous:   client -> [Primary] --write--> [Replica] --ack--> Primary --ack--> client
   client waits for replica; RPO = 0 (no loss) but latency = local + RTT (e.g., +40ms cross-DC)

 Asynchronous:  client -> [Primary] --ack--> client ; [Primary] --lazy--> [Replica]
   client waits only for primary (~1ms); if primary dies before shipping, that write is LOST (RPO > 0)

 Semi-sync:     primary waits for >=1 replica ack, not all N
   compromise: RPO ~0 with less latency than waiting for every replica
```
Worked number: 5 replicas, cross-DC RTT 40ms. Full sync to all = wait ~40ms and slowest replica. Semi-sync (wait for 1 of 5) = ~40ms but tolerant of 4 slow replicas. Async = ~1ms, risking the last few writes.

**Trade-offs / when NOT to use it:** Sync guarantees no loss but a slow/dead replica stalls every write (availability risk). Async is fast but loses recent writes on failover. Semi-sync balances but needs careful config (how many acks).

**Where you'll see it:** MySQL supports async (default) and semi-sync; PostgreSQL offers synchronous_commit levels; financial systems use sync for RPO = 0.

### Master-Slave (Primary-Replica) Replication
**What it is (plain English):** One node (master/primary) accepts all writes; one or more read-only nodes (slaves/replicas) copy from it and serve reads. Think one author, many photocopiers.

**The problem it solves:** Read-heavy workloads (often 90%+ reads). You scale reads horizontally by adding replicas, and you get failover candidates for free.

**How it works (mechanics):** All writes go to the primary, which streams its WAL to replicas; clients are routed — writes to primary, reads to any replica.
```
              writes
   client ----------> [PRIMARY] --WAL--> [Replica1] <-- reads
                          |         \--> [Replica2] <-- reads
                          \-------------> [Replica3] <-- reads
```
Worked number: primary handles 5,000 writes/s (its ceiling). Add 4 read replicas each serving 5,000 reads/s → 20,000 reads/s total, but writes are still capped at 5,000/s — replicas don't help writes. Replicas usually lag async by a few ms to seconds, so a read right after a write may be stale (the read-your-writes problem from Lesson 2).

**Trade-offs / when NOT to use it:** The primary is a single write bottleneck and a single point of failure for writes (until failover promotes a replica). Replica lag causes stale reads. Doesn't help write-heavy workloads — for that you shard or go master-master.

**Where you'll see it:** The default for MySQL, PostgreSQL, and Redis (replica reads). Most web apps run one primary + N read replicas.

### Master-Master (Multi-Primary) Replication
**What it is (plain English):** Two or more nodes each accept writes *and* replicate to each other. Any node can take a write, so there's no single write bottleneck or single write SPOF.

**The problem it solves:** Write scaling and write availability across regions — a user in the US writes to the US master, a user in the EU writes to the EU master, each with low latency, and they sync.

**How it works (mechanics):** Both nodes accept writes and replicate bidirectionally.
```
   US clients -> [Master-A] <==replicate==> [Master-B] <- EU clients
```
The hard part is **write conflicts**: if A sets `x=1` and B sets `x=2` on the same row at nearly the same time before replication, which wins?
```
 t0: A: x=1     B: x=2        (both accepted locally)
 t1: replicate -> conflict on x
 t2: resolve: last-write-wins (by timestamp) -> x=2, or app merge, or vector clocks
```
Common resolution: last-write-wins (needs synchronized clocks — risky), or CRDTs/app-level merge. Auto-increment IDs also collide, so you use offset ranges (A: odd, B: even) or UUIDs.

**Trade-offs / when NOT to use it:** Conflict resolution is genuinely hard and can silently drop writes (LWW discards the loser). Not worth it unless you truly need multi-region low-latency writes; most systems are better with one primary + sharding.

**Where you'll see it:** Multi-region active-active setups — CockroachDB, Cassandra (leaderless, effectively multi-master), Galera Cluster for MySQL, DynamoDB global tables.

### Quorum
**What it is (plain English):** In a system with N replicas, a quorum is the minimum number of nodes that must agree for an operation to count. You require a write to land on W nodes and a read to consult R nodes. Pick R and W so reads and writes overlap.

**The problem it solves:** It lets leaderless/replicated systems get strong consistency (or tunable consistency) without a single master, and stay available when some nodes are down.

**How it works (mechanics):** The rule: **if R + W > N, every read set overlaps every write set**, so a read is guaranteed to see the latest write. Worked example with N = 3:
```
 W=2, R=2  ->  R+W = 4 > 3  ->  STRONG (read always hits >=1 up-to-date node)
 W=1, R=1  ->  R+W = 2 <= 3 ->  fast but may read stale
 W=3, R=1  ->  R+W = 4 > 3  ->  strong; fast reads, but writes need all 3 (fragile)
```
Why overlap works: with N=3, W=2 write touches nodes {1,2}; any R=2 read touches at least one of {1,2} (pigeonhole), so it sees the new value. Availability: W=2 tolerates 1 node down and still commits.

**Trade-offs / when NOT to use it:** Higher W/R means stronger consistency but lower availability and higher latency (wait for more nodes). W = N means one dead node blocks all writes. Tuning is per-workload; there's no universally right R/W.

**Where you'll see it:** Dynamo-style systems — Cassandra (`QUORUM`, `ONE`, `ALL` per query), DynamoDB, Riak. This is the knob that makes them "tunable consistency" (Lesson 2's PACELC in action).

### Failover
**What it is (plain English):** Failover is the process of detecting that the primary died and promoting a replica to take its place so the system keeps working. Graded by **RTO** (Recovery Time Objective — how long you're down) and **RPO** (Recovery Point Objective — how much data you lose).

**The problem it solves:** A primary *will* eventually crash. Without automated failover, you're down until a human wakes up (minutes to hours). Failover shrinks that to seconds.

**How it works (mechanics):** (1) **Detect** — health checks/heartbeats miss a threshold (e.g., 3 missed at 1s intervals ≈ 3s). (2) **Elect** — pick the most up-to-date replica (consensus like Raft, or a monitor like Sentinel). (3) **Promote** — new primary starts accepting writes. (4) **Reroute** — update DNS/service discovery/VIP so clients follow.
```
 [Primary]X  heartbeat lost -> elect Replica2 (highest WAL offset) -> promote -> repoint clients
```
Worked number: detection 3s + election 2s + reroute (DNS TTL) 30s = ~35s RTO. If replication was async, RPO = the last unshipped writes (maybe 100ms–2s of data). The split-brain danger: if the old primary revives, you can get two primaries — fencing (STONITH) prevents it.

**Trade-offs / when NOT to use it:** Automated failover can trigger on false positives (a network blip) and cause split-brain or flapping. Async failover risks data loss; requiring sync/consensus adds latency. Manual failover is safer but slow.

**Where you'll see it:** Redis Sentinel, Patroni for PostgreSQL, MySQL Group Replication, Kubernetes, and every managed DB (RDS Multi-AZ failover ~60–120s).

### Hot Standby vs Warm Standby
**What it is (plain English):** A standby is a backup machine ready to take over. **Hot** = fully running and continuously in sync, ready to serve instantly. **Warm** = running and receiving updates but not serving traffic, needing a short promotion step. (Cold, for contrast, is powered off — minutes to hours.)

**The problem it solves:** They're the concrete implementations of failover targets, trading cost against how fast you recover (RTO).

**How it works (mechanics):**
```
 HOT:  [Primary] ==sync/near-sync==> [Standby: running + often serving reads]
       failover: near-instant, RTO seconds, RPO ~0
 WARM: [Primary] --async--> [Standby: running, applying WAL, NOT serving]
       failover: promote + warm caches, RTO ~minutes, RPO small (lag)
```
Worked comparison: Hot standby RTO ~5–30s, RPO ~0 (if sync), but you pay for a full second machine running hot 24/7 (~2x infra). Warm standby RTO ~1–5 min (needs promotion + cache warm-up), RPO seconds (async lag), cheaper because it isn't sized/tuned to serve peak. Cold RTO minutes–hours.

**Trade-offs / when NOT to use it:** Hot is expensive and, if sync, taxes write latency; overkill for non-critical systems. Warm risks a cold-cache latency spike right after promotion and small data loss. Match the choice to your RTO/RPO budget and the cost of downtime.

**Where you'll see it:** RDS Multi-AZ is effectively hot standby (sync replica, fast failover). Many teams keep a warm standby in a second region for disaster recovery, accepting minutes of RTO to save cost.

## Comparison Table

| Dimension | Synchronous | Semi-synchronous | Asynchronous |
|---|---|---|---|
| Client waits for | All replicas | ≥1 replica | Primary only |
| Write latency | Highest (+RTT) | Medium | Lowest (~1ms) |
| RPO (data loss) | 0 | ~0 | > 0 (recent writes) |
| Availability risk | Slow replica stalls writes | Tolerates some slow replicas | None from replicas |
| Best for | Financial ledgers | Balanced HA | Read scaling, geo replicas |

**Verdict:** Sync for zero-loss correctness, async for latency and read scaling, semi-sync when you want near-zero loss without paying for every replica.

## Common Misconceptions
- **Myth:** Adding read replicas scales writes. → **Reality:** Master-slave scales reads only; writes stay capped at the single primary. Write scaling needs sharding or multi-master.
- **Myth:** Master-master is a free upgrade. → **Reality:** It introduces write conflicts and split-brain risk; resolution (LWW/CRDTs) is hard and can drop writes.
- **Myth:** Quorum always means majority. → **Reality:** Quorum is any R,W with R+W > N for strong reads; W=3,R=1 on N=3 is a valid non-majority-read quorum.
- **Myth:** Failover means zero data loss. → **Reality:** With async replication, RPO > 0 — you lose the last unshipped writes; only sync gives RPO ≈ 0.
- **Myth:** A hot standby is just a backup. → **Reality:** It's a live, continuously-synced replica ready in seconds; a cold backup can take hours.

## Real-World Case
In October 2018, GitHub suffered a 24-hour degradation triggered by a 43-second network partition between its US East Coast and West Coast data centers. Their MySQL used a primary on the East Coast with replicas elsewhere. When the link dropped, the automated orchestrator (Orchestrator/Raft) promoted a West Coast replica to primary — but by the time the partition healed, writes had landed on *both* coasts. Reconciling those divergent write histories, safely and without losing customer data, took nearly a full day of careful manual work. The root lesson: automated failover across regions with async replication can create split-brain, and the recovery cost dwarfs the outage that triggered it. GitHub afterward invested heavily in consistency guarantees and failover tooling. This is quorum, failover, and RPO — all in one very expensive day.

## Self-Test (answers at the bottom)
1. State the difference between synchronous and asynchronous replication in terms of RPO and write latency.
2. In master-slave, you add 4 read replicas to a primary handling 5,000 writes/s and 5,000 reads/s each. What's your new max read and write throughput?
3. With N = 5 replicas, give a quorum (R, W) that guarantees strong consistency while tolerating 2 nodes being down for writes. Show the R+W > N check.
4. A network blip causes your orchestrator to promote a replica while the old primary is still alive. Name the failure, one thing that goes wrong, and one mechanism that prevents it.
5. Design sketch: A payments service needs RPO = 0 and RTO under 30s. Choose replication type, topology, standby type, and quorum, and justify each with a number.

## Interview Soundbites
- "Replication type is an RPO-vs-latency dial: sync gives RPO zero at the cost of write latency, async gives ~1ms writes at the cost of losing the last few writes on failover."
- "Master-slave scales reads, not writes — the primary is still the single write bottleneck; for write scale I shard or accept multi-master's conflict resolution."
- "Quorum strong consistency is just R + W > N so the read and write sets always overlap — tune R and W to trade consistency against availability and latency."

## Mini-Assignment
On paper (~30 min): (1) For N = 3 and N = 5, tabulate every sensible (R, W) pair, mark which give strong consistency (R+W > N), and note how many node failures each write quorum tolerates. (2) Design the failover sequence for a PostgreSQL primary with one hot and one warm standby: write the detect → elect → promote → reroute steps with a time estimate for each, compute total RTO, and state the RPO for sync vs async replication. Note where split-brain could occur and how you'd fence it.

## Recap & Tomorrow
- **Replication types:** sync (RPO 0, high latency), async (~1ms, RPO > 0), semi-sync (near-zero loss, moderate latency).
- **Master-slave:** one write primary + read replicas — scales reads, not writes; watch replica lag.
- **Master-master:** multi-primary for write scale/geo, at the cost of conflict resolution and split-brain.
- **Quorum:** R + W > N guarantees strong reads; the knob for tunable consistency vs availability.
- **Failover:** detect → elect → promote → reroute, graded by RTO/RPO; beware split-brain, use fencing.
- **Hot vs warm standby:** hot = live, RTO seconds, ~2x cost; warm = promote-on-demand, RTO minutes, cheaper.

Tomorrow, **Lesson 4 — Advanced Data Structures I**: LRU cache internals, graph representations, and Dijkstra's shortest-path algorithm worked by hand — the structures under nearly every Phase 2 design.

## Self-Test Answers
1. Synchronous: the primary waits for replica acknowledgment before confirming, so RPO = 0 (no committed write is lost) but write latency includes the round trip to the replica (e.g., +40ms cross-DC). Asynchronous: the primary confirms immediately (~1ms) and ships to replicas later, so latency is low but RPO > 0 — writes not yet shipped are lost if the primary dies.
2. Reads: 5,000 (primary, if used for reads) or offloaded — with 4 replicas at 5,000 reads/s each you get ~20,000 reads/s. Writes: still 5,000/s, because all writes go to the single primary; replicas don't add write capacity.
3. R = 3, W = 3 on N = 5: R+W = 6 > 5 → strong. W = 3 tolerates 2 nodes down (still reach 3 of 5). Alternatively W = 4, R = 2 (R+W = 6 > 5) tolerates only 1 write failure, so W = 3, R = 3 better meets "tolerate 2 down for writes."
4. Split-brain — two primaries accept writes simultaneously, so data diverges and reconciliation may lose or corrupt writes. Prevented by fencing (STONITH — "shoot the other node in the head") or requiring a consensus/quorum (Raft) so only one node can hold leadership.
5. RPO = 0 demands synchronous (or semi-sync) replication. Topology: single primary + a hot synchronous standby in another AZ (RPO ~0). Quorum/consensus-based promotion (Raft/Patroni) for safe election. RTO < 30s: heartbeat detection ~3s + election ~2s + reroute via low-TTL DNS or a VIP ~10s ≈ 15s, comfortably under 30s. Use fencing to prevent split-brain. Hot standby (not warm) because warm's promotion + cache warm-up would blow the 30s RTO.
