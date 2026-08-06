# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 56 of 63 — Phase 2: System Design Track (Design 28 of 35)
**Topic:** BookMyShow / Ticketmaster — Seat Booking + Concurrency
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Seat booking is the canonical *high-contention, must-be-correct* system: a specific seat (A12 for the 9pm show) is a unique, non-fungible resource that exactly one person may buy, and when a blockbuster or a Coldplay tour goes on sale, hundreds of thousands of people fight for the same few thousand seats in the same 60 seconds. Two people must never get seat A12; a seat must never be locked forever by someone who abandoned checkout; and the whole thing must survive a flash crowd 100x normal traffic. Ticketmaster, BookMyShow, and every airline-seat/event platform live and die on the seat-locking protocol and the virtual waiting room. This is where "just use a database transaction" meets its limits.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**ACID & Isolation Levels (taught in Phase 1, Lesson 2):** Atomicity, consistency, isolation, durability; isolation levels (Read Committed → Serializable) and the anomalies each prevents (dirty/non-repeatable/phantom reads). The seat purchase must be *serializable-equivalent* for the seat rows — two buyers of A12 must be serialized. Today: the final commit uses SELECT … FOR UPDATE or a conditional UPDATE to serialize seat ownership.

**Optimistic vs Pessimistic Concurrency & Locking (taught in Phase 1, Lessons 2 & 22):** Pessimistic = lock the row before you touch it (FOR UPDATE); optimistic = read a version, write only if version unchanged (CAS). Seat *reservation* uses a short-lived distributed lock (Redis); seat *purchase* uses a conditional DB update. Choosing between them per stage is the crux.

**Redis / Distributed Locks & TTL (taught in Phase 1, Lesson 24):** In-memory, atomic SETNX, TTL-based auto-expiry, Lua for multi-key atomicity, Redlock for multi-master. The 8-minute "seat held for you" timer is a Redis key with TTL — self-healing if the user vanishes. Central to avoiding permanently-locked seats.

**Rate Limiting & Queueing (taught in Phase 1, Lesson 11):** Token bucket / sliding window + a FIFO queue. The virtual waiting room is admission control: admit users into the buying flow at a rate the seat-map and DB can absorb, holding the rest in a fair queue.

**CAP / PACELC (taught in Phase 1, Lesson 2):** The seat-inventory writes are strictly CP — during a partition you refuse to sell rather than risk a double-sell. Browse/discovery (what movies/shows exist) is AP.

**Caching (taught in Phase 1, Lesson 24):** Seat-map rendering, show catalog, static content on CDN. The *availability* layer is subtle: you cache the seat map but overlay live lock/sold state.

**Message Queues / Kafka (taught in Phase 1, Lesson 21):** Decouple payment processing, ticket/PDF generation, email/SMS, and analytics from the synchronous booking path so the hot path stays fast.

**Consistent Hashing (taught in Phase 1, Lesson 19):** Shard seat inventory by show_id so each show's contention is isolated to a bounded set of nodes; a mega-event's show shards independently.

> **Recap box:**
> - ACID/isolation (L2): seat purchase must serialize on the seat row.
> - Optimistic vs pessimistic (L2/L22): Redis lock to reserve, conditional DB update to buy.
> - Redis locks+TTL (L24): 8-min hold = TTL key, self-healing.
> - Rate limiting/queue (L11): virtual waiting room = admission control.
> - CAP (L2): seat inventory CP; browse AP.
> - Kafka (L21): async payment/ticket-gen/notify off the hot path.
> - Consistent hashing (L19): shard inventory by show_id.

---

## Part 2 — The Interview Session

### Warm-Up Questions (Easy)

