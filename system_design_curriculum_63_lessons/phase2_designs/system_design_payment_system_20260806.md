# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 58 of 63 — Phase 2: System Design Track (Design 30 of 35)
**Topic:** Payment System (Razorpay / Stripe) — Ledger, Idempotency, Reconciliation
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
A payment system is the one design where **"eventually consistent" and "approximately correct" are unacceptable** — money must be conserved to the paisa, every state transition must be auditable, and the system must survive the fact that the actual movement of money happens inside *other* systems you don't control (card networks, banks, UPI switch, the acquirer). Stripe and Razorpay are, at their core, a **double-entry ledger** wrapped in an **idempotency layer**, orchestrating a **state machine** over slow, flaky, asynchronous external rails, then **reconciling** their own view against the bank's settlement files every night. The hard parts: never double-charging on a retry, never losing a webhook, correctly handling the "we don't know if it worked" ambiguity that is the *normal* case in payments, and proving at any instant that money in = money out. This is the most correctness-critical system in the entire curriculum.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**ACID & Isolation (taught in Phase 1, Lesson 2):** Strong ACID is non-negotiable here. A ledger entry write and the balance update must be atomic; double-entry means debit and credit are written in **one transaction** or neither. Serializable/repeatable-read isolation on account balances prevents lost-update/write-skew that would leak money.

**Idempotency & Exactly-Once (foundations in Lessons 9 & 21):** The internet gives you at-most-once or at-least-once; "exactly-once" is achieved via **idempotency keys + dedup**. Every mutating payment operation (charge, refund, payout) carries a client idempotency key; the same key returns the *same* result, never a second charge. This is the single most important concept today.

**Message Queues / Kafka (taught in Phase 1, Lesson 21):** Async orchestration of the payment lifecycle, webhook delivery to merchants, retry with backoff, DLQ for poison messages, and an **event log** that doubles as an audit trail. Ordering and at-least-once delivery semantics matter.

**State Machines / DDD (taught in Phase 1, Lesson 9):** A payment is a strict state machine (CREATED → AUTHORIZED → CAPTURED → SETTLED, plus FAILED/REFUNDED/DISPUTED). Illegal transitions must be impossible. The domain model is the payment aggregate.

**Redis / Locks & TTL (taught in Phase 1, Lesson 24):** Idempotency-key dedup store, distributed locks to serialize operations on a single payment/account, and short-lived state during multi-step flows.

**CAP / PACELC (taught in Phase 1, Lesson 2):** Payments are **strictly CP** — during a partition you *stop* and stay safe rather than risk a double-movement of money. Availability is sacrificed to correctness, always.

**Security / TLS / PCI / Tokenization (taught in Phase 1, Lessons 12 & 25):** PCI-DSS scope minimization — raw card data (PAN) never touches your core services; it's tokenized at the edge/vault. TLS everywhere, HSMs for keys, signed webhooks, OAuth for merchant API auth.

**Retries, Circuit Breakers, DR (taught in Phase 1, Lessons 10 & 14):** External rails are slow and fail; retries with idempotency, circuit breakers per acquirer/bank, and RPO≈0 (you cannot lose a committed financial record).

> **Recap box:**
> - ACID/double-entry (L2): debit+credit in one atomic transaction; balances conserved.
> - Idempotency (L9/L21): every mutation carries a key; same key → same result, never double-charge.
> - Kafka (L21): lifecycle orchestration, webhook delivery, DLQ, audit log.
> - State machine (L9): strict legal transitions only.
> - Redis (L24): idempotency dedup + per-payment lock.
> - CAP (L2): strictly CP — halt on partition, never risk double-movement.
> - PCI/tokenization (L12/L25): PAN never enters core; vault + HSM.
> - Retries/breakers/DR (L10/L14): idempotent retries, per-acquirer breakers, RPO≈0.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)

