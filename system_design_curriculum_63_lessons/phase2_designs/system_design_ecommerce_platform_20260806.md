# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 59 of 63 — Phase 2: System Design Track (Design 31 of 35)
**Topic:** E-Commerce Platform (Amazon — catalog, cart, orders)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
An e-commerce platform is deceptively simple to state ("browse, add to cart, buy") and brutally hard to build, because it fuses three systems with contradictory requirements: a read-heavy catalog that wants eventual consistency and aggressive caching, a session-scoped cart that wants availability over consistency, and an order/payment path that demands exactly-once semantics and durable, auditable state. Amazon, Flipkart, Shopify and Alibaba all converged on similar answers — separate the browse plane from the buy plane, treat inventory as the contended resource, and make checkout idempotent — but the details (how you reserve stock, how you avoid overselling during a flash sale, how you keep the catalog fresh) are where interviews live. What makes it genuinely hard is the money: a lost cart is an annoyance, a double-charge or an oversell during Prime Day is a headline.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**CAP / ACID / PACELC (taught in Phase 1, Lesson 2):** The catalog and cart tolerate eventual consistency (AP), while orders, payments and inventory decrements need strong consistency (CP) with ACID transactions. PACELC reminds you that even when there's no partition, you pay a latency cost for consistency — so you deliberately push the strongly-consistent surface as small as possible (just the inventory reservation and the order write) and let everything else be eventual. In today's design this is the single most important dividing line: the "browse plane" is AP, the "buy plane" is CP.

**SQL / NoSQL / B-tree vs LSM (taught in Phase 1, Lesson 23):** Orders and inventory live in a relational store (Postgres/Aurora/DynamoDB with transactions) because you need multi-row ACID and secondary indexes on B-trees for point lookups by order_id and user_id. The product catalog, being write-rare and read-massive, fits a document store (DynamoDB, MongoDB) or a denormalized read model. LSM-tree stores absorb the high write throughput of the event/clickstream firehose. Today you'll pick a different engine per subsystem — there is no single database.

**Caches / Redis / Memcached (taught in Phase 1, Lesson 24):** The catalog read path is fronted by a multi-tier cache: CDN for images and rendered product fragments, Redis for hot product records and price/availability, local in-process cache for the very hottest SKUs. Redis also holds the cart (as a hash per user) and distributed locks/reservations. Cache invalidation on price and inventory changes is the hard part we'll return to in Q15.

**Kafka / event-driven (taught in Phase 1, Lesson 21):** Order placement fans out to a dozen downstream consumers — payment, inventory, fulfillment, email, analytics, recommendations. An event log (Kafka) with the order as an immutable event decouples the synchronous checkout from the asynchronous fulfillment, and gives you replay for rebuilding read models. The outbox pattern bridges the ACID order write and the Kafka publish so you never lose an event.

**Idempotency & rate limiting (taught in Phase 1, Lessons 11 & 12):** Every mutating checkout call carries an idempotency key so a client retry after a timeout doesn't create a second order or a second charge. Rate limiting protects add-to-cart and checkout from bots and inventory-hoarding scripts during drops. In today's design the idempotency key is the backbone of exactly-once order creation.

**Consistent hashing (taught in Phase 1, Lesson 19):** Used to shard the cart store and the session store across Redis nodes, and to shard the orders table by user_id, so adding capacity during peak reshuffles a minimal fraction of keys. We'll apply it directly in Q14.

**Microservices / API gateway / DDD (taught in Phase 1, Lesson 9):** The natural bounded contexts — Catalog, Search, Cart, Inventory, Order, Payment, Fulfillment — become services with their own datastores. An API gateway does auth, rate limiting and request routing. DDD's aggregate boundary (the Order aggregate owns its line items) tells you where transactions must be atomic.

