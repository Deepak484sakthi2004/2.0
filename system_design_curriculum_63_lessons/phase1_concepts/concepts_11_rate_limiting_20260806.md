# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 11 of 63 — Phase 1: Foundations (Module 11 of 28)
**Module:** Rate Limiting
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Every public API you'll ever design needs rate limiting, and interviewers reach for it constantly because it forces you to reason about counters, time windows, memory, and distributed coordination all at once. Stripe caps you at 100 requests/second; GitHub gives 5,000 requests/hour; Twitter's rate limits are legendary. Without limiting, one buggy client in a retry loop or one attacker can exhaust your database connections and take everyone down — the "noisy neighbor" that becomes an outage. Today you'll learn the four core algorithms cold, the dimensions you can limit on, and how to enforce limits across a fleet where no single server sees all the traffic.

## Learning Objectives
By the end of this lesson you can:
- Implement and contrast token bucket, leaky bucket, fixed window, and sliding window.
- Explain the "double-burst" boundary flaw in fixed windows and how sliding windows fix it.
- Compute whether a request is allowed given rate, burst, and elapsed time — with real numbers.
- Choose a limiting dimension: per user, per IP, concurrency, priority, and drop-low-priority.
- Design distributed rate limiting with a shared store and handle its race conditions.

## The Lesson

### Token Bucket
**What it is (plain English):** Imagine a bucket that refills with tokens at a steady rate up to a cap. Each request must take one token to proceed; if the bucket is empty, the request is rejected (or waits). It allows short **bursts** (spend saved-up tokens) but limits the long-run average.

**The problem it solves:** Fixed-rate limiters reject legitimate bursty traffic (a user opening 10 tabs at once). Token bucket permits bursts up to the bucket size while still capping the sustained rate — the best of both.

**How it works (mechanics):** Two parameters: **refill rate** r (tokens/sec) and **capacity** b (max tokens). On each request: refill `tokens = min(b, tokens + elapsed × r)`, then if `tokens ≥ 1`, allow and subtract 1; else reject.
Worked example: r = 10 tokens/s, b = 10, bucket full.
```
t=0.0  10 requests hit at once → spend all 10 tokens → all allowed (burst!)
t=0.0  11th request → 0 tokens → REJECTED (429)
t=0.5  refill: 0 + 0.5×10 = 5 tokens → next 5 allowed
```
So it absorbs a burst of 10, then throttles to the 10/s refill. Just two numbers stored per key (token count + last-refill timestamp) — very memory-cheap.

**Trade-offs / when NOT to use it:** Bursts can briefly overwhelm a downstream that can't handle 10-at-once even if the average is fine — if you need *strictly smooth* output, use leaky bucket instead. Tuning capacity vs rate is a judgment call.

**Where you'll see it:** AWS API Gateway, Stripe, and most cloud APIs use token bucket; it's the default in NGINX (`limit_req` with burst) and Envoy.

### Leaky Bucket
**What it is (plain English):** Requests pour into a bucket (a FIFO queue) and **leak out at a constant rate**, like water through a hole. It smooths bursty input into a steady output stream. If the bucket overflows (queue full), new requests are dropped.

**The problem it solves:** Downstream systems (a payment processor, a legacy DB) often need a *steady* request rate, not bursts. Leaky bucket guarantees the output never exceeds the leak rate, no matter how spiky the input.

**How it works (mechanics):** Parameters: **leak rate** (requests/sec out) and **queue capacity**. Incoming requests enqueue; a scheduler dequeues at the fixed leak rate.
```
Bursty in:  ||||  ||||||||       (arrives in clumps)
Queue:      [ ][ ][ ][ ][ ]      (buffers, capacity 5)
Steady out: | | | | | | |        (leaks 1 every 100ms = 10/s)
```
Worked example: leak rate 10/s, capacity 5. A burst of 8 arrives instantly: 5 enqueue, 3 are **dropped** (overflow). The queue then drains at exactly 1 per 100 ms — output is perfectly smooth at 10/s.

**Trade-offs / when NOT to use it:** It adds **latency** — a request may sit in the queue waiting its turn (up to capacity/leak-rate = 5/10 = 500 ms here). Under sustained overload it drops the *newest* requests. Not ideal when you actually want to allow bursts (use token bucket) or when latency is sacred.

