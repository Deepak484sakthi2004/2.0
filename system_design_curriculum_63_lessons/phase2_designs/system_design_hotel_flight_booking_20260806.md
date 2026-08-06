# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 57 of 63 — Phase 2: System Design Track (Design 29 of 35)
**Topic:** Hotel / Flight Booking System — Inventory + Dynamic Pricing
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
A hotel/flight booking engine (Booking.com, Expedia, MakeMyTrip, Amadeus/Sabre GDS behind them) is a *low-contention-per-unit, high-fan-in-across-inventory* problem: unlike BookMyShow where 500K people fight over seat A12, here millions of searches fan out across millions of distinct properties/flights, most of which are *not* hot — but the inventory is **fungible up to a count** (a hotel has "8 Deluxe rooms left for these dates," an airline has "3 seats left in fare class Y"), the price changes by the second, and the *actual sellable unit is a date-range or a leg*, not a static row. The hard parts: modeling availability over continuous date ranges (a room booked Mar 3–5 blocks Mar 4 but not Mar 6), holding inventory you don't own (it lives in a third-party GDS/channel manager), and never overselling while sitting behind a slow, rate-limited external supplier. This is where "count-based inventory + dynamic pricing + eventual reconciliation with an external system of record" all collide.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**ACID & Isolation Levels (taught in Phase 1, Lesson 2):** Atomicity/consistency/isolation/durability; isolation levels (Read Committed → Serializable) and the anomalies each prevents. Overselling is a **phantom/write-skew** problem: two transactions both read "2 rooms left," both decrement, and you sold 3. The decrement of a count under contention must be serialized — done today with a conditional atomic `UPDATE ... WHERE available >= n` or `SELECT ... FOR UPDATE` on the inventory bucket.

**Optimistic vs Pessimistic Concurrency (taught in Phase 1, Lessons 2 & 22):** Pessimistic = lock the inventory row; optimistic = version/CAS. Because per-property contention is usually *low* (unlike a flash sale), **optimistic concurrency with a conditional decrement** is the default; pessimistic locks are reserved for the rare hot property (last room in a popular city on a holiday). Today: `UPDATE inventory SET available=available-1 WHERE available>=1` returning rows-affected.

**Redis / Distributed Locks & TTL (taught in Phase 1, Lesson 24):** Atomic ops, TTL auto-expiry, Lua for multi-key atomicity. Used for the temporary **price+inventory hold** during checkout (10–20 min "we're holding this room while you enter card details") and as the hot cache for availability. The hold is a TTL key so an abandoned checkout self-heals.

**Caching (taught in Phase 1, Lesson 24):** Read-heavy: search dwarfs booking (~1000:1). Search results, price quotes, and availability calendars are cached aggressively; the subtlety is caching a *price quote* that must remain honored at book time even as the live price moved.

**CAP / PACELC (taught in Phase 1, Lesson 2):** **Search/browse is AP** (stale availability by a few seconds is fine and unavoidable). **Booking/decrement is CP** — during a partition you refuse to confirm rather than risk an oversell. The system deliberately runs two consistency regimes.

**Message Queues / Kafka (taught in Phase 1, Lesson 21):** Decouple booking confirmation from downstream: ticketing/PNR generation, supplier confirmation, email/invoice, loyalty points, and — critically — **reconciliation** with the external GDS/channel manager runs off Kafka.

**Consistent Hashing (taught in Phase 1, Lesson 19):** Shard inventory by property_id / flight_id so a booking touches one shard, and hot properties can be isolated.

**Search & Indexing / Elasticsearch (foundations in Lessons 22–23):** Search is a geo + faceted-filter problem (city, dates, price, stars, amenities). An inverted-index/geo engine (Elasticsearch) serves search; the transactional store never serves free-text/geo search.

> **Recap box:**
> - ACID/isolation (L2): oversell = write-skew on a count; serialize the decrement.
> - Optimistic vs pessimistic (L2/L22): conditional `available>=n` decrement by default; pessimistic only for hot units.
> - Redis+TTL (L24): checkout hold = TTL key, self-healing; also hot availability cache.
> - Caching (L24): search 1000:1 over booking; honor the *quoted* price at book time.
> - CAP (L2): search AP, booking CP.
> - Kafka (L21): supplier confirmation + reconciliation async.
> - Consistent hashing (L19): shard by property_id/flight_id.
> - Search (L22/23): geo + faceted inverted index, separate from the OLTP store.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)