> **Recap box:**
> - Browse plane = AP + cache heavy; Buy plane = CP + ACID. Keep the CP surface tiny.
> - Different datastore per bounded context — no one database wins.
> - Redis holds cart, hot catalog, and reservations.
> - Kafka + outbox decouples checkout from fulfillment and gives replay.
> - Idempotency key = exactly-once orders and charges.
> - Consistent hashing shards cart, session, and orders-by-user.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Why do you separate the "browse/catalog" path from the "checkout/order" path architecturally, instead of serving both from one service and one database?
> **What a strong answer covers:** Different consistency needs (catalog eventual/AP, orders strong/CP — Lesson 2), wildly different read:write ratios (catalog ~1000:1 reads, orders ~1:1), different scaling axes (catalog scales with CDN+cache, orders scale with DB sharding), and blast-radius isolation — a catalog cache stampede must not take down checkout. Separation lets you cache the browse plane to death and keep the buy plane small, transactional, and independently deployable.
> **Common weak answer:** "Microservices are best practice, so we split them." No reasoning about consistency or read/write asymmetry — that's cargo-culting.
> **Mentor follow-up if they answer well:** If they're separate services with separate DBs, how does the product page show live inventory ("Only 2 left")? (Answer: it doesn't read the source-of-truth inventory DB on every page load — it reads a cached, slightly-stale availability signal, and only the checkout reservation hits the authoritative count.)

Q2. Estimate the storage for the product catalog and the read QPS at Amazon-India scale. Assume 300M SKUs and 50M daily active users.
> **What a strong answer covers:** Catalog: 300M SKUs × ~5 KB core record (title, attributes, price, seller refs — images live in object storage/CDN, not the DB) ≈ 1.5 TB for the primary record; with denormalized read models, reviews, and variants, call it 5–10 TB. Read QPS: 50M DAU × ~30 product views/day ÷ 86,400 s ≈ 17K QPS average, and peak is 5–10× average ≈ 100–170K QPS, almost all served from cache/CDN (>95% hit rate) so origin DB sees maybe 5–10K QPS. Images: 300M × ~5 images × 200 KB ≈ 300 TB in object storage, fully CDN-fronted.
> **Mentor follow-up:** During a flash sale one SKU gets 500K views in a minute — that's ~8K QPS on a single key. How do you serve it? (Local in-process cache + request coalescing/single-flight so only one origin fetch happens per node per TTL window.)

Q3. Where do you store the shopping cart — the orders SQL database, a NoSQL store, or Redis — and why?
> **What a strong answer covers:** Redis (with async persistence to a durable NoSQL store like DynamoDB for logged-in users). Cart is high-write, session-scoped, tolerant of eventual consistency, and must be fast and always-available — you never want "add to cart" to fail. Storing it in the orders SQL DB pollutes a precious transactional resource with churny writes. Key design: `cart:{user_id}` as a Redis hash of `{sku -> qty}`, TTL for guest carts, write-behind to DynamoDB for durability across devices.
> **Red flag answer:** "Put the cart in the orders table as pending orders." That couples an ephemeral, high-churn object to your ACID money path and creates junk rows you must garbage-collect; it also makes the cart unavailable if the orders DB is under load.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the end-to-end architecture for browse → add-to-cart → checkout → order confirmation. Draw it.
> **Key components expected:** CDN, API gateway, Catalog service + read replicas/cache, Search service (Elasticsearch), Cart service (Redis), Inventory service (strongly consistent store), Order service (ACID DB + outbox), Payment service (external PSP + idempotency), Kafka bus, Fulfillment/warehouse consumers, Notification service.
> **Architecture diagram (text):**
```
                         ┌──────────── CDN (images, static, product fragments) ───────────┐
                         │                                                                │
  Client ── HTTPS ──► API Gateway (auth/JWT, rate-limit, routing)                         │
                         │                                                                │
      ┌──────────────────┼───────────────────┬──────────────────┬─────────────────┐      │
      ▼                  ▼                   ▼                  ▼                 ▼      │
  Catalog svc        Search svc          Cart svc         Order svc          Payment svc │
  (read model)    (Elasticsearch)     (Redis + DDB)   (Aurora/DDB ACID)   (idempotent →  │
      │                                     │            │  + Outbox tbl      Stripe/PSP) │
      ▼                                     ▼            ▼                                │
  Redis (hot SKUs) ◄── CDC/invalidation  Redis cart   Inventory svc (CP store, reserve)  │
      │                                                 │                                 │
  Catalog DB (source of truth) ── CDC ──► Kafka ◄───────┘ (order.created, inv.reserved)   │
                                            │                                             │
        ┌───────────────────┬──────────────┼──────────────────┬──────────────────┐       │
        ▼                   ▼              ▼                  ▼                  ▼         │
   Fulfillment/WMS     Notification     Analytics       Recommendation      Search index  │
     consumer            consumer        (warehouse)      feature pipeline    updater      │
```
> **What separates SDE2 from SDE3 here:** An SDE2 draws the boxes. An SDE3 explains that the *only* synchronous, strongly-consistent steps in checkout are (1) reserve inventory and (2) write the order row in the same transaction (or saga), and that everything else — payment capture confirmation, fulfillment, email, index updates — is asynchronous off Kafka. The SDE3 also names the outbox pattern to bridge the order DB commit and the Kafka publish atomically, and notes that the CDN caches *rendered product fragments*, not just images, to cut origin load by another order of magnitude.

