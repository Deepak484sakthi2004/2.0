# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 30 of 63 — Phase 2: System Design Track (Design 2 of 35)
**Topic:** Distributed, Multi-Tenant Rate Limiter
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Every serious API — Stripe, GitHub, Cloudflare, AWS — sits behind a rate limiter, because without one a single misbehaving client can starve every other tenant. The naive version (a counter in memory) is a five-minute exercise; the real problem is doing it **accurately across a fleet of stateless gateway nodes**, with **per-tenant tiers**, at **millions of decisions per second**, adding **under a millisecond of latency**, while degrading gracefully when the shared counter store itself fails. The hard trade-off is precision vs coordination cost, and the interesting algorithms — sliding window log, sliding window counter, token bucket — each pick a different point on that curve.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Rate limiting algorithms (taught in Phase 1, Lesson 11):** Token bucket (tokens refill at rate R, burst up to capacity B), leaky bucket (fixed-rate queue drain), fixed window counter (count per calendar window — cheap but allows 2x burst at boundaries), sliding window log (exact, store every timestamp — accurate but memory-heavy), sliding window counter (weighted blend of current + previous window — the practical sweet spot). Today you'll pick per use case and defend it.

**Redis / caches (taught in Phase 1, Lesson 24):** Atomic ops (`INCR`, `EXPIRE`), Lua scripting for atomic multi-step logic, sorted sets (`ZADD`/`ZREMRANGEBYSCORE`) for sliding-window logs, TTLs. Redis is the shared-state backbone of a distributed limiter.

**Consistent hashing (taught in Phase 1, Lesson 19):** Shard rate-limit keys across a Redis cluster so `tenant:endpoint` keys distribute evenly and adding a node remaps ~1/N.

**Load balancers & API gateway (taught in Phase 1, Lesson 6 & 9):** The limiter typically lives *at* the API gateway / L7 LB, before requests reach business services. Decision must be made here, fast.

**Circuit breaker & resilience (taught in Phase 1, Lesson 10):** When the central counter store is down, the limiter must fail **open or closed** by policy, not crash the request path.

**CAP / consistency (taught in Phase 1, Lesson 2):** A distributed counter is a consistency problem. Perfect global accuracy needs coordination (latency); most systems accept small over-admission for availability.

> **Recap box:** Token bucket = burst-friendly · sliding window counter = accurate + cheap · Redis Lua = atomic decisions · shard keys by consistent hashing · limiter lives at the gateway · fail-open vs fail-closed is a business decision.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. You learned five algorithms in Lesson 11. For an API that must allow short bursts (a user hitting "sync" that fires 20 calls at once) but cap sustained rate at 10 req/s, which do you pick and why?
> **What a strong answer covers:** **Token bucket.** Capacity B = 20 (burst), refill rate R = 10 tokens/sec. The 20-call burst drains the bucket instantly and is allowed; sustained traffic is then gated at the refill rate. Fixed window would either reject the legitimate burst or allow 2x at the boundary; leaky bucket smooths *output* but rejects the burst. Token bucket uniquely separates burst capacity from sustained rate.
> **Common weak answer:** "Fixed window counter, count 10 per second" — kills the legitimate burst and suffers boundary bursts.
> **Mentor follow-up if they answer well:** Token bucket needs two values (tokens, last_refill_ts) updated atomically. On a distributed fleet, where do those live and how do you update them without a race?

Q2. Fixed window allows a burst at the window boundary. Quantify it: with a limit of 100/minute, what's the worst-case throughput a client achieves?
> **What a strong answer covers:** A client sends 100 requests at 11:00:59.9 (end of window 1) and 100 more at 11:01:00.1 (start of window 2) — **200 requests in ~200ms**, i.e., up to **2x the limit** across a window boundary. That's the fixed-window flaw. Sliding window counter fixes it by weighting: `count = current_window_count + prev_window_count × (overlap fraction)`.
> **Mentor follow-up:** Walk through the sliding-window-counter math for a request arriving 25% into the current minute.

