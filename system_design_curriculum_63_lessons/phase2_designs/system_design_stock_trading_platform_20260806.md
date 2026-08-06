# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 60 of 63 — Phase 2: System Design Track (Design 32 of 35)
**Topic:** Stock Trading Platform (order matching engine)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
A stock exchange's matching engine is the most latency- and correctness-sensitive system you will ever be asked to design: NASDAQ, NSE, and LSE match millions of orders per second with deterministic price-time priority, wall-to-wall microsecond latencies, and an absolute prohibition on losing or reordering a single order. Unlike web systems where you scale by adding stateless replicas, the core matching engine is deliberately a *single-threaded, single-writer, in-memory* state machine per symbol — because determinism and latency beat horizontal scale here, and you scale *out by sharding symbols*, not by parallelizing a book. What makes it hard is the trifecta: nanosecond-sensitive latency, exactly-once and totally-ordered event processing, and regulatory-grade durability where every order and every match must be reconstructable years later.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Heaps / priority queues / skiplists (taught in Phase 1, Lesson 5):** The order book is two priority structures — bids ordered by highest price then earliest time, asks by lowest price then earliest time. In practice you don't use a raw binary heap because you need efficient cancels and price-level aggregation; you use an array/tree of price levels each holding a FIFO queue. But the mental model of "best price on top, O(log n) or O(1) access to the top" comes straight from Lesson 5. This is the beating heart of today's design.