Q5. Trace a single "Place Order" click end-to-end. What happens, in order, and where can it fail?
> **Expected trace:**
> 1. Client POSTs `/checkout` with cart snapshot + idempotency key + payment token.
> 2. Gateway authenticates (JWT), rate-limits, routes to Order service.
> 3. Order service checks the idempotency store: if key seen, return the prior result (exactly-once).
> 4. Begin saga/transaction: call Inventory service to **reserve** each line item (decrement available, increment reserved) with a reservation TTL.
> 5. If any reservation fails → release the rest, return "out of stock," no order created.
> 6. Persist the Order row in status `PENDING_PAYMENT` + write `order.created` to the outbox table **in the same DB transaction**.
> 7. Commit. A relay publishes the outbox event to Kafka.
> 8. Payment service captures the charge via PSP with the same idempotency key.
> 9. On payment success → Order → `CONFIRMED`, reservation converted to committed decrement; on failure → compensate: release reservation, Order → `PAYMENT_FAILED`.
> 10. Kafka fans out to fulfillment, email, analytics.
> **Tricky part:** The window between "order committed as PENDING_PAYMENT" and "payment confirmed." Candidates get vague about who owns the reservation TTL and what happens if payment succeeds at the PSP but the confirmation is lost (network drop). Answer: reconcile via the idempotency key — re-query the PSP by key; never re-charge. The reservation TTL is the safety net that releases inventory if the order never confirms.

Q6. Design the key APIs for cart and checkout. What must be idempotent and paginated?
> **Expected API design:**
> - `GET /catalog/products?category=&cursor=&limit=` → cursor-based pagination (not offset — offset is O(n) deep and shifts under inserts).
> - `POST /cart/items` `{sku, qty}` → upsert; naturally idempotent per (user, sku) since it sets absolute qty, not delta.
> - `DELETE /cart/items/{sku}`.
> - `POST /orders` with header `Idempotency-Key: <uuid>` `{cart_id, address_id, payment_token}` → **must be idempotent**; returns 201 with order_id, or the *same* order on retry.
> - `GET /orders/{id}` and `GET /orders?cursor=&limit=` for history.
> **What to push on:** Versioning (`/v1/`, or content negotiation) so you can evolve the order schema; idempotency-key TTL (24–72h) and storage (Redis/DDB with the response cached against the key); pagination via opaque cursors encoding `(sort_key, last_id)` so results are stable under concurrent writes; and price integrity — the server re-prices at checkout from the authoritative price service, never trusting the client-sent price.

---

