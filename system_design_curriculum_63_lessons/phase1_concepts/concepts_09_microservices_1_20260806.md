# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 9 of 63 — Phase 1: Foundations (Module 9 of 28)
**Module:** Microservices I
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Almost every large-scale design question in Phase 2 — Uber, Netflix, Amazon checkout — is really a microservices question in disguise. Amazon famously moved from a single monolith to thousands of services so that a two-pizza team could ship without waiting on anyone else; Netflix runs 700+ microservices to stream to 260M+ subscribers. If you cannot explain *when* a monolith is the right call, *how* an API gateway fans traffic out, and *why* domain boundaries — not database tables — decide where you cut a service, you will design a distributed monolith that has all the pain of microservices and none of the benefit. Today we build that judgment.

## Learning Objectives
By the end of this lesson you can:
- Decide monolith vs microservices using team size, deploy cadence, and blast radius — not hype.
- Explain what an API gateway does and when you'd run one vs several.
- Design a REST API that is correctly resourced, versioned, idempotent, and paginated.
- Apply Domain-Driven Design to draw service boundaries around business capabilities.
- Spot a "distributed monolith" and say precisely why it is the worst of both worlds.

## The Lesson

### Monolith vs Microservices
**What it is (plain English):** A monolith is one deployable unit — all features compiled and shipped together, usually against one database. Microservices split that into many small, independently deployable services, each owning its own data and talking over the network (HTTP/gRPC/queues). Think one big restaurant kitchen vs a food court of specialized stalls.

**The problem it solves:** A monolith gets slow to change once many teams share it: every merge risks everyone, one bad deploy takes down all features, and you must scale the *whole* app even if only checkout is hot. Microservices let teams deploy independently and scale hot paths in isolation.

**How it works (mechanics):** Consider an e-commerce app: `Catalog`, `Cart`, `Orders`, `Payments`, `Search`. As a monolith they share one process and DB.
```
MONOLITH                        MICROSERVICES
+------------------+            [Catalog]  [Cart]  [Orders]
| catalog | cart   |              |db        |db      |db
| orders  | pay    |            [Payments]      [Search]
| search  |        |              |db            |db
+---one DB---------+            (independent deploys + DBs)
```
Real numbers: if `Search` needs 20 instances at Black Friday but `Payments` needs 4, a monolith forces you to run 20 copies of *everything* — ~5x wasted compute. Microservices scale each independently. But you now pay in network calls: a request that was 1 in-process function call may become 5 network hops, each adding ~1–5 ms plus failure probability.

**Trade-offs / when NOT to use it:** Microservices add distributed-systems tax: network latency, partial failure, eventual consistency, and heavy ops (service discovery, tracing, 10x more deploy pipelines). For a startup with 5 engineers and unclear domain boundaries, a **modular monolith** ships faster and is easier to reason about. Split only when team count and deploy contention actually hurt.

**Where you'll see it:** Amazon and Netflix are the canonical microservices shops; Shopify and Stack Overflow famously run large, deliberate monoliths at scale.

### API Gateway (Single & Multiple Gateways)
**What it is (plain English):** An API gateway is the single front door in front of your services. Clients hit the gateway; it authenticates, rate-limits, and routes each request to the right backend service, so clients never need to know your internal topology.

**The problem it solves:** Without a gateway, every client (web, iOS, Android) must know the address of every service, and each service must re-implement auth, TLS, rate limiting, and logging. That's duplicated cross-cutting logic and a security nightmare. The gateway centralizes it.

**How it works (mechanics):** It's a reverse proxy plus policy engine. A request flows:
```
Client → [API Gateway] → auth check → rate limit → route
                       ├─ /catalog/* → Catalog svc
                       ├─ /cart/*    → Cart svc
                       └─ /orders/*  → Orders svc
```
It can also **aggregate**: one mobile call `GET /home` fans out to Catalog + Cart + Recommendations and stitches one response, saving the phone 3 round trips (~150 ms on 3G).

*Single gateway:* one gateway for all clients — simplest, but becomes a bottleneck and a one-size-fits-none API. *Multiple gateways (BFF — Backend For Frontend):* one tailored gateway per client type — a mobile BFF returns lean payloads, a web BFF returns richer ones, a partner/public API gateway enforces stricter quotas. Netflix pioneered this.

**Trade-offs / when NOT to use it:** The gateway is a single point of failure and an extra ~1–3 ms hop, so it must be highly available and horizontally scaled. Overloading it with business logic recreates a monolith at the edge. For pure internal service-to-service traffic, skip the gateway and use a service mesh instead.

