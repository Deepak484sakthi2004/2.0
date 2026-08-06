# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 13 of 63 — Phase 1: Foundations (Module 13 of 28)
**Module:** Deployment Topologies
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Every design you sketch in Phase 2 eventually has to *run somewhere*, on real machines, with real failure modes. The difference between a junior and a senior answer to "how do you deploy this?" is whether you can reason about what happens when *one box dies at 3 AM*. A single-master design that looks clean on a whiteboard can take down a payment system for six hours; a master-master design that looks "more available" can silently corrupt data. Companies like GitHub, GitLab, and every bank live and die by these topologies. Today we build the ladder — from one lonely server to master-master replication — and then the packaging spectrum: VMs vs Docker vs Kubernetes.

## Learning Objectives
By the end of this lesson you can:
- Draw and critique the four classic database/server topologies and state each one's exact failure behavior.
- Compute availability and RPO differences between single-master and master-slave with real numbers.
- Explain *why* master-master invites write conflicts and how systems try to avoid them.
- Distinguish a VM, a container, and a Kubernetes-managed container by what they virtualize and their real startup/overhead numbers.
- Pick a topology for a given workload using durability and RTO/RPO reasoning, not vibes.

## The Lesson

### Single Master (single server / single node)
**What it is (plain English):** One machine does everything — it serves reads, serves writes, and holds the only copy of the data. Think of a shop with exactly one till: fast and simple, but if that till jams, the whole shop stops.

**The problem it solves:** Simplicity and cost. There is no replication lag, no split-brain, no coordination protocol. For an early-stage product doing 500 requests/sec on a single 32-core box with a local SSD, this is genuinely the right call — you ship in a day and there's nothing to debug.

**How it works (mechanics):** Clients connect straight to one node. Writes are applied and fsync'd to that node's disk; a read returns whatever is on that same disk. There is exactly one source of truth, so the data is always internally consistent.
```
        writes + reads
Clients ────────────────▶ [ Single Master ]
                                 │
                                 ▼
                            local disk (only copy)
```
**Trade-offs / when NOT to use it:** It is a single point of failure (SPOF). If the disk dies, your **RPO** (recovery point objective — how much data you lose) is *everything since the last backup*, and your **RTO** (how long to recover) is however long it takes to rebuild from tape or a snapshot — often hours. A single node with 99.9% availability is down ~8.7 hours/year. Never run money, health, or identity data this way in production.

**Where you'll see it:** Local dev, internal tools, a startup's MVP, a Redis cache where the data is disposable, and small self-hosted SQLite/Postgres apps.

### Master with Tape Backup (single node + periodic backup)
**What it is (plain English):** A single master, but you periodically copy its data to cheap, offline, durable storage — historically literal magnetic tape, today usually object storage like S3 Glacier. It's an insurance policy, not a hot standby.

**The problem it solves:** It bounds your data loss. Pure single-master loses *everything* on disk failure; adding backups means the worst case is "lose whatever changed since the last backup." Tape/Glacier is absurdly cheap (~$1/TB/month for cold storage) and physically separable from the server, so a fire or ransomware event doesn't take both.

**How it works (mechanics):** A scheduled job dumps the database (full weekly + incremental daily, or continuous WAL archiving) and ships it offsite.
```
[ Master ] ──nightly dump──▶ [ Tape / S3 Glacier ]   (offline, durable)
   live disk                     restore takes hours
```
Worked number: if you back up every 24h and the disk dies 23h in, your **RPO = 23 hours of data lost**. Restore (RTO) means fetching the tape, loading it, and replaying — commonly 2–12 hours because cold storage retrieval alone can take minutes to hours.

**Trade-offs / when NOT to use it:** Backups are for *durability*, not *availability* — during a restore you are still down. A backup you have never test-restored is not a backup; teams discover corrupt or incomplete dumps precisely when they need them. Don't rely on this alone for anything needing low RTO.

**Where you'll see it:** Regulatory/compliance archives, "3-2-1 backup rule" setups, and as the *last* line of defense behind replication in every serious shop.

### Master-Slave (primary-replica) Replication
**What it is (plain English):** One master takes all writes; one or more slaves (replicas) continuously copy the master's changes and serve reads. It's a primary till plus assistant tills that can only *ring up totals already recorded* — they can read, but only the primary records new sales.

**The problem it solves:** Two things at once: (1) read scaling — fan reads out across replicas; (2) fast failover/durability — if the master dies, promote a replica instead of restoring from tape. This slashes RTO from hours to seconds/minutes.

**How it works (mechanics):** The master streams its write-ahead log (WAL / binlog) to replicas, which apply it.
```
        writes            reads          reads
Clients ───▶ [ MASTER ] ──WAL──▶ [ Slave1 ]   [ Slave2 ]
                 │        ──WAL──────────────▶
             (source of truth)   (read-only copies)
```
- **Async replication:** master acks the client immediately, ships the log after. Fast, but if the master dies before the log reaches a replica, that write is lost → **RPO > 0** (typically the replication lag, e.g. 200 ms–2 s).
- **Sync replication:** master waits for a replica to confirm before acking → **RPO = 0**, but every write pays the round-trip (e.g. +1–5 ms).