### Data Modeling (Medium–Hard)
Q7. Design the core schema for products (with variants), inventory, and orders.
> **Expected schema:**
```sql
-- Catalog (read-optimized; source-of-truth may be a document store)
CREATE TABLE products (
  product_id   BIGINT PRIMARY KEY,
  title        TEXT,
  brand_id     BIGINT,
  category_id  BIGINT,
  attributes   JSONB,          -- flexible per-category attributes
  created_at   TIMESTAMPTZ
);
CREATE TABLE product_variants (       -- a SKU is a buyable variant
  sku          BIGINT PRIMARY KEY,
  product_id   BIGINT REFERENCES products,
  variant_attrs JSONB,               -- {size:'M', color:'blue'}
  price_cents  INT,
  seller_id    BIGINT
);

-- Inventory (strongly consistent, per SKU per warehouse)
CREATE TABLE inventory (
  sku          BIGINT,
  warehouse_id INT,
  available    INT NOT NULL,         -- CHECK (available >= 0)
  reserved     INT NOT NULL DEFAULT 0,
  version      BIGINT NOT NULL,      -- optimistic concurrency
  PRIMARY KEY (sku, warehouse_id)
);

-- Orders (ACID aggregate: order + lines committed together)
CREATE TABLE orders (
  order_id     UUID PRIMARY KEY,
  user_id      BIGINT NOT NULL,
  status       TEXT NOT NULL,        -- PENDING_PAYMENT|CONFIRMED|SHIPPED|CANCELLED
  total_cents  INT NOT NULL,
  idempotency_key UUID UNIQUE,       -- exactly-once guard
  created_at   TIMESTAMPTZ
);
CREATE TABLE order_items (
  order_id     UUID REFERENCES orders,
  sku          BIGINT,
  qty          INT,
  unit_price_cents INT,              -- price captured at purchase time
  PRIMARY KEY (order_id, sku)
);
CREATE TABLE outbox (                 -- transactional outbox
  id BIGSERIAL PRIMARY KEY, aggregate_id UUID, type TEXT,
  payload JSONB, published BOOLEAN DEFAULT FALSE, created_at TIMESTAMPTZ
);
```
> **Index choices and why:** `orders(user_id, created_at DESC)` for order history (B-tree, Lesson 23); unique index on `idempotency_key` so a duplicate insert *fails at the DB* rather than relying on app logic; `inventory` PK `(sku, warehouse_id)` for point reservation lookups; `product_variants(product_id)` to load all variants of a product. Store `unit_price_cents` on the order line so a later price change never rewrites history.
> **Partitioning key and why:** Orders partition by `user_id` (hash) — a user's history stays co-located, and reads (order history) never cross-shard. Inventory partitions by `sku` — reservations for a SKU serialize on one shard, which is exactly what you want for consistency. Catalog partitions by `product_id`.

Q8. A user opens order history: last 20 orders, newest first, for a user with 4,000 orders. How do you serve it efficiently?
> **Expected answer:** Covered by the composite index `orders(user_id, created_at DESC)` — the DB seeks directly to the user's newest orders; **keyset/seek pagination** using `WHERE user_id = ? AND created_at < ? ORDER BY created_at DESC LIMIT 20`, passing the last row's `created_at`+`order_id` as the cursor. Cache page 1 in Redis (`orderhist:{user}:p1`) since it's the overwhelmingly common read, invalidated on new order. Line items are loaded lazily only when an order is expanded.
> **Trap:** `OFFSET 3980 LIMIT 20` — the DB scans and discards 3,980 rows every time; deep offsets are O(offset) and get slower the older the page. It also double-counts or skips rows when new orders arrive between page loads. Keyset pagination is O(log n) and stable.