Q3. Where should the rate-limit decision physically happen — at the gateway, in each microservice, or client-side?
> **What a strong answer covers:** Primarily at the **API gateway / edge** (Lesson 9), before requests consume backend resources — one enforcement point, consistent policy, protects everything behind it. Client-side hints (e.g., `Retry-After`, remaining-quota headers) are advisory only — never trusted for enforcement. Per-service limits can add a second layer for internal fairness (bulkheading), but the primary tenant-facing limit is at the edge.
> **Red flag answer:** "In each microservice" as the *only* layer — every service re-implements it, state is fragmented, and a client burns backend capacity before being rejected.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design a distributed rate limiter for a multi-tenant API gateway fleet. Draw it.
> **Key components expected:** Gateway fleet (stateless) → local token-bucket cache (L1) → shared Redis cluster (L2, sharded by consistent hashing) → rules/config service (per-tenant tiers) → async metrics pipeline.
> **Architecture diagram (text):**
```
   Clients (many tenants)
        │
        ▼
 ┌──────────────── API Gateway Fleet (stateless, N nodes) ────────────────┐
 │  each node:                                                            │
 │   ┌──────────────┐   miss/sync    ┌───────────────────────────────┐    │
 │   │ L1 local     │◄──────────────►│  Redis Cluster (sharded)      │    │
 │   │ token bucket │  Lua atomic    │  key: {tenant}:{route}:{win}  │    │
 │   │ (approx)     │                │  INCR / ZADD  + EXPIRE         │    │
 │   └──────┬───────┘                └───────────────────────────────┘    │
 │          │ allow/deny                         ▲                        │
 │          ▼                                    │ pull rules             │
 │   forward or 429                     ┌────────┴────────┐               │
 └──────────────────────────────────────│ Rules/Config svc │──────────────┘
                                         │ per-tenant tiers │
                                         └─────────────────┘
   decisions ──async──► Kafka ──► metrics / billing / abuse detection
```
> **What separates SDE2 from SDE3 here:** SDE2 puts one Redis behind the fleet and calls it done. SDE3 adds the **L1 local approximate bucket** to absorb most decisions without a Redis round-trip on every request, syncs L1↔L2 periodically or on threshold, and explicitly chooses the **fail-open/closed** behavior for when Redis is unreachable. SDE3 also notes the config-propagation path so a tenant's tier change takes effect fleet-wide in seconds.

Q5. Trace one request through the limiter, end-to-end, including the Redis interaction.
> **Expected trace:** (1) Request arrives at gateway node with tenant identified (API key/JWT, Lesson 12). (2) Look up the tenant's limit tier from local rules cache. (3) Build key `rl:{tenant}:{route}:{window}`. (4) Execute an **atomic Lua script** on Redis: read current count/tokens, compute refill based on elapsed time, decide allow/deny, write back new state, set EXPIRE — all in one round-trip so no read-modify-write race. (5) Allow → forward request, add `X-RateLimit-Remaining` / `X-RateLimit-Reset` headers. (6) Deny → return `429 Too Many Requests` with `Retry-After`. (7) Emit decision event async.
> **Tricky part:** Candidates split the read and write into two Redis calls — that's a race under concurrency. The refill computation *and* the decrement must be one atomic Lua execution. Also the `EXPIRE` must be set every time (or the key leaks memory forever).

Q6. Design the API/contract for configuring and reporting limits.
> **Expected API design:**
> - Config: `PUT /tenants/{id}/limits` `{ "route": "/v1/*", "algo": "token_bucket", "rate": 100, "burst": 200, "window": "1s" }`.
> - Response headers on every limited call: `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset` (epoch), and on 429: `Retry-After` (seconds).
> - Standard body on reject: `429 { "error": "rate_limited", "retry_after": 3 }`.
> **What to push on:** Headers must be **accurate** or clients build bad backoff. `Retry-After` should reflect real token-refill time, not a constant. Versioning of the limit policy so a mid-flight config change is atomic per tenant. Idempotency isn't the concern here; **clock semantics** are — `Reset` must be server-authoritative.

---