Q1. Explain the three states a seat can be in during a booking flow, and what triggers each transition. Why can't a seat be just "available" or "sold"?
> **What a strong answer covers:** Three states: **AVAILABLE → HELD (locked/reserved) → SOLD** (plus HELD → AVAILABLE on timeout/abandon). You need the intermediate HELD state because payment takes time (10s–8min) and during that window the seat must be reserved for one user but not yet sold (payment might fail). Without HELD, either you sell before payment (risk unpaid seats) or you don't reserve during payment (two people pay for A12, one gets refunded — terrible UX). HELD is a Redis key with TTL so an abandoned checkout auto-releases.
> **Common weak answer:** "Available or sold, and I lock the row in the DB during checkout." Holding a DB row lock for 8 minutes of human think-time is a disaster — it pins a connection and a lock for minutes; 5000 concurrent holds exhaust the connection pool.
> **Mentor follow-up:** The user holds A12 and closes their laptop. Walk me through exactly how A12 becomes available again, and the exact failure if you implemented the hold as a DB row lock instead.

Q2. Coldplay goes on sale. 500,000 people hit "book" in the first minute for a 50,000-seat stadium. Estimate the request storm and the fundamental mismatch.
> **What a strong answer covers:** 500K users in ~60s = **~8,300 users/sec** arriving, but each user isn't one request — seat-map load, multiple seat-selection attempts, lock attempts, retries on failure = easily 10–20 requests/user → **~100K+ req/sec** peak. Yet there are only 50,000 seats total — **90% of these users cannot possibly get a ticket**, and most of the traffic is doomed contention on already-locked seats. The fundamental mismatch: demand (500K) vastly exceeds supply (50K), so the system's real job is *fair, orderly rejection*, not throughput. This is why you need a **virtual waiting room** to admit ~a few thousand at a time rather than letting 500K hammer the seat map.
> **Mentor follow-up:** If only 50K can win, why not just admit exactly 50K and reject the rest immediately? What breaks with that?

Q3. For the final "commit the purchase," would you use SELECT … FOR UPDATE, an optimistic version check, or a Redis lock as the source of truth? Defend it.
> **What a strong answer covers:** The **DB is the source of truth for SOLD**; Redis holds the transient reservation. Final purchase = a conditional atomic update: `UPDATE seats SET status='SOLD', order_id=? WHERE seat_id=? AND show_id=? AND status='HELD' AND held_by=?`. It succeeds for exactly one transaction; the seat's row is serialized by the DB. Redis lock reserved it (fast, cheap, avoids DB contention during selection), but you must re-verify at commit against the durable store — never trust Redis as the final authority for money-backed ownership (Redis can lose a key on failover). Belt-and-suspenders: Redis for reservation speed, DB conditional update for correctness.
> **Red flag answer:** "Redis SETNX is the source of truth, I write to DB later." A Redis failover/eviction could drop the lock and let two people commit; money is involved — the durable store must arbitrate the final sale.

---

### High-Level Design (Medium)

Q4. Design the end-to-end architecture for seat booking with a flash-sale-grade waiting room. Draw it.
> **Key components expected:** Client, CDN, virtual waiting-room service (queue + admission tokens), API GW, Booking service, Seat-inventory service (Redis lock layer + Postgres source of truth, sharded by show_id), Payment service, Kafka (async ticket-gen/notify/analytics), Ticket-generation service, Notification service, Show/catalog service (AP, cached).
> **Architecture diagram (text):**
```
 User --> [CDN/static] 
     --> [Virtual Waiting Room] --admit token(rate-limited)--> [API GW]
                 |  (FIFO queue in Redis, position updates)        |
                 v                                                  v
           holds 490K users                              [Booking svc]
                                                          |   1) load seat map (cache)
                                                          |   2) SETNX seat locks (Redis, TTL 8m)
                                                          |   3) create order (PENDING)
                                                          v
                                          [Seat Inventory: Redis locks + Postgres(source of truth)]
                                                          |         sharded by show_id
                                          [Payment svc] --> on success:
                                                          UPDATE seats HELD->SOLD (conditional)
                                                          |
                                          [Kafka] --> [Ticket-gen] [Notify] [Analytics]
```
> **What separates SDE2 from SDE3 here:** SDE3 splits the problem into **admission control (waiting room)** + **reservation (fast Redis lock)** + **durable sale (DB conditional update)** as three separate concerns with different consistency/throughput needs, and never lets the 490K queued users touch the seat map at all. SDE2 tends to put all 500K straight onto the booking service and then "add caching," which doesn't address that the bottleneck is *contention*, not compute. The insight: the waiting room converts an uncontrollable flash mob into a steady, bounded stream the inventory layer can actually serve.

