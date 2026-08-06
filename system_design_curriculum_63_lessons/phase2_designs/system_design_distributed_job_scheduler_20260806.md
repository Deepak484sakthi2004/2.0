# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 38 of 63 — Phase 2: System Design Track (Design 10 of 35)
**Topic:** Distributed Job Scheduler (Cron at Scale)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Every company that grows past a single crontab hits this wall: `cron` on one box is a single point of failure, doesn't scale, silently skips jobs when the box is down, and gives you no visibility. So they build a distributed scheduler — Google's Cron service, Airbnb's Chronos, Quartz clustered, Uber's Cadence/Temporal, AWS EventBridge Scheduler — to run millions of scheduled and delayed jobs reliably across a fleet. What makes this deceptively hard is the tension between **"never miss a job"** and **"never run a job twice"** in a world of crashing workers, clock skew, and network partitions: those two guarantees pull in opposite directions, and the honest answer is you pick at-least-once or at-most-once and engineer around the one you didn't pick. Add scale (hundreds of millions of timers due at 00:00), and the naive "poll a table WHERE next_run <= now" query melts.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Leader election / MVC / proxies (taught in Phase 1, Lesson 17):** To avoid two schedulers dispatching the same job, a single leader (or a partition owner) is elected via ZooKeeper/etcd. Leader election and its failure modes (fencing, split-brain) are central — a scheduler with two active leaders double-fires everything.

**Distributed lock service (Phase 2, Lesson 41 — built next; foundations in Lesson 3, 17):** We use locks/leases and fencing tokens so exactly one worker owns a job at a time. Quorum/leases from replication (Lesson 3) underpin this.

**Kafka / event-driven (taught in Phase 1, Lesson 21):** Due jobs are enqueued to a message queue for workers to pull, decoupling scheduling (deciding *when*) from execution (doing the work). Partitions, consumer groups, offset commit, and at-least-once delivery all apply directly. Also the delayed-message / retry-topic pattern.

**SQL/NoSQL, B-tree vs LSM (taught in Phase 1, Lesson 23):** The job store needs an index on `next_run_time`; B-tree range scans (`WHERE next_run <= now ORDER BY next_run`) vs an LSM store's write amplification for high-churn timers is a real design axis. Also the "hot partition of due timers" problem.

**Consistent hashing (taught in Phase 1, Lesson 19):** Partition millions of jobs across N scheduler nodes so each owns a slice of the timer space; rebalancing when nodes join/leave without a full reshuffle.

**Heaps / timing wheels (taught in Phase 1, Lesson 5):** In-memory, the next-due job is a min-heap keyed by fire time (O(log n)); a **hierarchical timing wheel** gives O(1) insert/expire for high-volume timers. The core scheduling data structure.

**CAP / idempotency / DR (taught in Phase 1, Lessons 2, 14):** At-least-once vs exactly-once, idempotency keys, RTO/RPO — if the scheduler is down for 5 minutes, do missed jobs fire on recovery (catch-up) or get skipped? That's an RPO decision.