**ACID + durability / WAL (taught in Phase 1, Lessons 2 & 23):** Every accepted order and every trade must survive a crash. The engine uses a write-ahead sequenced log (like a DB's WAL, Lesson 23): append the input event durably *before* applying it to in-memory state, so on restart you replay the log and reconstruct the exact book. Determinism (same inputs → same outputs) is what makes replay valid.

**Kafka / event-driven / total ordering (taught in Phase 1, Lesson 21):** The design is event-sourced: a single ordered input stream (the sequenced order flow) feeds the engine, and the engine emits an ordered output stream (executions, book updates). A partition per symbol gives per-symbol total order — exactly Kafka's per-partition ordering guarantee. Replay of the log rebuilds state, and downstream consumers (market data, clearing, risk) read the output stream.

**Leader election / consensus (taught in Phase 1, Lesson 17 & 3):** Because the engine is a single writer, HA means a hot-standby that must take over deterministically without losing or duplicating a sequence number. Leader election (Raft-style) picks the active engine; the sequencer/log is the consensus point. Quorum replication (Lesson 3) of the sequenced log ensures no acknowledged order is lost when the leader dies.

**Performance tuning / mechanical sympathy (taught in Phase 1, Lesson 22):** Microsecond latency means cache-friendly data layout, lock-free ring buffers (LMAX Disruptor), busy-spin threads pinned to cores, kernel-bypass networking, and zero garbage on the hot path. The matching loop must never allocate or block. This lesson is why the engine is single-threaded — you trade parallelism for determinism and predictable tail latency.

**Rate limiting / gateway (taught in Phase 1, Lessons 11 & 9):** The order gateway does auth, risk pre-checks (buying power, fat-finger limits), and per-account rate limiting *before* orders reach the sequencer, protecting the engine from abusive flow and enforcing regulatory throttles.

> **Recap box:**
> - Order book = price levels + FIFO queue per level (price-time priority), from Lesson 5.
> - WAL/sequenced log first, then apply — deterministic replay rebuilds state (Lessons 21/23).
> - One partition per symbol = per-symbol total order; single-threaded matching per symbol.
> - HA = hot standby + leader election + quorum-replicated log; never lose a sequence number.
> - Microsecond latency = lock-free ring buffer, core pinning, zero-GC hot path (Lesson 22).
> - Risk checks and rate limits happen at the gateway, before the engine.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Explain price-time priority and what it means for order matching.
> **What a strong answer covers:** Orders match best-price-first; among orders at the same price, earliest-submitted matches first (FIFO). A buy order matches against the lowest ask ≤ its limit price; a sell matches against the highest bid ≥ its limit. This makes matching *deterministic and fair* — given the same sequence of inputs, every replica produces the identical set of trades, which is the property that makes event-sourced replay valid.
> **Common weak answer:** "Highest bidder wins" — ignoring the time dimension and the FIFO queue within a price level, which is where most of the subtlety (and the reason for the data structure) lives.
> **Mentor follow-up if they answer well:** Does pro-rata matching (used in some futures/rates markets) change your data structure? (Yes — instead of FIFO you allocate proportionally to resting size, so the price level needs total-size tracking and a different fill algorithm.)

Q2. Estimate the throughput and latency budget for a top exchange. What order rate must the engine sustain and at what latency?
> **What a strong answer covers:** Peak order-entry rates at major exchanges are in the millions of messages/sec aggregate (NASDAQ has quoted ~tens of millions of messages/sec at peak across all symbols); a single busy symbol might see 100K–1M order events/sec at the open. Matching latency target is single-digit to tens of *microseconds* wire-to-wire inside the engine; end-to-end (gateway → match → ack) is tens to low-hundreds of microseconds. Contrast with web systems measured in milliseconds — this is 1,000× tighter. That budget is why the hot path is in-memory, single-threaded, and allocation-free.
> **Mentor follow-up:** If one symbol does 1M events/sec and matching takes ~1 µs each, are you CPU-bound? (Roughly at the edge — 1M × 1µs = 1s of CPU/s on one core; you keep headroom, and if a symbol exceeds one core you've hit the fundamental limit of the single-writer model and must question the symbol's own liquidity, not shard the book.)

Q3. Should the matching engine be multi-threaded to use all cores, or single-threaded? Defend the choice.
> **What a strong answer covers:** Single-threaded *per symbol*. Determinism (identical output for identical input, required for replay/HA/audit) is trivial single-threaded and extremely hard multi-threaded (lock contention, non-deterministic interleavings). Tail latency is more predictable without lock contention or cache-line bouncing. You use all your cores by running *many symbols across many engine shards*, each single-threaded — embarrassingly parallel across symbols, strictly serial within one. LMAX proved a single thread can do 6M+ ops/sec this way.
> **Red flag answer:** "Multi-thread the book with a lock per price level for throughput." That destroys determinism, adds unpredictable tail latency from lock contention, and makes crash-replay non-reproducible — disqualifying for an exchange.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the full order lifecycle architecture: from a client sending an order to a trade being cleared and market data published.
> **Key components expected:** Order gateway (auth, risk/pre-trade checks, protocol translation FIX→internal), Sequencer (assigns a global monotonic sequence + durably logs), Matching engine (single-threaded per symbol shard), Market data publisher, Drop-copy/execution reports back to clients, Clearing & settlement (async, T+1/T+2), Risk engine, Persistent event store.
> **Architecture diagram (text):**
```
 Clients (FIX/binary) ──► Order Gateway ──► Pre-trade Risk (buying power, fat-finger, throttle)
                                                   │
                                                   ▼
                                          SEQUENCER  ── append ──► Sequenced Log (WAL,
                                          (global monotonic seq)     quorum-replicated)
                                                   │                       │
                                    ┌──────────────┼───────────────┐  replay on restart
                                    ▼              ▼               ▼        │
                          Matching Engine   Matching Engine  Matching  ◄────┘
                          shard: AAPL..     shard: MSFT..    shard: ...
                          (single-thread,   (single-thread)  (hot standby each)
                           in-mem book)
                                    │ emits ordered output stream (fills, book deltas)
              ┌─────────────────────┼───────────────────────┬─────────────────┐
              ▼                     ▼                        ▼                 ▼
      Market Data Pub        Execution Reports         Clearing/Settlement  Audit/Regulatory
      (L1/L2/L3 feed)        (drop-copy to client)     (T+1, async)         store (immutable)
```
> **What separates SDE2 from SDE3 here:** SDE2 describes the flow. SDE3 places the *sequencer* as the single point of truth for ordering — the engine is a pure deterministic function of the sequenced input, so HA, replay, and audit all reduce to "replay the log." SDE3 also notes that the durable, quorum-replicated log must acknowledge *before* the order is considered accepted (so no acknowledged order is ever lost), and that the standby engine consumes the same sequenced stream so failover is just "the standby was already caught up."

Q5. Trace a limit buy order that partially fills, end-to-end.
> **Expected trace:**
> 1. Client sends FIX `NewOrderSingle` (buy 1000 AAPL @ 150.00).
> 2. Gateway authenticates, validates, runs pre-trade risk (does the account have buying power for 1000×150?).
> 3. Sequencer assigns seq #N, appends to the durable log, waits for quorum ack.
> 4. Engine (AAPL shard) consumes seq #N, walks the ask side: best ask 149.98 × 600 → match 600 @ 149.98 (price improvement), next ask 150.00 × 300 → match 300 @ 150.00. 900 filled, 100 remaining ≤ limit → rest on the book as a resting bid at 150.00.
> 5. Engine emits: two execution events (600, 300), a resting-order-added event, and L2 book deltas — all with output sequence numbers.
> 6. Execution reports go back to the client (partial fill 900, working 100); market data feed updates; clearing consumes fills asynchronously.
> **Tricky part:** Candidates get vague on *price improvement* (fill happens at the resting order's price, not the aggressor's limit) and on the ordering of emitted events — the fills, the book delta, and the ack must be totally ordered and consistent, because market-data subscribers reconstruct the book from the delta stream and any reordering corrupts every downstream book.

Q6. Design the order-entry API/protocol. What are the key messages and what must be idempotent?
> **Expected API design:** Industry standard is **FIX** (or a faster binary variant / exchange-native binary like OUCH/ITCH). Key messages: `NewOrderSingle`, `OrderCancelRequest`, `OrderCancelReplace` (amend), `ExecutionReport` (ack/fill/reject), `OrderCancelReject`. Every client order carries a **ClOrdID** (client order id) that is unique per session — this is the idempotency key: a resubmit of the same ClOrdID must be rejected/deduped, never double-booked. Cancels reference the original ClOrdID.
> **What to push on:** Session sequence numbers (FIX has per-session seqnums with gap-fill/resend so no message is silently lost); idempotency via ClOrdID uniqueness; the fact that amends are modeled as cancel-replace and *lose time priority* if price/size increases (a subtle rule candidates miss); and that reject reasons must be precise for regulatory reporting.

---

### Data Modeling (Medium–Hard)
Q7. Design the in-memory order book data structure and the durable event schema.
> **Expected schema:**
```
# In-memory order book (per symbol) — NOT a relational table; latency-critical
OrderBook {
  bids: TreeMap<price_desc, PriceLevel>   # or array indexed by price tick
  asks: TreeMap<price_asc, PriceLevel>
  orders: HashMap<order_id, OrderNode>    # O(1) cancel lookup
}
PriceLevel {
  price: int64 (in ticks)
  total_qty: int64
  fifo: DoublyLinkedList<OrderNode>       # time priority within level
}
OrderNode { order_id, account, qty_remaining, ts_seq, prev, next }
```
```sql
-- Durable, append-only event log (source of truth, replayable)
CREATE TABLE order_events (
  seq          BIGINT PRIMARY KEY,        -- global monotonic sequence
  symbol       TEXT NOT NULL,
  event_type   TEXT NOT NULL,             -- NEW|CANCEL|AMEND|TRADE
  cl_ord_id    TEXT,
  account_id   BIGINT,
  side         CHAR(1), price_ticks BIGINT, qty BIGINT,
  ts_ns        BIGINT NOT NULL,           -- nanosecond timestamp
  payload      BYTEA                       -- compact binary
);   -- append-only; never updated; partitioned by symbol
CREATE TABLE trades (
  trade_id BIGINT PRIMARY KEY, symbol TEXT, seq BIGINT,
  buy_order_id BIGINT, sell_order_id BIGINT,
  price_ticks BIGINT, qty BIGINT, ts_ns BIGINT
);
```
> **Index choices and why:** In memory: a `TreeMap`/red-black tree (or a direct-indexed array over the tick range for dense books) gives O(log n)/O(1) best-price access; the `orders` HashMap gives O(1) cancel by id; the per-level doubly-linked list gives O(1) FIFO append and O(1) removal. Durable log is keyed by `seq` (the only ordering that matters) and partitioned by symbol.
> **Partitioning key and why:** **Symbol.** Everything about a symbol — its book, its sequence stream, its engine shard — is co-located and single-writer. Cross-symbol operations essentially don't exist in matching (they exist in risk/portfolio, which is a different, async system).

Q8. A client wants their full order history and current working orders instantly. How do you serve it without touching the hot matching path?
> **Expected answer:** Never query the engine directly — the hot loop must not serve reads. Maintain a separate **read model** (CQRS): a consumer of the engine's output/execution stream materializes per-account order state into a fast store (Redis for working orders keyed by `account:working`, a columnar/OLAP store for history). Working orders are a small set updated by fills/cancels; history is append-only and queried with keyset pagination. This isolates read load from the latency-critical write path entirely.
> **Trap:** Letting the reporting/query layer read the engine's in-memory book or share its thread — any read contention on the matching thread blows the microsecond latency budget. The engine emits events; everyone else builds their own view.

Q9. Discuss the consistency model. Where is strong ordering non-negotiable, and where can you relax?
> **Expected answer:** Within a symbol, ordering is *absolutely* strong and total — the sequencer imposes a single global order and the engine is deterministic, so there is no "eventual consistency" in matching; a trade either happened at seq N or it didn't. This is a CP system for the core (Lesson 2): under partition you must stop accepting orders for a symbol rather than risk two engines matching divergently. What you *can* relax: market-data distribution to end clients is eventually consistent (feeds arrive with varying latency, snapshots + incremental deltas let a slow client catch up), and clearing/settlement is asynchronous (T+1/T+2). Cross-symbol portfolio risk is eventually consistent.
> **Mentor pushback:** "Your primary engine and hot standby are network-partitioned but both think they're leader — both keep matching AAPL. Now you have two divergent books. What prevents this?" Expected: fencing via the sequencer/consensus — only the holder of the current lease (Raft term / epoch) can commit to the log; the log is the single source of truth, so a partitioned old-leader's writes are rejected (stale epoch) and its trades never became real because they never got a committed sequence number. You'd rather halt the symbol than double-match — availability is sacrificed for correctness.

---

### Low-Level Design (Hard)
Q10. Design the core matching algorithm for a limit order against the book. Give the data structure and the fill loop.
> **Problem statement:** Given an incoming limit order, match it against the opposite side by price-time priority, generate fills, and rest any remainder — in microseconds, deterministically.
> **Naive solution:** Keep all resting orders in a single sorted list and linear-scan for matches; on each new order, sort and walk.
> **Why naive fails at scale:** Linear scan and re-sort are O(n) per order with n possibly in the hundreds of thousands at a busy price; you blow the microsecond budget and your latency is non-deterministic (depends on book depth). Cancels become O(n) searches.
> **Expected optimal approach:** Price levels in a balanced tree or a direct-indexed array over the tick range (O(1) to reach best price), each level a FIFO doubly-linked list (O(1) append/pop for time priority), plus a global `HashMap<order_id → node>` for O(1) cancel. Matching walks price levels from the best, filling FIFO within each level until the incoming order is exhausted or the price no longer crosses. Amortized cost is proportional to the number of resting orders actually consumed, not book size.
> **Pseudo-code or class diagram:**
```
match(incoming):                      # incoming is a limit order
    opp = (incoming.side == BUY) ? asks : bids
    while incoming.qty > 0 and opp.not_empty():
        best = opp.best_level()
        if not crosses(incoming.price, best.price, incoming.side):
            break                     # no more matchable price
        resting = best.fifo.head()    # earliest at this level (time priority)
        fill = min(incoming.qty, resting.qty)
        emit_trade(incoming, resting, best.price, fill)  # price = resting's price
        incoming.qty -= fill; resting.qty -= fill
        if resting.qty == 0:
            best.fifo.pop_head(); orders.remove(resting.id)
            if best.empty(): opp.remove_level(best.price)
    if incoming.qty > 0 and incoming.type == LIMIT:
        rest_on_book(incoming)        # add to own side, O(1)
```

Q11. Concurrency: how do you get microsecond throughput out of a single thread while durably logging every order, and what handles the producer→engine handoff?
> **Scenario:** The gateway/sequencer threads produce orders faster than any lock-based queue can hand them to the single matching thread without contention; and every order must be durably logged before matching.
> **Expected fix:** A **lock-free ring buffer** (LMAX Disruptor pattern, Lesson 22): a pre-allocated circular array where producers claim slots via a single atomic CAS on a sequence counter and the consumer (matching thread) reads with cache-line-friendly sequential access — no locks, no garbage, mechanical sympathy. A parallel consumer on the same ring durably writes each entry to the WAL/journal *before* the matching consumer processes it (dependency barrier), guaranteeing "logged before matched." Threads busy-spin pinned to dedicated cores; the hot path allocates nothing.
> **Follow-up — what if the matching thread (engine) dies?** The hot standby has been consuming the same durably-sequenced log and has an identical in-memory book (deterministic replay). Leader election (Lesson 17) promotes it; it resumes at the last committed sequence number. Nothing is lost because acceptance required a quorum-committed log entry; nothing is duplicated because the sequence number is the dedupe key. On cold restart, replay the WAL from the last snapshot to rebuild the exact book.

Q12. A network partition splits your quorum-replicated log, or a downstream market-data consumer falls behind and starts dropping. How do you handle it without corrupting books or losing regulatory data?
> **Scenario:** Partition between sequencer replicas; and a slow L2 feed consumer.
> **Expected handling:** For the log: require a majority quorum (Lesson 3) to commit a sequence; a minority partition cannot advance the sequence, so it stops accepting — the symbol *halts* rather than diverges (fail-closed, correctness over availability). For the slow consumer: market data is snapshot + incremental. A consumer that detects a gap (missing output seq) requests a fresh snapshot and replays deltas from there — it never guesses. The audit/regulatory store is a separate durable, append-only sink that must never drop; it's backed by the WAL itself, so as long as the log committed, the record exists. Poison/duplicate output events are deduped by output sequence number; nothing is reprocessed twice.

---

### Scaling to 10x / 100x (Hard)
Q13. You need 100× more throughput. Where does the design break first, and what's the fundamental limit?
> **Expected answer:** Aggregate throughput scales by adding **symbol shards** (more single-threaded engines on more cores/hosts) — that's near-linear and breaks late. The first *hard* wall is a **single hot symbol** whose order rate exceeds what one core can match (single-writer determinism means you cannot parallelize one book). Second is the **sequencer/log write path** — a global monotonic sequence across all symbols is a serialization point; you relieve it by sharding the sequencer *per symbol* (each symbol has its own ordered log) so there's no global bottleneck. Third is **market-data fan-out** bandwidth.
> **Numbers to ground the answer:** One core matches on the order of 1–6M simple ops/sec; at ~1µs/order a single symbol tops out near 1M orders/sec. If a symbol needs more, the answer isn't sharding the book (impossible without breaking price-time priority) — it's that no real single instrument sustains that; you'd examine whether it should be multiple listings. Market data at 100× can be many GB/s, solved with multicast and hierarchical fan-out, not unicast.

Q14. How do you shard the exchange, and what's the "hot symbol" analog of a hot shard?
> **Expected sharding strategy:** Shard by **symbol** across engine instances (consistent hashing of symbol → shard, Lesson 19, with vnodes so rebalancing during maintenance moves few symbols). Each shard owns a disjoint set of symbols, its own sequencer, log, engine, and standby. There is deliberately *no* cross-shard transaction in matching. Portfolio-level risk that spans symbols is computed asynchronously off the aggregated execution stream.
> **Hot spot problem:** A single ultra-liquid symbol (index future, mega-cap on earnings day) saturates its one core. You can't split its book. Mitigations: give hot symbols dedicated high-clock hosts and isolate them (one hot symbol per host so it doesn't starve neighbors), pin to the fastest cores, and apply pre-trade throttles/rate limits at the gateway to shed abusive flow. Detect via per-symbol event-rate and matching-latency metrics; rebalance symbol→host placement so no host hosts two hot symbols.