**Where you'll see it:** AWS API Gateway, Kong, NGINX, Netflix Zuul; the BFF pattern is standard at Netflix, SoundCloud, and Spotify.

### REST API Design
**What it is (plain English):** REST is a convention for HTTP APIs where you model your system as **resources** (nouns) addressed by URLs, and act on them with HTTP **verbs** (GET, POST, PUT, PATCH, DELETE). `GET /orders/42` reads order 42; `DELETE /orders/42` removes it.

**The problem it solves:** Ad-hoc RPC APIs (`/getOrderById`, `/deleteOrderNow`) are inconsistent and unpredictable. REST gives a uniform, cacheable, self-describing contract every client and proxy already understands.

**How it works (mechanics):** Core rules with real examples:
- **Resources are nouns, plural:** `/orders`, `/orders/42/items`. Never `/getOrders`.
- **Verbs carry intent:** `GET` (read, safe), `POST` (create), `PUT` (full replace, idempotent), `PATCH` (partial), `DELETE`.
- **Status codes mean things:** `200` OK, `201` Created, `400` bad input, `401` unauthenticated, `404` missing, `429` rate-limited, `503` overloaded.
- **Idempotency:** retries must not double-charge. `POST /payments` with header `Idempotency-Key: abc123` lets the server dedupe a retried call.
- **Pagination:** never `GET /orders` returning 10M rows. Use cursor pagination: `GET /orders?limit=50&after=cursor123` returns 50 rows + a next cursor.
- **Versioning:** `GET /v2/orders` or an `Accept: application/vnd.api.v2+json` header so you can evolve without breaking old clients.

```
POST /orders            201 Created  {id:42}
GET  /orders/42         200 OK
PATCH /orders/42 {qty:3} 200 OK
```