### Data Modeling (Medium–Hard)
Q7. Model the state each algorithm stores in Redis.
> **Expected schema:**
```
# Fixed / sliding window counter:
KEY  rl:{tenant}:{route}:{window_id}   VALUE int (INCR)   EXPIRE = 2×window
# Token bucket (hash):
KEY  rl:tb:{tenant}:{route}
     HSET  tokens <float>   last_refill_ts <epoch_ms>     EXPIRE = idle_timeout
# Sliding window LOG (exact) via sorted set:
KEY  rl:log:{tenant}:{route}
     ZADD <ts> <ts>                       # member=timestamp
     ZREMRANGEBYSCORE key 0 (now-window)  # drop old
     ZCARD key                            # current count
```
> **Index choices and why:** Sorted set gives O(log N) insert + O(log N + M) range-trim for the exact log; the counter is O(1) INCR. Choose per accuracy need.
> **Partitioning key and why:** Partition Redis by the **full rate-limit key** (`{tenant}:{route}`) via consistent hashing (Lesson 19). This co-locates all state for one tenant-route on one shard so the Lua script is single-shard atomic. Partitioning by tenant alone would hot-spot a whale tenant onto one shard.

Q8. A whale tenant sends 500K req/s — all their keys hash to one Redis shard. How do you serve that efficiently?
> **Expected answer:** (1) **L1 local buckets** absorb most decisions: each gateway node is allocated a *slice* of the tenant's global budget (e.g., global 500K/s ÷ 50 nodes = 10K/s local), enforced in-process with zero Redis calls; Redis only reconciles drift periodically. (2) For the portion that must be centrally coordinated, **spread the tenant's key across N sub-shards** (`{tenant}:{route}:{shard 0..N}`) and sum — trades exactness for throughput. (3) Cache the decision for very short windows.
> **Trap:** Doing one synchronous Redis Lua call per request for a 500K/s tenant — that single shard becomes the bottleneck (~100–200K ops/sec ceiling per node) and adds a network RTT to every request.

Q9. Distributed counter accuracy vs availability — what consistency do you actually need?
> **Expected answer:** Perfect global accuracy requires every decision to serialize through one authority — unacceptable latency. So you accept **bounded over-admission**: the local-bucket approach may let a tenant exceed their limit by roughly (nodes × local slice error) during sync gaps. This is CAP in action (Lesson 2): you choose **AP** — always make a fast local decision, reconcile eventually. Over-admitting a few percent is fine; adding 20ms of coordination latency to every API call is not.
> **Mentor pushback:** For a **billing/quota** limit (tenant pays per 1M calls, hard cap), over-admission means giving away free calls or overcharging. There you need stronger accuracy — centralized atomic counting or a sliding-window log — even at higher latency. So the consistency choice depends on whether the limit is *protective* (AP fine) or *contractual* (needs CP-ish accuracy). Naming that distinction is the senior answer.

---

### Low-Level Design (Hard)
Q10. Implement the token-bucket decision atomically in Redis so concurrent gateway nodes can't over-spend.
> **Problem statement:** N stateless nodes hit the same `rl:tb:{tenant}:{route}` key concurrently. Each must refill-by-elapsed-time then decrement one token, with no lost updates.
> **Naive solution:** `GET tokens`, compute in app, `SET tokens`. Two nodes read the same value, both allow — lost update, over-admission.
> **Why naive fails at scale:** Read-modify-write across the network is not atomic; under 500K/s the interleaving happens constantly, breaking the limit exactly when it matters (heavy load).
> **Expected optimal approach:** A single **Redis Lua script** (Redis executes scripts atomically, single-threaded) that reads tokens + last_refill, computes refill = elapsed × rate capped at burst, checks/decrements, writes back, sets EXPIRE — all server-side, one RTT.
> **Pseudo-code or class diagram:**
```lua
-- KEYS[1]=bucket key  ARGV: now_ms, rate_per_ms, burst, cost
local b = redis.call('HMGET', KEYS[1], 'tokens', 'ts')
local tokens = tonumber(b[1]) or tonumber(ARGV[3])   -- start full
local ts     = tonumber(b[2]) or tonumber(ARGV[1])
local refill = (tonumber(ARGV[1]) - ts) * tonumber(ARGV[2])
tokens = math.min(tonumber(ARGV[3]), tokens + refill) -- cap at burst
local cost = tonumber(ARGV[4])
if tokens >= cost then
  tokens = tokens - cost
  redis.call('HMSET', KEYS[1], 'tokens', tokens, 'ts', ARGV[1])
  redis.call('PEXPIRE', KEYS[1], 60000)
  return 1                                             -- ALLOW
else
  redis.call('HMSET', KEYS[1], 'tokens', tokens, 'ts', ARGV[1])
  redis.call('PEXPIRE', KEYS[1], 60000)
  return 0                                             -- DENY
end
```