Q1. Explain the difference between **authorization**, **capture**, and **settlement**. Why are they separate, and why does that separation force an asynchronous design?
> **What a strong answer covers:** **Authorization** = the issuer *holds* the funds and confirms the card is valid and has the money (a hold, not a movement). **Capture** = you tell the network "actually take it," moving from a hold to an owed amount (can be later, e.g., ship-then-charge; can be partial). **Settlement** = the actual inter-bank money movement, which happens in **batch, hours-to-days later**, via the card network's clearing files. They're separate because the card network is a batch/clearing system, not a real-time ledger — auth is real-time-ish, settlement is T+1/T+2. This forces async design: you can't block a user for two days, so your system records *its own* view of state immediately and **reconciles** against the network's settlement files later. The money isn't "done" when the user sees "success" — it's authorized; settlement confirms it.
> **Common weak answer:** "Charge the card, money moves, done." Conflates a real-time API response with settled funds — leads to designs that can't handle chargebacks, delayed capture, partial capture, or settlement mismatches.
> **Mentor follow-up:** A capture succeeds in the API but the transaction never appears in the next day's settlement file. What has your system concluded vs reality, and how do you catch it?

Q2. Estimate the throughput and storage for a Razorpay-scale processor: 10M transactions/day. Size the ledger and the write rate.
> **What a strong answer covers:** 10M txns/day ≈ **~115 txns/sec average**, but payments are **peaky** — festive sale / salary day / 8pm shopping spikes push **10–20x → ~2,000 TPS peak**. Each transaction is *not* one write: it produces multiple **ledger entries** (double-entry: at least a debit + credit, often 4–6 legs with fees, taxes, merchant payable, platform commission), plus state-machine transitions, plus webhook events. So ~10M txns → **50M+ ledger rows/day**. At ~500 bytes/row that's ~25 GB/day of ledger, ~9 TB/year *before* replication and audit copies — and it's **append-only and immutable** (you never delete/update a ledger entry; corrections are new compensating entries). This is why the ledger is its own high-durability, append-optimized store.
> **Mentor follow-up:** 2,000 TPS of ACID double-entry writes on a single Postgres primary — is that feasible, and what's your first move when it isn't?

Q3. A user clicks "Pay" and the network times out — you don't get a yes or no. What does the system do, and what must you *never* do?
> **What a strong answer covers:** A timeout is **not a failure** — it's an *unknown*. You must **never assume failure and retry as a fresh charge** (that risks a double-charge if the first actually succeeded). Correct handling: the operation carried an **idempotency key**; you either (a) **query the network's status endpoint** for that key/reference to resolve truth, or (b) leave the payment in a `PENDING`/`INITIATED` state and let the issuer's **asynchronous webhook / next reconciliation** resolve it, and (c) any retry reuses the *same* idempotency key so the network dedupes it to the original attempt. You surface "processing" to the user, not "failed." The invariant: an ambiguous outcome must converge to exactly one real outcome via idempotency + reconciliation, never via a blind retry.
> **Red flag answer:** "Retry the charge until it succeeds." Blind retries without an idempotency key are how you double-charge customers — the cardinal sin of payments.

---

### High-Level Design (Medium)