Q15. Caching / read-path design: market data must reach thousands of subscribers with minimal latency and no book corruption. Design it.
> **Expected layered cache design:** The engine emits an ordered incremental delta stream (L2/L3 book updates). A **market-data cache** maintains the current book snapshot per symbol in memory; new subscribers get a snapshot + then live deltas (snapshot-plus-incremental). Distribution is via **multicast** (or a fan-out tree of relays) so one send reaches thousands. Co-located subscribers get the lowest latency; remote ones get regional relays. A "conflated" feed (top-of-book only, coalesced) serves clients who can't consume the full firehose.
> **Cache invalidation trap:** There is no TTL-based invalidation — the book is a running state machine reconstructed from an *ordered* delta stream. The hard part is **gap handling and ordering**: if a subscriber misses delta seq #M, every subsequent update is meaningless until it resnapshots; and if deltas are ever delivered out of order, the reconstructed book is silently wrong. So the sequence number on every delta is sacred, and consumers must halt-and-resnapshot on any gap rather than apply an out-of-order update. Conflation must never drop a *price-level-removed* event or the client shows phantom liquidity.

Q16. Cost/efficiency: regulatory retention requires keeping every order/trade for years. How do you store petabytes affordably while keeping the hot path fast?
> **Expected answer:** Tiered storage. Hot tier: the in-memory book + recent WAL on NVMe for replay and same-day queries. Warm tier: recent days' event log in a fast columnar store for surveillance/analytics. Cold tier: compress the append-only event log (it's highly compressible binary — delta-encode prices as ticks, columnar Parquet, ~5–10× compression) and move to object storage (S3 Glacier-class) for the multi-year regulatory horizon, queryable on demand. Periodic **snapshots** of the book let you truncate WAL replay (replay from last snapshot instead of genesis). Batch the write to warm/cold tiers off the hot path so journaling stays microsecond-fast. The principle: the latency-critical hot path stays tiny and in-memory; durability and retention are handled by cheaper async tiers.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Explain deterministic replay in full: why is single-threaded, allocation-free, no-wall-clock-reads (use the sequenced timestamp, never `now()`) matching *required* for the WAL replay to reproduce the identical book bit-for-bit — and what one non-deterministic call (a random tiebreak, a `System.currentTimeMillis()`, an iteration over a HashMap) would silently corrupt failover?