**Trade-offs / when NOT to use it:** REST over-fetches (returns fields you don't need) and under-fetches (needs multiple calls for related data). For rich client-driven queries, **GraphQL** fits; for high-throughput internal calls, **gRPC** (binary, HTTP/2) is ~5–10x faster on the wire.

**Where you'll see it:** Stripe, GitHub, and Twilio publish textbook REST APIs — Stripe's idempotency keys are the reference implementation.

### Domain-Driven Design in Microservices
**What it is (plain English):** Domain-Driven Design (DDD) says: draw service boundaries around **business capabilities**, not technical layers. Each boundary is a **bounded context** with its own model and language, owning its own data. "Order" in Sales and "Order" in Shipping may mean different things — that's fine, they're different contexts.

**The problem it solves:** Teams that split by database table create services that are joined at the hip — change one, redeploy three. DDD gives you boundaries that change together for the same business reason, minimizing cross-service coupling.

**How it works (mechanics):** Steps: (1) **Event storming** — map the business as events: `OrderPlaced`, `PaymentCaptured`, `ItemShipped`. (2) Group events by who owns them into **bounded contexts**: Ordering, Payments, Fulfillment. (3) Each context becomes a service owning its tables; no other service touches those tables directly — only via API/events.
```
[Ordering] --OrderPlaced event--> [Payments] --PaymentCaptured--> [Fulfillment]
   owns orders DB                    owns ledger DB                 owns shipments DB
```
The measure: a well-cut boundary means a typical feature touches **one** service. If your "add discount code" feature edits Ordering, Payments, and Catalog at once, your boundaries are wrong.

**Trade-offs / when NOT to use it:** DDD needs real domain knowledge; on a greenfield product where the domain is still shifting weekly, premature boundaries become expensive to move. Start with a modular monolith, let the seams reveal themselves, then extract.

**Where you'll see it:** Uber and Amazon organize services around capabilities (Pricing, Dispatch, Payments); it maps directly to Conway's Law and the two-pizza team.

## Comparison Table

| Dimension | Monolith | Microservices |
|---|---|---|
| Deploy unit | One artifact, all-or-nothing | Per service, independent |
| Scaling | Whole app together | Per service (scale hot paths) |
| Blast radius of a bug | Entire app | One service (if isolated) |
| Data | Shared DB, easy joins | DB per service, no cross-joins |
| Latency | In-process call (~µs) | Network hop (~1–5 ms each) |
| Ops complexity | Low | High (discovery, tracing, N pipelines) |
| Best for | Small team, unclear domain | Many teams, clear domains, scale |

**Verdict:** Default to a modular monolith; graduate to microservices when team contention and independent scaling genuinely hurt — not before.

## Common Misconceptions
- **Myth:** Microservices are always better than a monolith. → **Reality:** They trade code complexity for operational and distributed-systems complexity; below a certain team size a monolith wins.
- **Myth:** A microservice = a small codebase. → **Reality:** Small is a symptom; the real definition is an independently deployable unit owning its own data.
- **Myth:** Services can share one database to save effort. → **Reality:** A shared DB couples deploys and schemas — that's a distributed monolith, the worst of both worlds.
- **Myth:** The API gateway is just a load balancer. → **Reality:** It also does auth, rate limiting, routing, and response aggregation — policy, not just distribution.
- **Myth:** REST means "use JSON over HTTP." → **Reality:** REST is about resources, verbs, statelessness, and uniform interfaces; JSON is incidental.

## Real-World Case
Around 2001, Amazon's monolith — the "Obidos" web app plus one giant Oracle database — had become a wall: teams queued for deploys, and a change in one area could break another. The response was the now-famous mandate that all teams expose data and function *only* through service interfaces, with no direct database sharing — and that those interfaces be designed as if they could one day be public. That decision seeded both the microservices architecture and, years later, AWS itself. The lesson wasn't "smaller is better"; it was that **hard interface boundaries** let independent teams move independently. The pain Amazon removed was human coordination cost, not CPU cycles.

## Self-Test (answers at the bottom)
1. Name two concrete costs microservices add that a monolith doesn't have.
2. What does an API gateway do beyond routing? Give three responsibilities.
3. A client says `GET /orders` returns all 4M orders and times out. Redesign the endpoint.
4. Your "apply coupon" feature requires code changes in Ordering, Payments, and Catalog simultaneously. What does DDD say is wrong, and how would you fix the boundaries?
5. Design sketch: A 6-person startup is building a food-delivery MVP. Monolith or microservices? Justify with team size, deploy cadence, and domain clarity, and describe the migration trigger that would make you split later.

## Interview Soundbites
- "A microservice isn't 'small code' — it's an independently deployable unit that owns its own data; shared databases turn microservices back into a distributed monolith."
- "The API gateway is the single front door: it centralizes auth, rate limiting, and routing so services and clients don't each re-implement cross-cutting concerns."
- "I cut services along bounded contexts, not database tables — a good boundary means a typical feature touches exactly one service."

## Mini-Assignment
(~30 min) Take a movie-ticket booking app. (1) List its features, then run a mini event-storming: write 8–10 domain events (`SeatReserved`, `PaymentCaptured`, …). (2) Group them into 3–4 bounded contexts and name a service per context, listing the data each owns. (3) Draw the API gateway in front and note which cross-cutting concerns it handles. (4) Write the REST endpoint signatures for booking a seat, including status codes and an idempotency strategy for the payment call.

## Recap & Tomorrow
- **Monolith vs microservices:** independent deploy + per-service scaling vs distributed-systems tax; default to modular monolith.
- **API gateway:** single front door for auth/rate-limit/routing/aggregation; use multiple BFFs to tailor per client.
- **REST design:** resources as nouns, verbs carry intent, status codes, idempotency keys, cursor pagination, versioning.
- **DDD:** cut services around bounded contexts (business capabilities), each owning its data; a good feature touches one service.

Tomorrow, **Lesson 10 — Microservices II**: how you actually ship and run these services — Docker & Kubernetes deployments, scaling, circuit breaking, observability, logging, and the design patterns that keep a fleet of services alive.

## Self-Test Answers
1. Network latency and partial failure on every inter-service call (a former in-process call is now a ~1–5 ms network hop that can fail), plus operational overhead: service discovery, distributed tracing, and N separate deploy/monitoring pipelines instead of one.
2. Authentication/authorization (verify tokens once at the edge), rate limiting/throttling (protect backends, return 429), and routing plus optional response aggregation (fan one client call out to several services and stitch the result). It also centralizes TLS termination and logging.
3. Add cursor-based pagination: `GET /orders?limit=50&after=<cursor>` returning 50 rows plus a `next` cursor, and support filtering (`?status=open`) and sorting. Never return an unbounded collection; the client pages until the cursor is null.
4. DDD says the boundaries are wrong — those three services are coupled and must change together, which is a sign the coupon logic really belongs in one bounded context (likely Ordering/Pricing). Fix by moving the discount model into a single context that owns pricing, and have others consume it via API or events rather than editing shared logic.
5. Monolith (modular). With 6 engineers there's little deploy contention, deploy cadence is fine as one pipeline, and a new food-delivery domain is still shifting weekly so boundaries are unclear — premature splits would be costly to move. Migration trigger: when teams grow past ~15–20 and start blocking each other on deploys, or one component (e.g., real-time driver tracking) needs to scale independently, extract that seam first.
