# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 42 of 63 — Phase 2: System Design Track (Design 14 of 35)
**Topic:** Distributed Transaction System (2PC / Saga Pattern)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
The moment you break a monolith into microservices, you lose the one thing a single database gave you for free: a transaction that atomically spans everything. Now "place an order" touches the order service, payment service, inventory service, and shipping service — four databases, four networks, four ways to half-fail. This is the problem behind every e-commerce checkout, every bank transfer, every travel booking (flight + hotel + car). The hard truth, formalized by the CAP theorem and the FLP result, is that you *cannot* have a fast, always-available, strongly-atomic transaction across independent services during a network partition — so the industry splits into two camps: **2PC** (strong atomicity, but a blocking coordinator that sacrifices availability) and **Saga** (high availability via local transactions + compensations, but only eventual consistency and no isolation). Knowing when to reach for which — and why "just use 2PC everywhere" gets you fired — is the whole session.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**CAP / ACID / PACELC (taught in Phase 1, Lesson 2):** The theoretical spine. ACID's Atomicity + Isolation are exactly what's hard to preserve across services. 2PC chooses C over A (blocks during partitions); Saga chooses A over C (eventual consistency, no isolation). PACELC reminds us that even without partitions, 2PC trades latency for consistency. Everything today is an application of this lesson.