**Where you'll see it:** Traffic shaping in networking hardware, Shopify's older limiter, and any pipeline feeding a rate-sensitive downstream (e.g., outbound email/SMS gateways).

### Fixed Window
**What it is (plain English):** Divide time into fixed buckets (e.g., each 1-minute clock window) and count requests per window. Allow up to N per window; reset the counter when the window rolls over. Simple as a tally on the wall that you wipe each minute.

**The problem it solves:** You want a dead-simple, cheap limiter: one counter per key per window, one `INCR`. Great when approximate limiting is acceptable.

**How it works (mechanics):** Store `count[key:window]`. On request, increment; if `count > N`, reject.
Worked example: limit = 100/min.
```
Window 10:00:00–10:00:59 → allow first 100, reject rest
Window 10:01:00–10:01:59 → counter resets to 0
```
Only one integer per key per window — extremely memory-light and fast.

**The flaw (double burst):** The counter resets on a clock boundary, so a client can send N at the *end* of one window and N at the *start* of the next:
```
100 requests at 10:00:59  ✓
100 requests at 10:01:00  ✓   ← 200 requests in a 2-second span!
```
That's **2× the intended rate** across the boundary — a real DoS vector.

**Trade-offs / when NOT to use it:** The boundary burst makes it unsuitable when the limit must be strict. It's fine for coarse quotas ("5,000/hour") where a brief 2× spike is tolerable.

**Where you'll see it:** GitHub's hourly limits, many simple internal quotas; often the first thing teams build before hitting the boundary bug.

### Sliding Window (Log & Counter)
**What it is (plain English):** A sliding window measures the *last* N seconds continuously rather than snapping to clock boundaries, eliminating the double-burst problem. Two flavors: the exact **log** and the approximate **counter**.

**The problem it solves:** It fixes fixed-window's boundary flaw: "no more than 100 requests in *any* rolling 60-second span," which is what you actually meant.

**How it works (mechanics):**

*Sliding window **log*** — store the timestamp of every request in a sorted set. On each request, drop timestamps older than `now − window`, then count what remains; allow if `< N`.
```
now=10:01:30, window=60s → keep only ts ≥ 10:00:30
if count in [10:00:30, 10:01:30] < 100 → allow
```
Exact, but stores up to N timestamps per key — 100 entries/user × millions of users = a lot of memory.

*Sliding window **counter*** — approximate, cheap. Keep the current and previous fixed-window counts and weight the previous one by how much of it still overlaps:
`estimate = curr + prev × (overlap fraction)`.
Worked example: limit 100/min. Previous minute had 90, current minute so far 20, and we're 25% into the current window (75% of the previous still overlaps):
`20 + 90 × 0.75 = 87.5 ≤ 100 → allow`. One more heavy burst pushes it over 100 and it rejects — smoothing away the boundary spike with just **two counters** per key.

**Trade-offs / when NOT to use it:** The log is exact but memory-heavy; the counter is ~99.9% accurate (Cloudflare measured a ~0.003% error rate) but assumes even distribution within a window. Choose log when exactness is legally/financially required, counter otherwise.

**Where you'll see it:** Cloudflare uses the sliding-window counter across its edge; Redis sorted-sets (`ZADD`/`ZREMRANGEBYSCORE`) implement the log.

### User / Concurrency / Location-ID / Server / Priority-Session & Drop-Low-Priority Rate Limiting
**What it is (plain English):** The *algorithm* decides "how many," but you also choose *what key* to limit on and *what to do* when over. These are the dimensions and policies.

**The problem it solves:** A single global limit is too blunt. You want fairness per user, protection per server, differentiation by customer tier, and graceful behavior under overload rather than a hard wall for everyone.

**How it works (mechanics):**
- **Per-user (API key):** key the limiter on user ID — "1,000 req/hour per account." Ensures one user can't starve others.
- **Per-IP / location-ID:** key on client IP or region — blocks a single abusive source; useful pre-login when there's no user ID. (Watch NAT: many users can share one IP.)
- **Concurrency limiting:** cap *in-flight* requests, not rate — "max 5 simultaneous requests per user." Protects against slow, heavy queries hogging connections regardless of arrival rate.
- **Per-server:** each instance caps its own load ("2,000 QPS/node") as a self-protection backstop even if the global limiter fails.
- **Priority-session / tiered:** premium users get a bigger bucket (10,000/hr) than free (100/hr) — business tiering.
- **Drop-low-priority (load shedding):** under overload, reject low-value traffic (batch, analytics, free tier) first and protect high-value (checkout, paid) traffic.
```
Overload → shed priority 3 (batch) → then 2 (free) → protect 1 (paid checkout)
```
Worked example: node capacity 2,000 QPS, inbound 2,500. Shed the 400 QPS of priority-3 analytics and 100 of free-tier reads, keeping all paid checkout traffic served.

