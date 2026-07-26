# Week 3 — System Design & the Data Layer (Days 15–21)

The design round tests whether you can take "millions of MAU" and turn it into deliberate trade-offs. The first four days build the vocabulary (replication, transactions, caching, messaging); the last three are full timed walkthroughs of the three classic Atlassian prompts.

## The 45-minute design framework (memorize the skeleton)

1. **Functional requirements** (3 min) — the 3–5 operations that matter. 2. **Non-functional requirements** (2 min) — scale, latency targets, availability vs consistency posture. 3. **Estimates** (3 min) — QPS, storage, fan-out; round aggressively. 4. **API sketch** (3 min). 5. **Data model** (5 min). 6. **High-level architecture** (10 min) — boxes and arrows, justify each. 7. **Deep dives** (12 min) — the 1–2 hardest sub-problems; this is where seniority shows. 8. **Failure modes & trade-offs** (5 min) — say the CAP choice out loud, name a failure and its mitigation, mention observability. Interviewers steer; the skeleton keeps you from rambling.

---

## Day 15 — Replication, Partitioning, CAP; Postgres Lab

### Replication

**Leader–follower:** writes to the leader, replicated to followers; reads scale on followers. **Async replication** ⇒ followers lag ⇒ a failover can *lose acknowledged writes*; **sync** ⇒ durability but one slow follower stalls writes; **semi-sync** (ack from ≥1 follower) is the common compromise. **Failover pitfalls** to name: split brain (two leaders — prevent with fencing tokens / epoch numbers), choosing the most-caught-up follower, clients with stale leader caches.