**HA / replication / quorum (taught in Phase 1, Lesson 3):** The 2PC coordinator is a SPOF; making it HA needs replication + consensus (this is why "2PC over Paxos" = 3PC-like or Spanner's approach exists). Quorum thinking underlies durable commit logs.

**Kafka / event-driven (taught in Phase 1, Lesson 21):** Sagas are usually implemented as event choreography or orchestration over a durable log/queue. The **transactional outbox** pattern (write DB row + outbox event in one local transaction, relay to Kafka) is the canonical way to avoid the dual-write problem. At-least-once delivery + idempotent handlers.

**Microservices / DDD (taught in Phase 1, Lesson 9):** Why we're here at all — service boundaries = database boundaries. DDD aggregates define what's a *local* (single-service, ACID) transaction vs what must span services (needs saga/2PC).

**Leader election / MVC (taught in Phase 1, Lesson 17):** The saga orchestrator / transaction coordinator is a single logical authority; making it fault-tolerant needs leader election and a durable state machine so an in-flight transaction survives coordinator crash.

**SQL/NoSQL & isolation (taught in Phase 1, Lesson 23):** Local ACID transactions, isolation levels, and locking (`SELECT FOR UPDATE`) are the building blocks each service uses internally; the distributed layer coordinates these local transactions.

**Idempotency / DR / retries (taught in Phase 1, Lessons 14, 11):** Every distributed-transaction step must be idempotent and retry-safe because the network will duplicate and re-deliver. RTO/RPO frames what a stuck transaction costs.

> **Recap box:**
> - 2PC = strong atomicity, synchronous, **blocking** coordinator → sacrifices availability (CP).
> - Saga = local txns + compensations → highly available, **eventual** consistency, **no isolation** (AP).
> - Transactional outbox solves the dual-write (DB + event) atomicity problem.
> - Every step must be idempotent + retryable; compensations must be commutative/idempotent too.
> - Service boundary = DB boundary; that's why the single-DB transaction is gone.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. In one paragraph, what problem do 2PC and Saga both solve, and how do they fundamentally differ?
> **What a strong answer covers:** Both preserve *atomicity* of a business operation spanning multiple services/databases ("all steps succeed or the effect is undone"). **2PC** does it synchronously with locks held across a prepare-then-commit protocol coordinated by a central coordinator — strong atomicity and isolation, but blocking and low-availability. **Saga** does it as a sequence of *local* transactions, each committed immediately; if a later step fails, previously-committed steps are undone by explicit **compensating transactions** — highly available and non-blocking, but only eventually consistent and with no isolation (other transactions can see intermediate states). 2PC = pessimistic + strong; Saga = optimistic + eventual.
> **Common weak answer:** "2PC is old and slow, Saga is the modern way" — cargo-culting; misses that 2PC gives isolation/atomicity Saga cannot, and is still correct for tightly-coupled, low-throughput, must-be-atomic cases (some financial cores, XA to a couple of databases).
> **Mentor follow-up if they answer well:** Saga gives up isolation. Give me a concrete anomaly that causes. (Dirty reads / lost updates: order marked "pending," another process sees the reserved inventory and acts on it before a compensation rolls it back.)

Q2. A checkout spans 4 services. Estimate the availability if each is 99.9% and you require all 4 in a synchronous 2PC. What does that tell you?
> **What a strong answer covers:** Serial dependency multiplies: 0.999^4 ≈ **0.996**, i.e., ~99.6% → from ~8.7h/yr downtime per service to ~**35h/yr** for the composite — 4x worse. And 2PC *blocks* holding locks during the window, so a slow/failed participant stalls the others. The lesson: synchronous cross-service atomicity degrades availability multiplicatively and is why high-throughput systems prefer async sagas (each local txn only needs its own service up at its own time). Add coordinator latency: 2PC is 2 round trips (prepare + commit) × network, so p99 latency also stacks.
> **Mentor follow-up:** How does a Saga change that availability math? (Steps are asynchronous and independently retryable — a temporarily-down inventory service doesn't fail the whole order; the saga parks and retries, trading latency for availability. Composite availability is no longer a strict product.)

Q3. What is the "dual-write problem" and why does it break naive event-driven sagas?
> **What a strong answer covers:** A service needs to both (a) commit a local DB change and (b) publish an event ("OrderCreated") so the next saga step proceeds. These are two systems (DB + Kafka) with no shared transaction — if it writes the DB then crashes before publishing, the saga stalls forever; if it publishes then fails to commit the DB, downstream acts on a phantom. Naive `db.commit(); kafka.send();` is not atomic. Fix: the **transactional outbox** — within the single local DB transaction, write the domain row *and* an `outbox` event row; a separate relay (CDC/poller) reliably ships outbox rows to Kafka at-least-once. Now DB and event are atomic because they're one local commit.
> **Red flag answer:** "Kafka's transactions make it atomic with my DB" — Kafka transactions are within Kafka; they don't span your external database. Believing this ships lost/phantom events.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design a distributed transaction system for order checkout (order, payment, inventory, shipping). Show both an orchestrated Saga and where you'd instead use 2PC. Draw it.
> **Key components expected:** A **Saga Orchestrator** (durable state machine per transaction), participant services each with a **local DB + transactional outbox**, a **message broker** (Kafka) for commands/events, an **idempotency/dedup store**, and per-participant **compensating handlers**. Contrast with a 2PC **Transaction Coordinator** + **prepare/commit log**.
> **Architecture diagram (text):**
```
ORCHESTRATED SAGA (the default at scale):
  Client ──POST /checkout──▶ Order Svc ──creates Saga──▶ ┌─ Saga Orchestrator ─┐
                                                          │ durable state machine│
                                                          │ (per-txn, HA, log)   │
                                                          └──────────┬───────────┘
       command/reply over Kafka (at-least-once, idempotent)         │
   ┌───────────────┬───────────────────┬──────────────────┬─────────┘
   ▼               ▼                   ▼                  ▼
 Payment Svc    Inventory Svc       Shipping Svc     (each: local ACID txn + OUTBOX)
 (charge /      (reserve /          (createLabel /
  refund*)       release*)           cancel*)         * = compensation
   │               │                   │
   └── outbox → CDC/relay → Kafka events ──▶ orchestrator advances / compensates

Forward path:  Reserve → Charge → CreateShipment → Confirm
On failure at step k: run compensations for k-1..1 in reverse (Release, Refund, ...)

WHERE 2PC INSTEAD (narrow, strong-atomicity cases):
  Coordinator ──PREPARE──▶ [P1, P2]   (each writes to WAL, holds locks, votes YES/NO)
             ◀──votes────
  Coordinator ──COMMIT/ABORT──▶ [P1, P2]   (all-yes → commit; any-no/timeout → abort)
  (used when: few participants, must be isolated+atomic, throughput low, e.g. XA across 2 DBs)
```
> **What separates SDE2 from SDE3 here:** SDE2 picks one pattern and applies it everywhere. SDE3 **decomposes by requirement**: uses local ACID within a service (free, always), Saga across services for the availability-critical high-throughput path (checkout), and reserves 2PC only for the rare "must be atomic + isolated + low throughput + few participants" island. They also immediately raise the **transactional outbox** for the dual-write problem and design **compensations as first-class**, not an afterthought — including that some actions (email sent, card captured) aren't cleanly reversible and need *semantic* compensation (refund, not "un-charge").

Q5. Trace a successful checkout and then a checkout that fails at the payment step, through the orchestrated saga.
> **Expected trace (success):**
> 1. Order Svc creates order `state=PENDING`, starts saga (orchestrator persists saga state `STARTED`).
> 2. Orchestrator → `ReserveInventory` command. Inventory reserves in a local txn, writes outbox `InventoryReserved`. Orchestrator records step 1 done.
> 3. → `ChargePayment`. Payment charges (idempotency key = sagaId), emits `PaymentCharged`.
> 4. → `CreateShipment`. Shipping creates label, emits `ShipmentCreated`.
> 5. Orchestrator marks saga `COMPLETED`, Order Svc sets order `CONFIRMED`.
> **Expected trace (failure at payment):**
> 1–2 as above (inventory reserved).
> 3. `ChargePayment` fails (card declined / payment svc down after retries). Payment emits `PaymentFailed`.
> 4. Orchestrator transitions to **compensating**: runs compensations for completed steps in reverse — `ReleaseInventory` (undo step 2). Order Svc sets order `CANCELLED`/`PAYMENT_FAILED`.
> **Tricky part:** Candidates forget that the compensation itself can fail (inventory svc down during release) — the orchestrator must **retry compensations to completion** (they're the "must eventually succeed" path) and, if truly stuck, alert for manual intervention / park in a "needs-attention" state. Also: distinguishing a *retryable* failure (timeout → retry the forward step) from a *terminal* failure (card declined → compensate) — retrying a declined card forever is a bug.

Q6. Design the orchestrator's API/state model and how participants communicate.
> **Expected API design:**
> - Orchestrator is a **durable state machine**: `Saga{ sagaId, type, currentStep, status: STARTED|RUNNING|COMPENSATING|COMPLETED|FAILED, steps:[{name, status, compensation, attempts}], payload }`.
> - Participant contract (per step): a **forward command** (`reserveInventory(sagaId, items)`) and a **compensation** (`releaseInventory(sagaId, items)`), both **idempotent** and keyed by `sagaId`+step. Commands sent over Kafka (durable, replayable); replies are events the orchestrator consumes.
> - Control API: `POST /sagas` (start), `GET /sagas/{id}` (status/audit), `POST /sagas/{id}/retry` (nudge a stuck step).
> **What to push on:** **Idempotency keys** are non-negotiable (`sagaId+stepName`) — the broker is at-least-once. **Timeouts per step** (a step that never replies must eventually be declared failed → compensate). **Ordering** — compensations run in strict reverse order. **Versioning** the saga definition (long-running sagas may span deploys; a saga started under v1 must finish under v1 semantics). **Semantic locks** (a status field like `PENDING` acting as an app-level lock) to mitigate the no-isolation problem.

---

### Data Modeling (Medium–Hard)
Q7. Model the orchestrator's persistence, the outbox, and the idempotency store.
> **Expected schema:**
```sql
-- Orchestrator: durable saga state (survives coordinator crash → recover in-flight sagas)
CREATE TABLE saga_instances (
  saga_id      uuid PRIMARY KEY,
  saga_type    text,
  status       text,            -- STARTED|RUNNING|COMPENSATING|COMPLETED|FAILED
  current_step int,
  payload      jsonb,
  created_at   timestamptz, updated_at timestamptz,
  version      bigint           -- optimistic concurrency on state transitions
);
CREATE TABLE saga_steps (
  saga_id uuid, step_no int, name text,
  status  text,                 -- PENDING|DONE|COMPENSATED|FAILED
  compensation text, attempts int, last_error text,
  PRIMARY KEY (saga_id, step_no)
);

-- Transactional OUTBOX (in EACH participant's DB, written in the SAME local txn as the domain change)
CREATE TABLE outbox (
  id uuid PRIMARY KEY, aggregate_id uuid, event_type text,
  payload jsonb, created_at timestamptz, published boolean DEFAULT false
);

-- Idempotency / inbox dedup (in EACH participant)
CREATE TABLE processed_messages (
  message_id text PRIMARY KEY,   -- = sagaId+step (dedup at-least-once redelivery)
  result     jsonb, processed_at timestamptz
);
```
> **Index choices and why:** `outbox(published, created_at)` for the relay to scan unpublished events in order. `processed_messages(message_id)` PK for O(1) dedup lookup. Saga state indexed by `status` + `updated_at` to find stuck/timed-out sagas for the recovery sweeper.
> **Partitioning key and why:** Partition saga state by `saga_id` (each saga is independent; no cross-saga transactions). Outbox and idempotency tables live *co-located with each participant's own DB* — that co-location is the whole point (they must share the local transaction). Never centralize the outbox away from the service's DB.

Q8. How does the outbox relay actually get events to Kafka reliably and in order, without a dual-write?
> **Expected answer:** Two implementations: (1) **Polling publisher** — a relay process `SELECT * FROM outbox WHERE published=false ORDER BY created_at`, publishes each to Kafka, marks `published=true`. Simple, but adds poll latency and a write for the flag. (2) **CDC (log tailing)** — Debezium tails the DB's WAL/binlog and streams new outbox rows to Kafka directly; lower latency, no flag write, and ordering preserved by the log. Either way delivery is **at-least-once** (relay can crash after publishing before marking) → consumers must dedup by `message_id` (the idempotency store, Q7). Ordering per aggregate is preserved by keying the Kafka topic by `aggregate_id`.
> **Trap:** Doing `db.commit()` then `kafka.send()` in app code (the dual-write) and calling it "good enough" — it loses events on crash between the two. Or publishing from the outbox but forgetting consumer-side dedup, so at-least-once redelivery double-applies.

Q9. Saga gives up isolation. Give the classic anomaly and how you mitigate it without a global lock.
> **Expected answer:** Saga's committed intermediate states are visible to others → three anomalies (from the Garcia-Molina saga paper's modern treatment): **lost update** (two sagas update the same record, one overwrites the other's uncommitted-conceptually change), **dirty read** (a saga reads data another saga will later compensate away), **fuzzy read** (reads differ across saga steps). Mitigations *without* distributed locks: (1) **Semantic lock** — a status flag (`order=PENDING`, `funds=HELD`) that signals "in-flight, don't act finally"; other transactions respect it. (2) **Commutative updates** — design operations so order doesn't matter (increment/decrement balance rather than set), so concurrent sagas compose correctly. (3) **Reread & reconciliation** — the compensation re-checks current state. (4) **Version/optimistic concurrency** on the record so a stale write is rejected.
> **Mentor pushback:** Scenario — inventory reserved by saga A (state HELD); saga B for a different order sees only "available count" already decremented, sells the last unit; A then compensates (releases) — now you oversold *from B's perspective* / undersold. Fix: the semantic-lock/held state must be counted correctly (reserved ≠ available, and available already excludes reserved), and compensation returns to available atomically — i.e., model *reserved* explicitly rather than just decrementing a single counter.

---

### Low-Level Design (Hard)
Q10. Hardest sub-problem: make the saga orchestrator survive its own crash mid-transaction with no lost or double-applied steps.
> **Problem statement:** The orchestrator is executing step 3 of a 5-step saga when its process crashes. On recovery (or failover to a standby), it must resume *exactly where it left off* — not re-run completed steps in a way that double-applies, nor skip the in-flight one.
> **Naive solution:** Keep saga state in memory / drive the saga from a single request thread. Crash = the saga is lost, order stuck PENDING forever, inventory reserved and never released.
> **Why naive fails at scale:** In-memory state doesn't survive a crash; a request-scoped saga can't outlive a deploy or a 30-min payment retry. You get **orphaned sagas** — leaked reservations, stuck money.
> **Expected optimal approach:** **Persistent state machine + event sourcing / write-ahead.** (1) Every state transition is durably written *before* the side-effecting command is sent (write-ahead: record "about to run step 3" → send command → on reply record "step 3 done"). (2) On recovery, a **sweeper** scans `saga_instances` for non-terminal sagas and re-drives them from `current_step`. (3) Because each participant command is **idempotent (keyed by sagaId+step)**, re-sending an in-flight command that actually already executed is a safe no-op — this is what makes "resume" correct even when you can't tell whether step 3 completed before the crash. (4) Leader election (Lesson 17) ensures only one orchestrator instance drives a given saga at a time; a standby takes over on failure. Temporal/Cadence generalize this as durable, replayable workflow execution.
> **Pseudo-code or class diagram:**
```
def drive(saga):                       # idempotent, re-entrant, safe to call on recovery
    while saga.status == RUNNING:
        step = saga.steps[saga.current_step]
        if step.status == PENDING:
            persist(saga, mark step ATTEMPTED)     # WRITE-AHEAD before side effect
            reply = send_command_idempotent(step.name, key=saga.id+step.no, timeout)
            if reply.ok:
                persist(mark step DONE); saga.current_step += 1
            elif reply.retryable and step.attempts < max:
                step.attempts += 1                 # retry same step
            else:                                  # terminal failure
                saga.status = COMPENSATING; break
    if saga.status == COMPENSATING:
        for s in reversed(completed_steps):
            run_compensation_until_success(s, key=saga.id+s.no)   # must eventually succeed
        saga.status = FAILED

# recovery sweeper (runs on the elected leader)
for saga in db.query("status NOT IN (COMPLETED, FAILED) AND updated_at < now-timeout"):
    drive(saga)      # resume; idempotency makes re-driving safe
```

Q11. Concurrency / race: two identical `POST /checkout` requests (user double-clicked, or client retried a timed-out request) create two sagas that both charge the card. Fix.
> **Scenario:** Client's first request timed out (but succeeded server-side); client retries → two charges.
> **Expected fix:** **Idempotency at the entry point.** The client sends an `Idempotency-Key` (or the order uses a natural key like cartId+userId). The order/checkout service does an atomic `INSERT ... ON CONFLICT DO NOTHING` on `(idempotency_key)` — the second request finds the existing saga and returns its result instead of starting a new one. Inside the saga, every step is *also* idempotent by `sagaId`, so even if two sagas somehow started, the payment `charge(idempotencyKey=sagaId)` at the payment provider (Stripe et al. all support idempotency keys) collapses duplicate charges. Two layers: dedupe the *saga creation*, and dedupe each *step*.
> **Follow-up (what if the two requests race the INSERT simultaneously?):** A unique constraint on `idempotency_key` makes the DB the arbiter — exactly one INSERT wins, the other gets a conflict and reads the winner's saga. No app-level lock needed; the DB's unique index is the concurrency primitive.

Q12. Failure deep-dive: in 2PC, the coordinator sends PREPARE, all participants vote YES and lock, then the coordinator crashes before sending COMMIT. What happens?
> **Scenario:** The infamous 2PC blocking window.
> **Expected handling:** Participants who voted YES are **stuck holding locks indefinitely** — they cannot unilaterally commit (maybe another participant voted NO) nor abort (maybe everyone voted YES and the decision was COMMIT). This is 2PC's fatal flaw: a coordinator crash in the uncertainty window **blocks** participants, holding locks and freezing those rows — sacrificing availability (CP). Mitigations: (1) **Write-ahead the decision** — coordinator persists COMMIT/ABORT to a durable log *before* notifying anyone, so on recovery it re-sends the decision (this makes it recoverable, not non-blocking). (2) **Participant timeout + peer communication** (3PC / cooperative termination) reduces but doesn't eliminate blocking, at the cost of an extra round trip and vulnerability to network partitions. (3) **HA coordinator via consensus** (Paxos/Raft — Spanner's approach) so the coordinator itself doesn't have a single point of failure. This is *exactly* why high-availability systems avoid 2PC and use sagas: a saga participant never blocks holding a distributed lock — it commits locally and relies on compensation. DLQ/alert for sagas; for 2PC, a stuck transaction is an operational incident.

---

### Scaling to 10x / 100x (Hard)
Q13. At 10x throughput (say 100k checkouts/sec), where does each approach break first?
> **Expected answer:** **2PC breaks almost immediately** — lock hold time × throughput = contention; holding row locks across two network round trips at 100k/sec means participants spend most of their time locked, latency balloons, and any slow participant stalls everyone (head-of-line blocking on locks). It simply doesn't scale for high-throughput hot rows. **Saga** scales far better (each local txn is fast, no distributed locks), but its bottlenecks become: (1) the **orchestrator throughput** — it's a stateful coordinator persisting every transition (mitigate by partitioning sagas across many orchestrator instances by sagaId); (2) the **outbox relay / Kafka** throughput and consumer lag; (3) **compensation storms** during a partial outage (a downstream failing causes mass compensations, doubling the write load). The orchestrator's state DB write rate (2× steps writes per saga) is the usual first wall.
> **Numbers to ground the answer:** 100k sagas/sec × 4 steps × 2 state writes ≈ 800k writes/sec on the orchestrator store → must shard by sagaId across many partitions. 2PC on a hot inventory row: if each 2PC holds a lock for even 20ms (2 RTT), a single hot row caps at ~50 txns/sec — 2000x short of 100k. That number alone kills 2PC for hot paths.

Q14. Sharding: how do you partition the orchestrator and its state so it scales horizontally? Hot spot?
> **Expected sharding strategy:** Partition saga instances by **hash(sagaId)** across N orchestrator nodes (consistent hashing, Lesson 19); each saga is fully independent (no cross-saga transactions), so this shards cleanly with no coordination. Each orchestrator node owns its shard's sagas, drives them, and runs the recovery sweeper for its own partition. Leader/ownership per partition via the coordinator so exactly one node drives a given saga.
> **Hot spot problem:** The hot spot isn't the orchestrator (sagaIds are uniform) — it's a **shared downstream resource**: e.g., a flash sale where 100k sagas all `ReserveInventory` on the *same SKU* row. That single row is the contention point regardless of how well you shard sagas. **Detect** via per-key lock-wait/latency on the inventory service. **Fix:** shard the *inventory counter* itself (split one SKU's stock into K sub-counters across shards, reserve from a random sub-counter, sum for total — trades exact-count reads for write spread), or move hot-SKU reservations to an in-memory atomic decrement (Redis `DECR`) with async DB persistence. The distributed-transaction layer can't fix a hot business row; you must shard the resource.

Q15. Caching strategy — what's cacheable in a distributed transaction system, and the invalidation pitfall?
> **Expected layered cache design:**
> - **Idempotency/dedup results** cached (Redis, keyed by idempotency key / message_id) so redelivered messages return the cached result in O(1) without re-hitting the DB — this cache *is* on the hot path.
> - **Saga definitions / step topology** cached in-memory (they change rarely).
> - **Read-only reference data** (product catalog, pricing) each service caches locally.
> - **Deliberately NOT cached:** the live saga *state* and any balance/inventory count that a transaction mutates — these must be read strongly-consistent from the source of truth, because a stale cached balance leads to double-spend / oversell.
> **Cache invalidation trap:** Caching **mutable transactional state** (an account balance, inventory count) and letting a saga read the stale value → it makes a decision (approve, reserve) on data that a concurrent saga already changed → lost update / oversell. Rule: transactional decisions read the authoritative row (with the local ACID txn / version check); caches are for *idempotency results* and *immutable reference data* only. The subtle one: an idempotency-result cache must be written **atomically with the effect** (or the effect committed first, cache is only an optimization checked against the durable `processed_messages` table) — a cache-only dedup that loses its entry re-runs a non-idempotent effect.

Q16. Cost/efficiency at scale.
> **Expected answer:** (1) **Prefer local ACID over distributed anything** — the cheapest distributed transaction is the one you avoid by aligning DDD aggregate boundaries so most operations are single-service (design your service boundaries to minimize cross-service transactions). (2) **Choreography over orchestration for simple flows** — no central orchestrator to run/scale (events flow service-to-service), cheaper for 2–3 step sagas; reserve orchestration for complex flows needing central visibility. (3) **Batch/coalesce** outbox relay publishes and state writes. (4) **Async everything** — don't hold threads/connections waiting on a slow participant; park the saga and resume on event (event-driven = no idle compute). (5) **Bound retries + DLQ** so failing sagas don't burn compute retrying forever. (6) Reserve 2PC (expensive: locks, blocking, 2 RTT) for the tiny set of flows that truly need atomic isolation; using it broadly is the biggest cost mistake.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Compare orchestration vs choreography for sagas with concrete failure/observability trade-offs. When does choreography become an anti-pattern? (Direction: choreography = each service listens for events and reacts, no central brain — decoupled, no orchestrator SPOF, cheap for simple flows; but the workflow logic is *smeared* across services (hard to see "what's the state of this order?"), cyclic event dependencies emerge, and debugging a stuck flow means tracing events across N services. Orchestration centralizes the state machine — clear status, easier compensation ordering, but the orchestrator is a component to build/scale/own. Choreography becomes an anti-pattern past ~4 steps or when you need central visibility/compensation control — that's when you switch to orchestration. Temporal is orchestration done as durable code.)

**H2.** Cross-cutting: exactly-once semantics end-to-end is claimed a lot. Prove why it's impossible and what "effectively once" really requires. (Direction: FLP + the two-generals nature — a sender can never know if the receiver processed a message before an ack was lost, so it must retry (→ at-least-once) or risk loss (→ at-most-once); true exactly-once delivery is impossible. "Effectively once" = at-least-once delivery + **idempotent** processing (dedup by message_id) + the effect and the dedup-record committed atomically. Kafka EOS gives this *within* Kafka via idempotent producers + transactional writes to Kafka; it does NOT extend to your external DB/side effects — that's the transactional-outbox + inbox-dedup job.)

**H3.** Multi-region: a saga spans services in different regions. How do you keep latency sane and handle a region partition mid-saga? (Direction: keep each saga's participants region-local where possible (data residency + latency); for genuinely cross-region steps, the async saga tolerates high inter-region latency far better than 2PC (which would hold locks across the WAN). On a region partition mid-saga, the durable saga state + idempotent resume means the saga *pauses* and resumes when connectivity returns — no data loss, just delay. Compensations remain runnable within-region. Avoid 2PC across regions at all costs — WAN RTT × lock hold = disaster.)

**H4.** Observability: how do you monitor and debug a fleet of long-running sagas? What pages you? (Direction: every saga has a correlation/trace ID propagated across all steps (distributed tracing, OTel) so you can see the full tree. Metrics: sagas by state, **stuck-saga count** (non-terminal + age > threshold — the key alert), compensation rate (spikes = downstream outage), step latency/retry/DLQ rates, outbox relay lag. A "saga audit log" (the persisted step history) is the debugging goldmine. Page on: rising stuck-saga backlog, compensation-storm, DLQ growth, orchestrator state-DB write saturation.)

**H5.** 'Undo a bad decision': you built checkout with XA/2PC across order+payment+inventory DBs and it's blocking under load / causing lock storms. Migrate to sagas without downtime. (Direction: introduce the saga orchestrator and per-service outbox alongside; convert one participant at a time to the local-txn-plus-compensation model (start with the least-coupled step), running the new async path in shadow and comparing outcomes; add idempotency keys and compensations; then cut the coordinator out step by step, finally removing XA. Because sagas relax isolation, you must add semantic locks (PENDING states) to cover anomalies the old 2PC isolation hid — this is the subtle migration risk. The durable outbox + idempotent handlers make the dual-running safe.)

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. **Ignoring the dual-write problem** — they draw `commit DB; publish event` and don't realize it's non-atomic and loses events on crash. The transactional outbox (or CDC) is the expected answer and its absence is a tell.
2. **Forgetting Saga has no isolation** — they treat compensation as "rollback" and miss that intermediate states are *visible* to others, causing dirty reads / lost updates / oversell. Senior answers add semantic locks, commutative updates, and reservation modeling.
3. **Reaching for 2PC by default** (or claiming "exactly-once delivery") — not understanding that 2PC blocks and destroys availability under partition/load, and that exactly-once is delivery-impossible so you need idempotent effectively-once. Using 2PC on a hot path is the classic scaling failure.

**The one insight that makes an answer truly impressive:**
Framing the entire problem as **choosing where each ACID guarantee gets relaxed and paying for it explicitly**: you keep Atomicity+Isolation *inside* each service (local ACID, free), relax cross-service Isolation for availability (saga + semantic locks to patch the anomalies you can't tolerate), preserve cross-service Atomicity via **idempotent local transactions + compensations + durable orchestrator state + transactional outbox**, and accept eventual Consistency — reserving 2PC only for the rare island where blocking is affordable and isolation is mandatory. Candidates who explicitly say "compensations must be idempotent and must *eventually* succeed, and some actions need *semantic* compensation (refund, not un-charge) because they're irreversible" are clearly staff-level.

**Suggested follow-up reading:**
- Garcia-Molina & Salem, "Sagas" (1987) — the original paper; and Chris Richardson's microservices.io patterns (Saga, Transactional Outbox, Transaction Log Tailing).
- Google Spanner paper (2PC over Paxos + TrueTime) and Pat Helland's "Life Beyond Distributed Transactions" — the canonical case for why you avoid distributed transactions.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate — especially Lessons 2 (CAP/ACID), 21 (Kafka/outbox), 9 (microservices/DDD), 17 (leader election), 23 (isolation/locking).
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to distributed transactions (2PC/Saga). Expected answers include real algorithms/patterns (2PC prepare-commit, saga compensation, transactional outbox, CDC, idempotency keys, semantic locks), specific failure modes (coordinator-crash blocking window, dual-write loss, saga isolation anomalies, oversell), and real numbers. Cross-referenced Phase 1 lessons throughout.