Q11. Clock skew: gateway node A's clock is 3s ahead of node B's. Both compute token refill from `now`. What breaks?
> **Scenario:** Refill = elapsed × rate uses `now - last_refill_ts`. If different nodes pass their own skewed `now`, refill is miscomputed — a fast clock grants extra tokens.
> **Expected fix:** Use a **single authoritative clock — Redis's own `TIME` command** inside the Lua script — never the gateway's local clock, for the refill math. All nodes then reason against one monotonic source. Keep node clocks NTP-synced anyway for logging/headers, but the *decision* clock is Redis's. This mirrors the leader/coordination lesson (Lesson 17): one source of truth beats N drifting ones.
> **Follow-up:** What if Redis fails over to a replica whose clock differs? Use `PEXPIRE`-based relative TTLs and elapsed deltas rather than absolute cross-node comparisons, and pin the TIME source to the primary.

Q12. Redis cluster is unreachable (partition or full outage). What does the limiter do on each request?
> **Scenario:** Central state store down during peak (Lesson 2 partition).
> **Expected handling:** Policy-driven **fail-open vs fail-closed**, wrapped in a circuit breaker (Lesson 10): when Redis errors exceed a threshold, the breaker opens and requests fall back to **L1 local buckets only** (approximate, per-node limits) — this is *fail-open-ish*: protective limits still roughly hold, availability preserved. For contractual/billing limits you may **fail-closed** (reject or downgrade to a conservative local cap) to avoid giving away unmetered usage. Never let a Redis outage either (a) crash the request path or (b) silently remove all limits and let a DDoS through. Emit a loud alert; degrade, don't collapse.

---

### Scaling to 10x / 100x (Hard)
Q13. At 10M decisions/sec across the fleet, where does it break first?
> **Expected answer:** The **shared Redis tier** — specifically hot shards for whale tenants and the aggregate ops/sec ceiling. A Redis node does ~100–200K ops/sec; 10M/s naively = 50–100 nodes *if every request hits Redis*. That's why the L1 local-bucket layer is mandatory at this scale — it must absorb 90%+ of decisions so Redis only sees reconciliation traffic. Second bottleneck: config propagation lag when thousands of tenants have distinct policies.
> **Numbers to ground the answer:** 10M/s, target <1ms added latency. A Redis RTT within a DC is ~0.2–0.5ms — acceptable per-call but not at 10M/s of them. With 90% served locally, Redis sees ~1M/s → ~6–10 shards. Latency budget: L1 decision is ~microseconds.

Q14. Shard the limiter state. What key, and how do you handle a hot tenant?
> **Expected sharding strategy:** Consistent hashing (Lesson 19) on `{tenant}:{route}` so all state for a tenant-route is single-shard (atomic Lua stays single-shard). Even distribution across tenants; adding a shard remaps ~1/N.
> **Hot spot problem:** A whale tenant's `{tenant}:{route}` is one key → one shard, unsplittable by hashing alone. Detect via per-shard ops/sec and per-key top-N. Fix: (a) **local budget slicing** — give each gateway node a fixed fraction of the tenant's limit, no Redis on the hot path; (b) **key sub-sharding** — split into `{tenant}:{route}:{0..k}` and enforce `limit/k` per sub-key, summing approximately. Both trade exactness for spread.

