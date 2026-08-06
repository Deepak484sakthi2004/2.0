# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 37 of 63 — Phase 2: System Design Track (Design 9 of 35)
**Topic:** API Gateway
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Every microservices company — Netflix (Zuul), Amazon (API Gateway), Kong, Lyft/Envoy — eventually builds or buys an API gateway because the alternative is chaos: 300 services each re-implementing auth, rate limiting, TLS, retries, and logging, inconsistently. The gateway is the single front door: one place for cross-cutting concerns, one place clients talk to, one place to enforce policy. What makes it hard is that it sits on the **critical path of 100% of traffic** — every millisecond of gateway latency and every fraction of a percent of gateway unavailability multiplies across the whole platform, so it must be blisteringly fast, near-perfectly available, and stateless enough to scale horizontally while still doing stateful-feeling things like rate limiting and auth. Get it wrong and it becomes the bottleneck and the single point of failure it was supposed to eliminate.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Microservices / API gateway / REST / DDD (taught in Phase 1, Lesson 9):** The gateway is the edge of a microservices topology. It does request routing, protocol translation, and API composition, and defines the boundary between the public API contract and internal service decomposition (DDD bounded contexts). Today we build it: routing table, plugins, and the BFF pattern.

**Load balancers (taught in Phase 1, Lesson 6):** A gateway is an L7 (application-layer) reverse proxy — it reads HTTP headers/paths to route, unlike an L4 LB that only sees IP:port. It does content-based routing, and itself sits behind an L4 LB for horizontal scale. Health checks, connection pooling, and LB algorithms (round-robin, least-connections, EWMA) all apply to upstream selection.

**Rate limiting (taught in Phase 1, Lesson 11):** The single most important gateway plugin. Token bucket / sliding window log / sliding window counter, enforced centrally so one abusive client can't take down the fleet. Distributed rate limiting across gateway nodes (shared Redis counters) is a core hard problem today.

**OAuth / JWT (taught in Phase 1, Lesson 12):** The gateway terminates authentication — validates JWTs (signature via JWKS), or exchanges OAuth tokens, so downstream services trust an internal identity header. Stateless JWT validation vs. token introspection trade-off is central.

**Circuit breaker / observability (taught in Phase 1, Lesson 10):** The gateway wraps every upstream call in a circuit breaker (Hystrix/resilience4j pattern) so one slow service doesn't exhaust gateway threads and cascade. It's also the ideal place to emit uniform metrics, traces (inject trace IDs), and access logs for every request.

**TLS / WAF / Zero Trust (taught in Phase 1, Lesson 25):** The gateway terminates TLS (offloading crypto from services), is the natural home for a WAF (OWASP rules, injection/XSS filtering), and enforces zero-trust: authenticate every request at the edge, mTLS to upstreams.

**Networking / DNS / OSI (taught in Phase 1, Lesson 7):** Gateway operates at L7 but cares about the whole stack — keep-alive connections, HTTP/2 multiplexing to clients, connection pooling to upstreams, DNS-based service discovery.