**H2.** Regulatory: MiFID II / SEBI require nanosecond-accurate, clock-synchronized (PTP/GPS) timestamps and full order-audit-trail reconstruction. How do you guarantee timestamp integrity across gateway, sequencer, and engine, and prove to a regulator that trade at seq N happened before order at seq N+1?

**H3.** Deploy a new matching-engine version with a bug-fix to the matching rules *without halting the market and without a single divergent trade*. Discuss shadowing the new engine against the live sequenced stream, comparing outputs bit-for-bit before cutover, and the maintenance-window vs hot-swap trade-off.

**H4.** What do you instrument to detect, in real time, that matching latency p99.9 has crept from 8µs to 40µs (Lesson 10/22)? Name the metrics: per-symbol matching latency histogram, ring-buffer occupancy, GC pauses (should be zero), WAL fsync latency, sequencer commit latency, feed gap counters, and how you alert before customers notice.

**H5.** You shipped with a *global* sequencer (one monotonic sequence across all symbols) and it's now the throughput ceiling. Migrate to per-symbol sequencing without losing ordering guarantees or a single event. Describe the dual-log transition, per-symbol epoch handoff, and rollback.

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Trying to make the matching engine multi-threaded or horizontally scalable *within a symbol* for throughput. The correct model is single-threaded-per-symbol + shard-by-symbol; determinism and tail latency beat parallelism here, and one book fundamentally cannot be parallelized without breaking price-time priority.
2. Forgetting determinism. They read the wall clock, use a HashMap iteration order, or allocate on the hot path — any of which makes crash-replay and hot-standby failover produce a *different* book, which in an exchange is catastrophic and undetectable until it's too late.
3. Treating durability as "write to the DB after matching." It's the opposite: sequence-and-durably-log *before* applying, with quorum ack before acceptance — the log is the source of truth and the engine is a deterministic projection of it.

**The one insight that makes an answer truly impressive:**
The matching engine is not a service that mutates a database — it's a *deterministic state machine driven by a single ordered, durable input log*. Once you internalize that, everything falls out for free: HA is "the standby replays the same log," audit is "read the log," recovery is "replay from snapshot," and correctness under partition is "only the quorum-committed sequence is real." Candidates who frame the whole exchange as event-sourcing over a sequenced log — rather than as CRUD over an order table — are thinking like exchange engineers.

**Suggested follow-up reading:**
- Martin Thompson & the LMAX Disruptor paper/talks (mechanical sympathy, lock-free ring buffer, 6M ops/sec single thread).
- NASDAQ ITCH/OUCH protocol specs and the "How to build an exchange" talks; Jane Street / Jump Trading engineering writeups on deterministic replay.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