Q5. Trace one user from entering the waiting room to receiving their ticket. Include the timers.
> **Expected trace:**
> 1. User hits sale → assigned a queue position (Redis sorted set / token); polls or gets SSE position updates. No seat-map access yet.
> 2. Admission control releases users at a controlled rate (say a few hundred/sec) → user gets a short-lived **admission token** (JWT, TTL ~10 min) allowing entry to the booking flow.
> 3. User loads seat map (cached layout + live availability overlay).
> 4. User selects seats → Booking service does `SETNX lock:{show}:{seat} {userId} EX 480` (8-min hold) for each; on any failure (seat just taken), release the ones already locked, tell user to reselect.
> 5. Order created PENDING; user has the 8-min timer to pay.
> 6. Payment succeeds → conditional `UPDATE ... HELD->SOLD`; emit Kafka event.
> 7. Ticket-gen (async) creates PDF/QR; notification sends email/SMS. User sees confirmation.
> **Tricky part:** The two timers — admission-token TTL and seat-hold TTL — and what happens when payment succeeds *after* the seat-hold expired (user paid at 8:05). You must handle "payment succeeded but hold expired and seat was resold": reconcile via idempotent payment + auto-refund, and ideally extend/verify the hold is still held right before capturing payment. Candidates who ignore the expired-hold-but-paid case have a money bug.