**Trade-offs / when NOT to use it:** IP limiting punishes NATed users unfairly; concurrency limiting needs accurate in-flight accounting; tiering and shedding require classifying every request, adding complexity. Over-segmenting keys multiplies counters and memory.

**Where you'll see it:** Stripe/Twilio tier by API key; AWS sheds/prioritizes; Envoy and Netflix's concurrency-limits library cap in-flight requests adaptively.

### Distributed Rate Limiting
**What it is (plain English):** When many servers behind a load balancer each see only *part* of a user's traffic, no single node knows the true total. Distributed rate limiting coordinates a global count across all of them.

**The problem it solves:** With 10 API servers and a per-server limit of 100/min, a user could actually do 1,000/min — 10× the intended limit — because each node counts independently. You need one shared source of truth.

**How it works (mechanics):** Keep the counter in a shared fast store (Redis). Every node does an atomic increment on the same key:
```
INCR ratelimit:user42:10:01   → returns new global count
if returned > 100 → reject
EXPIRE the key at window end
```
Because `INCR` is atomic, concurrent nodes don't corrupt the count. Real numbers: Redis handles ~100K+ ops/sec on one node, and each check is a ~1 ms round trip. To avoid a per-request Redis hit, nodes often use a **local token bucket refilled from Redis in batches** (e.g., grab 20 tokens at a time) — trading a little precision for far fewer network calls. Race conditions on read-modify-write are solved with atomic `INCR`, Lua scripts (check-and-set in one round trip), or Redis's `CL.THROTTLE`.

**Trade-offs / when NOT to use it:** The shared store adds ~1 ms latency and is a single point of failure — if Redis is down you must **fail open** (allow) or **fail closed** (reject); most choose fail-open to protect availability. Perfect global accuracy costs a network call per request; batching trades exactness for throughput.

**Where you'll see it:** Redis-backed limiters everywhere; Cloudflare and Kong do edge-distributed limiting; Lyft's `ratelimit` service backs Envoy globally.

## Comparison Table

| Dimension | Token Bucket | Leaky Bucket | Fixed Window | Sliding Log | Sliding Counter |
|---|---|---|---|---|---|
| Allows bursts | Yes (up to b) | No (smooths) | At boundary (bug) | No | No |
| Output shape | Bursty→capped | Perfectly smooth | Stepwise | Smooth | Smooth |
| Memory/key | 2 values | queue | 1 counter | N timestamps | 2 counters |
| Accuracy | Good | Exact rate | Poor at edges | Exact | ~99.9% |
| Latency added | None | Queuing delay | None | None | None |

**Verdict:** Token bucket is the sensible default (burst-friendly, cheap); leaky bucket when you need smooth output; sliding-window counter when you need strict "any rolling window" accuracy without the log's memory cost.

## Common Misconceptions
- **Myth:** Fixed window and sliding window behave the same. → **Reality:** Fixed window allows a 2× burst across the boundary; sliding window closes that hole.
- **Myth:** Token bucket and leaky bucket are the same thing. → **Reality:** Token bucket allows bursts and adds no latency; leaky bucket forbids bursts and queues (adds latency).
- **Myth:** Per-server limits give you a global limit. → **Reality:** With N servers you get up to N× the limit; you need a shared store for a true global cap.
- **Myth:** Rate limiting is just about blocking attackers. → **Reality:** It's mainly fairness and self-protection — stopping one client (often a buggy retry loop) from starving the rest.
- **Myth:** Sliding-window log is always best because it's exact. → **Reality:** Its memory (N timestamps/user) is often prohibitive; the counter is 99.9% accurate for a fraction of the cost.

## Real-World Case
In 2017 Stripe published how they rate-limit their API, and the design is a masterclass. They run multiple limiters at once: a **request-rate limiter** (token bucket per API key) to cap sustained calls, and a **concurrency limiter** to cap simultaneous in-flight requests — because a handful of slow, expensive requests can exhaust workers even when the request *rate* looks fine. They also **load-shed by priority**, reserving capacity for critical traffic (charges) and shedding less-critical calls first during incidents. All of it is backed by Redis for a shared global count, and they deliberately return a clear `429` with a `Retry-After` header so well-behaved clients back off. The takeaway: real rate limiting is several coordinated limiters on different dimensions, not one magic number.