Worked number: 1 master + 3 replicas, reads are 90% of a 10,000 QPS load → 9,000 read QPS spread over 3 replicas = 3,000 each, master handles just 1,000 writes.

**Trade-offs / when NOT to use it:** Replicas are behind (replication lag) → a user who writes then immediately reads from a replica may see *stale* data ("read-your-writes" violation). Failover isn't free: promoting a replica needs coordination, and a botched failover creates two masters. Writes still bottleneck on one node.

**Where you'll see it:** Default topology for MySQL, PostgreSQL, MongoDB replica sets; the backbone of nearly every OLTP system in Phase 2.

### Master-Master (multi-primary) Replication
**What it is (plain English):** Two or more nodes each accept writes *and* replicate to each other. Two tills that both ring up sales and try to keep each other's books in sync. Sounds twice as good; it's twice as dangerous.

**The problem it solves:** Write availability and geo-locality. If one master is in Mumbai and another in Virginia, local users write to the nearby master (low latency), and either can keep serving writes if the other is unreachable — no promotion step needed.

**How it works (mechanics):** Each master applies local writes and asynchronously ships them to peers, which apply them too.
```
Mumbai users ──▶ [ Master A ] ◀──replicate──▶ [ Master B ] ◀── Virginia users
                     both accept writes; each converges toward the other
```
The hard part is **conflict resolution**. If A sets `balance=100` and B sets `balance=120` at the same instant on the same row, which wins? Strategies: last-write-wins by timestamp (can silently drop the other write), version vectors/CRDTs (converge without loss for certain data types), or app-level merge. Worked example: two masters, network partition for 3 s, each takes 50 writes to row X → 100 conflicting versions to reconcile.

**Trade-offs / when NOT to use it:** Split-brain and conflicts are inherent, not bugs. For strongly-consistent data (bank balances, inventory counts) master-master is usually the *wrong* answer — you can double-spend. Use it only where writes are naturally partitioned by key/region or the data type is conflict-free (counters, sets via CRDTs).

**Where you'll see it:** Multi-region Cassandra/DynamoDB global tables, Galera cluster for MySQL, and CRDT-based collaborative apps (Figma-style). Rarely for single-row financial truth.

### Virtual Machines vs Docker Containers vs Kubernetes
**What it is (plain English):** Three points on the "how do I package and run my app" spectrum. A **VM** is a whole simulated computer (its own OS kernel). A **container** shares the host kernel and isolates just your app + its libraries — lighter. **Kubernetes (K8s)** isn't a packaging format; it's the *orchestrator* that runs thousands of containers across many machines and keeps them alive.

**The problem it solves:** VMs solved "run many isolated apps on one physical server safely." Containers solved "VMs are heavy and slow to boot, and 'works on my machine' portability." Kubernetes solved "I now have 5,000 containers across 200 machines — who schedules, restarts, and load-balances them?"

**How it works (mechanics):**
```
VM:         [ App | Guest OS kernel ] on [ Hypervisor ] on [ Host OS ] on hardware
Container:  [ App | libs ]  on [ Container runtime ]  sharing ONE [ Host OS kernel ]
K8s:        many containers packed into Pods, scheduled across a fleet of nodes
```
Real numbers: a VM image is often 1–10 GB and boots in ~30–60 s with ~500 MB–1 GB kernel/OS overhead each. A container image is ~50–500 MB and starts in ~50–500 ms with near-zero kernel overhead — so one host can pack far more containers than VMs. Kubernetes adds a control plane that reschedules a crashed container in seconds.

**Trade-offs / when NOT to use it:** VMs give the strongest isolation (separate kernels — better for hostile multi-tenant or different OSes) at the cost of density and speed. Containers share the kernel → weaker isolation boundary (a kernel exploit crosses tenants). Kubernetes is powerful but heavy: don't run K8s for three services — the operational tax (etcd, upgrades, YAML) will outweigh the benefit.

**Where you'll see it:** AWS EC2 (VMs), Docker everywhere in CI/CD, Kubernetes at Google (Borg's successor), Spotify, Airbnb; we go deep on K8s in Lesson 16.

## Comparison Table

| Dimension | Single Master | Master + Backup | Master-Slave | Master-Master |
|---|---|---|---|---|
| Write availability on node loss | None (down) | None (restore) | Promote replica (secs–mins) | Peer keeps serving |
| RPO (data loss) | Everything | Since last backup | ~lag (async) / 0 (sync) | ~lag + conflicts |
| RTO (time to recover) | Hours | Hours | Seconds–minutes | ~0 |
| Read scaling | No | No | Yes (replicas) | Yes |
| Consistency risk | Low | Low | Stale reads | Write conflicts |