**Replication lag anomalies and their fixes:** *read-your-own-writes* (route the writer's reads to the leader briefly, or track a session LSN and only read from replicas that have caught up); *monotonic reads* (pin a user to one replica); these phrases said naturally are strong signals.

**Quorums:** N replicas, W write acks, R read acks; `W + R > N` gives read-sees-latest overlap (e.g., N=3, W=2, R=2). Leaderless systems (Dynamo-style) add read repair and hinted handoff — one sentence each is enough.

### Partitioning (sharding)

**Range** partitioning: efficient range scans, but hot ranges (time-ordered keys all hit the newest shard). **Hash**: even spread, no range scans. **Consistent hashing with virtual nodes:** ring of hash space; adding a node moves only ~1/N of keys; vnodes smooth the imbalance — this is the answer to "how do you add a shard without rehashing everything?" **Hot keys:** a celebrity ticket/board — mitigate by salting the key (spread across `key#1..key#10`, aggregate on read) or a dedicated cache in front. **Secondary indexes:** local (per-shard; queries scatter-gather all shards) vs global (index itself is partitioned; consistent-ish but writes touch two places).

### CAP and PACELC, said correctly

Partitions *will* happen, so P is not optional; CAP asks what you sacrifice **during** a partition: refuse some requests (CP) or serve possibly-stale data (AP). PACELC adds: **E**lse (no partition), you still trade **L**atency vs **C**onsistency — cross-region sync replication is the everyday example. Never say "we choose CA."

### Postgres lab

Schema for the lab (and Day 21's design):

```sql
CREATE TABLE projects (id BIGSERIAL PRIMARY KEY, key TEXT UNIQUE NOT NULL, name TEXT NOT NULL);
CREATE TABLE tickets (
  id BIGSERIAL PRIMARY KEY,
  project_id BIGINT NOT NULL REFERENCES projects(id),
  status TEXT NOT NULL,           -- 'todo' | 'in_progress' | 'done'
  assignee_id BIGINT,
  title TEXT NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE comments (
  id BIGSERIAL PRIMARY KEY,
  ticket_id BIGINT NOT NULL REFERENCES tickets(id),
  author_id BIGINT NOT NULL,
  body TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

The board query — `WHERE project_id = ? AND status = ? ORDER BY updated_at DESC LIMIT 50` — wants a **composite index matching filter columns then sort column**:

```sql
CREATE INDEX idx_tickets_board ON tickets (project_id, status, updated_at DESC);
```

**Column order matters:** the index serves equality on the leading columns then provides the ordering; an index on `(status, project_id, …)` also works for this query, but only `project_id`-leading serves "all tickets in project" too. **Reading `EXPLAIN ANALYZE`:** Seq Scan (full table — fine for tiny tables, red flag at scale), Index Scan (walk index, fetch rows), Index Only Scan (everything needed is *in* the index — enable by `INCLUDE (title)` covering columns), Bitmap Heap Scan (many matches: collect then fetch). Watch `rows=` estimated vs actual — a big mismatch means stale statistics (`ANALYZE`). **When indexes don't help:** low selectivity (status with 3 values *alone*), functions on the column (`lower(title)` needs an expression index), leading wildcard `LIKE '%x'`. **Pagination:** `OFFSET 100000` reads and discards 100k rows; **keyset pagination** (`WHERE (updated_at, id) < (?, ?) ORDER BY updated_at DESC, id DESC LIMIT 50`) is O(page) — always give this answer.

### Review

Write your SQL-vs-NoSQL framework. A defensible one: relational by default (transactions, ad-hoc queries, constraints); reach for DynamoDB-style when access patterns are known and fixed, scale is huge, and you're willing to model *for* the queries; document stores when the aggregate is naturally one blob; the real question is always "what are my access patterns and consistency needs," never fashion.

---

## Day 16 — Transactions, Isolation, Consensus; DynamoDB Lab

### Isolation levels and their anomalies

| Anomaly | What happens | Prevented from |
|---|---|---|
| Dirty read | read uncommitted data | READ COMMITTED |
| Non-repeatable read | same row differs within one txn | REPEATABLE READ |
| Phantom | re-run query returns new rows | RR (mostly) / SERIALIZABLE |
| Lost update | two read-modify-writes, one overwritten | RR with locks, or atomic ops / `SELECT … FOR UPDATE` |
| Write skew | two txns read overlapping data, write disjoint rows, invariant breaks (two doctors both go off-call) | SERIALIZABLE only |

**Snapshot isolation (MVCC):** each transaction reads a consistent snapshot; writers don't block readers. Postgres's REPEATABLE READ is SI; its SERIALIZABLE is **SSI** — optimistic, detects dangerous read/write dependency cycles and aborts one txn (so retry loops are mandatory). Practical toolkit: `SELECT … FOR UPDATE` for lock-then-update, optimistic version columns (`UPDATE … WHERE version = ?`, check rowcount) for low-contention paths.

### Distributed transactions → sagas

**2PC** (prepare, then commit) is blocking: a died coordinator leaves participants holding locks in doubt — why nobody loves it at scale. The service-world alternative: **sagas** — a sequence of local transactions, each with a **compensating action** for rollback (reserve inventory → charge card fails → release inventory). *Choreography* (services react to each other's events) vs *orchestration* (a coordinator drives the steps) — orchestration is easier to reason about and debug; say that.

### Raft, at intuition level

Nodes are follower/candidate/leader; time divides into **terms**. Followers that miss heartbeats become candidates and request votes; **majority** vote ⇒ leader. The leader appends client commands to its log, replicates, and **commits once a majority has the entry**; a candidate can't win without a log at least as complete as the majority's, which is why committed entries survive elections. That paragraph, delivered calmly, is all a design round needs — it's what backs etcd/Consul, and conceptually ZooKeeper (ZAB).

### DynamoDB lab: single-table design

Dynamo thinking inverts SQL thinking: **list the access patterns first, then shape keys to serve each with one query.** Partition key (PK) decides placement; sort key (SK) orders within the partition; queries hit exactly one PK (plus SK conditions).

Access patterns: get project → get tickets by project (newest first) → get one ticket + its comments → get tickets by assignee.

| Entity | PK | SK | Notes |
|---|---|---|---|
| Project meta | `PROJ#123` | `META` | |
| Ticket | `PROJ#123` | `TICKET#2026-07-15T…#T987` | SK sorts by time; query `begins_with(SK, 'TICKET#')` descending |
| Comment | `TICKET#T987` | `COMMENT#2026-07-16T…#C55` | one query returns a ticket's comments in order |
| Assignee index | GSI: PK=`USER#42`, SK=`TICKET#…` | | GSIs are **eventually consistent** — say it |

**Hot partitions:** per-partition throughput is capped, so a mega-project's `PROJ#123` can throttle. Mitigation: **write sharding** — suffix the PK (`PROJ#123#0..#9`), write to a random shard, fan-in on read (10 parallel queries, merge). Also know: item limit 400 KB (big blobs → S3 pointer), `TransactWriteItems` exists for small multi-item atomicity, on-demand vs provisioned capacity, and **when to refuse Dynamo**: ad-hoc analytics, many-way joins, unknown future queries.

### Review

Flashcards: define write skew with the on-call example · SI vs serializable in Postgres · why 2PC blocks · saga compensation · why a Raft leader needs a majority to commit.

---

## Day 17 — Caching; the Distributed Rate Limiter Answer

### Cache-aside, done honestly

Read: try cache → miss → read DB → populate cache with TTL. Write: **update DB, then invalidate (delete) the cache key** — deleting beats setting because two concurrent writers setting can leave the *older* value written last. The interview-grade subtlety — the stale-set race: reader misses → reads old DB value → *writer updates DB and deletes key* → reader populates cache with the stale value it read. It's rare (needs a read spanning a write) and bounded by TTL; harder mitigations: short TTLs, **delayed double delete**, or CDC-driven invalidation (the binlog/outbox stream deletes keys — authoritative ordering). Knowing this race exists is a senior tell.

**Write-through** (write cache+DB synchronously: consistent, adds latency) and **write-behind** (write cache, flush async: fast, risks loss) complete the taxonomy.

### Stampede protection

Hot key expires → thousands of requests miss simultaneously → DB gets crushed ("thundering herd"). Layered fixes: **per-key mutex / request coalescing** (one loader per key, everyone else waits on its future — `computeIfAbsent` over a `CompletableFuture` cache does this in-process); **jittered TTLs** (no synchronized expiry); **probabilistic early refresh** (each hit near expiry refreshes with small probability); **stale-while-revalidate** (serve the stale value, refresh in background). Name at least two under pressure.

### Redis structures worth knowing cold

Strings (+`INCR` counters, `SETNX` locks/dedup), hashes (object fields), sorted sets (leaderboards; also sliding-window rate limiting via `ZADD` timestamp members + `ZREMRANGEBYSCORE` + `ZCARD`), sets, lists, streams (consumer groups — Redis's mini-Kafka), and TTL on any key. Single-threaded command execution ⇒ each command is atomic; multi-step atomicity ⇒ **Lua**.

### The distributed rate limiter (closing Day 8's open question)

State moves to Redis; atomicity comes from a Lua script (Redis runs scripts atomically):

```lua
-- KEYS[1]=bucket key  ARGV: 1=capacity 2=refill_per_ms 3=now_ms 4=requested
local data   = redis.call('HMGET', KEYS[1], 'tokens', 'ts')
local tokens = tonumber(data[1]) or tonumber(ARGV[1])
local ts     = tonumber(data[2]) or tonumber(ARGV[3])
tokens = math.min(tonumber(ARGV[1]), tokens + (tonumber(ARGV[3]) - ts) * tonumber(ARGV[2]))
local allowed = 0
if tokens >= tonumber(ARGV[4]) then tokens = tokens - tonumber(ARGV[4]); allowed = 1 end
redis.call('HMSET', KEYS[1], 'tokens', tokens, 'ts', ARGV[3])
redis.call('PEXPIRE', KEYS[1], 60000)
return allowed
```

Same lazy-refill math as Day 8 — one algorithm, two deployment shapes. Trade-offs to voice: a Redis round-trip per check (mitigate: local token batches), Redis as a dependency (fail-open or fail-closed is a *product* decision — say both options), clock is Redis-side (pass `now` in, as above, or use Redis `TIME`).

### Build: caching the board

Cache key `board:{boardId}:v{version}` holding rendered board JSON; a ticket-update event `INCR`s `board:{boardId}:ver`, orphaning old entries to TTL death — **versioned keys make invalidation a pointer bump** and eliminate delete races entirely. Sketch the read path (get ver → get blob → miss ⇒ rebuild+set) in ~30 lines of pseudocode in your notes.

### Review — Kafka vs SQS table (fill from memory tomorrow morning)

Ordering (per-partition vs FIFO-group) · replay (yes: offsets/retention vs no: consumed = gone) · fan-out (consumer groups vs SNS→many queues) · throughput ceiling · ops burden (self/MSK vs zero) · DLQ (build it vs built-in redrive) · typical pick (event backbone, streams vs task queues, decoupling).

---

## Day 18 — Kafka, SQS, and the Outbox Pattern

### Kafka mental model

A topic is **partitions** of append-only logs; a message's key hashes to a partition; **ordering holds only within a partition** — so "orders for the same ticket stay ordered" means "key by ticketId." **Consumer groups:** each partition is owned by exactly one member; parallelism ≤ partition count; membership changes trigger **rebalancing** (brief pauses — name it as an operational reality). **Offsets** are consumer-owned progress markers; commit *after* processing ⇒ at-least-once (duplicates on crash between process and commit); commit *before* ⇒ at-most-once. **Retention** by time/size makes replay possible; **compaction** keeps the latest value per key (changelog topics). **Durability dials:** `acks=0/1/all` with `min.insync.replicas` — `acks=all` + ISR≥2 survives a broker loss. **"Exactly once":** idempotent producers kill retry-duplicates; Kafka transactions cover consume-process-produce *within Kafka*; end-to-end with external systems still requires **idempotent consumers** — always land there.

### SQS mental model

At-least-once, roughly unordered (standard) with a **visibility timeout**: a received message hides until you delete it; crash ⇒ it reappears (the redelivery mechanism). FIFO queues give per-`MessageGroupId` ordering + dedup at lower throughput. **DLQ redrive** after `maxReceiveCount` is built in — the operational win. No replay after deletion. Rule of thumb: SQS for task queues and decoupling with minimal ops; Kafka when you need replay, ordering at scale, multiple independent consumers of the same stream, or stream processing.

### The transactional outbox (the pattern that glues week 3 together)

Problem: "update DB **and** publish an event" cannot be atomic across two systems — crash between them and you get silent divergence. Solution: in the **same DB transaction** as the business write, insert the event into an `outbox` table; a relay publishes outbox rows to Kafka and marks them sent (or CDC — Debezium — tails the WAL and publishes). Delivery becomes **at-least-once**, so consumers dedup: `processed_events(event_id PRIMARY KEY)` inserted in the consumer's own transaction — insert conflict ⇒ already processed ⇒ skip. Outbox + idempotent consumer is the honest exactly-once story; say the phrase.

### Build: ticket-update pipeline design doc

Write it end to end: ticket write txn (tickets row + outbox row) → relay/CDC → `ticket-events` topic keyed by ticketId (`acks=all`) → consumers: board-view updater (Day 17's version bump), notification trigger (Day 19), search indexer (Day 21) — each with its own group, own pace, own DLQ. Failure walkthrough: relay down (events delay, nothing lost — the outbox is durable), consumer poison message (retries → DLQ → alert), duplicate delivery (dedup table absorbs it). This one diagram is reusable in *all three* interview designs.

---

## Day 19 — Design Walkthrough: Global Notification Service (Opsgenie-like)

Run it timed (45 min) before reading. Framework applied:

**Functional:** ingest alerts/events; route by user preference (channels: push/email/SMS; quiet hours); dedup; per-user and per-provider rate limits; delivery tracking; escalation policies (notify on-call → no ack in 5 min → escalate). **Non-functional:** 10 M MAU; high availability (an alerting system that's down is a product contradiction — availability over consistency, AP-leaning); p99 ingest→dispatch < 5 s; at-least-once delivery, dedup for effectively-once.

**Estimates:** 50 M notifications/day ≈ 580/s average; design for 10× peak ≈ 6 K/s. Notification row ~1 KB ⇒ 50 GB/day raw; 90-day retention ⇒ ~4.5 TB — partitioned store, archive to blob storage.

**API:** `POST /v1/events {source, dedupKey, severity, payload, targets}` → 202 + eventId; `GET /v1/notifications/{id}` status; CRUD for preferences and escalation policies.

**Architecture:** stateless ingest API (validate, authenticate) → **Kafka `events`** (key = dedupKey → per-alert-stream ordering) → **Router** consumers: dedup (`SETNX dedup:{key} EX 300` in Redis — atomic claim, TTL-bounded), resolve targets, apply preferences/quiet hours, expand escalation step 0 → per-channel topics → **Channel workers** (push/email/SMS) each with: per-provider **token-bucket** rate limiting (Day 8/17, load-bearing at last), provider client with timeouts + **circuit breaker** (Day 23), retries with backoff+jitter, then per-channel **DLQ**; delivery receipts → `delivery_attempts` table → status API. **Escalation timers:** "no ack in 5 min" = a delayed job — your Day 12 scheduler, distributed: a `scheduled_escalations` table polled by workers claiming rows with `FOR UPDATE SKIP LOCKED` (name that clause), cancelled by ack events.

**Data model:** `notifications(id, event_id, user_id, channel, status, created_at)` partitioned by time; `delivery_attempts(notification_id, attempt, provider_status, at)`; `preferences(user_id, channel, quiet_hours…)`; `escalation_policies(team_id, steps JSONB)`.

**Deep dives to offer:** (1) dedup correctness — Redis `SETNX` is a race-free claim; crash after claim but before send ⇒ suppressed alert, so pair with a reconciliation sweep of unsent claims; (2) provider outage — breaker opens, traffic fails over to secondary provider (email: two ESPs), queue absorbs the gap; (3) multi-region — active-active ingest, Kafka per region + cross-region async mirror for the user's home region to dispatch; dedup key claims in the home region keep effectively-once.

**Failure modes said aloud:** duplicate events (dedup layer) · thundering retry herd (jitter + retry budget) · slow provider (bulkheaded per-channel pools — one channel can't starve others) · hot user/team (per-user rate limit + digesting: collapse N alerts into one summary). **Consistency sentence:** "At-least-once end to end with idempotent dedup at the edges; I'd rather double-page than never page."

Self-grade with the 8-axis design rubric (Week 4 appendix). List every follow-up you couldn't answer.

---

## Day 20 — Design Walkthrough: Collaborative Editor (Confluence-like)

**The core problem:** N users edit one document; naive last-write-wins destroys keystrokes. Two families:

**Operational Transformation (OT):** operations (`insert(pos, "x")`, `delete(pos, n)`) are **transformed** against concurrent ops so intent survives ("your insert at 5 happened before my insert at 3, so mine shifts yours to 6"). Needs a **central server as the serializer** — which is exactly what a SaaS editor has. Google-Docs-lineage; transformation functions are notoriously fiddly to prove correct, but the data stays compact.

**CRDTs (sequence CRDTs like RGA/Yjs):** every character/chunk gets a **globally unique, totally ordered ID** (e.g., fractional position + siteId); insert = "place ID between these two IDs," delete = **tombstone**. Concurrent ops **commute** — any order converges with no transformation and no central serializer ⇒ true offline/P2P support. Costs: metadata overhead and tombstone growth (mitigated by GC/compaction in mature libraries).

**Decision (state it, don't hedge):** with a central server anyway, **server-serialized OT is simpler to operate**; choose CRDTs when offline-first or peer sync is a requirement. Either way the *architecture* below barely changes — say that too.

**Architecture:** client ↔ **WebSocket gateway** (sticky routing: hash docId → gateway/doc-shard) → **Doc service**: each hot document is owned by **exactly one worker** ("single-writer per doc" — an actor): it holds the doc in memory, applies/serializes ops, assigns each a monotonically increasing **revision number**, broadcasts to subscribers. Persistence: **append ops to a log** (Kafka topic per shard, or a `doc_ops` table) + **snapshot every N ops** to blob storage; cold open = latest snapshot + replay tail. **Presence** (cursors, avatars) is ephemeral — fan out via pub-sub, never persist; separate message type so presence spam can't crowd out edits.

**Deep dives:** (1) reconnect/catch-up — client sends last-seen revision; server replays `rev+1..head` (this is why the log is the source of truth); (2) doc-owner failover — ownership lease in a coordination store (or Kafka partition assignment does it for free); new owner rebuilds from snapshot+log; in-flight unacked ops are client-retried with **client-generated op IDs** ⇒ idempotent apply; (3) very large docs — chunk into blocks, lazy-load, per-block revisioning.

**Estimates:** 100 K concurrently open docs, avg 3 editors, 1 op/s/editor peak ⇒ ~300 K ops/s across the fleet ⇒ shard by docId over ~100 doc-service nodes; each op ~100 B ⇒ trivial bandwidth, the constraint is fan-out connections. **CAP sentence:** "Within a doc: strong ordering via single-writer (CP-flavored); across docs: nothing to coordinate; presence: happily eventual."

Afternoon: write the comparison note (OT vs CRDT — one table, one recommendation) and the sync-protocol message sketch (`{type: op|ack|presence|catchup, docId, rev, opId, payload}`).

---

## Day 21 — Design Walkthrough: Jira-Style Board + Week Review

**Requirements:** boards of columns of ticket cards; drag-drop reorder; filter; real-time multi-viewer updates; permissions; search. Read-heavy (viewers ≫ editors). NFRs: board load p99 < 300 ms; updates visible to co-viewers < 2 s; permission checks always enforced; search may lag (say it explicitly: eventual consistency *disclosed* is a feature, hidden is a bug).

**Read path:** `GET /boards/{id}` → permission check → Redis versioned cache (Day 17: `board:{id}:v{n}` blob) → miss ⇒ build from Postgres via the Day 15 composite index → set. **Write path:** ticket mutation txn (+outbox) → `ticket-events` → (a) board-view updater bumps version + optionally pre-warms, (b) **WebSocket fanout service** pushes patch events to subscribed viewers (subscribe on board open; patches carry ticket diffs; client falls back to refetch on gap detection via event sequence numbers), (c) search indexer updates Elasticsearch. One diagram — three consumers — reuses Day 18 wholesale.

**Drag-drop ordering — the signature deep dive:** integer positions ⇒ inserting between 5 and 6 renumbers everything after (write storm + conflict swamp). **Lexicographic rank keys** (Jira's real "LexoRank"): position is a string; between "aaa" and "aab" insert "aaam" — O(1) writes, occasional rebalancing when keys grow long (background job). Concurrent drops of two cards between the same neighbors: same-rank tie ⇒ tiebreak by ticket id, next rebalance spreads them. This one is worth rehearsing verbatim — it's Atlassian's own product problem.

**Permissions:** project→role→user grants; enforce at the service on every read (never trust the cache to filter); cache *decisions* per (user, project) with short TTL + event-driven bust on grant changes. The cache-poisoning trap to name: never key shared cached views by data one user can see but another can't — the board blob must be permission-uniform, or per-visibility-cohort.

**Search:** CDC → ES index (`title, body, labels, project`), filtered by permission at query time via project-id terms filter. Lag disclosed in UI ("recently updated results may take a minute").

**Afternoon — the week-3 exit review.** For each of the three designs, verify the doc states: one explicit consistency trade-off · one failure mode + mitigation · estimates that survive arithmetic re-check · the deep dive you'd steer toward. Anything missing gets fixed today; these three docs are your week-4 mock substrate.

**Retro:** rank your comfort — notification / editor / board — lowest becomes Mock 2's prompt family.

**[→ Week 4 — Observability, Mocks & the Values Round](week4-observability-mocks-values.md)**