Q4. Design the core architecture of a payment gateway/processor: from merchant API call through the card network, with the ledger and reconciliation. Draw it.
> **Key components expected:** Merchant SDK/API, API GW (auth, rate limit), **Idempotency layer**, **Payment orchestrator** (state machine), **Tokenization/Vault** (PCI), **Acquirer/rail adapters** (card networks, UPI switch, netbanking — with circuit breakers), **Double-entry Ledger service** (ACID, append-only), **Webhook delivery service**, **Reconciliation service** (ingests bank settlement files), **Kafka** (event backbone), Merchant dashboard/reporting.
> **Architecture diagram (text):**
```
 Merchant --> [API GW: authN, rate-limit] --> [Idempotency Layer (Redis+DB)]
                                                     |  (dedup on Idempotency-Key)
                                                     v
                                           [Payment Orchestrator]  --- strict state machine
                                             |          |        \
                        tokenize card -->[Vault/HSM]    |         \--> [Ledger svc] (double-entry, ACID, append-only)
                        (PCI scope)                     v                      |
                                        [Rail Adapters] --> Card Networks / UPI switch / Banks
                                          (circuit breaker, retry, idempotent ref)   |
                                                     |  <-- async webhooks / status  |
                                                     v                               v
                                                 [Kafka: payment events / audit log] ----> [Webhook Delivery] --> Merchant
                                                     |
                                     [Reconciliation svc] <-- nightly bank settlement files (MIS/T+1)
                                        matches internal ledger vs bank; flags breaks -> Ops
```
> **What separates SDE2 from SDE3 here:** SDE3 makes the **ledger the source of truth and the rails a system of record you reconcile against**, treats **every external interaction as idempotent + asynchronously resolvable**, and designs reconciliation as a first-class subsystem (not an afterthought) because *the network's settlement file, not your API response, is the final word on money*. SDE2 tends to model the flow as a synchronous request/response ("call network, get result, update DB") and bolts on reconciliation later, missing that ambiguity/async is the *normal* path and that the ledger — not the payment status column — is where correctness lives.

Q5. Trace a single card payment from merchant checkout to settled, including the ambiguous-outcome and webhook paths.
> **Expected trace:**
> 1. Merchant server → `POST /orders` (creates an order/intent, amount, currency) → your API returns an order_id + client token.
> 2. Customer enters card in your hosted/SDK field → PAN goes to the **Vault**, returns a token; core services only ever see the token (PCI scope minimized).
> 3. `POST /payments` with Idempotency-Key → orchestrator dedups, creates payment CREATED, writes a *pending* ledger reservation, calls the **rail adapter** to **authorize**.
> 4. Auth response: success → state AUTHORIZED, ledger records the authorization. Timeout/unknown → state PENDING; kick off status-poll / await webhook.
> 5. **Capture** (immediately or later) → state CAPTURED; ledger writes the capture legs (merchant payable, platform fee, tax).
> 6. Emit Kafka event → **webhook delivery** notifies merchant (signed, retried with backoff until acked).
> 7. **Overnight:** bank sends settlement/MIS file → **reconciliation** matches each internal captured payment to a settled line → state SETTLED; funds enter the merchant's payout cycle. Unmatched = a **break**, sent to Ops.
> **Tricky part:** Steps 4 and 7 are where money is *at risk of divergence*. A payment can be CAPTURED in your system but **absent from settlement** (network dropped it) or **present in settlement but you show FAILED** (your auth timed out but it actually went through). Reconciliation is the truth-maker; the API response is provisional. Candidates who end the trace at "API returns success" miss that settlement — not the 200 OK — is when money is real.

Q6. Design the idempotency and payments APIs.
> **Expected API design:**
> - `POST /v1/orders` → `{order_id, amount, currency, status}` (an intent; safe to create).
> - `POST /v1/payments` — **required** header `Idempotency-Key: <uuid>`; body `{order_id, method, token}` → `{payment_id, status}`. Same key + same body → returns the *original* result (200 with the stored response); same key + *different* body → `409` (misuse).
> - `POST /v1/payments/{id}/capture` (Idempotency-Key) — for delayed/partial capture, body `{amount}`.
> - `POST /v1/refunds` (Idempotency-Key) — body `{payment_id, amount}` → refund is *also* idempotent and *also* double-entry.
> - `GET /v1/payments/{id}` — status query (used to resolve ambiguity).
> - **Webhooks** to merchant: signed (HMAC), with `event_id` so the *merchant* can dedupe; delivered at-least-once with retries.
> **What to push on:** **Idempotency-Key semantics**: scope (per-merchant), retention window (24h–7d), storing the *response* not just the key, and the same-key-different-body conflict. **Signed webhooks + event_id** so merchants dedupe (you deliver at-least-once, so merchants *will* see duplicates). **Refunds are first-class idempotent money-movements**, not a "reverse" side-effect. Amounts always in **minor units (integer paise/cents)** — never floats.