**Verdict:** Climb only as high as your durability/availability needs demand — most OLTP systems live happily at master-slave; reserve master-master for partitioned or conflict-free writes.

## Common Misconceptions
- **Myth:** Master-master doubles both availability and safety. → **Reality:** It raises write availability but introduces conflicts/split-brain; for single-row truth it's often less safe.
- **Myth:** A replica is a backup. → **Reality:** Replication faithfully copies your mistakes too — a `DROP TABLE` replicates in milliseconds. You still need real backups.
- **Myth:** Containers are lightweight VMs. → **Reality:** They share the host kernel; they don't boot a guest OS. Different isolation model entirely.
- **Myth:** Sync replication means zero performance cost. → **Reality:** Every write pays a network round-trip to the replica.
- **Myth:** Kubernetes replaces Docker. → **Reality:** K8s *orchestrates* containers (built with Docker/OCI images); they're different layers.

## Real-World Case
In January 2017, GitLab.com suffered a now-famous outage. During a replication problem, an engineer, trying to fix a lagging secondary, ran a directory-removal command against the *primary* database instead of the replica, deleting ~300 GB of live data. Then the real horror unfolded: of their five backup/replication mechanisms, none worked as expected — the Postgres dumps were silently failing due to a version mismatch, and the disk snapshots weren't enabled for that host. They ultimately restored from a six-hour-old staging copy, losing ~6 hours of user data. The lesson the whole industry took: replication is not backup, and an untested backup is a hope, not a plan. GitLab published the entire incident live — a masterclass in why RPO/RTO must be *proven*, not assumed.

## Self-Test (answers at the bottom)
1. What is the single point of failure in a single-master deployment, and what is its RPO if the only copy of data is lost?
2. In master-slave, what causes a "read-your-writes" violation, and roughly how large is the window?
3. A master-master pair is partitioned for 5 seconds and each side takes 200 writes to the same key. What problem do you now have and name one resolution strategy?
4. Give the approximate startup time and image size difference between a VM and a container, and explain *why* the container is faster.
5. Design sketch: A fintech ledger must never lose a committed transaction and must survive one datacenter failure. Which topology and replication mode do you choose, what's your target RPO, and why do you avoid master-master for the balance table?

## Interview Soundbites
- "Replication buys you availability; backups buy you durability — they solve different problems, and a replica happily copies your `DROP TABLE`."
- "Master-master isn't 'more available master-slave' — it trades a promotion step for a conflict-resolution problem, so I only use it for region-partitioned or CRDT-friendly writes."
- "A container shares the host kernel and starts in milliseconds; a VM boots its own kernel in tens of seconds — that density and speed gap is the whole reason Docker won CI/CD."

## Mini-Assignment
On paper (~30 min): Take a hypothetical e-commerce app doing 20,000 QPS (85% reads). (1) Design a master-slave layout: how many replicas do you need if one replica caps at 4,000 read QPS, and what QPS does the master carry? (2) Now the business wants a second region for latency. Sketch whether you'd use read-replicas-across-regions or master-master, and list two data categories (e.g., product catalog vs. order counter) and which topology each should use. Justify each with an RPO/consistency argument in one line.

## Recap & Tomorrow
- **Single master:** simplest, but a SPOF with catastrophic RPO/RTO.
- **Master + backup:** bounds data loss to the backup interval; durability, not availability.
- **Master-slave:** read scaling + fast failover; watch replication lag and stale reads.
- **Master-master:** write availability and geo-locality at the price of conflicts/split-brain.
- **VM vs Docker vs K8s:** whole-OS isolation vs shared-kernel speed vs fleet-scale orchestration.

Tomorrow, **Lesson 14 — Disaster Recovery & Business Continuity**: we turn RTO/RPO from acronyms into an architect's guarantee, covering backup strategies, multi-region DR, and failover drills.

## Self-Test Answers
1. The single node itself (its disk/host) is the SPOF. With no replica or backup, RPO = all data ever written, since the only copy is gone; RTO is however long a full rebuild takes (often hours or unbounded).
2. A user writes to the master, then a subsequent read is routed to a replica that hasn't yet applied that write (replication lag). The window is the lag itself — commonly ~200 ms to a couple of seconds for async replication.
3. You have up to 400 conflicting versions of that key to reconcile (split-brain divergence). Resolution strategies: last-write-wins by timestamp (lossy), version vectors/CRDTs (lossless for compatible types), or application-level merge logic.
4. A VM image is ~1–10 GB and boots in ~30–60 s; a container image is ~50–500 MB and starts in ~50–500 ms. The container is faster because it shares the host kernel and skips booting a full guest OS — it only starts your process and its libraries.
5. Choose master-slave with **synchronous** replication to a replica in a second datacenter, targeting **RPO = 0** (no committed transaction lost) with failover to the sync replica for low RTO. Avoid master-master for the balance table because concurrent writes to the same balance can conflict and be resolved lossily (double-spend); a ledger needs a single authoritative writer per account.
