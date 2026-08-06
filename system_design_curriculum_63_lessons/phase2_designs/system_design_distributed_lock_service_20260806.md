# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 41 of 63 — Phase 2: System Design Track (Design 13 of 35)
**Topic:** Distributed Lock Service (ZooKeeper / Chubby)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Google's Chubby and Apache ZooKeeper are the quiet backbone of the datacenter: they don't store your data, they store the *truth about who's in charge* — leader election, config, membership, and coarse-grained locks — for systems like GFS, Bigtable, Kafka, and HBase. The genius is packaging a full consensus protocol (Paxos/Zab/Raft) behind a dead-simple filesystem API so ordinary services get linearizable coordination without implementing Paxos themselves. The hard part is that a *wrong* answer here doesn't slow you down — it causes two masters, split-brain, and silent data corruption, so correctness dominates every trade-off.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Leader election (taught in Phase 1, Lesson 17):** The core service this system *provides* and also *uses internally*. Internally the ensemble elects one leader to serialize all writes; externally, clients use ephemeral+sequential nodes to elect leaders among themselves. Understanding "exactly one leader, provably" is the whole game.

**CAP / ACID / PACELC (taught in Phase 1, Lesson 2):** A lock service is emphatically **CP** — during a partition it sacrifices availability (the minority side stops serving writes) to never violate consistency. This is the opposite choice from Dynamo and must be justified: a lock that's "eventually consistent" is not a lock.

**HA / replication / quorum (taught in Phase 1, Lesson 3):** The ensemble is 3 or 5 nodes; a write commits only after a **majority quorum** (2 of 3, 3 of 5) persists it. Majority quorum is what makes split-brain impossible — two disjoint majorities can't exist. RTO/nines math from this lesson sets ensemble sizing.

**Consensus / replicated state machine (foundational, reinforced across Lessons 2–3, 17):** Paxos (Chubby), Zab (ZooKeeper), and Raft (etcd) all implement a replicated log: agree on an ordering of operations, apply them deterministically, and every replica ends in the same state. We'll compare Zab vs Raft explicitly.