---

### Data Modeling (Medium–Hard)

Q7. Design the double-entry ledger and payment tables. Show why double-entry, not a balance column.
> **Expected schema:**
```sql
-- LEDGER: immutable, append-only. Each MONEY MOVEMENT = a balanced transaction of >=2 entries.
CREATE TABLE ledger_transactions (
  txn_id        UUID PRIMARY KEY,
  reference_id  UUID,                 -- payment_id / refund_id / payout_id
  created_at    TIMESTAMPTZ NOT NULL,
  description   TEXT
);
CREATE TABLE ledger_entries (
  entry_id     BIGSERIAL PRIMARY KEY,
  txn_id       UUID NOT NULL REFERENCES ledger_transactions,
  account_id   BIGINT NOT NULL,       -- merchant_payable, platform_fee, tax, gateway_suspense...
  direction    CHAR(1) NOT NULL,      -- 'D' debit / 'C' credit
  amount_minor BIGINT NOT NULL CHECK (amount_minor > 0),   -- integer paise, never float
  currency     CHAR(3) NOT NULL,
  created_at   TIMESTAMPTZ NOT NULL
);
-- INVARIANT enforced per txn_id: SUM(debits) == SUM(credits). No row is ever UPDATEd/DELETEd.

-- PAYMENTS: the state machine / lifecycle view (mutable status, but transitions logged)
CREATE TABLE payments (
  payment_id      UUID PRIMARY KEY,
  merchant_id     BIGINT, order_id UUID,
  amount_minor    BIGINT, currency CHAR(3),
  status          VARCHAR,            -- CREATED/AUTHORIZED/CAPTURED/SETTLED/FAILED/REFUNDED/DISPUTED
  method          VARCHAR, rail_ref  VARCHAR,   -- acquirer/network reference
  idempotency_key TEXT,
  created_at TIMESTAMPTZ, updated_at TIMESTAMPTZ,
  UNIQUE (merchant_id, idempotency_key)          -- dedup scope
);
CREATE TABLE payment_transitions (   -- audit of every state change
  payment_id UUID, from_state VARCHAR, to_state VARCHAR, at TIMESTAMPTZ, reason TEXT
);
CREATE TABLE idempotency_keys (
  merchant_id BIGINT, key TEXT, request_hash TEXT,
  response_body JSONB, status_code INT, created_at TIMESTAMPTZ,
  PRIMARY KEY (merchant_id, key)
);
```
> **Index choices and why:** `payments UNIQUE(merchant_id, idempotency_key)` is the *database-enforced* dedup guard. `ledger_entries(txn_id)` to fetch/validate a balanced transaction. `ledger_entries(account_id, created_at)` for account statements/balance derivation. Balance is **derived** (SUM of entries) or maintained in a separate periodically-checkpointed balance table — never a mutable column you race on.
> **Partitioning key and why:** Partition the ledger by **time (append-only, range by created_at/month)** for archival + write locality, and shard **payments by merchant_id** so a merchant's activity and their reconciliation stay co-located and one merchant can't hot-spot another. Why double-entry over a balance column: a single mutable `balance` column is a race (lost updates) and, more importantly, **non-auditable** — you can't prove *why* a balance is what it is. Double-entry makes every balance the provable sum of immutable, balanced movements; corrections are compensating entries, so history is never rewritten.

Q8. How do you compute a merchant's available balance / payout amount efficiently without summing millions of ledger rows every time?
> **Expected answer:** Don't `SUM()` the full history on every read. Maintain **running balance checkpoints/snapshots** per account (e.g., a `balance_snapshots(account_id, as_of, balance_minor)` written periodically), then balance = last snapshot + SUM(entries since snapshot). This bounds the sum to a small recent window. For payouts, compute the **settled + cleared** balance (only ledger entries whose payment reached SETTLED and passed the payout hold/reserve window). Reads can also be served from a **materialized balance table updated transactionally with each ledger write** (write-through), reconciled against the derived sum periodically as a self-check.
> **Trap:** Storing a single mutable `balance` and updating it with `balance = balance + x` under concurrency → lost updates (two credits, one overwrites) → money materializes or vanishes. Or `SELECT SUM(amount) FROM ledger_entries WHERE account_id=?` across years of rows on every dashboard load → table-scan meltdown. Snapshots + append-only entries give both correctness and speed.