## Self-Test (answers at the bottom)
1. In one sentence each, how do token bucket and leaky bucket differ in their treatment of bursts?
2. Explain the fixed-window boundary bug with a concrete two-window example.
3. Token bucket: r = 5 tokens/s, capacity = 5, bucket empty at t=0. A request arrives at t=0.4s. Is it allowed? Show the math.
4. You run 8 API servers, each enforcing 50 req/min per user locally. What's the real per-user limit, and how do you fix it?
5. Design sketch: Design rate limiting for a payments API with free (100/hr) and premium (10,000/hr) tiers, protection against a single abusive IP before login, and graceful behavior when a node is overloaded. Name the algorithm, the keys, and the overload policy.

## Interview Soundbites
- "Token bucket allows bursts and adds no latency; leaky bucket smooths to a constant output but queues — I pick based on whether the downstream can tolerate bursts."
- "Fixed windows have a boundary bug that lets through 2× the limit across the reset; sliding-window counter fixes it with just two counters and ~99.9% accuracy."
- "With N servers you need a shared atomic counter in Redis for a true global limit — otherwise each node counts independently and you allow N× the intended rate."

## Mini-Assignment
(~30 min) Implement (in pseudocode) two limiters for a 100-request-per-minute limit: (1) a token bucket — store token count and last-refill timestamp, refill on each request, and trace 15 requests arriving in a burst at t=0 then one every 50 ms, marking allow/reject; (2) a sliding-window counter — store previous and current window counts, and compute the weighted estimate for a request arriving 20 seconds into the current minute when the previous minute saw 80 requests and the current has 30 so far. State whether it's allowed and why.

## Recap & Tomorrow
- **Token bucket:** refill r/s up to capacity b; allows bursts, no added latency, two values/key — the default.
- **Leaky bucket:** FIFO queue leaking at a constant rate; perfectly smooth output but adds queuing latency and drops overflow.
- **Fixed window:** one counter/window; cheapest but allows a 2× burst across the boundary.
- **Sliding window:** log (exact, N timestamps) or counter (weighted estimate, ~99.9%, two counters) — fixes the boundary bug.
- **Dimensions/policies:** per-user, per-IP/location, concurrency, per-server, tiered priority, and drop-low-priority load shedding.
- **Distributed:** shared atomic counter (Redis `INCR`) for a true global limit; batch tokens locally to cut round trips; decide fail-open vs fail-closed.

Tomorrow, **Lesson 12 — OAuth & Authentication**: who's allowed to make those requests in the first place — auth/authorization/resource servers, OAuth2 flows, JWTs, and sessions vs tokens.

## Self-Test Answers
1. Token bucket **allows** bursts up to its capacity (spend saved-up tokens) while capping the average and adds no latency; leaky bucket **forbids** bursts, buffering and releasing at a constant rate so output is smooth (at the cost of queuing delay).
2. Limit 100/min. A client sends 100 requests at 10:00:59 (allowed in that window) and another 100 at 10:01:00 (a new window, counter reset). That's 200 requests in a ~1-second span — 2× the intended rate — because the counter snapped to the clock boundary.
3. At t=0.4s the bucket has refilled `0 + 0.4 × 5 = 2` tokens (capped at 5). 2 ≥ 1, so the request is **allowed** and one token is consumed, leaving 1.
4. Real limit is up to 8 × 50 = **400 req/min** per user, because each server counts independently. Fix by moving the counter to a shared store (Redis) and doing an atomic `INCR` on one key per user+window so all 8 nodes share one global count (or give each node a fair share of the global budget coordinated centrally).
5. Use **token bucket per API key** sized by tier (free bucket 100/hr, premium 10,000/hr), keyed on user ID, with counts in Redis via atomic `INCR` for a global limit. Add a **per-IP** limiter for pre-login endpoints to stop a single abusive source. Add a **per-server** concurrency cap as self-protection, and under node overload **drop low-priority** traffic first (analytics/free reads) while protecting paid charge requests. Return `429` with `Retry-After`; fail-open if Redis is unreachable to preserve availability.