> **Recap box:**
> - "Never miss" (at-least-once) vs "never double-run" (at-most-once) — pick one, engineer the other with idempotency.
> - Leader election / partition ownership prevents double-dispatch; fencing tokens prevent zombie double-run.
> - Timing wheel = O(1) timer insert/fire; min-heap = O(log n) next-due.
> - Decouple *scheduling* (when) from *execution* (do) via a queue.
> - Index/partition on `next_run_time`; beware the thundering herd of timers due at 00:00.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Why not just run `cron` on a beefy server (or a few of them)? Name the concrete failure modes.
> **What a strong answer covers:** (1) **SPOF** — box down = every job silently skipped, no catch-up, no alert. (2) **No coordination** — run cron on 3 boxes for HA and every job fires 3x (no dedup). (3) **No visibility** — did the job run? succeed? retry? cron gives you a mail spool, not observability. (4) **Doesn't scale** — one machine can't hold millions of timers or execute heavy jobs. (5) **No dynamic scheduling** — you can't add a "run this once in 30 minutes" job at runtime without editing crontab. A distributed scheduler solves all five: HA via leader election, exactly-one dispatch, durable job store, retries, and a runtime API.
> **Common weak answer:** "cron doesn't scale" — true but shallow; misses the double-fire-under-HA problem, which is the actual reason naive HA fails.
> **Mentor follow-up if they answer well:** You run cron on 3 boxes with a distributed lock so only one fires. What still breaks? (Clock skew — each box's cron fires at its own "now"; and the lock holder can GC-pause past the fire time. Leads into fencing.)

Q2. Estimate the load: 100M scheduled jobs, of which ~2M are due in any given minute, but with a spike of 50M all scheduled for 00:00 UTC daily. Size the dispatch throughput.
> **What a strong answer covers:** Steady state: 2M/min ≈ **33k dispatches/sec** — modest. The killer is the **00:00 thundering herd**: 50M jobs due in the same second → you cannot dispatch 50M/sec. Options: (a) **jitter/spread** — add random offset so "midnight" jobs spread over a window (0–300s), flattening 50M into ~166k/sec; (b) **bucketed time wheels** where a bucket holds all jobs for a second and you drain it as fast as workers allow, letting the backlog trail by seconds (acceptable for most jobs). Storage: 100M jobs × ~500B ≈ **50 GB** of job metadata — fits in a sharded DB, index on next_run is the concern. Worker fleet: to drain 50M in 5 min = 166k/sec; at 100 jobs/sec/worker → ~1,600 workers during the burst (autoscaled).
> **Mentor follow-up:** Users scheduled exactly 00:00:00. Is silently jittering them acceptable? (For cache-warm/report jobs yes; for a job that must fire at a legal deadline, no — you need a "strict time" flag and provision for the peak. Surface the trade-off.)

Q3. At-least-once vs exactly-once delivery for job execution — which do you pick and why?
> **What a strong answer covers:** True exactly-once execution across crashes is impossible in an asynchronous system (a worker can complete the job then die before ack'ing — did it run?). So you pick **at-least-once dispatch + idempotent execution** = *effectively once*. The scheduler guarantees a due job is dispatched at least once (never lost); the job handler is written to be idempotent (keyed by a jobRunId) so a duplicate dispatch is a no-op. The alternative — at-most-once — risks silently dropping jobs on failure, unacceptable for billing/notification jobs. State it as a conscious CAP/idempotency choice (Lesson 2).
> **Red flag answer:** "The queue gives exactly-once so I don't worry about duplicates." Even Kafka's "exactly-once" is a within-Kafka transactional guarantee, not end-to-end across your side-effecting worker — believing otherwise causes double-charges.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the distributed job scheduler. Draw it. Separate the "decide when" plane from the "execute" plane.
> **Key components expected:** A **job store** (durable, sharded, indexed on next_run), a **scheduler/dispatcher tier** (partitioned, each partition owned by one node via leader election, holding an in-memory timing wheel/heap of near-due jobs), a **message queue** (Kafka/SQS) between dispatch and execution, a **stateless worker fleet** pulling and running jobs, an **execution/result store** for status/retries, and a **control API** to CRUD jobs. Coordination via ZooKeeper/etcd (partition assignment, leader election, fencing tokens).
> **Architecture diagram (text):**
```
   Client/API ──create/cancel job──▶ Job Store (sharded by jobId, index on next_run_time)
                                          │  (durable source of truth)
                                          ▼
   ┌──────────── SCHEDULER TIER (partitioned; each partition owned by 1 node) ───────────┐
   │  Coordinator (ZooKeeper/etcd): assigns time/hash partitions, leader election,        │
   │  fencing tokens.                                                                      │
   │  Each node: load due-soon jobs (next 1–5 min) into in-memory TIMING WHEEL / min-heap  │
   │  → on fire time: mark 'dispatched', enqueue to queue with jobRunId + fencing token    │
   └───────────────────────────────────────────────────────────────────────────────────┘
                                          │ enqueue due jobs
                                          ▼
                          Message Queue (Kafka/SQS)  ── DLQ for poison jobs
                                          │ pull (at-least-once)
        ┌──────────────┬─────────────────┼──────────────┬──────────────┐
        ▼              ▼                 ▼              ▼              ▼
    Worker         Worker            Worker         Worker         Worker  (stateless, autoscaled)
    (idempotent by jobRunId; ack on success; report status)
        │
        └──▶ Execution Store (run status, attempts, next_retry) ──▶ reschedule recurring/retry
                                          ▲
                             Recurring jobs: on completion, compute next_run from cron expr,
                             write back to Job Store.
```
> **What separates SDE2 from SDE3 here:** SDE2 draws a table + a poller. SDE3 (1) **separates scheduling from execution** with a queue so heavy/slow jobs never block the timer loop; (2) **partitions the timer space** so no single node owns all timers and there's no global lock contention; (3) explicitly places the **at-least-once boundary** (dispatch marks 'dispatched' in the store *before* enqueue, so a crash after enqueue re-dispatches — chosen over losing the job); and (4) makes recurring-job rescheduling happen *after* execution using the cron expression, not by pre-materializing infinite future runs.

Q5. Trace a recurring job ("every day at 09:00, send digest") from creation to its second run.
> **Expected trace:**
> 1. `POST /jobs` with cron `0 9 * * *` → validated, stored with `next_run = today/tomorrow 09:00`, `state=SCHEDULED`, sharded by jobId.
> 2. The owning scheduler partition, minutes before 09:00, loads due-soon jobs and inserts this into its in-memory timing wheel.
> 3. At 09:00 the wheel fires → scheduler atomically CAS's the store row `SCHEDULED→DISPATCHED` (with a generated `jobRunId` + fencing token), then enqueues `{jobId, jobRunId, token}` to the queue. (Store-then-enqueue = at-least-once.)
> 4. A worker pulls it, checks idempotency (jobRunId not already executed), runs the handler, on success writes `run: SUCCESS` to the execution store and acks the queue.
> 5. **Reschedule:** on success, compute the *next* `next_run` from the cron expression (next 09:00) and CAS the store row back to `SCHEDULED` with the new time. Second run repeats from step 2.
> **Tricky part:** Candidates pre-generate all future occurrences (infinite rows) or reschedule *before* execution (so a crashed run never reschedules → job silently stops forever). Correct: reschedule from the cron expr *after* a run is durably recorded, and compute next_run relative to the *scheduled* time (not "now") to avoid drift.

Q6. Design the API for the scheduler.
> **Expected API design:**
> - `POST /v1/jobs` — `{ name, schedule: {type: "cron"|"once"|"rate", expr|runAt|interval}, target: {type:"http"|"queue"|"lambda", ...}, payload, timeout, retryPolicy:{max, backoff}, idempotencyKey }` → returns `jobId`. Idempotent on `idempotencyKey` so a client retry doesn't create duplicate jobs.
> - `GET /v1/jobs/{id}` / `DELETE /v1/jobs/{id}` (cancel) / `PATCH` (pause/resume/reschedule).
> - `GET /v1/jobs/{id}/runs?limit=50` — execution history (status, attempts, timing) for observability.
> - `POST /v1/jobs/{id}/trigger` — fire now, out of schedule (for backfills/testing).
> **What to push on:** **Idempotency** on create (network retries mustn't create N copies). **Time zones & DST** — a "9am" cron in a TZ with DST is genuinely hard (does 2:30am run 0, 1, or 2 times on the transition day?); store TZ, compute occurrences with a real TZ library. **Cancellation race** — cancel arriving after dispatch but before execution needs a tombstone the worker checks. **Versioning** `/v1/`. **Overlap policy** — if a run is still going when the next fires, allow-concurrent / skip / queue?

---

### Data Modeling (Medium–Hard)
Q7. Model the job store and the run/execution store. What's indexed and partitioned how?
> **Expected schema:**
```sql
CREATE TABLE jobs (
  job_id        uuid PRIMARY KEY,
  name          text,
  schedule_type text,              -- cron | once | rate
  schedule_expr text,              -- "0 9 * * *"
  timezone      text,              -- "Asia/Kolkata"
  next_run_time timestamptz,       -- the hot column
  time_bucket   bigint,            -- floor(next_run/granularity) for sharded scanning
  state         text,              -- SCHEDULED | DISPATCHED | PAUSED | CANCELLED
  fencing_token bigint,            -- monotonic, bumped each dispatch
  target        jsonb,
  payload       jsonb,
  retry_policy  jsonb,
  version       bigint             -- optimistic concurrency
);
-- Index for the due-scan (partitioned scan, see Q8):
CREATE INDEX ON jobs (time_bucket, next_run_time) WHERE state='SCHEDULED';

CREATE TABLE job_runs (
  job_id     uuid,
  run_id     uuid,                 -- = idempotency key for execution
  scheduled_for timestamptz,
  started_at timestamptz, finished_at timestamptz,
  attempt    int,
  status     text,                 -- PENDING|RUNNING|SUCCESS|FAILED|DEAD
  next_retry_at timestamptz,
  PRIMARY KEY ((job_id), run_id)
);
```
> **Index choices and why:** A partial index on `(time_bucket, next_run_time) WHERE state='SCHEDULED'` keeps the hot due-scan cheap and avoids scanning executed/paused rows. B-tree range scan (Lesson 23) is ideal for `WHERE next_run_time <= now` — but see the hot-partition trap below.
> **Partitioning key and why:** Shard the store by `hash(job_id)` for even write/CRUD load. But the *scan* for due jobs must be partitioned by **time_bucket** and assigned to scheduler nodes so each node scans only its buckets — otherwise every node scans the whole table. This is the key modeling decision: hash-shard for CRUD, time-bucket for the due-scan.

Q8. The due-scan `SELECT ... WHERE next_run <= now` runs every second across the fleet. Why does the naive version melt, and how do you fix it?
> **Expected answer:** Naive: every scheduler node runs `SELECT * FROM jobs WHERE next_run<=now AND state='SCHEDULED' ORDER BY next_run LIMIT N` every second. Problems: (1) **all nodes scan and race for the same due rows** → contention + double-dispatch; (2) at 00:00 the index range for that instant contains 50M rows → a giant scan + lock storm on one B-tree region (**hot partition**); (3) high polling frequency hammers the DB. Fix: (a) **partition the timer space** — each node owns a set of `time_bucket`s (via consistent hashing + coordinator assignment) so exactly one node scans a given bucket, no races. (b) **Two-level scheduling**: DB scan pulls due-*soon* jobs (next few minutes) into an in-memory **timing wheel**, and the wheel — not the DB — fires at second granularity, so the DB is polled coarsely (every few min) not every second. (c) **Claim atomically** — `UPDATE ... SET state=DISPATCHED, fencing_token=token WHERE job_id=? AND state=SCHEDULED` (CAS) so only one node wins a row even if partitioning has a seam during rebalance.
> **Trap:** Polling the DB every second for second-precision. The timing wheel decouples precision (in-memory, ms) from DB load (coarse). Also `ORDER BY ... LIMIT` without partition ownership → every node fights over the same top-N rows.

Q9. Consistency vs availability: the scheduler is partitioned and one partition's owner is unreachable (partition). Do its jobs still fire? What do you sacrifice?
> **Expected answer:** This is a CAP choice (Lesson 2). If a partition owner is unreachable, you can either (a) **CP-lean**: don't let anyone else fire that partition's jobs until ownership is safely reassigned via the coordinator (risk: jobs are *late* during the failover window — you sacrifice timeliness/availability of firing to guarantee no double-fire), or (b) **AP-lean**: let a standby take over quickly (risk: if the "unreachable" owner is actually alive but partitioned and still firing, you double-fire — sacrifice safety). The safe default: **CP for dispatch ownership** (a lease with fencing; a new owner can only take over after the old lease provably expires), accepting that jobs may fire a few seconds late during failover. Missed windows are handled by catch-up (Q12).
> **Mentor pushback:** "Just use a short lease so failover is fast." A short lease under a GC pause makes the old owner lose the lease while still executing → the fencing token is what saves you (the queue/target rejects the stale token), not the lease length. Lease length trades failover speed vs false-expiry rate; fencing makes false-expiry safe.

---

### Low-Level Design (Hard)
Q10. Hardest sub-problem: fire millions of timers at 00:00 with second precision, without a per-timer DB poll and without a thundering herd. Design the timer engine.
> **Problem statement:** Efficiently store and fire up to 50M timers due within the same second, at ms/second precision, using bounded CPU/memory, while new timers arrive continuously.
> **Naive solution:** A min-heap of all 100M timers, or a DB `WHERE next_run<=now` poll every second.
> **Why naive fails at scale:** A 100M-entry heap costs O(log n) per insert and holds huge memory; and heaps don't help the "50M due in the same tick" burst — you still pop 50M. The DB poll is the hot-partition melt from Q8.
> **Expected optimal approach:** **Hierarchical timing wheel** (the Linux kernel / Kafka `SystemTimer` structure) for near-term timers + DB for the long tail. A timing wheel is an array of buckets each representing a time slot (e.g., 512 buckets × 1s = ~8.5 min of near-term); inserting a timer is **O(1)** (compute bucket = fire_time mod wheel size), and each tick advances one bucket and fires everything in it — **O(1) amortized**, no per-timer comparison. Hierarchical wheels (seconds wheel → minutes wheel → hours wheel) cover long ranges: a far-future timer sits in a coarse wheel and cascades down to finer wheels as it approaches. For the 00:00 burst: the bucket for that second holds 50M entries; you drain it into the queue as fast as workers pull, letting a bounded backlog trail — you don't need to *execute* 50M in one second, just *enqueue* them, and even that you spread with **jitter**. Only load near-due timers (next few min) into the wheel from the DB; the DB holds everything else.
> **Pseudo-code or class diagram:**
```
class HierarchicalTimingWheel:
    wheels = [Wheel(tick=1s,  size=60),   # seconds
              Wheel(tick=60s, size=60),   # minutes
              Wheel(tick=1h,  size=24)]   # hours
    def add(timer):
        delay = timer.fire_at - now
        w = coarsest wheel where delay >= wheel.tick   # place in appropriate level
        bucket = w.buckets[(timer.fire_at / w.tick) % w.size]
        bucket.append(timer)                            # O(1)
    def tick():                                         # called every 1s
        expired = seconds_wheel.advance_one_slot()
        for t in expired: dispatch(t)                   # enqueue with jobRunId+token
        if seconds_wheel.wrapped():                     # cascade coarser wheels down
            for t in minutes_wheel.advance_one_slot():
                if t.fire_at - now < 60s: add(t)  # re-insert into finer wheel
                else: dispatch(t)

def dispatch(t):
    if CAS(store, t.job_id, SCHEDULED->DISPATCHED, new_token):   # atomic claim
        enqueue(queue, {t.job_id, run_id, token, jitter_delay})
```
> Kafka's purgatory / `SystemTimer` and Netty's `HashedWheelTimer` are real implementations of exactly this.

Q11. Concurrency / the zombie double-run: a worker claims a long job, GC-pauses for 40s past its lease, the scheduler declares it dead and re-dispatches, then the paused worker wakes and finishes too. Both run. Fix it.
> **Scenario:** Job has a 30s visibility/lease. Worker A claims it, stalls (GC/network), lease expires, scheduler re-dispatches to worker B. B runs it (e.g., charges a card). A un-pauses and *also* completes the charge. Double side-effect.
> **Expected fix:** **Fencing tokens** (the canonical answer, Lesson 17 idea applied). Each dispatch carries a monotonically increasing fencing token. The side-effecting resource (payment API, DB write, the queue ack) records the highest token it has seen and **rejects any operation with a lower token**. When A wakes with token 41 but B already committed with token 42, A's write is rejected — the resource, not the lock, enforces safety. Combine with: (1) idempotency by `run_id` so even same-token retries are no-ops; (2) sensible visibility timeouts ≥ p99 job duration so re-dispatch is rare; (3) worker heartbeats to *extend* the lease for legitimately long jobs so they aren't falsely reclaimed.
> **Follow-up (what if the lock holder dies for real?):** That's the *good* case — the lease expires, the new owner takes over with a higher token, and since the dead worker never comes back, there's no conflict. Fencing is precisely for the ambiguous "is it dead or just slow?" case where you can't tell.

Q12. Failure deep-dive: the entire scheduler tier is down for 8 minutes (deploy gone wrong). Jobs that should have fired at 09:01–09:08 were missed. What happens on recovery?
> **Scenario:** Scheduled jobs' fire times passed while the dispatcher was down.
> **Expected handling:** This is an **RPO/catch-up policy** decision (Lesson 14), and it should be **per-job configurable**: (1) **Catch-up / fire-missed** — on recovery, the due-scan finds all `state=SCHEDULED AND next_run < now` and dispatches them (they were never marked dispatched, because store-then-enqueue means an un-enqueued job is still SCHEDULED). Good for "must run" jobs (billing). (2) **Skip-to-next** — for jobs where a stale run is worthless (e.g., "warm cache every 5 min"), skip missed occurrences and just schedule the next future one, optionally with a **misfire threshold** ("if more than 5 min late, skip"). Quartz literally has misfire policies for this. (3) **Coalesce** — if 8 occurrences of a recurring job were missed, run it once, not 8 times, to avoid a stampede. Also: recovering nodes must re-acquire partition ownership via the coordinator before firing (don't fire on stale ownership), and the queue absorbs the recovery burst. DLQ any job that repeatedly fails (poison).

---

### Scaling to 10x / 100x (Hard)
Q13. At 10x (1B jobs, 20M due/min), where does it break first?
> **Expected answer:** The **due-scan against the job store** and the **coordinator/partition-assignment** break first. (1) Even partitioned, scanning 1B rows' worth of buckets and keeping near-due timers in memory strains each node's RAM (a timing wheel of tens of millions of near-due timers). (2) The single job store becomes a write hot spot for recurring jobs constantly rescheduling `next_run` (write amplification — Lesson 23, favors LSM). (3) The **queue** must sustain 20M+ enqueues/min with the 00:00 spike — Kafka partition count and worker consumer-group rebalance become the limit. Fix: more time-partitions and scheduler nodes, LSM-backed store or a dedicated timer store, jitter to flatten spikes, and separate "hot recurring" from "cold one-shot" storage tiers.
> **Numbers to ground the answer:** 20M/min ≈ 333k dispatch/sec steady; 00:00 spike jittered over 5 min = flatten a potential 500M into ~1.6M/sec. 1B jobs × 500B = ~500 GB store → must be sharded (say 50 shards × 10 GB). Timing wheel holding 5 min of near-due at 333k/sec = ~100M entries in RAM across the fleet → spread over ~50+ scheduler nodes.

Q14. Sharding: how do you partition the timer space across scheduler nodes, and what's the hot spot?
> **Expected sharding strategy:** Partition by **time_bucket** (and within a bucket, by hash of jobId for further spread), assigned to scheduler nodes via **consistent hashing + a coordinator** (ZooKeeper/etcd) so each bucket has exactly one owner (no double-scan). Consistent hashing (Lesson 19) means adding/removing a scheduler node only reassigns ~1/N of buckets, not a full reshuffle. Ownership is leased so a dead node's buckets get reassigned.
> **Hot spot problem:** The classic hot spot is a **single popular fire time** — everyone schedules `0 0 * * *` (midnight) or `0 * * * *` (top of hour), so one time_bucket is enormous while neighbors are empty. Partitioning by time alone concentrates that bucket on one node. **Detect** via per-bucket cardinality metrics. **Fix:** sub-shard a hot bucket by `hash(jobId) % K` so K nodes each own a slice of the midnight bucket; and apply **jitter** at schedule time (spread "midnight" over a window) unless the job demands strict timing. Also watch **cron alignment skew** — cron expressions cluster on round numbers (00, 15, 30, 45), so buckets on those boundaries are systematically hotter.

Q15. Caching strategy — what's worth caching in a scheduler, and the invalidation pitfall?
> **Expected layered cache design:**
> - **Near-due timers in memory** (the timing wheel) *is* the primary cache — the DB is the durable backing store, the wheel is the hot working set of "jobs firing soon." Load next N minutes; the DB isn't hit at fire time.
> - **Cron-expression parse results** cached (parsing `0 9 * * *` + TZ/DST math is non-trivial; cache the compiled schedule per job).
> - **Partition/ownership map** cached from the coordinator with a watch, so you're not querying etcd per dispatch.
> **Cache invalidation trap:** The dangerous one is a **cancel/update racing the in-memory wheel**. A user cancels a job that's already loaded into a scheduler node's timing wheel — the DB row is now `CANCELLED` but the in-memory copy will still fire. Fix: the worker (or dispatch step) must **re-validate against the store** (CAS `SCHEDULED→DISPATCHED`) at fire time — the wheel is an optimization, the store is truth, so a cancelled job fails the CAS and is dropped. Never treat the in-memory wheel as authoritative for side effects. Also invalidate/reload the wheel on job update (reschedule) via a change notification, or accept that the CAS-at-fire catches stale entries.

Q16. Cost/efficiency at scale.
> **Expected answer:** (1) **Jitter/spread** to avoid provisioning for a 00:00 peak you only hit for one second — flattening lets you run a smaller steady worker fleet. (2) **Autoscale workers** on queue depth (they're stateless) — scale to zero between bursts for cheap one-shot workloads (serverless/Lambda targets). (3) **Tiered timer storage** — hot near-due timers in RAM/Redis, the vast cold future/recurring tail in cheap durable storage (LSM/object store); only promote to RAM as they approach. (4) **Coalesce missed recurring runs** so recovery doesn't run (and bill for) 100 redundant executions. (5) **Batch enqueue and batch DB reschedule** writes rather than per-job round trips. (6) Use an LSM store for the high-churn `next_run` rewrites (recurring jobs) to reduce write amplification vs a B-tree with constant in-place updates.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Clock skew: your scheduler nodes disagree on "now" by up to 200ms (or worse without NTP). How does this cause jobs to fire early/late or double, and how do you bound it? (Direction: never trust a single node's wall clock for ordering/fencing — use a monotonic sequence (fencing token) for safety, not timestamps. Sync via NTP/chrony, cap acceptable skew, and for cross-node correctness use the coordinator's notion of ownership + fencing rather than "whoever's clock says it's time." Google's TrueTime/Spanner bounds uncertainty explicitly and *waits out* the uncertainty window; most schedulers instead tolerate second-level imprecision and rely on idempotency for the double-fire that skew can cause.)

**H2.** Multi-tenancy / fairness: one tenant schedules 100M jobs and starves everyone else's dispatch. How do you isolate? (Direction: per-tenant quotas on job count and dispatch rate; fair-share scheduling across tenant queues (weighted round-robin over per-tenant partitions) so no tenant monopolizes workers; separate queues/worker pools (bulkheads) for critical vs bulk tenants; rate-limit the create API. Noisy-neighbor at the 00:00 burst is the acute case — cap per-tenant burst share.)

**H3.** Zero-downtime deploy of the scheduler tier without missing fire times or double-firing during the rollout. (Direction: rolling deploy with **graceful ownership handoff** — a node being replaced releases its partitions to the coordinator, which reassigns to healthy nodes *before* it exits (drain, don't kill); leases + fencing ensure that even if handoff is imperfect, no double-fire. In-flight jobs already enqueued are unaffected (queue + stateless workers). Blue-green the control API; the durable store is the continuity anchor. Catch-up policy covers any brief gap.)

**H4.** Observability: what do you instrument, and what pages you? (Direction: **scheduling latency** = actual_fire_time − scheduled_time, p99 (the core SLO — are jobs on time?); **missed/late jobs** count; **queue depth & consumer lag** (leading indicator of worker starvation); **dispatch dedup rate** (double-dispatch caught by CAS — spikes signal ownership churn); **execution success/retry/DLQ rates** per job; **partition ownership churn**. Trace each run by run_id end to end. Page on: fire latency SLO breach, growing missed-job count, DLQ growth, coordinator/leader flapping.)

**H5.** 'Undo a bad decision': you built it with the DB-poll model (`SELECT WHERE next_run<=now` every second, all nodes) and it's melting the DB and double-firing. Migrate to partitioned timing wheels without downtime. (Direction: introduce the coordinator + time-bucket partitioning incrementally — assign buckets to nodes so each polls only its buckets (kills the all-nodes race immediately), add the CAS-claim to stop double-fire, then layer the in-memory timing wheel to move from per-second polling to coarse loads. Run old and new dispatch paths in shadow (new path marks a shadow flag, compare fired sets) before cutting over. The job store stays the durable truth throughout, so it's a dispatch-layer migration, not a data migration.)

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. **Claiming exactly-once execution.** It's impossible across crashes; the right frame is at-least-once dispatch + idempotent (run_id) execution + fencing tokens for the slow-worker double-run. Candidates who say "the queue gives exactly-once" haven't thought about the side-effecting worker dying post-completion.
2. **Polling the DB every second from every node.** Ignores the hot-partition melt at 00:00 and the double-dispatch race. The two-level design (coarse DB load → in-memory timing wheel → CAS claim) is the senior answer.
3. **Rescheduling recurring jobs before/independently of execution**, so a crashed run silently stops the job forever, and pre-materializing infinite future occurrences. Reschedule *after* a durably recorded run, from the cron expression, relative to the scheduled time (not "now") to avoid drift.

**The one insight that makes an answer truly impressive:**
Naming the **"never miss vs never double-run" tension** explicitly and resolving it structurally: store-then-enqueue makes dispatch at-least-once (never miss), CAS-claim + partition ownership prevents double-*dispatch*, and **fencing tokens** prevent double-*execution* by a zombie worker — three different mechanisms for three different double-fire causes (double dispatch, slow worker, failover overlap). Plus using a **hierarchical timing wheel** to get O(1) timer handling and jitter to defeat the thundering herd. Candidates who separate "double dispatch" from "double execution" and assign the right mechanism to each are operating at staff level.

**Suggested follow-up reading:**
- "Hashed and Hierarchical Timing Wheels" (Varghese & Lauck) — the foundational timer paper; and Kafka's `SystemTimer`/purgatory implementation.
- Quartz Scheduler misfire-instruction docs, and Uber's Cadence/Temporal architecture (durable timers via event sourcing) + "How to do distributed locking" (Kleppmann) on fencing tokens.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate — especially Lessons 17 (leader election), 21 (Kafka), 23 (B-tree vs LSM), 5 (heaps/timing wheels), 19 (consistent hashing).
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to a distributed cron/job scheduler. Expected answers include real algorithms (hierarchical timing wheel, CAS-claim, fencing tokens, cron/DST math), specific failure modes (00:00 thundering herd, zombie double-run, missed-window catch-up), and real numbers. Cross-referenced Phase 1 lessons throughout.