Q9. Inventory and catalog have opposite consistency needs. Where do you accept eventual consistency and where do you demand strong, and what's the concrete failure if you get it wrong?
> **Expected answer:** Catalog/price display and the "In stock" badge are eventually consistent — served from cache with seconds-of-staleness, which is fine because the *authoritative* check happens at reservation time. Inventory decrement at checkout is strongly consistent (single-shard, serialized on `sku`) with a `CHECK(available >= 0)` invariant. Order creation is strongly consistent (ACID) so you never have an order without a corresponding inventory reservation. This is textbook PACELC (Lesson 2): keep the CP surface to reservation+order only.
> **Mentor pushback:** "Flash sale, 100 units, 50,000 people click buy in the same second on the same SKU — your eventually-consistent 'In stock' badge said yes to all of them. What actually happens?" Expected: 100 reservations succeed at the single-shard authoritative counter (serialized via optimistic version or `SELECT ... FOR UPDATE`), the other 49,900 get "out of stock" at checkout — the badge being stale is *acceptable UX*, overselling is *not*. If they oversold, they let the badge be authoritative instead of the reservation.

---

### Low-Level Design (Hard)
Q10. Design the inventory reservation system so you never oversell, even at 50K concurrent checkouts on one SKU.
> **Problem statement:** Multiple concurrent checkouts decrement a shared, small `available` count; the invariant is `available >= 0` and total sold ≤ stock, with high throughput and low latency, and reservations that auto-expire if checkout is abandoned.
> **Naive solution:** Read `available`, check `> 0`, write `available - qty` — a read-modify-write with no concurrency control.
> **Why naive fails at scale:** Classic lost-update race — two transactions both read `available = 1`, both pass the check, both write `0`, two orders sold, one unit oversold. At 50K concurrent, you oversell heavily.
> **Expected optimal approach:** Make the decrement atomic and serialized per SKU. Two production-grade options: (a) **Conditional atomic decrement in the DB**: `UPDATE inventory SET available = available - :qty, reserved = reserved + :qty, version = version + 1 WHERE sku = :sku AND warehouse_id = :w AND available >= :qty` — the `available >= :qty` guard makes it a compare-and-set; 0 rows updated ⇒ out of stock. (b) **Redis atomic reservation** for hot SKUs: a Lua script that checks-and-decrements a counter atomically (single-threaded Redis serializes), with the durable DB reconciled asynchronously. Reservations carry a TTL; a sweeper releases expired ones back to `available`. For extreme hot keys, shard the counter into N sub-counters ("inventory splitting") to spread contention, summing for display.
> **Pseudo-code or class diagram:**
```
reserve(sku, warehouse, qty, reservation_id, ttl):
    # atomic, serialized per sku
    rows = UPDATE inventory
           SET available = available - qty,
               reserved  = reserved  + qty,
               version   = version + 1
           WHERE sku=sku AND warehouse_id=warehouse AND available >= qty
    if rows == 0: return OUT_OF_STOCK
    INSERT reservation(reservation_id, sku, qty, expires_at = now()+ttl)
    return RESERVED

confirm(reservation_id):   # on payment success
    UPDATE inventory SET reserved = reserved - qty WHERE ...   # committed sale
    DELETE reservation WHERE reservation_id=...

# background sweeper (every few seconds)
for r in reservations WHERE expires_at < now():
    UPDATE inventory SET available = available + r.qty,
                         reserved  = reserved  - r.qty WHERE sku=r.sku
    DELETE reservation WHERE reservation_id = r.id
```

Q11. Two devices for the same logged-in user check out the same cart simultaneously. Or: a client times out on `POST /orders` and retries. How do you prevent a duplicate order/charge?
> **Scenario:** Same idempotency key (or same user+cart) arrives twice within milliseconds; a naive service creates two orders and charges twice.
> **Expected fix:** Idempotency key is the primary guard (Lesson 12). The unique DB constraint on `orders.idempotency_key` means the second insert fails with a unique-violation, which the service catches and translates into "return the existing order." Store the *response* against the key in Redis/DDB with a TTL so concurrent retries that arrive before the first commit block on a lock keyed by the idempotency key and then read the cached result. The payment call carries the same key to the PSP, so even the charge is deduplicated at the provider. For the two-device race specifically, the reservation step also serializes on the SKU, so only one order can hold the stock.
> **Follow-up — what if the lock holder dies mid-checkout?** The idempotency lock has a TTL (lease), so a dead holder's lock expires and a retry can proceed; the reservation also has a TTL and self-releases. The order is either committed (PENDING_PAYMENT persisted) or not — if it committed, retries return it; if it didn't, the retry starts fresh. No orphaned charge because payment only happens after the order row exists and carries the key.