**Caching / TTL / sessions (taught in Phase 1, Lesson 24):** Clients cache reads and hold **sessions** with heartbeats; ephemeral nodes and locks are tied to session liveness via a timeout (the lock's "lease"). Session/lease timeout tuning is central to correctness under GC pauses.

**Observability & health checks (taught in Phase 1, Lesson 10):** Heartbeats, session expiry, and watch notifications are the health-check machinery; we instrument leader elections, request latency, and outstanding sessions.

> **Recap box:**
> - Lock service = CP, always: sacrifice availability to never allow two lock holders.
> - Majority quorum (2/3, 3/5) mathematically prevents split-brain.
> - Zab/Paxos/Raft = replicated log → identical state on every replica.
> - Ephemeral node + session heartbeat = a lease; expiry auto-releases the lock.
> - Fencing tokens, not just locks, protect the resource from a paused holder.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Why can't you just use a row in MySQL (`SELECT ... FOR UPDATE`) or a `SETNX` in a single Redis as your distributed lock? What does ZooKeeper give you that they don't?
> **What a strong answer covers:** A single MySQL/Redis is a single point of failure and typically not linearizable across failover — a Redis primary can ack a `SETNX`, fail over to a replica that never received it, and grant the same lock twice (the Redlock controversy). ZooKeeper replicates the lock state via consensus across a majority, so an acknowledged lock survives any minority failure and is linearizable. It also provides **automatic release on client death** (ephemeral nodes tied to sessions) and **watch notifications** so waiters don't poll — neither of which a naive DB row gives you.
> **Common weak answer:** "Redis SETNX with a TTL is a distributed lock." Ignores failover safety and, worse, the process-pause problem where the TTL expires but the holder is still acting.
> **Mentor follow-up if they answer well:** Even with a perfect lock service, the lock alone doesn't make the *protected resource* safe. Why not? (Fencing tokens.)

Q2. Size the ensemble. Why 3 or 5 nodes and not 4 or 6? What availability does a 5-node ensemble give?
> **What a strong answer covers:** Quorum = floor(N/2)+1. N=3 tolerates 1 failure (quorum 2); N=5 tolerates 2 (quorum 3). Even sizes are wasteful: N=4 also only tolerates 1 failure (quorum 3) but costs more and has *more* ways to lose quorum than N=3 — so odd sizes give strictly better failure-tolerance-per-node. Availability: a 5-node ensemble stays writeable through any 2 simultaneous node failures. Writes get *slower* as N grows (more nodes must ack), so you don't go beyond 5–7 for a single ensemble; you scale reads with observers/learners instead.
> **Mentor follow-up:** If reads scale by adding non-voting observers, why not add 20 voters for durability? (Write latency and quorum-formation cost grow with voter count.)

Q3. Explain ephemeral vs persistent znodes and why ephemeral+sequential is the standard lock recipe.
> **What a strong answer covers:** A **persistent** znode outlives the client that created it (config, membership roots). An **ephemeral** znode is deleted automatically when the creating client's session ends (crash, network loss, timeout) — this is what makes locks self-healing: a dead holder's lock evaporates. **Sequential** appends a monotonic counter to the name (`lock-0000000042`). The lock recipe: create an ephemeral-sequential child under `/lock/`; you hold the lock iff you have the lowest sequence number; otherwise watch *only* the node just below you. This avoids the herd and guarantees FIFO fairness.
> **Red flag answer:** "Everyone watches the lock node and races to grab it when released." That's the **herd effect** — a release wakes all N waiters who all storm the leader; the correct recipe wakes exactly one.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the high-level architecture of a ZooKeeper-like lock/coordination service. Draw it.
> **Key components expected:** An **ensemble** of 3/5 servers (one leader, rest followers), a **Zab/Raft replicated log** (proposal + majority ack + commit), an in-memory **hierarchical namespace (ZNode tree)** with an on-disk **WAL + periodic snapshots**, **sessions** with heartbeats, and **watches** for notifications. Optional **observers** (non-voting) for read scale-out.
> **Architecture diagram (text):**
```
      Clients (each holds a session, heartbeats every tickTime)
        │   writes ─────────────┐         reads (any server, may be stale)
        │                       ▼
        │                 ┌───────────┐
        │   ┌────read─────│  Follower │◀─┐
        ▼   │             └─────┬─────┘  │  fsync WAL + apply
   ┌────────▼──┐  Zab proposal   │        │
   │  LEADER   │──broadcast──────┼────────┤
   │ (serial-  │◀──ACK majority──┤        │
   │  izes all │                 │        │
   │  writes)  │──COMMIT────────▶│  ┌─────▼─────┐
   └─────┬─────┘                 └─▶│  Follower │
         │                          └───────────┘
         │  in-memory ZNode tree (all servers identical after commit):
         │     /            (persistent)
         │     /service/leader   (ephemeral, election)
         │     /locks/resourceX/lock-000000041 (ephemeral+sequential)
         │                      /lock-000000042
         │  Durability: WAL (fsync before ack) + fuzzy snapshots
         ▼
   Observers (non-voting) ── serve reads only, don't slow the quorum
```
> **What separates SDE2 from SDE3 here:** SDE2 shows leader+followers+quorum. SDE3 nails the *asymmetry*: **all writes funnel through the single leader and are totally ordered** (that's what gives linearizable writes), while **reads can be served by any follower and may be stale** — so ZooKeeper is linearizable for writes but only *sequentially consistent* for reads unless you issue a `sync()` first. Knowing that reads are stale-by-default and how to force freshness is the differentiator.

Q5. Trace acquiring a lock end to end, including the leader replicating the write.
> **Expected trace:**
> 1. Client ensures a live **session** (connected, heartbeating within the negotiated timeout).
> 2. Client sends `create("/locks/X/lock-", EPHEMERAL_SEQUENTIAL)` — routed to the leader (a follower forwards writes to the leader).
> 3. Leader assigns the next **zxid** (monotonic transaction id), writes a Zab **PROPOSAL** to its WAL, and broadcasts to followers.
> 4. Each follower fsyncs the proposal to its WAL and returns **ACK**.
> 5. On **majority** ACK, leader sends **COMMIT**; all servers apply it to the in-memory tree. Client gets its znode name, e.g., `lock-0000000042`.
> 6. Client lists children of `/locks/X`; if it has the **lowest** sequence → it holds the lock. Else it sets a **watch on the next-lower** child and blocks.
> 7. When the holder's session ends or it deletes its node, ZooKeeper fires the watch on exactly the next waiter, who re-checks and acquires.
> **Tricky part:** The write is only durable/linearizable after majority fsync+commit — a lock "granted" locally by the leader before commit is not safe. And a read of the children list can be stale on a follower; if the client needs the authoritative view it must `sync()` then read.

Q6. Design the API and the canonical lock recipe on top of it.
> **Expected API design:** Primitive ops: `create(path, data, flags{PERSISTENT|EPHEMERAL|SEQUENTIAL})`, `delete(path, version)`, `exists(path, watch)`, `getData/setData(path, data, version)`, `getChildren(path, watch)`, `sync(path)`. Version numbers give **optimistic concurrency** (CAS on setData). Sessions: `create session (timeout)`, `heartbeat/ping`.
> Lock recipe (the important part):
```
lock(resource):
  myNode = create("/locks/"+resource+"/lock-", EPHEMERAL_SEQUENTIAL)
  loop:
    children = getChildren("/locks/"+resource)  # sorted
    if myNode is lowest: return HELD
    pred = greatest child with seq < mine
    if exists(pred, watch=True):  wait for watch  # watch ONLY predecessor
    # if pred vanished between getChildren and exists, loop and re-check
```
> **What to push on:** Why watch *only the predecessor* (herd avoidance — a release notifies exactly one waiter, O(1) not O(N)). Idempotency of `create` under retries (a network blip may leave a node you can't see — use a client-embedded GUID in the node name to recognize your own on reconnect). Read staleness and when `sync()` is required.

---

### Data Modeling (Medium–Hard)
Q7. Design the znode namespace and the on-disk durability format.
> **Expected schema:**
```
Namespace (in-memory tree, fully replicated):
  /                                  persistent
  ├── /config/appA                   persistent, versioned data
  ├── /election/service1
  │      ├── n_0000000007            ephemeral+sequential (candidate)
  │      └── n_0000000008
  ├── /locks/resourceX
  │      ├── lock-0000000041         ephemeral+sequential (holder)
  │      └── lock-0000000042         ephemeral+sequential (waiter)
  └── /members/svcB
         ├── host-17  (ephemeral)    membership: dies with the host
         └── host-23  (ephemeral)

ZNode stat: czxid, mzxid, version, cversion, aversion,
            ephemeralOwner(sessionId or 0), dataLength, numChildren

On disk (per server):
  WAL / txnlog: append-only, fsync BEFORE acking a proposal
  snapshots:  periodic "fuzzy" snapshot of the tree; recovery = load
              latest snapshot + replay txnlog tail from its zxid
```
> **Index choices and why:** The tree is fully in-memory (dataset is small, coordination metadata, not user data) so reads are O(path depth) hashmap lookups — microsecond. Children are kept sorted for the lock/leader recipes. The WAL is the only fsync in the hot path; snapshots bound recovery time.
> **Partitioning key and why:** There is **no partitioning** — this is the deliberate design. The entire namespace is replicated on every server; you scale *reads* with observers, not by sharding, because sharding would break the global total order that makes linearizable coordination possible. Data volume must stay small (typically <a few GB, znodes <1MB each) — it's a coordination kernel, not a database.

Q8. Thousands of clients watch config at `/config/appA` for changes. How do you deliver updates without melting the leader?
> **Expected answer:** **Watches** are one-shot server-side triggers: a client registers a watch on `getData("/config/appA", watch=True)`; on the next change the server sends exactly one notification, then the client must re-read and re-register. The notification is tiny (just "this node changed") — the client fetches the new data itself, so the update payload isn't multicast through the write path. Reads (including the re-read) are served by the client's connected **follower/observer**, offloading the leader entirely. For a config that many watch, this is O(watchers) small notifications, not O(watchers) writes.
> **Trap:** Polling `getData` in a loop (turns N clients into a constant read storm), or expecting watches to be persistent/queued — they're **one-shot** and can miss rapid successive changes, so the client must always re-read on notification and treat the watch as an edge trigger, not a change log. Assuming watches guarantee you see every intermediate value is wrong.

Q9. Frame the consistency guarantees precisely. Where is ZooKeeper NOT linearizable, and when does that bite?
> **Expected answer:** ZooKeeper guarantees **linearizable writes** (all writes totally ordered by zxid through the single leader) and **FIFO client order** (a single client's ops apply in send order). But **reads are served locally by followers and can be stale** — they're sequentially consistent, not linearizable, unless preceded by `sync()`. So a client can write, another client reads a follower that hasn't applied that write yet, and sees the old value.
> **Mentor pushback:** Leader election via ephemeral node: node A creates `/election/leader`, becomes leader, then A's network hiccups and its session expires — ZK deletes the ephemeral node and B creates it, becoming leader. But A hasn't *noticed* its session died yet (notification is asynchronous) and keeps acting as leader → **two leaders briefly**. Eventual consistency of the *client's view of its own session* is the gap. Fix: A must stop all privileged action the instant it loses quorum contact / on `SESSION_EXPIRED`, and — critically — the protected resource must require a **fencing token** so B's higher token invalidates A's stale writes even during the overlap window.

---

### Low-Level Design (Hard)
Q10. Design leader election among client processes using this service, herd-free and correct.
> **Problem statement:** N candidate processes must agree on exactly one leader; on the leader's failure a new one is elected promptly; no split-brain; and a release must not wake all N candidates (herd).
> **Naive solution:** Every candidate tries to `create("/election/leader", EPHEMERAL)`; whoever succeeds is leader; everyone else watches that node and re-races when it disappears.
> **Why naive fails at scale:** The re-race is a **herd**: when the leader dies, all N-1 candidates get the watch, all N-1 issue `create` simultaneously, hammering the leader with a thundering herd and O(N) contention on every failover. With 5,000 candidates that's a 5,000-way stampede per election.
> **Expected optimal approach:** The **sequential-ephemeral queue**: each candidate creates `/election/n_<seq>`; the lowest sequence is leader; each *non-leader* watches **only the node immediately below it**. When any node dies, exactly one watcher (its immediate successor) is notified and re-evaluates — O(1) notifications per failover, and it naturally forms a FIFO succession line.
> **Pseudo-code or class diagram:**
```
elect():
  me = create("/election/n_", EPHEMERAL_SEQUENTIAL)   # e.g. n_0000000008
  while True:
    kids = sorted(getChildren("/election"))
    if me == kids[0]:
        become_leader()                # I have the lowest seq
        return
    pred = kids[index_of(me) - 1]       # exactly the node below me
    if exists("/election/"+pred, watch=True):
        wait_for_watch()               # sleep until pred disappears
    # else pred already gone -> loop, re-evaluate (may now be lowest)
  # On losing quorum contact or SESSION_EXPIRED: resign() IMMEDIATELY
```

Q11. A lock holder undergoes a 30-second stop-the-world GC pause. Its session lease expires, ZK releases the lock, and another client acquires it. Then the first client wakes up mid-write. Walk the disaster and the fix.
> **Scenario:** Client A holds the lock on resource R and starts writing to R. A stalls (GC/VM freeze) longer than the session timeout. ZK deletes A's ephemeral node → lock free → Client B acquires it and starts writing R. A resumes, still *believing* it holds the lock, and completes its write → two concurrent writers → corruption. This is the classic "lock expired but process alive" failure (Kleppmann's fencing example).
> **Expected fix:** The lock alone is insufficient; you need **fencing tokens**. Each lock grant carries a monotonically increasing token (ZooKeeper's zxid or the znode's sequence/version number works). The protected resource (DB, storage service) **records the highest token it has seen and rejects any write with a lower token**. B gets token 43, writes it; A's delayed write carries token 42 < 43 → the resource rejects it. The lock service can't prevent a paused process from acting, but the *resource* can fence out stale actors. Also: tune session timeout above worst-case GC, and have A check session validity right before the critical write (necessary but not sufficient — fencing is the real fix).
> **Follow-up:** What if the lock holder simply dies (not paused)? Then ephemeral-node auto-delete on session expiry cleanly releases the lock; the successor's watch fires; fencing token still increments so any in-flight packets from the dead client are rejected. Clean case — the paused-but-alive case is the dangerous one.

Q12. The ensemble leader crashes mid-broadcast: it had proposed zxid 500 to some followers but not others, and hadn't committed. How does the system recover without losing committed data or applying uncommitted data?
> **Scenario:** Leader L broadcasts proposal 500, followers F1 and F2 ACK and persist it to WAL, F3 hasn't received it, and L dies before sending COMMIT. Which state is authoritative?
> **Expected handling:** Zab recovery has two guarantees — **(a) any proposal committed on the old leader must be committed by the new one, and (b) any proposal that was never committed must be discarded/skipped**. Election picks the follower with the **highest last zxid** (most up-to-date log) as new leader. In the **SYNC/recovery phase (Zab's discovery + synchronization)** the new leader forces all followers to converge to its log: followers with *extra* uncommitted proposals **truncate** them (TRUNC), followers *missing* proposals get them (DIFF/SNAP). A new **epoch** is started (higher epoch bits in the zxid) so stale packets from the old leader are ignored. Net: if 500 was replicated to a majority it survives (it's in the new leader's log); if only to a minority it may be truncated — but it was never acked to the client as committed, so no committed data is lost. Raft (etcd) does the equivalent via term numbers, the "leader completeness" property, and the rule that a leader only commits entries from its own term.

---

### Scaling to 10x / 100x (Hard)
Q13. At 100x clients, where does a coordination service break first?
> **Expected answer:** **Write throughput and the WAL fsync**, because every write is serialized through the single leader and must fsync on a majority before ack — you cannot shard your way out without breaking the global order. ZK does ~10K–50K writes/sec on good hardware and hundreds of thousands of reads/sec; if your workload is write-heavy (lots of lock churn, ephemeral create/delete), the leader's fsync and broadcast fan-out are the wall. Secondary limits: **session/heartbeat load** (each client pings every tickTime; 1M clients × ping = leader CPU/network pressure) and **watch fan-out** (a change on a hotly-watched node notifies all watchers at once — a notification herd).
> **Numbers to ground the answer:** tickTime default 2s, session timeout typically 2–20× tickTime (4–40s). A single ensemble comfortably handles low tens of thousands of writes/sec; beyond that you must reduce write rate (coarser locks, batching) or federate into multiple ensembles by domain. Reads scale to hundreds of thousands/sec by adding observers.

Q14. You can't shard the namespace without losing global order. So how do you scale, and how do you handle a hot znode everyone watches/locks?
> **Expected sharding strategy:** You **federate by responsibility**, not by hash: run *multiple independent ensembles*, each owning a disjoint coordination domain (e.g., one per service/tenant/region), so each keeps its own total order. Within an ensemble, scale **reads** with non-voting **observers** (they receive commits but don't vote, so they don't slow the quorum) and keep the voter set at 3–5.
> **Hot spot problem:** A single lock/znode with thousands of contenders is a fundamental serialization point — no amount of hardware fixes contention on one lock. Detect via watch-count and per-node request metrics. Fixes: (1) **Lock striping / sharding the lock** — split one coarse lock into K sub-locks (`/locks/X/shard-<h(k)%K>`) so contention drops K-fold when the protected work is partitionable. (2) **Coarser granularity** (hold longer, acquire less often) to cut create/delete churn. (3) Replace a hot *read* config node's polling with proper one-shot watches. (4) For pure leader election, the predecessor-watch recipe already makes failover O(1) — don't let everyone watch the leader node.

Q15. Design the read/caching strategy and its hardest correctness pitfall.
> **Expected layered cache design:** Clients keep a **local cache** of znodes they've read and rely on **watches** for invalidation (edge-triggered: on change notification, drop cache entry and re-read). The connected server (follower/observer) is itself an in-memory replica, so a "cache miss" read is still RAM-fast. Observers act as a read-scaling tier. There is no CDN/Redis layer — the data is tiny and correctness-critical.
> **Cache invalidation trap:** Because watches are **one-shot and asynchronous**, and follower reads are **stale until applied**, a client can (1) read value v1, (2) the value changes to v2 and back to v1 (or to v2, v3) faster than the notification round-trip, and the client may **miss intermediate states** or briefly serve stale v1 after v2 committed. For anything where staleness is unsafe (e.g., "am I still the leader?"), you must `sync()` before the read to force the follower to catch up to the leader's latest zxid — accepting the extra latency. Trusting a cached/local read for a correctness decision without `sync()` is the classic bug.

Q16. Keep it efficient and safe at scale.
> **Expected answer:** (1) **Coarse-grained locks** — Chubby's explicit guidance: hold locks for meaningful durations (hours), not microseconds, to minimize load and lock churn; use ZK for coordination, not high-frequency mutual exclusion. (2) **Snapshotting + WAL trimming** to bound disk and recovery time. (3) **Batch writes** where possible; use multi-op transactions (`multi()`) to commit several changes as one atomic proposal, amortizing fsync. (4) **Observers** to serve read-heavy workloads cheaply without adding quorum cost. (5) Keep dataset tiny (don't store blobs in znodes; store pointers) — memory and snapshot size stay small, recovery stays fast. (6) Right-size session timeouts to worst-case pauses to avoid spurious expirations and re-elections (each false expiry is expensive churn).

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Zab vs Raft vs Multi-Paxos — compare precisely. Zab's guarantee that a new leader's log is a superset before it serves (primary-order), Raft's term + leader-completeness + "only commit entries from current term" rule, epoch/term fencing of stale leaders, and why ZooKeeper reads are not linearizable while writes are. When would you pick etcd (Raft, gRPC, watches, leases) over ZooKeeper today?

**H2.** Fencing tokens end-to-end: why a correct lock service is *still* insufficient for resource safety (the paused-holder problem), how monotonic tokens (zxid/version) must be enforced *at the resource*, and how this composes with idempotency keys. Explain why Redlock is unsafe under process pauses and clock drift.

**H3.** Zero-downtime ensemble reconfiguration: dynamic reconfiguration (ZK 3.5 `reconfig`, Raft joint-consensus) to add/remove/swap voters without losing quorum, rolling upgrades that never take two voters down at once, and cross-datacenter ensembles (WAN latency inflating write commit time — why you keep the quorum in one region and use observers elsewhere).

**H4.** Observability: outstanding requests, request latency p99, follower sync latency, number of leader elections (should be near zero — frequent elections signal instability), open sessions/connections, watch count, and pending syncs. Which metric is your split-brain / instability alarm (a spike in leader-election rate) versus a client-side bug (runaway watch/session churn)?

**H5.** "Undo a bad decision": your team used ZooKeeper for a fine-grained, high-frequency lock (per-request), and lock churn is overwhelming the leader with ephemeral create/delete. Migrate to coarse-grained leases + fencing tokens (or move that hot path to a local/optimistic scheme, reserving ZK for true leader election), without a coordination outage or a correctness gap during the transition. Tell that story.

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Thinking the lock alone guarantees mutual exclusion at the resource. It doesn't — a paused holder can act after expiry; you need **fencing tokens** enforced at the protected resource.
2. Assuming ZooKeeper reads are linearizable. Writes are; reads are stale-by-default and need `sync()`. This misconception causes real split-brain bugs.
3. Using it as a database or for high-frequency fine-grained locking. It's a small, in-memory, fully-replicated coordination kernel — coarse locks, small data, low write rate.

**The one insight that makes an answer truly impressive:**
Separating the two overlapping failure modes and matching each to its correct mechanism: a *crashed* holder is handled by ephemeral-node auto-release (the easy case), but a *paused-but-alive* holder can only be made safe by fencing tokens at the resource — the lock service cannot solve it because it can't distinguish "dead" from "slow." Naming that fencing tokens live at the *resource*, not the lock, and that reads need `sync()` for freshness, is the SDE3 tell.

**Suggested follow-up reading:**
- "The Chubby lock service for loosely-coupled distributed systems" (Burrows, Google, OSDI 2006) and "ZooKeeper: Wait-free coordination for Internet-scale systems" (Hunt et al., USENIX ATC 2010).
- Martin Kleppmann, "How to do distributed locking" (the fencing-token / Redlock critique), and the Raft paper "In Search of an Understandable Consensus Algorithm" (Ongaro & Ousterhout).

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