Q1. Contrast the inventory model here with BookMyShow's seat booking (Lesson 56). Why is a room-night or a fare-class seat *not* modeled as a unique row per unit?
> **What a strong answer covers:** BookMyShow inventory is **non-fungible and finite per identity** — seat A12 is a specific thing exactly one person buys. Hotel/flight inventory is **fungible up to a count over a dimension**: "8 Deluxe rooms available for the night of Mar 4" — you don't care *which* physical room; you care that the count doesn't go below zero. So the unit of inventory is a **(property, room_type, date) → available_count** bucket for hotels, and **(flight, date, fare_class) → seats_left** for flights. You decrement a counter, you don't flip a specific row's status. Also, hotel inventory is over a *continuous date range* — booking Mar 3–5 decrements the buckets for Mar 3 and Mar 4 (checkout day Mar 5 is free again), so a single booking touches multiple date buckets atomically.
> **Common weak answer:** "One row per room, mark it sold." That explodes for date ranges (you'd need a row per room per night) and misses that the customer buys a *room type for a range*, not a specific room — room assignment happens at check-in, not at booking.
> **Mentor follow-up:** A guest books Mar 3–5 and another books Mar 4–6 in the same hotel with 1 Deluxe room. Which date buckets collide, and what's the exact decrement each transaction must do atomically?

Q2. Estimate the read/write asymmetry. A metasearch site does 5,000 searches/sec at peak; what booking rate should you design the transactional core for, and what does that imply?
> **What a strong answer covers:** Look-to-book ratios in travel are brutal — commonly **200:1 to 1000:1** (people compare obsessively, abandon, re-search). At 5,000 searches/sec and a ~500:1 ratio, that's **~10 bookings/sec** actually hitting the CP transactional core. Each search, though, fans out: a single "flights BLR→SFO, flexible dates" query can expand to dozens of date/route combinations and hit multiple suppliers. So **search is the scale problem (caching, Elasticsearch, fan-out), booking is the correctness problem (small volume, must be exactly right).** Sizing: the OLTP inventory DB handles tens of writes/sec — trivially — but the search/pricing tier handles millions of price computations/sec and is where all the machinery goes.
> **Mentor follow-up:** If booking is only ~10/sec, why is overselling still a real, frequent production incident at these companies?

Q3. The price a user saw in search results is ₹8,400. At checkout 4 minutes later, the live price is ₹9,100. What do you charge, and how do you design for it?
> **What a strong answer covers:** You **honor the quoted price** for the life of the quote — you issue a **price quote token** (a signed, TTL'd object: `{fare_key, price, currency, expires_at, inventory_snapshot}`) at search/select time, cache it, and at book time you validate the token and charge the quoted price if it hasn't expired (typically 5–15 min). If it expired, you re-quote and show the new price *before* charging (never silently charge more). The quote is a promise with an expiry; dynamic pricing changing underneath is expected. For flights, the GDS/airline may *reject* the fare at book time even within the window ("fare no longer available") — so the quote is best-effort and the book call must handle a re-price/rejection gracefully.
> **Red flag answer:** "Charge whatever the live price is at checkout." Silent price increases are a trust and often a legal/regulatory problem; and "always charge the search price forever" is the opposite failure — you'd sell at stale, wrong prices.

---

### High-Level Design (Medium)

Q4. Design the end-to-end architecture for a hotel+flight booking platform that aggregates its own inventory *and* third-party suppliers (GDS/channel managers). Draw it.
> **Key components expected:** Client, CDN, API GW, **Search service** (Elasticsearch geo+facet index + result cache), **Pricing service** (dynamic pricing + quote tokens), **Availability service** (Redis hot counts + Postgres source of truth, sharded by property/flight), **Supplier-integration layer** (adapters to GDS/Amadeus/Sabre/channel managers with circuit breakers + rate limiters), **Booking/Order service** (CP, holds + confirm), **Payment service**, **Kafka** (confirmation, PNR/voucher gen, reconciliation, notify), **Reconciliation service** (keeps our availability in sync with suppliers).
> **Architecture diagram (text):**
```
 User --> [CDN] --> [API GW]
                       |
     +-----------------+----------------------+------------------+
     v                 v                       v                  v
[Search svc]      [Pricing svc]         [Availability svc]   [Booking/Order svc] (CP)
 ES geo+facet      dynamic price +       Redis hot counts      holds -> confirm
 result cache      QUOTE TOKEN(TTL)      Postgres SoT          idempotent
     |                 |                  (shard by prop/flt)        |
     |                 |                       |                     v
     |                 |                       |               [Payment svc]
     v                 v                       v                     |
                 [Supplier Integration Layer]  <-- circuit breaker, rate limit, retry
                  Amadeus/Sabre GDS, hotel channel managers (external, slow, flaky)
                       |
                    [Kafka] --> [PNR/Voucher gen] [Notify] [Reconciliation svc] [Analytics]
                                                              |
                                              polls/ingests supplier availability deltas
                                              back into Availability svc (eventual sync)
```
> **What separates SDE2 from SDE3 here:** SDE3 treats the **supplier layer as a semi-trusted, slow, eventually-consistent system of record you don't fully control** and designs the booking flow around *two-phase* confirmation (we tentatively hold → we ask the supplier → supplier is the final authority for merchant/opaque inventory) plus a reconciliation loop that continuously repairs drift. SDE2 tends to model inventory as if we own it all in our own DB, which works for *own inventory* but silently oversells *supplier* inventory because our count is always stale relative to the GDS. The insight: for merchant-model inventory you *are* the source of truth (own DB is authoritative); for agency/GDS inventory the *supplier* is authoritative and you're maintaining a cache that must reconcile.

Q5. Trace a hotel booking end-to-end for supplier (GDS) inventory, including the hold and the two-phase confirm.
> **Expected trace:**
> 1. User searches city+dates → Search svc (ES) returns candidate properties; availability + price fetched (cached, or live-called to supplier for long-tail).
> 2. User selects a room → Pricing svc returns a **quote token** (price, TTL 10 min, fare/rate key).
> 3. User clicks Book → Booking svc creates order PENDING, places a **local hold** (Redis TTL, decrements our cached count optimistically) and calls the **supplier adapter** to place a *supplier-side hold/pre-book* (validate rate + availability against the GDS — the authoritative check).
> 4. Supplier confirms rate still valid & held → Booking svc captures payment (idempotent).
> 5. On payment success → call supplier **book/commit** to get the confirmation number/PNR/voucher → mark order CONFIRMED → emit Kafka event.
> 6. Async: voucher/PNR PDF, email/invoice, loyalty, and a reconciliation event to true-up our cached count.
> **Tricky part:** Step 3–5 is a **distributed transaction across a system you don't own**. If payment succeeds but the supplier *book* call fails or times out, you've charged the customer with no confirmed room — you must run a saga: retry the supplier book idempotently; if unrecoverable, **auto-refund/void** and compensate (offer alternative). Candidates who treat the supplier call as a normal in-house service call have a money-and-inventory bug. Also: the supplier is the source of truth for GDS inventory, so your local decrement is optimistic and *must* be reconciled — never confirm to the user before the supplier commits for opaque inventory.

Q6. Design the key APIs: search, quote, hold, and confirm.
> **Expected API design:**
> - `GET /v1/search?type=hotel&city=BLR&checkin=2026-09-03&checkout=2026-09-05&guests=2&filters=...` → paginated results (cursor-based, not offset — deep pagination over millions of results). Cache-Control set; results carry a `search_id`.
> - `POST /v1/quote` body `{result_id, room_type, checkin, checkout}` → `{quote_token, price, currency, expires_at}`. The quote_token is signed and self-contained.
> - `POST /v1/holds` header `Idempotency-Key`; body `{quote_token, guest_details}` → `{hold_id, expires_at}` or `409` (inventory gone / price changed → re-quote). Places local + supplier hold.
> - `POST /v1/bookings/{hold_id}/confirm` header `Idempotency-Key`; body `{payment_token}` → `{booking_id, confirmation_number, status}`.
> - `DELETE /v1/holds/{hold_id}` — explicit release.
> **What to push on:** **Idempotency on hold and confirm** — network retries against a slow supplier are constant; a retried confirm must not create two supplier bookings (double-charge + double-room). Use the Idempotency-Key to dedupe at the booking svc *and* pass a stable client reference to the supplier so their side dedupes too. **Cursor pagination** (offset pagination over millions of hotels with live-changing availability produces duplicates/skips). **Quote token carries the price** so book time is stateless and honors the promise. Versioned APIs because supplier fare rules change.

---

### Data Modeling (Medium–Hard)

Q7. Design the core inventory + booking data model for hotels, correctly handling date ranges.
> **Expected schema:**
```sql
-- OWN-INVENTORY AVAILABILITY: one row per (property, room_type, DATE) = one night bucket
CREATE TABLE room_inventory (
  property_id   BIGINT,
  room_type_id  BIGINT,
  stay_date     DATE,                 -- a single night
  total         INT NOT NULL,
  available     INT NOT NULL,         -- decremented on booking; CHECK (available >= 0)
  base_price    NUMERIC(10,2),        -- dynamic pricing overlays this
  version       INT DEFAULT 0,
  PRIMARY KEY (property_id, room_type_id, stay_date),
  CHECK (available >= 0)
);
CREATE INDEX ON room_inventory (property_id, stay_date);   -- availability calendar reads

-- BOOKINGS
CREATE TABLE bookings (
  booking_id      UUID PRIMARY KEY,
  user_id         BIGINT,
  property_id     BIGINT,
  room_type_id    BIGINT,
  checkin         DATE, checkout DATE,   -- decrements buckets [checkin, checkout)
  rooms           INT,
  quoted_price    NUMERIC(12,2), currency CHAR(3),
  status          VARCHAR,               -- PENDING/HELD/CONFIRMED/CANCELLED/REFUNDED
  supplier        VARCHAR,               -- OWN / AMADEUS / SABRE / channel_x
  supplier_ref    VARCHAR,               -- PNR / confirmation number (nullable until confirm)
  idempotency_key TEXT UNIQUE,
  created_at      TIMESTAMPTZ
);

-- FLIGHT inventory bucket: (flight_id, dep_date, fare_class) -> seats_left
CREATE TABLE flight_inventory (
  flight_id BIGINT, dep_date DATE, fare_class CHAR(1),
  seats_total INT, seats_left INT CHECK (seats_left >= 0), price NUMERIC,
  PRIMARY KEY (flight_id, dep_date, fare_class)
);
```
> **Index choices and why:** PK `(property_id, room_type_id, stay_date)` makes each night's decrement a single-row PK hit and makes calendar reads for a property a range scan on `stay_date`. `CHECK(available >= 0)` is the *database-enforced* oversell guard — even a buggy service can't push it negative.
> **Partitioning key and why:** Shard by **property_id** (hotels) / **flight_id** (flights) via consistent hashing. A booking for one property over a date range touches multiple night buckets that all live on the *same shard* (same property_id) → the multi-night decrement is a **single-shard transaction**, no cross-shard 2PC. Never shard by stay_date — a single booking's nights would smear across shards. Hot property? Isolate its property_id to dedicated shard(s).

Q8. A user searches "hotels in Goa, Dec 24–27, 2 rooms, pool + free cancellation, under ₹10k/night, 4★+." How do you serve this efficiently?
> **Expected answer:** This is a **geo + faceted-filter + range** query — the transactional store is the wrong tool. Serve it from **Elasticsearch**: geo-shape/geo-distance for "in Goa," term filters for amenities (pool, free-cancellation), range filter for price/stars, sorted by rank/price. But ES holds *searchable metadata + approximate/cached availability*, not the authoritative live count. Flow: ES returns candidate property IDs → **availability service** overlays live counts (from Redis hot cache; DB on miss) for the specific date range → filter out sold-out → apply **dynamic pricing** → return. For "2 rooms Dec 24–27" you must check *every night bucket* has ≥2 available. Cache the ES result set by (geo+filters) with short TTL; cache availability separately with even shorter TTL since it moves.
> **Trap:** Trying to run the geo+facet search in Postgres (`WHERE amenities @> ... AND ST_DWithin(...)`) at scale — it can't sort/rank millions of properties across facets fast. Or storing live per-night availability *inside* the ES document and reindexing on every booking — the reindex rate would equal the booking rate and ES near-real-time refresh (~1s) makes it stale anyway. Separate **searchable attributes (ES, slow-changing)** from **live availability (Redis/DB, fast-changing)**.

Q9. Where do you place the CP/AP line, and give a concrete scenario where eventual consistency on availability actively hurts.
> **Expected answer:** **Search/availability display = AP** (Redis cache, ES, replicas) — showing a room as available a few seconds after it sold is acceptable and unavoidable at 1000:1 look-to-book. **The decrement at booking = CP** — the conditional `UPDATE room_inventory SET available=available-2 WHERE available>=2` on the authoritative shard, serialized, so we never go negative. For **own inventory** our DB is the authoritative CP store. For **GDS/supplier inventory** the *supplier* is authoritative and our count is a replicated cache that's *always* somewhat stale.
> **Mentor pushback:** The scenario where eventual consistency bites: **the last room on a holiday for a hot property**, where our cached count says "1 left" but the supplier already sold it via another channel (Expedia sold the same physical room). Two users on two platforms both "book" the last room — our optimistic decrement succeeds locally, but the supplier's *book* call fails for one of them ("no availability"). Fix: for hot/last-unit inventory, degrade to **synchronous supplier verification before confirming** (don't trust the cache for the last unit); accept slower checkout for the tail case. And run the reconciliation loop tighter (poll supplier deltas more frequently) for high-velocity inventory. The general rule: eventual consistency is fine except at the *boundary* (last unit), where you must fall back to the authoritative source.

---

### Low-Level Design (Hard)

Q10. The core sub-problem: **oversell prevention across a continuous date range with multi-night atomicity**, while some inventory is yours and some is a slow external supplier's. Design it.
> **Problem statement:** A booking for [checkin, checkout) must atomically decrement *every* night bucket by `rooms`, succeeding only if *all* nights have enough, and must be correct even when the true authority is an external GDS.
> **Naive solution:** Loop over nights, decrement each with its own transaction; or check availability then decrement in separate steps.
> **Why naive fails at scale:** (1) Per-night separate transactions = **partial decrement**: nights 1–2 succeed, night 3 is sold out → you've now wrongly held 2 nights and must compensate; under concurrency this leaks inventory. (2) Check-then-decrement is a classic **TOCTOU race** → two bookings both pass the check, both decrement, oversell. (3) For supplier inventory, your local decrement means nothing — the supplier may reject at book time.
> **Expected optimal approach:** For **own inventory**: one transaction, one conditional multi-row update guarded by the DB CHECK:
> - `UPDATE room_inventory SET available = available - :rooms WHERE property_id=:p AND room_type_id=:rt AND stay_date >= :checkin AND stay_date < :checkout AND available >= :rooms;`
> - Then assert **rows-affected == number_of_nights**. If not (some night lacked capacity), the transaction rolls back → all-or-nothing atomicity via a single transaction + the `available>=:rooms` predicate serializing the decrement per row. The `CHECK(available>=0)` is the last-line guard.
> For **supplier inventory**: local optimistic hold → **synchronous supplier pre-book (authoritative check)** → payment → **supplier commit** → confirm. A **saga** with compensations: if commit fails after payment, refund. Reconciliation continuously repairs the local cache.
> **Pseudo-code or class diagram:**
```
def book(property, room_type, checkin, checkout, rooms, quote):
    nights = (checkout - checkin).days
    if quote.supplier == "OWN":
        with tx():
            n = execute("""UPDATE room_inventory SET available=available-:r
                           WHERE property_id=:p AND room_type_id=:rt
                             AND stay_date>=:ci AND stay_date<:co
                             AND available>=:r""", ...)
            if n != nights:            # some night lacked capacity
                raise SoldOut()        # tx rolls back -> all-or-nothing
        return confirm_local(...)
    else:  # GDS / channel manager — supplier is source of truth
        local_hold(property, room_type, checkin, checkout, rooms)   # optimistic, TTL
        pre = supplier.prebook(quote.rate_key)      # authoritative availability+price
        if not pre.ok: release_local_hold(); raise SoldOut_or_RePrice()
        pay = payment.capture(idempotency_key)      # idempotent
        try:
            ref = supplier.commit(pre.token, client_ref=idempotency_key)  # idempotent
        except SupplierError:
            payment.refund(idempotency_key); release_local_hold(); raise
        return confirm(supplier_ref=ref)
```

Q11. Two concurrent bookings for the last 2 rooms of a hot property over overlapping ranges (A wants Mar 3–5, B wants Mar 4–6), only 1 room some nights. Walk the race and prove correctness.
> **Scenario:** Inventory: Mar 3 has 2, Mar 4 has 1, Mar 5 has 2. A books [Mar3,Mar5) → needs Mar3,Mar4. B books [Mar4,Mar6) → needs Mar4,Mar5. Both contend on **Mar 4 (only 1 available)**.
> **Expected fix:** Each runs the single guarded `UPDATE ... WHERE available>=1` over its night range. The two transactions both try to decrement the **Mar 4 row** — the DB serializes row writes (row lock / write-write conflict under the isolation level). Say A commits first: Mar 4 goes 1→0, A's rows-affected = 2 (Mar3+Mar4) = nights → A succeeds. B's update now sees Mar 4 with `available>=1` **false**, so B's Mar 4 row isn't updated → B's rows-affected = 1 (only Mar5) ≠ 2 nights → B rolls back entirely (all-or-nothing) → B gets SoldOut. Exactly one wins Mar 4; no oversell. The `CHECK(available>=0)` guarantees even a logic bug can't go negative.
> **Follow-up (what if the lock holder dies?):** For own inventory the decrement is a sub-second DB transaction — if the service dies mid-transaction, the DB rolls it back (durability/atomicity), no leaked inventory. For the *checkout hold* (Redis TTL) held during payment think-time, a dead service leaves the hold to expire via TTL and a sweeper reconciles the DB `available` back up. For supplier inventory, a dead service after `supplier.commit` but before we recorded the ref → the reconciliation loop finds the orphan supplier booking (by client_ref = idempotency_key) and either attaches it to the order or cancels it.

Q12. Own-inventory booking: payment succeeded, but the supplier `commit` call timed out (for a GDS booking) — you don't know if the room is actually booked with the airline/hotel. The customer is charged. Handle it.
> **Scenario:** Payment captured; supplier commit ambiguous (timeout); customer has money debited, no confirmation number.
> **Expected handling:** Never decide from a timeout. Make `supplier.commit` **idempotent via a stable client reference** (= our idempotency key / order id) so a retry returns the *same* booking rather than creating a second. Immediately **query the supplier's status/retrieve endpoint** using the client_ref (or wait for their async confirmation webhook) to resolve truth. Order sits in a `CONFIRMING` state (not CONFIRMED, not FAILED) — do **not** tell the user it failed. Resolution: if the supplier actually booked → attach supplier_ref, mark CONFIRMED, send voucher; if it truly failed → **auto-refund** (idempotent) and offer rebooking. Kafka + a reconciliation job drains late webhooks; a **DLQ** captures unresolvable cases for a human ops/finance queue. The invariant: **payment capture and supplier commit must both be idempotent and must converge to exactly one of {confirmed+charged, refunded+not-booked}** — never "charged but no room" as a terminal state.

---

### Scaling to 10x / 100x (Hard)

Q13. At 100x scale (a mega sale — 500K searches/sec), where does it break first?
> **Expected answer:** **Search fan-out + pricing computation** breaks first, not booking. A single flexible-date flight query fans out to dozens of route/date combos × multiple suppliers; dynamic pricing recomputes per result. At 500K searches/sec with fan-out 20–50, that's **tens of millions of price computations/sec** and, worse, **supplier API calls** — the GDS/channel managers are **rate-limited** (e.g., Amadeus/Sabre cap you at a few thousand TPS) and *cannot* be scaled by you. So the first hard wall is **the external supplier rate limit**, mitigated only by caching and by *not* live-calling suppliers for every search. Next is the **ES/search cluster** (query + geo compute). Booking (~few hundred/sec even at 100x) remains comfortable because look-to-book stays ~500:1.
> **Numbers to ground the answer:** 500K searches/sec × avg fan-out 30 = 15M candidate evaluations/sec — all must be cache-served; if even 1% miss to a supplier capped at 5K TPS, that's 150K TPS demanded against a 5K ceiling → 30x over → you *must* cache-serve ≥99.97% of availability/price. Booking at 500:1 = ~1,000 bookings/sec → trivial for a sharded OLTP store. The scaling story is entirely "protect the supplier and search tiers."

Q14. Apply consistent hashing (Lesson 19): shard inventory. Handle the hot property/route (a single sold-out-fast property on New Year's).
> **Expected sharding strategy:** Shard by **property_id (hotels) / flight_id (flights)** with virtual nodes. A booking's multi-night decrement stays single-shard (same property_id) → no distributed transaction. Search is served from ES (independently sharded by geo/route), decoupled from the OLTP shards.
> **Hot spot problem:** A single mega-demand property (a Goa resort on New Year's) or a route (BLR→GOI on Dec 31) concentrates writes/reads on one shard and, worse, on **one row (the hot night bucket)**. **Detect:** per-shard/per-row ops/sec and lock-wait time. **Fix:** (1) Pin the hot property_id to **dedicated shard(s)** (bulkhead) so it can't starve neighbors. (2) For the single-hot-row problem (last rooms on the hot night), switch that night from *optimistic decrement* to a **short pessimistic serialization** or a Redis atomic counter fronting the DB (the last-unit case is exactly where you accept slower, strictly-serialized decrements). (3) Cache read availability hard so the hot row isn't hammered by reads. Never shard by stay_date/date — you'd fragment a single booking across shards.

Q15. Design caching: search results, price quotes, and availability. What's the hardest to invalidate?
> **Expected layered cache design:**
> - **CDN:** static property/route metadata, images, descriptions, geo tiles.
> - **Search result cache (Redis):** keyed by normalized (geo+filters+date) → candidate IDs; short TTL (30–120s) — search results tolerate staleness.
> - **Availability cache (Redis):** per (property, room_type, date) count, or per (flight,date,fare) seats-left; very short TTL + write-through on booking; the hot read path.
> - **Price quote cache:** the issued quote token (price honored for its TTL) — this is a *promise*, not a convenience cache.
> **Cache invalidation trap:** **Availability** is the hardest — it changes on every booking (ours) *and* out-of-band whenever a supplier sells the same inventory on another channel (Expedia sells "our" GDS room). You can invalidate on *your* writes, but you **cannot see the supplier's other-channel writes** — so pure TTL isn't enough for hot inventory; you need the **reconciliation loop** (poll/subscribe to supplier availability deltas) to push corrections, and for the last unit you bypass cache entirely and hit the authoritative supplier. The trap is treating supplier availability like your own write-through cache; it's a cache of a system you don't fully observe, so it needs active reconciliation, not just invalidation.

Q16. Cost/efficiency at scale, given the traffic is search-dominated and supplier calls are expensive/metered?
> **Expected answer:** The dominant costs are (1) **supplier API calls** (often literally billed per call, and rate-limited) and (2) **search/pricing compute**. Strategies: **aggressive availability/price caching** so ≥99.9% of searches never touch a supplier (the single biggest cost lever); **request coalescing/deduplication** (many users searching the same popular route+dates → collapse into one supplier call, fan the result out); **precompute/prewarm** popular routes and hot-date availability on a schedule (holidays are predictable — pre-cache Goa for New Year's). **Async everything** post-confirmation (voucher/PNR gen, email, loyalty) via Kafka. **Tiered storage**: hot near-term dates in Redis/fast DB, far-future and past bookings in cheaper storage. **Cursor pagination** to avoid deep-offset scans. The discipline: every avoided supplier call is direct money saved and rate-limit headroom preserved — caching here isn't just latency, it's the P&L.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Explain the reconciliation loop for supplier inventory in detail: how do you keep a local availability cache correct against a GDS you can only poll, and how do you bound the oversell window? *(Expected: subscribe to supplier availability deltas where available (channel-manager push), else poll on a frequency proportional to inventory velocity — hot inventory polled seconds, cold hourly. Compute drift = local_count − supplier_count and correct. Bound oversell by (a) never confirming supplier inventory without a synchronous pre-book authoritative check, (b) tightening poll frequency for high-velocity/last-unit inventory, and (c) treating our local count as a lower-bound optimization, not the truth. The window equals your reconciliation lag + pre-book latency; the pre-book call closes it for the actual sale.)*

**H2.** Cross-cutting: multi-currency, GDPR, and PCI. A European user books a US hotel priced in USD, paying in EUR, and later invokes right-to-erasure. Walk it. *(Expected: price in supplier currency, convert with a locked FX rate stamped into the quote token — honor the quoted converted amount; store both charged currency and rate for auditing. PCI: never store raw PAN — tokenize via the payment gateway (Lesson 12/25), keep card data out of your systems entirely. GDPR right-to-erasure: PII (name, contact) is anonymized/deleted, but the *financial+booking record* must be retained for tax/audit — so you pseudonymize PII while keeping the transaction, and propagate erasure to suppliers/processors as data controller obligations require.)*

**H3.** Operational: roll out a new dynamic-pricing model across a live platform without mispricing or downtime. *(Expected: shadow-run the new pricing model computing prices in parallel and logging deltas vs the live model (no customer impact); canary on a small traffic slice / low-risk market; guardrails — clamp price changes within a band, alert on anomalous swings; feature-flag per market/segment; keep the quote-token mechanism so in-flight quotes are always honored by whatever model issued them; instant rollback via flag. Never flip pricing globally at once — a pricing bug is direct revenue loss.)*

**H4.** Observability: what do you instrument to catch overselling and supplier degradation *before* customers do? *(Expected: oversell counter (bookings that failed at supplier commit after local success), supplier pre-book/commit success rate + latency + circuit-breaker state per supplier, reconciliation drift magnitude and lag, look-to-book conversion, quote-expiry rate, payment-captured-but-not-confirmed count (the money-at-risk metric), cache hit ratio on availability. Leading indicator: rising supplier commit-failure or reconciliation drift signals oversell risk before refunds spike — throttle to synchronous verification when drift crosses a threshold.)*

**H5.** 'Undo a bad decision': you launched storing per-night availability *inside* the Elasticsearch document and reindexing on every booking. It's melting ES and serving stale sold-out rooms. Migrate live. *(Expected: separate concerns — ES holds only slow-changing searchable attributes; move live availability to a dedicated Redis+Postgres availability service overlaid at query time. Dual-read during transition (ES doc availability vs new service), compare, then cut search to overlay the availability service and stop writing availability into ES. Backfill the availability service from the OLTP source of truth. Verify sold-out accuracy and ES load drop in canary. The principle: never couple a fast-changing count to a near-real-time search index whose refresh lag guarantees staleness.)*

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Modeling inventory as unique rows per physical unit (BookMyShow-style) instead of **count-per-(unit, date-bucket)** with atomic multi-night decrements — this breaks date ranges and misses that the sellable thing is a room-type-over-a-range.
2. Treating third-party (GDS/channel) inventory as if you own it. For supplier inventory the **supplier is the source of truth**; your count is a reconciled cache, and you must do an authoritative pre-book before confirming or you *will* oversell out-of-band sales.
3. Ignoring the look-to-book asymmetry (500:1+). The scale problem is **search + supplier calls + pricing**, not the tiny booking write volume; and the biggest cost lever is caching to avoid metered supplier calls.

**The one insight that makes an answer truly impressive:**
The explicit **dual-sourced-inventory model**: own inventory is CP in your DB (single-transaction guarded multi-night decrement with a `CHECK(available>=0)` backstop), while supplier inventory follows a **saga with an authoritative pre-book/commit and a continuous reconciliation loop**, and the recognition that eventual consistency on availability is fine *everywhere except the last unit*, where you deliberately degrade to synchronous authoritative verification. Naming *why* the boundary (last unit / hot night) needs different consistency than the body of the distribution is what separates a real practitioner from a textbook answer.

**Suggested follow-up reading:**
- Expedia / Booking.com engineering blogs on availability caching and "cache invalidation for supplier inventory."
- Amadeus/Sabre GDS integration docs and the concept of "farelogix / pre-book (PNR) vs book" flows; plus any writeup on saga-based distributed transactions (Garcia-Molina & Salem, "Sagas," 1987).

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