Q6. Design the key APIs: enter queue, lock seats, and confirm purchase.
> **Expected API design:**
> - `POST /v1/waitroom/{event}/enter` → `{queue_token, position, eta}`; `GET /v1/waitroom/status` polls position (or SSE stream).
> - `POST /v1/shows/{show_id}/seats/lock` — header: admission token; body `{seat_ids:[...]}`; header `Idempotency-Key`. Returns `{hold_id, expires_at}` or `409` with the conflicting seats.
> - `POST /v1/orders/{hold_id}/confirm` — body `{payment_token}`, header `Idempotency-Key`. Returns `{order_id, tickets}`.
> - `DELETE /v1/holds/{hold_id}` — explicit release when user backs out.
> **What to push on:** Idempotency everywhere (retries under load are constant; a retried lock must not create a second hold, a retried confirm must not double-charge). Locking multiple seats must be **all-or-nothing** (either get all 4 seats together or none — a group doesn't want to be split). `409` returns *which* seats conflicted so the client can re-render. Admission token validated server-side on every booking call — no token, no seat map.

---

### Data Modeling (Medium–Hard)

Q7. Design the core data model: shows, seats, holds, and orders.
> **Expected schema:**
```sql
-- SEATS: one row per physical seat per show (source of truth for SOLD)
CREATE TABLE seats (
  show_id BIGINT, seat_id VARCHAR,      -- composite PK
  section VARCHAR, row VARCHAR, number INT, price_tier VARCHAR,
  status VARCHAR NOT NULL,              -- AVAILABLE/HELD/SOLD
  held_by BIGINT, hold_expires_at TIMESTAMPTZ,
  order_id UUID, version INT DEFAULT 0,
  PRIMARY KEY (show_id, seat_id)
);
CREATE INDEX ON seats (show_id, status);      -- availability query
-- ORDERS
CREATE TABLE orders (
  order_id UUID PRIMARY KEY, user_id BIGINT, show_id BIGINT,
  seat_ids TEXT[], amount NUMERIC, status VARCHAR,   -- PENDING/PAID/FAILED/REFUNDED
  idempotency_key TEXT UNIQUE, created_at TIMESTAMPTZ, version INT DEFAULT 0
);
CREATE TABLE shows ( show_id BIGINT PK, event_id BIGINT, venue_id BIGINT, starts_at TIMESTAMPTZ, ... );
```
```
-- LIVE HOLD (Redis, the fast reservation layer, TTL-backed)
SET lock:{show_id}:{seat_id} {user_id} NX EX 480    # 8-minute hold, self-releasing
```
> **Index choices and why:** (show_id, status) to render availability fast per show; PK (show_id, seat_id) makes the conditional seat update a single-row primary-key hit (no scan). orders idempotency_key unique to dedupe retries.
> **Partitioning key and why:** Shard everything by **show_id** (consistent hashing). All contention for a given show is confined to one shard, and a seat purchase touches exactly one show's rows — no cross-shard transaction. A mega-event's show can be given its own dedicated shard(s). Never shard by seat_id or user_id — you'd need cross-shard coordination to hold 4 seats atomically, and hot events would smear across all shards. The natural unit of contention and consistency is the show, so that's the shard key.

Q8. Rendering the live seat map for a hot show: how do you serve "current availability of 50,000 seats" to thousands of concurrent viewers without hammering the DB?
> **Expected answer:** The **seat layout** (which seats exist, geometry, price tiers) is static → cache/CDN. The **availability overlay** (each seat's AVAILABLE/HELD/SOLD) is dynamic and stored in Redis (a bitmap or hash per show: seat→state). Reads hit Redis, not Postgres. Push deltas: when a seat is locked/sold, publish a small delta event so open seat maps update via WebSocket/SSE rather than re-fetching all 50K. So the client gets the static layout once + a stream of tiny state deltas. Postgres is touched only on the write path (lock-verify/commit), never for rendering.
> **Trap:** Serving the seat map from `SELECT * FROM seats WHERE show_id=?` on every render → 50K rows × thousands of viewers × frequent refresh = DB meltdown. Or invalidating a cached seat map on every single lock (churns constantly under a flash sale). Use Redis availability + delta push.

Q9. Seat inventory is CP. But the seat-map *display* is AP (cached, slightly stale). Reconcile the two — a user sees A12 green, clicks, and it's gone.
> **Expected answer:** Display is intentionally eventually-consistent (AP) — showing A12 as available a second after someone locked it is acceptable and unavoidable at this scale. The **lock attempt is the consistency boundary**: `SETNX lock:show:A12` is atomic and authoritative — if it fails, the user is told "just taken, pick another," and the client re-renders. So the display is optimistic/stale, but the *action* is strongly consistent. The user experiences "seat looked free but wasn't" occasionally — that's correct and expected; the alternative (strongly-consistent live map for 50K seats × thousands of viewers) is infeasible. Never sell based on the cached display; always re-verify at lock and again at commit.
> **Mentor pushback:** During the SETNX-succeeded-but-DB-commit-not-yet window, the seat is HELD in Redis but still AVAILABLE in Postgres. If Redis fails over and loses the key, two users could both SETNX. Fix: the final conditional `UPDATE ... WHERE status='AVAILABLE/HELD' AND held_by=?` in the durable, CP Postgres is the ultimate arbiter — even if Redis double-grants, only one DB update wins; the loser's payment is voided/refunded. Redis optimizes the common path; Postgres guarantees correctness.

---

### Low-Level Design (Hard)

Q10. The core problem: the virtual waiting room. Design fair admission for 500K users onto a system that can only serve a few thousand active bookers, with real-time position feedback.
> **Problem statement:** Admit users into the booking flow at a controlled, fair (roughly FIFO) rate; hold the rest in a queue with live position updates; protect the seat-inventory + DB from the flash mob; survive users leaving and rejoining.
> **Naive solution:** Let everyone in, rely on the DB/lock layer to reject losers; or a simple "you are in a queue" page with random admission.
> **Why naive fails at scale:** (1) Letting 500K hit the seat map turns 90% of load into doomed contention retries that starve the 10% who could actually buy — the DB and Redis lock layer collapse under contention, not throughput. (2) Random/unfair admission enrages users (someone who arrived at t=0 waits behind someone at t=30s) and invites bot abuse. (3) No backpressure = cascading failure.
> **Expected optimal approach:** A token-based FIFO admission queue in Redis. On arrival, `ZADD queue:{event} {arrival_ts} {user_id}` (sorted set = fair ordering). A rate-controlled **admission worker** pops the front at a target rate (tuned to what inventory can serve, e.g., 300/sec) and issues a signed **admission token** (JWT with short TTL). Position feedback: user's rank = `ZRANK`; ETA = rank / admission-rate. Enforce single-entry per user (dedupe by user_id/device to stop queue-stuffing). Admitted-but-inactive users' tokens expire, freeing capacity. Optionally a "waiting room" edge (Cloudflare-style) so the queue page itself is served from CDN and never touches origin.
> **Pseudo-code or class diagram:**
```
def enqueue(user, event):
    redis.zadd(f"queue:{event}", {user.id: now()}, nx=True)   # idempotent, FIFO
    return {"position": redis.zrank(f"queue:{event}", user.id)}

# admission worker, runs at controlled rate per event
def admit_loop(event, rate_per_sec, inventory_headroom):
    while sale_open(event):
        n = min(rate_per_sec, inventory_headroom())          # backpressure
        batch = redis.zpopmin(f"queue:{event}", n)           # front of queue
        for user_id, _ in batch:
            token = sign_jwt(user_id, event, ttl=600)        # 10-min entry pass
            push_admission(user_id, token)
        sleep(1)

def position(user, event):                                    # live feedback
    rank = redis.zrank(f"queue:{event}", user.id)
    return {"position": rank, "eta_sec": rank / admission_rate(event)}
```

Q11. Two users click seat A12 within the same millisecond during a flash sale. Walk the exact race from both requests and prove only one wins — including the abandoned-hold case.
> **Scenario:** Requests U1 and U2 both target lock `lock:show9:A12` at t=0; separately, U3 held A12 earlier but abandoned checkout.
> **Expected fix:** `SET lock:show9:A12 U1 NX EX 480` — Redis is single-threaded per key, so exactly one of U1/U2's SETNX returns OK; the other returns nil and is told to pick another seat. That's the reservation-layer serialization. At purchase, the DB conditional `UPDATE seats SET status='SOLD', order_id=? WHERE show_id=9 AND seat_id='A12' AND status IN ('AVAILABLE','HELD') AND (held_by=? OR status='AVAILABLE')` — single-row PK update, serialized by the DB, so even if Redis mis-granted, only one commit sticks. **Abandoned hold (U3):** U3's `lock:show9:A12` has `EX 480`; when U3 vanishes, the key auto-expires at 8 min and A12 returns to AVAILABLE with zero manual cleanup — this is why the hold is a TTL key, not a DB lock. The DB's `hold_expires_at` column is reconciled by a lightweight sweeper for durability, but Redis TTL is the primary self-healing mechanism.
> **Follow-up (what if the lock holder dies?):** If the Booking service instance that set the lock crashes, the lock still expires via its TTL (state lives in Redis, not the process). If Redis itself fails over mid-hold, the replica may or may not have the key — but since the *durable sale* is arbitrated by the Postgres conditional update, a lost Redis lock at worst frees a seat early (someone else can grab it) or lets two people hold-then-attempt-pay, where the DB serializes the winner and the loser is auto-refunded. No double-sale reaches a confirmed ticket.

Q12. Payment gateway is slow/times out during peak. A user's card was charged but our confirm call to the gateway timed out — we don't know if it succeeded. The seat hold is about to expire. Handle it.
> **Scenario:** Payment ambiguity (timeout) + imminent hold expiry.
> **Expected handling:** Never decide from a timeout. Use an **idempotent payment with an idempotency key** so a retry doesn't double-charge; query the gateway's status endpoint (or wait for its async webhook) to resolve the true outcome. Meanwhile, **freeze the seat**: extend the hold (or move the seat to a PAYMENT_PENDING state) rather than let TTL expire and resell a seat the user may have paid for. Resolution: if payment succeeded → complete the sale (conditional HELD→SOLD; if the seat was somehow already resold, refund and apologize/upgrade); if it failed → release the seat. Kafka + a reconciliation job handles late webhooks. The rule: an ambiguous payment must **pin the seat until resolved**, and the payment path must be idempotent so retries/webhooks converge on one charge. A DLQ captures unresolvable cases for manual finance review.

---

### Scaling to 10x / 100x (Hard)

Q13. At 100x (a global tour on-sale: millions in the queue, ~100K+ booking req/sec if admitted), where does it break first?
> **Expected answer:** Without the waiting room, the **Redis lock layer for the hot show's shard** breaks first — all contention funnels to the seat rows of one show on one shard; even at 100K+ ops/sec a single hot key/shard saturates. That's *why* the waiting room exists: it caps admitted concurrency so the inventory layer never sees the full mob. With the room, the next bottleneck is the **admission queue Redis** (millions in one sorted set — ZADD/ZRANK still fine, but position polling from millions is a read storm → serve position via CDN/SSE, not per-poll DB). Then payment-gateway throughput (external, rate-limited) becomes the true ceiling on actual sales.
> **Numbers to ground the answer:** 50K seats sold in, say, 5 minutes = ~167 sales/sec — that's the *real* achievable sale rate, bounded by payment + inventory commit. So admission is tuned to a few hundred/sec, not 100K. The system's job is to make a 100K/sec mob politely become a 200/sec orderly line. Queue in Redis: millions of members × ~50B = a few hundred MB — fits; the risk is the *poll* fan-out, not storage.

Q14. Apply consistent hashing (Lesson 19): shard seat inventory. What about the single mega-event that's hotter than everything else combined?
> **Expected sharding strategy:** Shard by **show_id** via consistent hashing with virtual nodes — each show's contention isolated, and a seat transaction is single-shard (no distributed transaction to hold 4 seats). Most shows share shards.
> **Hot spot problem:** One mega-event (Coldplay) dwarfs all other traffic and would overwhelm whatever shard owns its show_id. **Detect:** per-shard/per-show ops/s. **Fix:** give the mega-event **dedicated shard(s)** (pin its show_id to isolated nodes so it can't starve normal shows — bulkhead). Within the event, if a single stadium show is still too hot, sub-partition by **section** (seat locks for Section A on one node, Section B on another) since seat groups rarely cross sections — this spreads the hot key space while keeping group-booking (usually same section) single-node. The waiting room's admission rate is the primary defense; sharding by section is the second line. Never shard by seat_id globally — atomic multi-seat holds would go cross-shard.

Q15. Design caching: seat map, availability, and show catalog. What's the hardest invalidation?
> **Expected layered cache design:**
> - **CDN:** seat-map layout/geometry, show images, venue info, the waiting-room page itself.
> - **Redis availability layer:** per-show seat-state (hash or bitmap), the live source for rendering; updated on every lock/sale.
> - **Delta push (WebSocket/SSE):** open seat maps receive tiny per-seat state deltas rather than re-fetching — keeps thousands of viewers current cheaply.
> - **Catalog cache:** shows/events/venues (AP), long TTL, event-invalidated on schedule changes.
> **Cache invalidation trap:** The seat-availability overlay changes thousands of times per second during a flash sale — you cannot treat it as a cache to invalidate; it's a **live materialized view in Redis** with delta streaming, not a TTL cache. The mistake candidates make is caching the *rendered seat map including availability* and then trying to bust it on every lock — the invalidation rate equals the write rate and the cache becomes pure overhead. Separate the *static layout* (heavily cached) from the *live availability* (Redis + deltas). That split is the whole trick.

Q16. Cost/efficiency at scale, given traffic is extremely bursty (idle most of the time, then a mega on-sale)?
> **Expected answer:** The defining cost trait is **burstiness** — 99% idle, then a 100x spike for a known event at a known time. Cuts/strategies: **autoscale on schedule** (pre-warm capacity minutes before an announced on-sale; scale to zero-ish between events) — you know when Coldplay tickets drop, so provision proactively, not reactively. Serve the waiting room and static seat layout from **CDN/edge** so the origin barely scales. Keep the expensive CP inventory tier small and protected behind the waiting room (admission caps the load it ever sees). **Async** everything non-critical (ticket PDF/QR gen, emails, analytics) off the hot path via Kafka. Tiered storage for completed-event data (archive past shows). The discipline: don't pay for peak capacity 24/7 — pay for the ability to *pre-scale for scheduled spikes*, and let the waiting room flatten the curve so the pricey transactional core stays modest.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Redis TTL frees abandoned holds, but Redis is AP and can lose a key on failover. Design the reservation so no double-sale is *ever* possible even if Redis loses locks, and explain why the DB conditional update is your true linearization point. *(Expected: Redis is a performance optimization for the hold; the durable, single-row PK conditional UPDATE in Postgres is the linearization point that serializes the sale — even if Redis double-grants a lock, only one DB commit succeeds and the loser is refunded. Optionally fencing tokens; optionally SERIALIZABLE isolation on the commit txn. The key admission: never let money-backed ownership be decided by an AP store.)*

**H2.** Bots and scalpers buy out the inventory in seconds. What's the anti-abuse design across the queue and booking, and how do you keep it fair without blocking real users? *(Expected: dedupe queue entries per user/device/payment-instrument, CAPTCHA/proof-of-work at queue entry, per-user seat caps, velocity/anomaly detection, device fingerprinting, delayed reveal of the queue; balance false positives — err toward friction at queue entry, not at payment, so a real user isn't rejected after selecting seats.)*

**H3.** Roll out a new locking protocol (say switching hold TTL semantics) across a live platform with events in progress, no downtime. *(Expected: feature-flag per event; new events use the new protocol while in-flight events finish on the old; canary on a low-stakes event, watch double-sell rate (should be zero), hold-leak rate, and abandon-recovery; keep the DB conditional update unchanged as the safety net so a bug in the new hold layer can't cause a double-sale during migration.)*

**H4.** What do you instrument to know a flash sale is healthy in real time? Name specific metrics and a leading indicator of trouble. *(Expected: admission rate vs queue depth, seats-sold/sec, lock-contention/lock-fail rate, hold-to-purchase conversion, payment success rate + gateway latency, hold-expiry/abandon rate, waiting-room dwell time. Leading indicator: rising lock-fail rate or payment-gateway latency signals the pipe clogging before sales stall — throttle admission rate down in response.)*

**H5.** You launched holding seats via `SELECT … FOR UPDATE` for the whole checkout, and connection pools are exhausting under load (locks held for minutes of human think-time). Migrate to the Redis-TTL hold model without downtime. *(Expected: introduce Redis SETNX holds alongside, dual-write during transition, move the *hold* to Redis-TTL while keeping the *final sale* as a short DB transaction (FOR UPDATE only for the sub-second commit, never for think-time); flip reads of hold-state to Redis behind a flag, verify no double-sell in canary, then drop the long-lived DB locks. The principle: DB locks are for milliseconds, TTL keys are for minutes of human time.)*

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Holding a database row lock (`SELECT … FOR UPDATE`) across the user's payment think-time — pins connections/locks for minutes and exhausts the pool under load. Holds belong in a TTL key; DB locks are for the sub-second commit only.
2. Ignoring that the real problem is *fair rejection of a demand-supply mismatch* (500K want 50K seats). Without a virtual waiting room, 90% of load is doomed contention that starves the winners.
3. Trusting Redis as the source of truth for a money-backed sale. Redis reserves fast; the durable DB conditional update is the true linearization point.

**The one insight that makes an answer truly impressive:**
The clean three-layer decomposition — **admission control (waiting room)** to flatten the mob, **fast reservation (Redis TTL lock)** to avoid DB contention during selection, and **durable arbitration (single-row PK conditional UPDATE in Postgres)** as the linearization point for the actual sale — with each layer chosen for its consistency/throughput profile, and the explicit statement that even total Redis failure cannot produce a double-confirmed ticket because the DB commit is the real serialization point.

**Suggested follow-up reading:**
- Ticketmaster / "Smart Queue" and Cloudflare "Waiting Room" engineering write-ups.
- Martin Kleppmann, "How to do distributed locking" (the Redlock debate) — required reading for why Redis locks need a durable backstop.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