> **Recap box:**
> - Gateway = L7 reverse proxy = single front door for cross-cutting concerns.
> - Must be stateless & horizontally scalable; shared state (rate-limit counters) goes to Redis.
> - Terminates TLS + auth (JWT/OAuth) so upstreams trust an internal identity header.
> - Circuit breakers + timeouts + retries with budget prevent cascading failure.
> - It's on 100% of the critical path — latency and availability dominate every trade-off.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. What is an API gateway and what does it do that a plain L4 load balancer (Lesson 6) does not?
> **What a strong answer covers:** An L4 LB routes on IP:port and forwards TCP without understanding content. A gateway is an **L7 reverse proxy** that reads the HTTP request (path, method, headers, body) and does content-based routing, plus cross-cutting concerns: authentication/authorization, rate limiting, TLS termination, request/response transformation, API composition/aggregation, protocol translation (REST↔gRPC), caching, and observability injection (trace IDs, metrics, logs). The one-liner: a gateway centralizes what would otherwise be duplicated in every microservice.
> **Common weak answer:** "It's a load balancer for microservices." Conflates L4 and L7 and misses that the gateway's job is cross-cutting *policy*, not just distribution.
> **Mentor follow-up if they answer well:** If it does all that on 100% of traffic, what's the risk you've just created and how do you mitigate it? (It's a new SPOF/bottleneck — mitigate with statelessness + horizontal scale + a dumb L4 LB in front, and keep the data plane hot-path minimal.)

Q2. Estimate the gateway fleet size for a platform doing 200k requests/sec at peak, if each gateway node handles ~10k rps at acceptable latency and CPU. Include headroom.
> **What a strong answer covers:** 200k ÷ 10k = 20 nodes at 100% — never run at 100%. Target ~50–60% CPU for headroom against spikes and node failures → ~34–40 nodes. Add N+2 for AZ failure tolerance and rolling deploys. Round to **~40 nodes across 3 AZs** (~14/AZ so losing one AZ still serves). Per-node memory dominated by connection buffers + JWKS/route-table cache, modest (a few GB). Note the real constraint is often not CPU but **file descriptors / connection count** (keep-alive to clients + pooled connections to hundreds of upstreams).
> **Mentor follow-up:** Your p99 latency added by the gateway — what's an acceptable budget and where does it go? (Target <5–10ms added; budget goes to TLS handshake amortized by keep-alive, JWT verify ~sub-ms with cached keys, rate-limit Redis check ~1ms, routing lookup ~µs.)

Q3. JWT validation at the gateway: validate the signature locally, or call the auth service to introspect every token? Trade-off?
> **What a strong answer covers:** **Local JWT validation** (verify signature with the issuer's public key fetched from JWKS, check `exp`, `aud`, `iss`) is stateless, fast (sub-ms), and requires no network call — the default for scale. **Introspection** (`/introspect` call to the auth server per request) gives instant revocation and works with opaque tokens, but adds a network hop and load on the auth service on every request. The standard answer: local validation for the hot path + short token TTLs (5–15 min) so revocation lag is bounded, plus a revocation/denylist check (small, cached in Redis) for the "must revoke now" case (compromised token, logout).
> **Red flag answer:** "Introspect every token" (kills latency and makes auth service a SPOF) or "trust the token without checking `aud`/`exp`/signature" (accepts forged/expired tokens).

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the high-level architecture of an API gateway. Draw it. Separate control plane from data plane.
> **Key components expected:** L4 LB → stateless gateway data-plane nodes (the request-processing pipeline: TLS term → auth → rate limit → routing → transform → upstream call → response) → upstream microservices via service discovery; plus a **control plane** (config/route/policy store, admin API) that pushes config to data-plane nodes; shared **Redis** for distributed rate-limit counters and denylist; **JWKS cache** for keys; observability sinks (metrics, traces, logs).
> **Architecture diagram (text):**
```
                         DNS ──▶ L4 LB (per AZ, anycast/ELB)
                                      │
        ┌─────────────────┬──────────┼──────────┬─────────────────┐
        ▼                 ▼          ▼          ▼                 ▼
   ┌───────────────────────── DATA PLANE (stateless gateway nodes) ─────────────────────────┐
   │  Request pipeline (per request):                                                        │
   │  TLS term → WAF → AuthN(JWT/JWKS) → AuthZ → RateLimit(Redis) → Route match →             │
   │  Transform/compose → CircuitBreaker+Timeout+Retry → Upstream(mTLS) → Transform response  │
   └─────────────────────────────────────────────────────────────────────────────────────────┘
        │                 │                                       │
        ▼                 ▼                                       ▼
   Shared Redis      JWKS cache (public keys)             Service Discovery (Consul/K8s DNS)
   (rate counters,                                              │
    denylist)                              ┌───────────┬────────┼────────┬───────────┐
        ▲                                   ▼           ▼        ▼        ▼           ▼
        │                              Svc A        Svc B     Svc C    Svc D  ...   Svc N
   ┌── CONTROL PLANE ──┐   push config (routes,     (upstream microservices; mTLS)
   │ Admin API +       │──▶ policies, rate limits)
   │ Config Store (etcd)│    to all data-plane nodes (xDS / config sync)
   └───────────────────┘
        │
        └──▶ Metrics (Prometheus) / Traces (OTel) / Access logs (ELK)
```
> **What separates SDE2 from SDE3 here:** The SDE2 draws one gateway box. The SDE3 **splits control plane from data plane** (Envoy's model): the data plane is the hot path that must never block on config changes; the control plane distributes config asynchronously (xDS-style) so a config push never stalls live traffic, and a control-plane outage leaves the data plane running on last-known-good config. They also flag that the gateway must be *stateless* so any node handles any request, with shared state externalized to Redis.

Q5. Trace one request end-to-end: a mobile client calls `GET /api/v2/orders/123` through the gateway. What happens at each hop?
> **Expected trace:**
> 1. DNS → L4 LB → picks a gateway node (least-conn/round-robin), reuses a keep-alive connection.
> 2. **TLS termination** (session resumption if returning client). WAF rule scan on the request.
> 3. **AuthN:** extract Bearer JWT, verify signature against cached JWKS public key, check `exp`/`aud`/`iss`, check Redis denylist for revocation. Attach internal identity header (`X-User-Id`, scopes) and strip any client-supplied identity headers (spoofing defense).
> 4. **AuthZ:** does this identity/scope allow `GET /orders`? (policy check, cached).
> 5. **Rate limit:** atomic token-bucket decrement in Redis keyed by `userId:route`. If exhausted → `429` with `Retry-After`, short-circuit.
> 6. **Route match:** longest-prefix/path-pattern match `/api/v2/orders/*` → `order-service` v2; resolve instance via service discovery.
> 7. **Circuit breaker check** for order-service; if open, fail fast → fallback/`503`. Else forward with timeout (say 800ms) over a pooled mTLS connection, inject `X-Trace-Id`/`X-Request-Id`.
> 8. Upstream responds; optional response transform (field filtering for mobile), record metrics/trace/log, return to client.
> **Tricky part:** Candidates forget **header hygiene** — the gateway must *strip* inbound `X-User-Id`/internal headers so a malicious client can't impersonate a user, then *set* them from the validated token. Also forgetting that trace-ID propagation must be injected *at the edge* so the whole downstream call tree is correlatable.

Q6. Design the routing configuration and admin API. How do teams register routes?
> **Expected API design:**
> - Route object: `{ id, match: {host, pathPrefix, methods, headers}, upstream: {service, versionWeights}, plugins: [auth, rateLimit(cfg), transform, cors], timeout, retries }`.
> - Admin/control-plane API: `POST /admin/routes`, `PUT /admin/routes/{id}`, `GET /admin/routes` — declarative, versioned, validated before publish, then pushed to data plane. GitOps-friendly (routes as YAML in a repo, CI applies).
> - Data plane exposes only the proxied API surface; admin API is on a separate port/network, access-controlled.
> **What to push on:** **Versioning** — support `/api/v2/` path or header-based versioning and weighted routing (`v1: 90%, v2: 10%`) for canaries. **Idempotency** — gateway should honor/propagate `Idempotency-Key` for safe retries of POSTs. **Longest-prefix match** determinism and conflict resolution when two routes overlap (specificity ordering). **Config validation & atomic swap** — a bad route push must be rejected or rolled back, never partially applied to some nodes.

---

### Data Modeling (Medium–Hard)
Q7. Model the routing table and rate-limit state. What lives where — in the data plane vs Redis vs control plane?
> **Expected schema:**
```
# CONTROL PLANE (etcd/Consul) — source of truth, pushed to data plane
routes: [
  { id, host, path_prefix, methods[], header_match{},
    upstream_service, version_weights{v1:90,v2:10},
    plugins:[ {type:"rate_limit", limit:1000, window:"1m", key:"user"},
              {type:"auth", mode:"jwt", jwks_uri},
              {type:"transform", ...} ],
    timeout_ms, retries, circuit_breaker{err_threshold, half_open_after} } ]
policies: { auth scopes, WAF rules, CORS }

# DATA PLANE (in-memory, hot) — compiled route trie for O(path length) match
route_trie: prefix-tree of path segments -> route handler + plugin chain
jwks_cache: {kid -> public_key}, refreshed on rotation

# REDIS (shared cross-node state)
rl:{userId}:{route}   -> token-bucket {tokens, last_refill_ts}   (TTL = window)
denylist:{jti}        -> revoked token id                        (TTL = token exp)
```
> **Index choices and why:** Compile routes into a **trie/radix tree** keyed by path segments so matching is O(path length), not O(#routes) linear scan — critical when you have thousands of routes. Header/method matching layered on the trie node.
> **Partitioning key and why:** Rate-limit counters partition naturally by `userId`/`apiKey` (Redis Cluster hash slot on the key) — spreads load and keeps one user's counter on one node for atomic ops. Denylist keyed by `jti` (JWT ID).

Q8. Distributed rate limiting: counters must be shared across 40 stateless gateway nodes. How, efficiently, without a Redis round-trip killing latency?
> **Expected answer:** Central counter in Redis using an **atomic token-bucket via a Lua script** (read tokens + last_refill, refill by elapsed×rate, decrement, write back — all atomic in one round trip, ~1ms). To avoid a Redis hit on *every* request, use a **two-tier / local-token-lease** approach: each gateway node leases a batch of tokens from Redis (e.g., 50 at a time), serves from its local bucket, and only re-leases when depleted — cutting Redis calls ~50x at the cost of slightly coarser global accuracy. Sliding-window-counter (two adjacent fixed windows, weighted) gives smoother enforcement than fixed-window without storing a full log.
> **Trap:** A local-only counter per node → the effective limit is `limit × node_count` (a 1000/min limit becomes 40,000/min with 40 nodes). Or a naive `GET`-then-`SET` on Redis → race condition where two nodes both read 1 remaining token and both allow. Must be atomic (Lua/`INCR` with expiry).

Q9. The control plane pushes a bad config (a route pointing to a dead upstream) to the data plane. Consistency vs availability — what does the data plane do?
> **Expected answer:** The data plane must favor **availability** over config freshness: it runs on **last-known-good** config and only atomically swaps to new config after validation. A config push is *eventually consistent* across nodes (some get v5 before others for a few seconds) — that's fine because routing is idempotent. But the swap on each node must be **atomic** (compile-then-flip a pointer), never partially applied. If a pushed config fails validation (syntax, dangling upstream), reject at the control plane; if it passes validation but the upstream is genuinely dead, the **circuit breaker** handles it at request time, not the config layer.
> **Mentor pushback:** What if two admins push conflicting routes simultaneously? Use optimistic concurrency (version/`If-Match` on the config store, etcd revision numbers) so the second push is rejected with a conflict, forcing a re-merge — never last-write-wins silently clobbering a route.

---

### Low-Level Design (Hard)
Q10. Hardest sub-problem: prevent a single slow upstream from exhausting the gateway and cascading to unrelated routes. Design the resilience layer.
> **Problem statement:** `payment-service` degrades to 5s latency. Requests to it pile up holding gateway threads/connections; if the gateway uses a shared thread pool, it starves and now *even `GET /products`* (a healthy service) times out. One sick service takes down the whole gateway.
> **Naive solution:** A single global timeout and one shared worker pool for all upstreams.
> **Why naive fails at scale:** Shared resources create **head-of-line blocking**: slow payment requests consume all threads/connections, so healthy routes can't get served — the classic cascading failure. A global timeout alone doesn't isolate; requests still occupy the pool for the full timeout.
> **Expected optimal approach:** **Bulkheading + circuit breaking + timeouts + bounded retries.** (1) **Bulkhead:** separate connection pool / concurrency limit *per upstream* so payment's saturation can't consume products' capacity. (2) **Per-upstream circuit breaker:** track error/timeout rate over a rolling window; when it crosses a threshold (e.g., >50% errors in 10s) → **open** → fail fast (immediate `503`/fallback, no thread held); after a cooldown → **half-open** → let a trickle through to test recovery → **closed** on success. (3) **Aggressive timeouts** (p99-based, e.g., 800ms) so no request lingers. (4) **Retries with a budget** — retry only idempotent requests, cap total retries at ~10% of traffic (retry budget) so retries don't amplify a brownout into an outage.
> **Pseudo-code or class diagram:**
```
class UpstreamProxy:
    breaker   = CircuitBreaker(err_threshold=0.5, window=10s, cooldown=5s)
    bulkhead  = Semaphore(max_concurrent=200)   # per-upstream isolation
    pool      = ConnectionPool(size=200)

    def call(req):
        if breaker.state == OPEN: return fallback_or_503()   # fail fast
        if not bulkhead.try_acquire(): return 503_overloaded()  # shed load
        try:
            resp = pool.send(req, timeout=800ms)
            breaker.record_success(); return resp
        except (Timeout, 5xx):
            breaker.record_failure()
            if req.idempotent and retry_budget.allow():
                return call_next_instance(req)   # bounded retry, diff instance
            return fallback_or_503()
        finally: bulkhead.release()
```
> Note Netflix Zuul/Hystrix built exactly this; Envoy does it with per-cluster circuit breakers + outlier detection (ejecting bad instances).

Q11. Concurrency/race: the JWKS keys rotate. Mid-rotation, some requests fail signature validation. Walk the race and fix.
> **Scenario:** Auth server rotates its signing key (new `kid`). New tokens are signed with the new key, but the gateway's JWKS cache still has only the old key → valid new tokens get rejected as "unknown kid" → mass 401s during rotation.
> **Expected fix:** **Overlap + lazy refresh keyed by `kid`.** (1) Auth server publishes *both* old and new keys in JWKS during an overlap window, and signs with the new key only after the overlap starts. (2) Gateway caches keys by `kid`; on a token whose `kid` is not in cache, it **triggers a JWKS refetch** (rate-limited, single-flight so 40 nodes don't stampede the JWKS endpoint) before rejecting. (3) Only reject after a refresh still lacks the `kid`. Single-flight/coalescing is the concurrency crux — use a per-`kid` lock so concurrent misses trigger one fetch, not thousands.
> **Follow-up (what if the JWKS endpoint is down during a miss?):** Serve on the stale-but-valid cached keys (don't hard-fail auth because the key server blinked); alert; and never evict the last-known-good keys on a fetch error. Availability of auth > freshness of keys, within the token TTL window.

Q12. Failure deep-dive: Redis (holding rate-limit counters) becomes unreachable. What does the gateway do — fail open or fail closed?
> **Scenario:** The shared Redis for rate limiting has a network partition from the gateway fleet.
> **Expected handling:** This is a deliberate **fail-open vs fail-closed** policy decision, ideally configurable per route. For most public APIs, **fail open** on rate limiting: if Redis is unreachable, allow the request (temporarily unlimited) rather than 429-ing all legitimate traffic — availability of the platform beats perfect rate enforcement for a few seconds. But fall back to a **local per-node limit** (conservative, so a single node can't be fully abused) as a middle ground. For *security-critical* limits (login/OTP endpoints, where unlimited = brute-force risk), **fail closed** or drop to a strict local limit. Add: circuit-break the Redis calls so you're not blocking each request waiting on a dead Redis (fast local decision), use a Redis replica/cluster to make total unavailability rare, and emit a loud alert. DLQ isn't relevant here, but the key idea is: **the rate limiter's own failure must not take down the gateway.**

---

### Scaling to 10x / 100x (Hard)
Q13. At 10x (2M rps), where does the gateway break first?
> **Expected answer:** Usually **connection/file-descriptor exhaustion and Redis hot keys** before raw CPU. (1) Connection count: keep-alive connections from clients × pooled connections to hundreds of upstreams can blow past fd limits and ephemeral port ranges — tune `ulimit`, enable HTTP/2 multiplexing to reduce client conns, size upstream pools. (2) Rate-limit Redis becomes a hot spot — a popular API key's counter is one Redis key on one shard getting hammered; fix with local token leasing (Q8) to cut Redis QPS. (3) TLS handshake CPU if keep-alive/session-resumption isn't tuned. (4) Config-push fan-out to thousands of nodes stresses the control plane.
> **Numbers to ground the answer:** At 2M rps ÷ 10k/node ≈ 200 nodes. If each does an uncached Redis rate-limit call, that's 2M Redis ops/sec on the rate-limit cluster — near a single Redis shard's ceiling; leasing 50 tokens/lease cuts it to ~40k/sec. Added latency budget still <10ms p99.

Q14. Horizontal scaling: the gateway is stateless, so scaling out is "add nodes" — but what shared state fights you, and how do you shard it?
> **Expected sharding strategy:** The gateway data plane scales trivially (stateless behind L4 LB — any node serves any request). The state that fights you is (a) **rate-limit counters** and (b) **any session/sticky state**. Shard rate-limit counters in **Redis Cluster by the limit key** (`userId`/`apiKey`) so each user's counter lives on one shard, atomic ops stay local, and load spreads across shards. Avoid sticky sessions entirely — externalize any session to Redis so nodes stay interchangeable and you can drain/replace any node.
> **Hot spot problem:** One whale customer or a viral endpoint concentrates rate-limit traffic on a single Redis key/shard. **Detect** via per-key Redis latency/ops metrics. **Fix** with local token leasing (batches reduce hits on the hot key), or split a hot limit into N sub-counters (`rl:{user}:{route}:{shard0..N}`) summed periodically, trading exactness for spread. Also client-IP hashing at the L4 LB can create AZ imbalance — prefer least-connections.

Q15. Caching at the gateway: what can it cache and how do you invalidate? Where's the danger?
> **Expected layered cache design:**
> - **Response cache** for cacheable GETs (public, `Cache-Control` honoring) — L1 in-node LRU + optionally an L2 shared Redis, keyed by URL + relevant headers (`Vary`). Respect `ETag`/`max-age`; the gateway is a great place for a shared micro-cache to absorb read spikes.
> - **JWKS / public keys** cached by `kid`, refreshed on rotation.
> - **Route/config** compiled and cached in-memory (the trie).
> - **AuthZ decisions** cached briefly (per token+resource) to avoid re-evaluating policy each request.
> **Cache invalidation trap:** The dangerous one is **caching authenticated/personalized responses** — a response cache keyed only by URL will serve user A's `/me` to user B. Rule: never cache responses without incorporating the identity (or simply never cache when an `Authorization` header is present unless the response is explicitly public). Second trap: a **short micro-cache (1–5s)** on a hot public GET can absorb a thundering herd, but stale-while-revalidate must be used carefully so an origin error doesn't get cached and pinned.

Q16. Cost/efficiency at scale.
> **Expected answer:** (1) **TLS session resumption + keep-alive + HTTP/2** to amortize expensive handshakes — crypto is a top CPU cost at the edge. (2) **Connection pooling & multiplexing to upstreams** — don't open a new TCP+TLS per request. (3) **Local token leasing** to slash Redis calls (Q8) — network + Redis fleet cost. (4) **Micro-caching** hot public GETs to offload upstreams. (5) **Response compression** (gzip/brotli) at the edge to cut egress bandwidth (a real dollar cost). (6) Offload static/large payloads to CDN so the gateway isn't a byte pump. (7) Use an efficient data plane (Envoy/nginx/Go/Rust) — a JVM gateway with per-request thread pools costs far more RAM/CPU than an event-loop proxy at the same rps.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Explain the control-plane / data-plane split with xDS (Envoy). Why must config distribution be async and eventually consistent, and what breaks if the data plane blocks on the control plane? (Direction: data plane serves traffic from in-memory config; control plane streams updates via xDS (LDS/RDS/CDS/EDS). If the data plane synchronously fetched config per request or blocked on a control-plane push, a control-plane hiccup would stall live traffic — instead it runs on last-known-good and applies updates atomically out-of-band. Eventual consistency of config across nodes is acceptable because routing is idempotent.)

**H2.** Multi-tenancy & security: how do you isolate tenants, prevent one tenant's traffic/limits from affecting another, and defend against header spoofing and confused-deputy attacks? (Direction: per-tenant rate limits and bulkheads; strip all client-supplied trust headers at the edge and re-inject validated identity; mTLS to upstreams so a leaked internal header alone can't impersonate; per-tenant WAF/quotas; audit logging. Confused deputy: never let the gateway forward a client-controlled internal auth token — mint a fresh internal identity.)

**H3.** Zero-downtime deploy of the gateway itself — it's on 100% of traffic. (Direction: rolling deploy across AZs with connection draining (stop accepting new, finish in-flight, deregister from LB via failing readiness while liveness stays up); blue-green or canary a small % of gateway nodes with automated rollback on error-rate/latency SLO breach; config changes decoupled from binary deploys (control-plane push, no restart); ensure graceful shutdown so keep-alive connections aren't reset.)

**H4.** Observability: what does the gateway uniquely enable, and the 4 golden signals + alerts you'd instrument? (Direction: the gateway is the one place to get uniform RED metrics — Rate, Errors, Duration — per route and per upstream, plus saturation (connections, CPU, Redis latency). It injects the root trace/request ID for distributed tracing across the whole call tree. Alerts: gateway 5xx rate, added-latency p99, upstream circuit-breaker open events, Redis rate-limit latency, 429 rate spikes (abuse), and per-upstream error budgets. Access logs sampled for volume.)

**H5.** 'Undo a bad decision': you built a monolithic gateway doing heavy per-request BFF aggregation for the mobile app, and it's now a fat bottleneck coupling all teams. Migrate. (Direction: split the aggregation/composition (BFF) concern out of the shared gateway into per-client BFF services behind the gateway, leaving the gateway to do only thin cross-cutting concerns (auth, rate limit, routing). Migrate route by route behind weighted routing/flags, shadow-compare responses, then retire the fat paths. Keeps the hot path lean and decouples teams. Alternatively adopt a service mesh (sidecars) for service-to-service concerns and keep the gateway for north-south edge traffic only.)

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. **Stateful rate limiting done wrong** — either local-only counters (limit multiplies by node count) or naive GET-then-SET races on Redis. They miss atomic Lua token buckets and local leasing.
2. **No blast-radius isolation** — a single global thread pool / timeout, so one slow upstream cascades. They don't mention bulkheads, per-upstream circuit breakers, or retry budgets.
3. **Making the gateway a fat monolith** that does heavy business logic/aggregation on the hot path, or forgetting the control-plane/data-plane split, so config changes and business logic couple everything and it becomes the SPOF it was meant to remove.

**The one insight that makes an answer truly impressive:**
Framing the gateway as a **thin, stateless data plane on the critical path plus an out-of-band control plane** — and treating every gateway feature through the lens of "this runs on 100% of traffic, so it must be O(1)/O(path) hot-path, fail-safe (fail-open where availability wins, fail-closed where security wins), and never block on external systems." Candidates who reason about the **retry budget** (retries capped as a fraction of traffic so retries can't amplify a brownout into an outage) and **fail-open vs fail-closed per concern** are clearly senior.

**Suggested follow-up reading:**
- Envoy architecture docs (data plane) + the xDS protocol; Matt Klein's writing on Envoy/service mesh.
- Netflix Tech Blog: "Zuul 2" (async, non-blocking gateway) and the Hystrix/resilience4j circuit-breaker design.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate — especially Lessons 6 (LBs), 9 (microservices/gateway), 11 (rate limiting), 12 (OAuth/JWT), 10 (circuit breaker).
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to an API gateway. Expected answers include real algorithms (token bucket via Lua, sliding window, circuit-breaker state machine, radix-tree routing), specific failure modes (JWKS rotation stampede, Redis fail-open, upstream cascade), and real numbers. Cross-referenced Phase 1 lessons throughout.
