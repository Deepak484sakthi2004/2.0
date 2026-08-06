# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 55 of 63 — Phase 2: System Design Track (Design 27 of 35)
**Topic:** Swiggy / Zomato — Food Delivery Platform
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Food delivery is a *three-sided* real-time system — customer, restaurant, and delivery partner — with a brutal constraint the ride-hailing world doesn't have: the food gets cold. Unlike Uber (match then go), here you must sequence a multi-stage state machine (order → restaurant accept → cook → assign rider → pickup → deliver) where the assignment must be timed so the rider arrives roughly when the food is ready, not before (idle rider) or after (cold food). Swiggy, Zomato, and DoorDash all live or die on (a) a search/discovery layer that ranks tens of thousands of restaurants by location, ETA, and personalization, and (b) a batching/assignment engine that lets one rider carry multiple orders on one trip without wrecking delivery time.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Geospatial Indexing (taught in Phase 1, Lessons 4 & 5):** Quadtrees/geohash/H3 for "which restaurants and riders are near this customer." Restaurants are *static* points (index once), riders are *moving* points (live index). Today: serviceability ("can this restaurant deliver to this address?") and rider assignment both hinge on cell-based radius queries.

**Elasticsearch / Inverted Index & Tries (taught in Phase 1, Lesson 5):** Inverted index maps term→documents for full-text menu/restaurant search; tries power autocomplete. We use it for "biryani near me," filtered by cuisine, veg/non-veg, price, rating, and geo — a multi-filter ranked query, not a key lookup.

**Kafka / Event-Driven (taught in Phase 1, Lesson 21):** The order lifecycle is a stream of state-change events (PLACED → CONFIRMED → PREPARING → READY → PICKED_UP → DELIVERED) consumed by notifications, rider assignment, analytics, and the restaurant tablet. Partition by order_id for per-order ordering.

**SQL/NoSQL & B-tree vs LSM (taught in Phase 1, Lesson 23):** Orders/payments are transactional (Postgres/MySQL, ACID); the restaurant catalog and menu are read-heavy documents (Mongo/Elasticsearch); the order-event log and rider-location history are write-heavy time-series (Cassandra/LSM). Right store per access pattern.

**Caching / Redis (taught in Phase 1, Lesson 24):** Menu cache, restaurant "is open + serviceable" cache, live rider-location index, cart state. Menus change slowly but are read enormously — perfect cache candidates with event-based invalidation.

**CAP / PACELC (taught in Phase 1, Lesson 2):** Discovery/search is AP (a slightly stale rating is fine); order-placement + payment is CP (never double-charge, never lose an order); inventory/menu-item-availability is a middle case.

**Microservices / API Gateway / DDD (taught in Phase 1, Lesson 9):** Bounded contexts — Discovery/Search, Cart, Order, Restaurant (menu/availability), Assignment/Logistics, Delivery-partner, Pricing/Promotions, Payments, Notifications. Keeps the search fanout isolated from the order state machine.

**Consistent Hashing (taught in Phase 1, Lesson 19):** Shard the live rider-location index and the order store by city/geo so a hot city stays bounded and radius queries keep locality.

> **Recap box:**
> - Geospatial (L4/L5): restaurants static-indexed, riders live-indexed; serviceability = radius query.
> - Elasticsearch/tries (L5): multi-filter ranked restaurant search + autocomplete.
> - Kafka (L21): order state-machine events, partition by order_id.
> - SQL/NoSQL (L23): orders=ACID SQL, catalog=doc, event log=Cassandra.
> - Redis (L24): menu + serviceability + live rider index + cart.
> - CAP (L2): discovery AP, order/payment CP.
> - DDD (L9): Search/Cart/Order/Restaurant/Assignment/Payments contexts.
> - Consistent hashing (L19): shard rider index + orders by city.

---