Q9. Payments are strictly CP. Give a precise scenario where choosing availability over consistency would corrupt money, and defend halting instead.
> **Expected answer:** Payments sacrifice availability for consistency (CP). Scenario: a **network partition between the orchestrator and the ledger DB primary**. If you chose availability — let a secondary accept writes or skip the ledger write and "fix later" — two things break: (1) you might **capture a payment without a durable balanced ledger entry** (money moved in the world, unrecorded → your books don't balance), or (2) **split-brain double-processing** — both partitions independently process the same refund → double refund. Defense: on partition, the correct behavior is to **stop taking that action** (return "processing"/503, queue it) rather than move money you can't durably and consistently record. A declined-but-safe payment is recoverable (customer retries); a double-refund or an unrecorded capture is a **real financial loss and an audit failure**. RPO must be ~0 for committed financial records.
> **Mentor pushback:** "But halting hurts revenue during an outage." Correct — and you mitigate with multi-AZ synchronous replication and fast failover so the CP store stays available *most* of the time; but when forced to choose, an unavailable payment costs a retry, a wrong ledger costs money and regulatory trust. You never trade correctness for availability where money moves.

---

### Low-Level Design (Hard)

Q10. The core sub-problem: **exactly-once payment execution under retries and concurrent duplicate requests**. Design the idempotency layer end-to-end.
> **Problem statement:** The same logical payment may arrive multiple times — client retry on timeout, load-balancer replay, at-least-once queue redelivery, user double-click — and it must execute **exactly once**, with concurrent duplicates safely serialized, returning the identical result.
> **Naive solution:** "Check if key exists; if not, process; then store the key." (check-then-act).
> **Why naive fails at scale:** It's a **TOCTOU race** — two concurrent requests with the same key both pass the "exists?" check before either writes it, and both process → double-charge. Also fails if the process crashes *after* charging the network but *before* storing the key → a retry reprocesses.
> **Expected optimal approach:** Make the idempotency record the **serialization point via an atomic insert**, and persist the *result*, not just the key:
> 1. `INSERT INTO idempotency_keys(merchant_id, key, request_hash, state='IN_PROGRESS') ON CONFLICT DO NOTHING`. The unique constraint means exactly one concurrent request "wins" the insert; the losers get a conflict → they **wait/poll** for the winner's result (or return "in progress").
> 2. The winner processes the payment. Crucially, it passes the **same idempotency key as the reference to the external rail** so even the *network* dedupes if we retried at that layer.
> 3. On completion, in the **same transaction as the ledger write**, update the idempotency record to `COMPLETED` with the stored `response_body`. Tie the idempotency record and the ledger entry to the same DB transaction so a crash can't leave "charged but not recorded."
> 4. Any later request with the same key returns the stored response verbatim. Same key + different body → `409` (client misuse). Records expire after a retention window (e.g., 24h–7d).
> **Pseudo-code or class diagram:**
```
def process_payment(merchant, key, body):
    row = db.insert_if_absent(idempotency_keys, (merchant, key),
                              request_hash=hash(body), state="IN_PROGRESS")
    if row is CONFLICT:
        existing = db.get(idempotency_keys, (merchant, key))
        if existing.request_hash != hash(body): raise Conflict(409)
        if existing.state == "COMPLETED": return existing.response      # exactly-once replay
        else: return wait_or_202(existing)                              # winner still working
    # we are the sole winner:
    with tx():                                    # ledger write + key update atomic
        result = rail.authorize(body, client_ref=key)   # network also dedupes on client_ref
        write_double_entry_ledger(result)               # debit == credit
        db.update(idempotency_keys, (merchant,key), state="COMPLETED", response=result)
    return result
```

Q11. Two refund requests for the same payment arrive concurrently (merchant retried), and separately, a partial-refund and a full-refund race. Prevent double-refund.
> **Scenario:** Payment P (₹1000) captured. R1 and R2 both refund ₹1000 (dup), or R1 refunds ₹400 while R2 refunds ₹1000 concurrently → risk of refunding more than captured.
> **Expected fix:** Two guards. (1) **Idempotency key on the refund** — R1 and R2 sharing a key dedupe to one refund (the exactly-once machinery above). (2) For *distinct* refunds that could over-refund, serialize on the payment and enforce a **balance invariant in the ledger transaction**: acquire a per-payment lock (Redis lock or `SELECT ... FOR UPDATE` on the payment row), then within the transaction assert `sum(refunds_so_far) + this_refund <= captured_amount` before writing the refund's double-entry legs. If it would exceed, reject. Because refunds are ledger entries, the invariant is checkable atomically. The FOR UPDATE serializes R1 and R2 so the second sees the first's committed refund.
> **Follow-up (what if the lock holder dies?):** A Redis lock has a TTL, so a dead holder's lock auto-expires and another worker proceeds — but the **durable guard is the DB**: the refund's balance-invariant check (`sum(refunds) <= captured`) runs inside the committed transaction, so even if the lock is lost and two workers proceed, only one refund can commit without violating the invariant; the DB serializes the payment row. The lock is an optimization to reduce contention/wasted work; the ledger invariant is the correctness backstop.

Q12. A merchant webhook endpoint is down for 3 hours during a sale. Meanwhile some acquirer webhooks *to you* are delivered twice and out of order. Handle both directions.
> **Scenario:** Outbound webhook delivery failing (merchant down) + inbound acquirer webhooks duplicated/reordered.
> **Expected handling:** **Outbound:** never lose an event — webhook delivery is backed by a **durable queue (Kafka)** with **retry + exponential backoff + jitter** over hours, and a **DLQ** after max attempts; merchants can **replay** missed events via an events API. Each event has a stable `event_id` and HMAC signature so the merchant **dedupes** (you're at-least-once). **Inbound (acquirer → you):** treat every inbound webhook as **at-least-once and possibly out-of-order**. Dedupe on the acquirer's reference/event id. Handle reordering by making transitions **idempotent and order-tolerant via the state machine** — e.g., a late "authorized" webhook arriving after "captured" must **not** regress state; transitions are guarded (`CAPTURED` doesn't go back to `AUTHORIZED`). Use the event's authoritative status + timestamps, and if truly ambiguous, **query the acquirer's status endpoint** to get ground truth. The state machine's guarded transitions are what make out-of-order webhooks safe.

---

### Scaling to 10x / 100x (Hard)

Q13. At 100x (festive peak, ~20K TPS of financial writes), where does it break first?
> **Expected answer:** The **single ACID ledger primary** breaks first — double-entry writes are the hardest to scale because they demand strong consistency and can't be naively partitioned (a transaction touching multiple accounts wants atomicity). A single Postgres primary tops out around low-thousands of complex ACID TPS; at 20K TPS it's saturated on write throughput and lock contention on hot accounts (e.g., the platform's own fee/suspense accounts that *every* transaction touches). Next bottleneck: the **idempotency store** under retry storms, and **hot merchant accounts** during a single huge sale.
> **Numbers to ground the answer:** 20K txns/sec × ~5 ledger legs = **~100K ledger inserts/sec** — far beyond one primary. The hottest row is a shared platform account credited on *every* txn (100K writes/sec to one row = impossible under row-lock serialization). This is the crux: you must remove single-hot-row contention (see Q14) and shard the ledger while preserving atomicity where it's truly needed.

Q14. Apply consistent hashing / partitioning (Lesson 19): shard the ledger and payments. How do you shard money without breaking double-entry atomicity, and handle the hot platform account?
> **Expected sharding strategy:** Shard **payments by merchant_id** (a merchant's activity is self-contained; reconciliation is per-merchant). For the **ledger**, the challenge is that a transaction spans multiple accounts. Approaches: keep each *balanced transaction* (all its entries) on a **single shard** by choosing a shard key that co-locates the accounts of one movement (e.g., by merchant_id, with platform-side legs handled via a technique below) so double-entry stays a single-shard ACID transaction — avoid cross-shard 2PC on the hot path. Partition the ledger by **time** for archival and by merchant for locality.
> **Hot spot problem:** The shared **platform fee/suspense account** is touched by every transaction → one hot row. **Detect:** lock-wait time and write QPS on that account. **Fix:** **account sharding / striping** — split the single logical platform account into **N sub-accounts (shards)** (`platform_fee_00 .. platform_fee_63`); each transaction writes to a randomly/hash-chosen sub-account, spreading 100K writes/sec across 64 rows (~1.5K each). The logical balance = SUM over sub-accounts, computed periodically. This is the standard "shard the hot counter/account" pattern. Also **batch** low-value ledger writes where regulation allows, and keep the truly-atomic core minimal.

Q15. Design caching for a payment system — what can and cannot be cached, and what's the dangerous cache?
> **Expected layered cache design:**
> - **Cacheable (safe):** merchant configuration/keys, routing rules (which acquirer for which card BIN), fee schedules, currency/FX rates (with locked snapshots), fraud-model features — all read-heavy, slow-changing.
> - **Idempotency store (Redis + DB):** a cache-like fast path for dedup, but **backed by the durable unique constraint** — Redis speeds the common "already seen" check; the DB is authoritative.
> - **Read replicas** for dashboards/reporting/statements (eventually consistent reads are fine for *viewing*).
> **Cache invalidation trap:** **Never cache balances or payment state as the source of truth for a decision that moves money.** The dangerous mistake is caching a merchant's *balance* and authorizing a payout from the cached value — a stale cache can approve a payout the merchant can't cover (double-spend). Balance for *decisions* must be read from the authoritative ledger (or a transactionally-consistent snapshot); the cache is fine for *display* but must be labeled as such. The rule: cache facts that are safe when stale (config, FX display); never let a money-moving decision read a cache that could be stale.

Q16. Cost/efficiency at scale — payments generate massive immutable data with long regulatory retention. Optimize.
> **Expected answer:** Ledger + audit data is **append-only, immutable, and must be retained for years** (RBI/PCI/tax often 7–10 years) — storage is the dominant long-term cost. Strategies: **time-tiered storage** — recent (hot) months in fast OLTP/Postgres, older in cheaper columnar/object storage (S3 + Parquet) queryable for audit/dispute but not on the hot path; **compression** of archived ledger (columnar compresses financial data well). **Batch** non-critical writes (analytics events, some webhook fan-out) and settlement processing (it's inherently batch/T+1). **Reserve synchronous, expensive ACID capacity only for the money-moving core**; push reporting/analytics to replicas and a warehouse. Rail/acquirer costs: **smart routing** (route each card to the cheapest acquirer that will approve it) directly cuts interchange/processing fees. The discipline: keep the expensive strongly-consistent core small and hot-data-only; everything historical, analytical, or displayable goes to cheaper tiers.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Explain how you'd implement **automated reconciliation** between your internal ledger and the bank's daily settlement file, including how you classify and resolve "breaks." *(Expected: ingest the bank MIS/settlement file (T+1), match each settled line to an internal captured payment by rail_ref/UTR; classify breaks — (a) in-ledger-not-in-bank = network dropped or delayed, (b) in-bank-not-in-ledger = we missed a capture/webhook, (c) amount mismatch = fee/FX discrepancy. Auto-resolve the common cases with rules, post compensating ledger entries for confirmed discrepancies, and route genuine breaks to an Ops queue with SLAs. The settlement file — not your API — is ground truth for money; reconciliation is what makes the books provably correct. Track break rate and aging as a health metric.)*

**H2.** Cross-cutting: PCI-DSS scope and multi-currency. How do you keep 95% of your services *out* of PCI scope, and how do you handle a payment authorized in USD but settled in INR? *(Expected: tokenize the PAN at a small, isolated, PCI-certified vault/edge (hosted fields / iframe), so raw card data never enters core services — they only see network tokens; this shrinks the audited surface to the vault. Multi-currency: store amounts in minor units per currency, stamp a locked FX rate into the payment at authorization, keep separate ledger accounts per currency (never mix currencies in one balance), and record FX conversion as its own balanced ledger transaction so gains/losses are auditable.)*

**H3.** Operational: deploy a change to the ledger-writing path with zero risk of money corruption and zero downtime. *(Expected: ledger changes are the highest-risk deploys — use expand/contract (backward-compatible schema first), shadow-write and compare new vs old ledger logic in production with no customer effect, canary on a tiny merchant slice, run a continuous invariant checker (sum(debits)==sum(credits), balances reconcile) as a guardrail that auto-halts rollout on any imbalance, and keep instant rollback. Never do an in-place mutation of ledger data; corrections are always new compensating entries. Feature-flag per merchant.)*

**H4.** Observability: what must you instrument to know money is safe *right now*? *(Expected: real-time ledger balance invariant (global sum of debits == credits — any drift is a P0), authorization success rate + latency per acquirer/method, capture-to-settlement match rate and reconciliation break rate/aging, idempotency conflict rate, webhook delivery success + DLQ depth, payment-stuck-in-PENDING count (money in limbo), refund/dispute rates. The single most important alert: a **ledger imbalance** — it means money is being created or destroyed; page immediately and halt affected flows.)*

**H5.** 'Undo a bad decision': you launched with a single mutable `balance` column per account and are seeing lost updates and inexplicable balances under load. Migrate to double-entry with zero money lost and full auditability. *(Expected: introduce the append-only ledger alongside; dual-write every movement as balanced entries while still maintaining the legacy column; backfill historical movements into the ledger and reconcile derived balances against the legacy column, investigating every mismatch (those mismatches *are* the bugs you're fixing). Once derived balances match for a sustained period, cut reads over to ledger-derived (snapshot + delta) balances, then retire the mutable column. The migration itself surfaces the money that was silently lost/created by the racy column.)*

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. Treating a timeout/ambiguous network response as a failure and blindly retrying → **double-charge**. Ambiguity is the normal case; resolve it with idempotency keys + status queries + reconciliation, never blind retry.
2. Using a mutable `balance` column instead of an **append-only double-entry ledger**. It races (lost updates) and, worse, is non-auditable — you can't prove why a balance is what it is.
3. Ending the mental model at "API returned 200 = money moved." The **settlement file, not the API response, is ground truth**; reconciliation is a first-class subsystem, not an afterthought.

**The one insight that makes an answer truly impressive:**
Recognizing that a payment system is fundamentally **an idempotency layer + a double-entry ledger + a reconciliation loop over asynchronous, untrusted external rails**, and that the ledger — with its provable invariant `sum(debits) == sum(credits)` enforced per transaction and monitored globally in real time — is the correctness anchor, while the network's settlement file is the external truth you continuously reconcile against. Candidates who name the *global ledger imbalance* as their top P0 alert, and who explain hot-account striping to scale double-entry writes, are operating at a genuine principal level.

**Suggested follow-up reading:**
- Stripe's "Idempotency" and "Designing robust and predictable APIs with idempotency" engineering posts; Stripe's ledger/"Increment" and double-entry writeups.
- Square/Uber engineering on double-entry ledgers and financial reconciliation; Martin Kleppmann, *Designing Data-Intensive Applications*, ch. on consistency + the "turning the database inside out" event-log view.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