Q15. Caching / layering strategy for the limiter itself.
> **Expected layered cache design:** **L1** — in-process per-node token buckets holding a slice of each tenant's budget, refreshed from Redis on a short interval or when local tokens run low. **L2** — Redis cluster as the authoritative shared counter. **Config cache** — per-tenant rules pulled from the config service and cached locally with a short TTL + push invalidation so tier changes propagate in seconds. There's no CDN layer here — decisions are per-request and can't be edge-cached — but the L1/L2 split is the caching insight.
> **Cache invalidation trap:** When a tenant's tier changes (e.g., upgraded to a higher limit), stale L1 buckets on the fleet keep enforcing the old cap. Invalidate by pushing config version bumps to all nodes and expiring local buckets on version change; otherwise a customer pays for an upgrade and still gets throttled — a support nightmare.

Q16. Keep it efficient/cheap at 100x.
> **Expected answer:** (1) **Local-first decisions** eliminate most Redis calls — the single biggest cost/latency win. (2) **Batch reconciliation** — sync local↔Redis in aggregate, not per request. (3) Use **counters (O(1) INCR) over sorted-set logs** wherever exactness isn't contractual — logs are memory-heavy (one entry per request). (4) Aggressive **EXPIRE** so idle tenant keys don't accumulate in Redis memory. (5) Async, sampled metrics rather than per-decision logging to the pipeline. (6) Co-locate Redis shards with gateway AZs to cut RTT and cross-AZ egress.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Derive the exact sliding-window-counter formula and its error bound vs a true sliding log. (`estimate = curr_count + prev_count × (1 − elapsed_in_curr / window)`. It assumes uniform distribution within the previous window; worst-case error occurs when previous traffic was front/back-loaded — bounded by prev_count. Explain when that error is acceptable: protective limits yes, billing no.)

**H2.** Multi-tenancy fairness: one tenant's abuse must not degrade others even though they share the Redis shard and gateway CPU. How? (Per-tenant keys isolate counters; add **bulkheading** — CPU/connection quotas per tenant at the gateway (Lesson 10) — so a whale can't monopolize the shared decision path. Separate Redis shard pools for top-tier tenants if needed.)

**H3.** Roll out a new limit policy fleet-wide with zero dropped-or-double-counted requests. (Version the policy; nodes atomically swap to the new version on a config push; carry over existing bucket state (don't reset counters to zero mid-window or you grant a free burst). Canary the policy on a subset of nodes first, watch 429 rate.)

**H4.** What do you instrument and alert on? (429 rate per tenant/route, allow/deny ratio, Redis op latency p99, per-shard ops/sec (hot-shard detection), L1↔L2 drift/over-admission estimate, circuit-breaker open events, config-propagation lag. Alert on sudden 429 spikes (attack or a bad config push) and on Redis latency creeping — the leading indicator of the whole limiter degrading.)

**H5.** You shipped a sliding-window **log** (sorted set per request) and Redis memory is exploding at scale. Tell the migration to sliding-window **counter**. (Recognize the log stores O(requests) members — untenable at 10M/s. Migrate per-route: dual-run counter+log on a sample to validate the counter's error is acceptable, then cut over, keeping the log only for the few routes needing exactness/audit. Lesson: don't buy exactness you don't need — it costs memory linearly in traffic.)

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. **Non-atomic read-modify-write** on the shared counter — the single most common bug. It must be one atomic Lua (or `INCR`) operation.
2. **Assuming one central Redis call per request scales** — at millions/sec you need L1 local buckets absorbing most decisions; central-only is a latency and cost trap.
3. **Not specifying fail-open vs fail-closed** for a Redis outage — leaving the most important resilience decision undefined.

**The one insight that makes an answer truly impressive:**
Frame the whole design around the accuracy-vs-coordination trade-off: *protective* limits can be approximate and AP (local buckets, eventual reconciliation), while *contractual/billing* limits need exactness and pay for coordination. One system, two consistency regimes chosen per limit type — that's the senior lens.

**Suggested follow-up reading:**
- Stripe Engineering — "Scaling your API with rate limiters" (token bucket + Redis, the tiered-limit taxonomy).
- Cloudflare blog — "How we built rate limiting capable of scaling to millions of domains" (sliding window counter, sharded counters).

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