Q12. Payment succeeds at the PSP but your service crashes before recording confirmation. On restart, what's the state and how do you recover without double-charging or shipping an unpaid order?
> **Scenario:** Order is `PENDING_PAYMENT`, money left the customer, your DB doesn't know.
> **Expected handling:** A reconciliation job (and/or PSP webhook) re-queries the PSP *by idempotency key* for every stale `PENDING_PAYMENT` order older than N minutes. If the PSP reports captured → transition to `CONFIRMED`, emit `order.confirmed`. If not captured and reservation expired → release inventory, mark `PAYMENT_FAILED`/`CANCELLED`. The PSP webhook is itself idempotent (dedupe on event id). The Kafka fanout to fulfillment is triggered only by `order.confirmed`, never by `PENDING_PAYMENT`, so nothing ships until money is confirmed. DLQ captures events that repeatedly fail downstream so fulfillment poison messages don't block the topic. This is the saga's compensating-action design applied to the money path.

---

### Scaling to 10x / 100x (Hard)
Q13. You're at 100K browse QPS and 2K orders/sec. Push to 100×. What breaks first?
> **Expected answer:** The *browse* plane scales almost for free with more cache/CDN — it breaks last. The first thing to break is **hot-key inventory contention** on trending SKUs: a single SKU's reservation is serialized on one shard, so a viral product becomes a single-writer bottleneck no matter how many app servers you add. Second is the **orders database write path** — 2K→200K orders/sec of ACID writes with the outbox exceeds a single primary; you need sharding by user_id and possibly a write-optimized store. Third is **payment PSP rate limits** and the reconciliation backlog.
> **Numbers to ground the answer:** 200K orders/sec × ~4 line items × (order write + reservation + outbox) ≈ 2M+ transactional row ops/sec — far past a single Postgres primary (~tens of thousands of write txns/sec). Hot SKU: even at 10K reservations/sec on one shard you're latency-bound at a few ms each. Browse at 10M QPS is fine at 97% cache hit → 300K origin QPS spread across replicas.

Q14. Apply consistent hashing (Lesson 19): how do you shard orders, cart, and inventory, and what's the hot-spot risk?
> **Expected sharding strategy:** Orders and cart shard by **hash(user_id)** with consistent hashing + virtual nodes, so a user's data is co-located and adding a shard remaps only ~1/N of keys. Inventory shards by **hash(sku)** so reservations for a SKU serialize on one node. Choosing user_id for orders means order-history reads are single-shard; choosing sku for inventory means the contention point is naturally partitioned per product.
> **Hot spot problem:** A viral SKU hashes to one inventory shard and melts it. Detect via per-key QPS metrics / heavy-hitter sketch (Count-Min). Fix with **counter sharding**: split that SKU's stock into K logical sub-counters (`sku:shard0..shardK`), route each reservation to a random sub-counter, and rebalance/aggregate for display — turning one hot key into K warm keys. Same trick for a celebrity seller's SKUs. For orders, a mega-seller isn't a hot shard because we shard by *buyer*, not seller.

Q15. Design the caching layers for the catalog and the cache-invalidation strategy when price or inventory changes.
> **Expected layered cache design:** L0 = CDN caches images and rendered product-card fragments (TTL minutes, purge on publish). L1 = in-process LRU on each app node for the hottest few thousand SKUs (Lesson 4/24), with single-flight to prevent stampede. L2 = Redis holding the canonical hot product record and a separate short-TTL availability key. Origin catalog DB is last resort. Read path checks L1→L2→DB, back-filling on the way up.
> **Cache invalidation trap:** Price and availability change far more often than product metadata, so you must cache them *separately* from the immutable product body — otherwise every price tick invalidates the whole 5 KB record. Use CDC (Lesson 21) from the catalog/price DB to publish `price.changed`/`inventory.changed` events that surgically evict just the price/availability keys, not the product body. Never cache authoritative inventory for the *reservation* — display availability can be stale (short TTL, e.g. 5–30s), but the checkout reservation always hits the source of truth. The trap candidates fall into: caching the "in stock" flag with a long TTL and then overselling because they trusted the cache at checkout.