## Part 2 — The Interview Session

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. A customer opens the app on their home screen. What has to happen to show the restaurant list, and which parts are cacheable vs must be live?
> **What a strong answer covers:** Resolve the delivery address → H3/geohash cell → query serviceable restaurants (those whose delivery polygon covers this cell AND currently open AND accepting orders). The *restaurant catalog, menu, ratings, images* are highly cacheable (change slowly, read massively) — serve from Redis/CDN. What must be near-live: **is-open** (hours + manual "pause" toggle), **is-accepting-orders** (kitchen overwhelmed → auto-off), **current ETA** (depends on live rider supply + kitchen load), and **item availability** (sold out). So: cache the catalog, live-check availability/open-state overlaid at read time.
> **Common weak answer:** "Query the DB for all restaurants and filter by distance." Misses serviceability polygons, misses that open/accepting/ETA are live, misses that this is a ranked search not a scan.
> **Mentor follow-up if they answer well:** A restaurant pauses ordering for 10 minutes because the kitchen is slammed. How does that reflect on 50,000 customers' home screens within seconds without you invalidating 50,000 cache entries?

Q2. Estimate the order and search load. Assume 20M daily orders, peaking heavily at lunch (12–2pm) and dinner (7–10pm). What's peak order QPS, and how does search QPS compare?
> **What a strong answer covers:** 20M/day but *not* uniform — maybe 60% of orders in ~4 peak hours. 12M orders / 4h = 3M/h = ~830 orders/sec average in peak, with spikes to ~2–3K orders/sec. Search/browse is 20–50x order volume (people browse, don't all order): ~830 × 30 ≈ **25K search req/sec** at peak, spiking higher. So the system is *read-dominated by discovery*; order writes (~thousands/sec) are modest for a tuned Postgres cluster but the *search + menu reads* (tens of thousands/sec) dominate and drive the caching/Elasticsearch design. Storage: order row ~2KB × 20M/day = 40GB/day → tiered.
> **Mentor follow-up:** Lunch peak is a thundering herd at exactly 12:00. How do you keep the search tier from melting when a metro's worth of office workers all open the app in the same 3 minutes?

Q3. Where do you store the menu — Postgres, MongoDB, or Elasticsearch? Note it's read 1000:1 vs writes and needs full-text search.
> **What a strong answer covers:** The *source of truth* menu is a document (MongoDB or Postgres JSONB) because a menu is a nested tree (categories → items → variants → add-ons). But *search* is served by **Elasticsearch** — denormalize restaurant+menu into search documents with geo, cuisine, price, rating fields, so "veg biryani under ₹300 within 4km rated 4+" is one filtered ranked query. Redis caches the rendered menu per restaurant for the menu-page read. Write path: menu edit → source doc → CDC/event → reindex ES + bust Redis. Never make Elasticsearch the source of truth (it's a search index, not a transactional store).
> **Red flag answer:** "Postgres with LIKE '%biryani%'." Full-table LIKE scans don't scale, no relevance ranking, no geo filter fusion.

---

### High-Level Design (Medium)

Q4. Design the end-to-end architecture: discovery, order placement, and delivery assignment. Draw it.
> **Key components expected:** Client, API GW, Discovery/Search (Elasticsearch + geo), Serviceability service, Menu/Restaurant service (Mongo + Redis), Cart service (Redis), Order service (Postgres, ACID, state machine), Pricing/Promo, Payments, Assignment/Logistics engine, live Rider-location index (Redis GEO), Restaurant tablet gateway, Notifications, Kafka backbone, Cassandra event/history store.
> **Architecture diagram (text):**
```
 Customer App --> [API GW] --> [Discovery/Search]--ES(geo+text)
        |                          [Serviceability]--polygons(Redis)
        |                          [Menu svc]--Mongo + Redis cache
        |--> [Cart svc (Redis)]
        |--> [Order svc]--(state machine)--> [Postgres ACID]
                 |  \--> [Pricing/Promo] \--> [Payments]
                 v
           [Kafka: order-events] --------------------------------+
                 |                    |                |          |
        [Assignment/Logistics]  [Notifications]  [Restaurant   [Cassandra
                 |                                  Tablet GW]   order/loc hist]
        GEOSEARCH riders (Redis GEO, sharded by city/H3)
                 |
        [Rider App] <--assign/push--> live location stream
```
> **What separates SDE2 from SDE3 here:** SDE3 calls out the **timing coupling**: assignment isn't "find nearest rider now," it's "find a rider who arrives when food is READY." That needs a *food-ready-time prediction* (per restaurant, per item count, per current kitchen load) feeding the assignment engine, so the rider is dispatched at `now = ready_time - travel_time`. SDE2 assigns at order time and the rider waits 12 minutes at the counter (wasted supply) or the food sits (cold). The prep-time model is the non-obvious core.

Q5. Trace an order from "Place Order" to "Delivered." Where can it fail and how is each stage confirmed?
> **Expected trace:**
> 1. Cart → Order service creates order (status=PLACED) in Postgres, with idempotency key; payment authorized (not yet captured).
> 2. Order pushed to restaurant tablet; restaurant CONFIRMS (or auto-confirm) → status=CONFIRMED, capture payment. If restaurant rejects → refund auth, notify, optionally re-suggest.
> 3. Kitchen PREPARING; prep-time model predicts ready_at.
> 4. Assignment engine, timed to ready_at, runs rider search + assignment → rider ACCEPTS → status=RIDER_ASSIGNED.
> 5. Rider reaches restaurant, marks PICKED_UP (geo-fenced check-in); status=PICKED_UP.
> 6. Rider delivers, OTP/confirm at door → DELIVERED; settle rider payout.
> **Tricky part:** The restaurant-accept step and the rider-assignment step are *decoupled in time* and both can fail independently — restaurant rejects after payment capture (need refund saga), or no rider is found for a READY order (food cooling — escalate surge/pay incentive, widen radius). Candidates who model it as a single synchronous transaction miss that this is a **long-running saga** with compensating actions, spanning minutes, coordinated by Kafka events.

Q6. Design the key APIs: search restaurants, place order, and update order status (restaurant + rider).
> **Expected API design:**
> - `GET /v1/discovery?lat&lng&q=biryani&cuisine=&veg=true&sort=eta` → ranked serviceable restaurants + ETA. Cursor-paginated (`after` token), not offset (deep offset is slow in ES).
> - `POST /v1/orders` — body `{cart_id, address_id, payment_method}`, header `Idempotency-Key`. Returns `{order_id, status:PLACED}`. 
> - `PATCH /v1/orders/{id}/status` — restaurant tablet (CONFIRMED/READY) and rider (PICKED_UP/DELIVERED) transitions, each validated against the state machine (illegal transitions rejected), each idempotent.
> - `POST /v1/riders/{id}/location` — bidi stream, same as ride-hailing.
> **What to push on:** Idempotency on order create (double-tap / retry must not double-order or double-charge). State-machine validation on status updates (can't go DELIVERED before PICKED_UP). Cursor pagination for search. Optimistic version on the order row so two concurrent updates (rider + support agent) don't clobber.

---

### Data Modeling (Medium–Hard)

Q7. Design the core data model: orders, restaurant/menu, serviceability, and the order event log.
> **Expected schema:**
```sql
-- ORDERS (Postgres, ACID)
CREATE TABLE orders (
  order_id UUID PRIMARY KEY,
  customer_id BIGINT, restaurant_id BIGINT, rider_id BIGINT,
  status VARCHAR NOT NULL,           -- PLACED..DELIVERED/CANCELLED
  address_h3 CHAR(15), total NUMERIC, promo_id BIGINT,
  ready_at TIMESTAMPTZ, assigned_at TIMESTAMPTZ, delivered_at TIMESTAMPTZ,
  idempotency_key TEXT UNIQUE, version INT DEFAULT 0,
  created_at TIMESTAMPTZ
);
CREATE INDEX ON orders (restaurant_id, status);   -- kitchen queue
CREATE INDEX ON orders (rider_id, status);        -- rider active order
CREATE INDEX ON orders (customer_id, created_at DESC);
CREATE TABLE order_items (order_id UUID, item_id BIGINT, qty INT, price NUMERIC, addons JSONB);
```
```
-- MENU (MongoDB doc — nested tree, denormalized for read)
{ restaurant_id, name, categories:[{ name, items:[
    { item_id, name, price, veg, in_stock, variants:[...], addons:[...] } ]}],
  hours, is_paused, avg_prep_min }
-- SERVICEABILITY (Redis): set of H3 cells a restaurant delivers to
SADD  serve:{restaurant_id}  <h3cell> ...        # or store delivery polygon
-- ORDER EVENTS (Cassandra, append-only)
PRIMARY KEY ((order_id), event_ts)               # full audit trail per order
```
> **Index choices and why:** (restaurant_id, status) drives the live kitchen queue; (rider_id, status) finds a rider's current job in O(1); (customer_id, created_at) for order history. Menu in Mongo because it's a variable-depth nested document that would need many joins in SQL.
> **Partitioning key and why:** Orders shard by **city_id / geo** (consistent hashing) so a city's order volume and its kitchen queues colocate; keeps cross-shard joins rare. Order-events Cassandra partitioned by order_id (bounded — tens of events per order). Rider live-index Redis by H3 cell. Don't shard orders by customer_id — restaurants query by restaurant_id and geo, and you'd scatter a single restaurant's queue.

Q8. The restaurant tablet needs its live incoming-order queue, refreshed instantly. How do you serve "all active orders for restaurant R" at scale?
> **Expected answer:** Push, not poll. The tablet holds a persistent connection (WebSocket) subscribed to restaurant R's channel; order-events from Kafka fan out to that channel so a new order or status change lands in <1s. Backing query on reconnect: `SELECT ... FROM orders WHERE restaurant_id=R AND status IN (active)` served off the (restaurant_id, status) index, cached briefly in Redis. The active set per restaurant is small (a handful to dozens), so it's cheap.
> **Trap:** Having 500K restaurant tablets poll `GET /orders?restaurant=R` every 2 seconds = 250K QPS of DB hits for mostly-unchanged data. Push via a pub/sub channel; poll only as reconnect fallback.

Q9. Item availability: a dish sells out mid-lunch. Discovery is AP (cached), but placing an order for a sold-out item is bad. Where's the consistency line?
> **Expected answer:** Discovery/browse can show slightly stale availability (AP) — acceptable, it's a big catalog. But at **cart-add and order-placement time**, do a *live* availability check against the source of truth (the availability flag is a small, hot Redis key per item with event-based invalidation from the restaurant's "mark sold out" action). So: eventual consistency for discovery, read-your-writes/strong check at the transactional boundary. If an item goes out between cart and pay, the order service rejects/prompts substitution rather than confirming a phantom order.
> **Mentor pushback:** Two customers add the last portion of a limited special simultaneously. Availability isn't a hard inventory count for most items — but for genuine limited stock (e.g., "10 thalis today") you need a real decrement. Fix: `DECR stock:{item}` in Redis (atomic) at order time; if it goes negative, reject and roll back. For unlimited items, availability is a boolean toggle, no decrement — don't over-engineer inventory for the common case.

---

### Low-Level Design (Hard)

Q10. The core problem: rider assignment with batching. One rider should carry 2–3 orders on one trip when routes overlap, without any single order's delivery time blowing past SLA. Design it.
> **Problem statement:** Given a stream of orders becoming READY with pickup (restaurant) and drop (customer) locations and target delivery times, and a fleet of riders (each with current position + current load 0–3 orders), assign orders to riders and sequence each rider's pickups/drops to maximize orders-per-trip while keeping each order under its delivery SLA.
> **Naive solution:** Assign each READY order to the single nearest free rider, one order per rider, greedily.
> **Why naive fails at scale:** (1) One-order-per-rider wastes ~40% of delivery capacity when many orders share a corridor — economics don't work. (2) Greedy nearest ignores *timing*: dispatch at order-time strands the rider at a slow kitchen. (3) Naive batching (just stack orders on whoever's near) can push order #1's food to sit 20 min while the rider fetches order #2 — SLA breach. This is a **capacitated vehicle routing problem with time windows (VRPTW)** — NP-hard.
> **Expected optimal approach:** (a) **Prep-time-aware timing:** predict ready_at per order (ML on restaurant, item count, hour, live kitchen backlog); trigger assignment at `ready_at − predicted_travel`. (b) **Batched assignment window:** every few seconds per zone, take the pool of soon-ready orders and idle/low-load riders and solve a **min-cost assignment / insertion heuristic** — for each order, evaluate inserting it into each candidate rider's existing route (cheapest-insertion), reject insertions that push any order past its SLA (time-window constraint). Score = marginal detour cost. Use a greedy insertion + local-search (2-opt) refinement rather than exact VRP solve (too slow for real-time). (c) Constrain batch size to 2–3 and require route overlap (order pickups/drops within a corridor) so batching never hurts. Fall back to single-order assignment when no good batch exists.
> **Pseudo-code or class diagram:**
```
def assign_window(ready_orders, riders, zone):     # every ~3s per zone
    for order in sorted(ready_orders, key=lambda o: o.ready_at):
        best = None
        for r in nearby_riders(order.pickup, riders, max_load=3):
            route = cheapest_insertion(r.route, order.pickup, order.drop)
            if route and no_sla_violation(route):   # time-window feasible
                cost = route.added_distance + route.added_time_penalty
                best = min_by_cost(best, (r, route, cost))
        if best:
            r, route, _ = best
            reserve(r, order); r.route = two_opt(route)     # local search polish
            offer(r, order)
        else:
            escalate(order)          # widen radius / rider incentive / surge
```

Q11. Restaurant confirms and payment is captured, but then no rider can be found for 8 minutes and the food is READY and cooling. Meanwhile the customer cancels. Walk the concurrency/consistency handling.
> **Scenario:** Order is CONFIRMED + paid, food READY, assignment failing, customer hits Cancel — all racing.
> **Expected fix:** The order is a **saga** with compensating transactions. Cancel is a state transition guarded by optimistic version: `UPDATE orders SET status=CANCELLED WHERE id=? AND version=? AND status IN ('READY','CONFIRMED')`. If it succeeds, fire the compensation chain — refund (full/partial per policy since food was made), notify restaurant to stop, and *withdraw the pending assignment* (if a rider was mid-offer, the offer lock's TTL frees them; if just assigned, recall). If a rider *accepted* in the same instant the customer cancelled, the version check serializes it: whichever commits first wins, the loser sees a stale-version error and re-reads the now-terminal state. Never let cancel and assign both "succeed" — the order row's version is the serialization point.
> **Follow-up (what if the lock holder dies?):** If the assignment engine crashes holding a rider's offer lock, the Redis lock TTL (e.g., 20s) frees the rider; the order returns to the unassigned pool and the next window re-picks it. The saga state lives durably in Postgres + Kafka, so a crashed assignment worker doesn't lose the order — a healthy worker resumes from the last committed event.

Q12. A promo code "50% off first order, max ₹100" is applied. Under a flash campaign, thousands of users redeem simultaneously and some try to reuse it across accounts. How do you prevent over-redemption and double-spend?
> **Scenario:** Concurrent promo redemption / budget overrun / reuse abuse.
> **Expected handling:** Promo has a **global budget** and **per-user limit**. Global budget = atomic `DECRBY promo:{code}:budget <discount>` in Redis; if it goes below zero, reject (campaign exhausted) — this serializes thousands of concurrent redemptions without a DB lock. Per-user "first order only" = a uniqueness constraint keyed on (promo_code, user_id) enforced in Postgres (unique index) so a retry/second-account-same-device is caught; device-fingerprint + payment-instrument checks for cross-account abuse. Apply the discount inside the order-create transaction so a failed payment rolls back the redemption (compensating INCRBY). Idempotency key ensures a retried order doesn't double-decrement the budget. DLQ + reconciliation job catches any drift between Redis budget and actual redeemed orders nightly.

---

### Scaling to 10x / 100x (Hard)

Q13. At 10x (200M orders/day, lunch spikes to ~25–30K orders/sec, search ~500K+ QPS), where does it break first?
> **Expected answer:** The **discovery/search tier** breaks first — it's already 20–50x order volume and it's the thundering herd at 12:00 sharp. Elasticsearch query load + the per-request serviceability + live-availability overlay is the hot path. Second: the assignment engine's per-zone solver becomes CPU-bound as pool sizes grow (VRP insertion cost grows with candidates × routes). Third: notification fan-out (every state change × 200M orders × multiple parties). Order writes to Postgres (~30K/s sharded across cities) are manageable if sharded.
> **Numbers to ground the answer:** Search 500K QPS → heavy ES cluster + aggressive Redis caching of ranked results per (cell, filter-set) with 30–60s TTL. Assignment: if a zone has 500 open orders × 200 riders, naive all-pairs insertion is 100K evaluations/window/zone — cap candidate riders per order (nearest ~20) to keep it bounded. 200M orders × ~6 events × ~3 recipients ≈ 3.6B notifications/day → dedicated push infra + batching.

Q14. Apply consistent hashing (Lesson 19): shard the orders store and the live rider index. Handle the Friday-night hot city.
> **Expected sharding strategy:** Orders sharded by **city_id** (consistent hashing with vnodes) — natural locality (a restaurant, its orders, its riders, its customers cluster in a city), keeps kitchen-queue and assignment queries single-shard. Rider live-index by H3 cell within city. Search (ES) sharded by geo routing so a query hits the shard(s) for the customer's region.
> **Hot spot problem:** Friday 8pm in a metro concentrates load on that city's shard. **Detect:** per-shard order QPS + p99 write latency. **Fix:** the metro city is itself sub-sharded by zone (H3 res-7) across multiple physical nodes — dynamic split of the hottest city so it spans N shards while smaller cities share one. Vnodes let you add capacity to just the hot arc. Read-replicas absorb the read/discovery load for that city. Never shard purely by order_id hash — you destroy the city/restaurant locality that makes kitchen queues and assignment cheap.

Q15. Design caching: menus, serviceability, search results, and the "restaurant paused" flash. How do you invalidate?
> **Expected layered cache design:**
> - **CDN:** restaurant images, static menu assets, logos.
> - **L2 Redis — menu cache:** per-restaurant rendered menu, event-invalidated on menu edit (menu-edit event → bust key + reindex ES). High hit rate; menus rarely change.
> - **L2 Redis — serviceability cache:** which restaurants serve a given H3 cell, TTL minutes; rebuilt on delivery-zone change.
> - **L2 Redis — search-result cache:** ranked results keyed by (cell, filter-set), TTL 30–60s — this is what saves you at the 12:00 thundering herd (a metro's worth of similar queries collapse onto shared cache entries).
> - **Availability/open-state overlay:** small hot per-item / per-restaurant boolean keys checked live and overlaid on cached results, so a cached menu never shows a sold-out item as available.
> **Cache invalidation trap:** The "restaurant paused ordering" flash — one toggle must suppress that restaurant across millions of *cached* search results instantly, and you can't bust every cached result set. Solution: keep the pause flag as a hot Redis key and filter it *at read time* as an overlay on cached results (don't bake open-state into the cached search doc). The cache holds the slow-changing catalog; the fast-changing state (open/paused/sold-out/ETA) is a thin live overlay. Mixing them is the classic mistake.

Q16. Cost at scale — where does the money go and how do you cut it without hurting delivery time?
> **Expected answer:** Biggest cost is **delivery-partner payouts** (the physical fleet) — software-side, batching orders (2–3 per trip) is the single biggest lever, directly cutting cost-per-order by improving orders-per-rider-hour; the whole VRP engine exists to reduce this. Infra costs: the search/ES cluster and notification fan-out. Cuts: aggressive search-result caching (thundering-herd collapse), tiered storage for order history (hot 30 days in Postgres, archive to S3/Parquet for analytics), downsample rider-location history, batch non-urgent notifications, spot instances for stateless search/assignment tiers, and precompute ETA/prep-time features offline. The discipline: optimize *batching quality* first (it moves the dominant real-world cost) and *search caching* second (it moves the dominant infra cost); don't micro-optimize order-DB storage, it's cheap by comparison.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Explain how you'd model food-prep-time prediction as the coupling variable between kitchen and dispatch, and why a wrong prediction is asymmetric in cost. *(Expected: regression on restaurant id, item count/complexity, hour-of-day, live kitchen backlog, weather; the cost is asymmetric — under-predict ready_time and the rider idles at the counter wasting supply; over-predict and the food sits cooling and customer NPS drops. Tune the loss to be asymmetric, and feed live kitchen signals, e.g., recent completion rate, back into it.)*

**H2.** Multi-tenancy and marketplace fairness: how do you keep a large chain (McDonald's) from starving small restaurants in search ranking, and prevent a noisy-neighbor restaurant's traffic from degrading others on a shared shard? *(Expected: ranking blends relevance + geo + ETA + a fairness/diversity term rather than pure popularity; per-restaurant rate limits and shard-level isolation/quotas so one viral restaurant's read storm is bulk-headed; separate the hot chain onto its own cache namespace.)*

**H3.** Deploy a new ranking/assignment algorithm across 500 cities without a bad model tanking delivery times region-wide. *(Expected: feature-flag per city/zone, shadow mode (log new model's assignment vs actual), then canary one city, watch delivery-time p95, cancel rate, rider-idle, orders-per-trip; auto-rollback on regression. Assignment is stateless so blue-green the workers; the order/rider stores stay put.)*

**H4.** What do you instrument to catch a delivery-time SLA breach *before* customers complain? Name the metrics and a leading indicator. *(Expected: order-to-delivery p50/p95 per zone, ready-to-pickup wait (rider late or early), assignment latency, "no rider found" rate, kitchen-accept latency, batching ratio (orders/trip), prep-time prediction error. Leading indicator: rising ready-to-pickup wait or "no rider found" in a zone signals rider supply collapse ~10 min before delivery SLA breaches.)*

**H5.** You launched with synchronous order placement (one big transaction: create order + charge + notify restaurant + assign rider). It's fragile. Migrate to the saga/event-driven model without downtime. *(Expected: strangler-fig — introduce Kafka order-events alongside the monolith, move one stage at a time (start with notifications, then assignment, then payment capture) behind flags to consume events, verify parity with the sync path in shadow, then flip stages to async and remove the synchronous coupling last; keep compensating transactions defined before you decouple so a failed stage can roll back. Never big-bang a payment path.)*

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Treating it as "Uber for food" — they copy ride-matching and forget the *food-ready timing coupling* and the *three-sided* state machine. Assignment must be timed to prep-time, not order-time.
2. Modeling order placement as one synchronous transaction instead of a long-running **saga** with compensating actions (restaurant reject, no-rider, cancel-after-cook).
3. Baking fast-changing state (open/paused/sold-out/ETA) into cached search documents, then being unable to invalidate the "restaurant paused" flash across millions of cached results. It must be a live overlay.

**The one insight that makes an answer truly impressive:**
Batching as capacitated VRP with time windows, solved with cheapest-insertion + local search under an SLA constraint — and understanding that batching is simultaneously the biggest cost lever and the biggest SLA risk. The candidate who says "I'd only batch when route overlap is high enough that no order's time window is violated, and I'd time dispatch to predicted food-ready, not order-placement" is operating at staff level.

**Suggested follow-up reading:**
- DoorDash Engineering: "Next-Generation Optimization for Dasher Dispatch" and Swiggy Engineering blog on "Hyperlocal" search & assignment.
- Literature on VRPTW (Vehicle Routing Problem with Time Windows) heuristics — Solomon insertion, 2-opt/Or-opt local search.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