Q16. Cost and efficiency at 100× scale — where's the money going and how do you cut it?
> **Expected answer:** Biggest line items: object storage + CDN egress for images (300 TB+ and growing), the transactional order DB, and Kafka retention. Cuts: (1) Image tiering — serve WebP/AVIF, responsive sizes, aggressive CDN caching so origin egress collapses; move cold product images to cheaper storage tiers. (2) Order data lifecycle — keep hot orders (last 90 days) in the fast ACID store, archive older orders to columnar/object storage (Parquet on S3) queryable on demand; 90% of order reads are recent. (3) Batch the outbox relay and analytics writes rather than per-event round trips. (4) Compress Kafka with a short hot-retention and tiered storage for replay. (5) Reserve/spot instances for the stateless browse fleet, right-sized. The principle: money follows the *buy* plane's durability and the *image* plane's egress — optimize those two hardest.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Explain the transactional outbox precisely: why can't you just "write the order, then publish to Kafka"? Walk through the dual-write failure and how the outbox + a CDC relay (Debezium) gives you atomic commit-and-publish with at-least-once delivery, and how consumers dedupe to reach effectively-exactly-once.

**H2.** Multi-region and data residency: an EU customer's order and PII must stay in-region (GDPR), but the catalog is global. How do you partition the buy plane by region while keeping one logical catalog, and how do you handle a customer who travels? Discuss regional order shards, a global product read model, and the "right to be forgotten" cascade across Kafka-derived read models.

**H3.** Deploy the Order service during Prime Day with zero downtime and a schema change to the `orders` table (adding a column). Walk through the expand/contract migration, backward-compatible reads/writes, blue-green vs canary for a stateful service, and how you'd roll back if error rate spikes 1% post-deploy.

**H4.** What do you instrument (Lesson 10) so that during a flash sale you *know* whether you're overselling, dropping carts, or losing orders? Name the specific metrics (reservation success rate, oversell counter = must be 0, checkout p99, PSP capture latency, outbox lag, Kafka consumer lag) and the SLO-based alerts.

**H5.** You launched with the cart in the orders SQL DB and it's now the checkout bottleneck. Design the live migration to Redis+DynamoDB without losing a single cart or taking downtime — dual-write, backfill, shadow-read verification, cutover, and the rollback plan.

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Treating inventory display and inventory reservation as the same thing — they cache the "in stock" flag and then oversell because they let the cache be authoritative at checkout. Display is eventual; reservation is strong. Always.
2. Forgetting the dual-write problem between the order DB and Kafka. They "save the order then publish an event," which silently loses events on a crash between the two. The outbox pattern is the expected, named answer.
3. Making checkout a big distributed transaction across payment, inventory, and shipping. Real systems use a saga with compensating actions and keep the truly-atomic part tiny (order row + reservation), reconciling payment asynchronously.

**The one insight that makes an answer truly impressive:**
The idempotency key is not just a duplicate-order guard — it's the *end-to-end correlation key* that threads through the client retry, the order row, the payment capture at the PSP, and the reconciliation job, giving you effectively-exactly-once semantics across three independently-failing systems without a single distributed lock. Candidates who see the idempotency key as one concept spanning the whole money path, rather than a local dedupe trick, are operating at SDE3+.

**Suggested follow-up reading:**
- Amazon's Dynamo paper (2007) — availability-first design and the shopping-cart anti-entropy example.
- "Transactional Outbox" and "Saga" chapters in Chris Richardson's *Microservices Patterns*; and Stripe's engineering blog on idempotency keys.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
