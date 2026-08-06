# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 10 of 63 — Phase 1: Foundations (Module 10 of 28)
**Module:** Microservices II
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Yesterday you learned how to *cut* services. Today is how you *ship and keep them alive* — the part interviewers probe when they ask "what happens when one service gets slow?" A fleet of 700 Netflix services or thousands of Uber services only works because of Docker for packaging, Kubernetes for orchestration, circuit breakers to stop cascading failure, and observability to see what's actually happening. Get this wrong and one slow dependency takes down your whole product — exactly how a single overloaded service cascaded into a full outage in more than one famous incident. This lesson gives you the operational vocabulary every senior design round expects.

## Learning Objectives
By the end of this lesson you can:
- Explain what Docker containers give you over VMs and how Kubernetes schedules and heals them.
- Configure horizontal scaling with an HPA and reason about target utilization numbers.
- Describe a circuit breaker's three states and pick threshold values.
- List the three pillars of observability and what each answers.
- Design structured, correlated logging that survives across service hops.
- Name and apply core microservices patterns: saga, CQRS, sidecar, and service mesh.

## The Lesson

### Deployments with Docker & Kubernetes
**What it is (plain English):** Docker packages a service plus its exact dependencies into an **image** that runs identically anywhere as a **container**. Kubernetes (K8s) is the orchestrator that runs thousands of those containers across many machines — deciding where each runs, restarting crashed ones, and rolling out new versions. Docker is the shipping container; Kubernetes is the port.

**The problem it solves:** "Works on my machine" dies because the container carries its own libc, runtime, and config. And by hand nobody can place 5,000 containers across 300 servers, restart the dead ones, and roll updates without downtime — K8s automates that.

**How it works (mechanics):** A container shares the host kernel (unlike a VM's full guest OS), so it boots in ~100 ms vs a VM's ~30 s and adds near-zero overhead. K8s runs containers inside **Pods**, scheduled onto **Nodes**:
```
Deployment (desired: 5 replicas)
   → ReplicaSet ensures 5 Pods exist
      → Scheduler places Pods on Nodes by CPU/mem
Service (stable virtual IP) → load-balances across the 5 Pods
```
A **rolling update** replaces pods in batches (e.g., 25% at a time) with health checks between, so a bad build stops after the first batch — zero-downtime deploys. **Liveness/readiness probes** let K8s restart hung pods and route traffic only to ready ones.

**Trade-offs / when NOT to use it:** K8s is powerful and *complex* — a real cluster needs networking, ingress, secrets, and RBAC expertise. For a single small service, K8s is overkill; a managed platform (ECS, Cloud Run, Fly) or even one VM is simpler and cheaper. Don't adopt K8s for prestige.

**Where you'll see it:** Google (Borg, K8s's ancestor), Spotify, Airbnb, and most cloud-native shops; managed variants EKS/GKE/AKS.

### Scaling
**What it is (plain English):** Scaling is adding capacity to handle load. **Vertical** = bigger machine (more CPU/RAM). **Horizontal** = more machines/replicas behind a load balancer. Microservices favor horizontal because you can add cheap identical copies.

**The problem it solves:** Traffic is spiky — checkout at Black Friday may be 10x a Tuesday. A fixed fleet either wastes money (sized for peak) or falls over (sized for average). Autoscaling matches capacity to demand.

**How it works (mechanics):** In K8s the **Horizontal Pod Autoscaler (HPA)** watches a metric and adjusts replica count:
```
desiredReplicas = ceil(currentReplicas × currentMetric / targetMetric)
```
Worked example: target CPU = 50%, current = 5 pods averaging 90% CPU.
`ceil(5 × 90 / 50) = ceil(9) = 9 pods`. HPA scales 5 → 9. When load drops to 20%: `ceil(5 × 20 / 50) = 2` pods (with a cooldown to avoid flapping). Stateless services scale trivially; stateful ones need sharding or a shared data tier because a new replica has no data. **Load balancers** (L4/L7) spread requests across replicas — round-robin, least-connections, or latency-aware.

**Trade-offs / when NOT to use it:** Horizontal scaling assumes statelessness — sticky sessions or in-memory state break it. Scaling also has floors: a downstream database is often the real bottleneck, so adding app replicas just moves the queue. Vertical scaling is simpler but has a hard ceiling and a single point of failure.

**Where you'll see it:** Every cloud autoscaling group; Netflix scales predictively (ahead of the evening viewing peak) rather than purely reactively.

### Circuit Breaking
**What it is (plain English):** A circuit breaker wraps calls to a dependency and, when that dependency starts failing, "trips" — it stops sending calls for a while and fails fast instead of piling up. Named after the electrical breaker that cuts power before a fire.

**The problem it solves:** Without it, a slow dependency causes callers to block on threads waiting for timeouts. Threads exhaust, the caller stops responding, *its* callers block — the failure **cascades** upstream until the whole system is down. The breaker contains the blast radius.

**How it works (mechanics):** Three states:
```
CLOSED  → calls flow. Count failures.
   (failures ≥ threshold, e.g. 50% of last 20 calls) → OPEN
OPEN    → reject immediately (fail fast / fallback) for a cooldown (e.g. 10 s)
   (cooldown elapsed) → HALF-OPEN
HALF-OPEN → allow a few trial calls.
   success → CLOSED   |   failure → OPEN again
```
Worked example: threshold = 50% over a 20-request window, cooldown 10 s. If 11 of the last 20 calls to `Payments` fail, the breaker opens; for 10 s callers instantly get a fallback ("payment queued, retry shortly") instead of hanging 30 s on a timeout. After 10 s it lets ~3 trial calls through; if they succeed it closes. This turns a 30-second hang into a 1-ms fast-fail.

**Trade-offs / when NOT to use it:** A breaker tuned too sensitive trips on transient blips and hurts availability; too loose and it never protects you. It also needs a sensible **fallback** — a cached value, a default, or a graceful error. It doesn't fix the failing dependency; it just stops the bleeding.

**Where you'll see it:** Netflix Hystrix (the original), Resilience4j, Envoy/Istio outlier detection; standard in every mature service mesh.

### Observability
**What it is (plain English):** Observability is your ability to answer "what is happening inside the system?" from the outside, using its outputs. Its three pillars are **metrics** (numbers over time), **logs** (discrete events), and **traces** (the path of one request across services).

**The problem it solves:** In a monolith you attach a debugger. Across 50 services, a single slow user request touches 8 of them — you can't debugger that. You need to *see* where time went and which service errored without reproducing it.

**How it works (mechanics):** Each pillar answers a different question:
- **Metrics** (e.g., Prometheus): aggregated numbers — p99 latency, error rate, QPS. Answers "*is* something wrong?" Cheap, always-on. Track the RED method: Rate, Errors, Duration.
- **Traces** (e.g., OpenTelemetry/Jaeger): one request gets a **trace ID**; each hop is a **span** with start/end. Answers "*where* is it slow?"
```
trace_id=abc  [Gateway 5ms]→[Orders 8ms]→[Payments 240ms]←slow!→[DB 230ms]
                total = 253ms; Payments/DB is the culprit
```
- **Logs**: detailed event lines for the forensic "*why*." Correlated by trace ID.

Together: a metric alert fires (p99 spiked), a trace shows Payments is slow, and its logs show a DB lock. A rule of thumb: **SLO** budgets like "99.9% of requests < 300 ms" turn observability into alerting.

**Trade-offs / when NOT to use it:** Full tracing on every request is expensive at scale, so systems **sample** (e.g., 1% of traces, but 100% of errors). High-cardinality metrics (per-user labels) can blow up your monitoring bill. Observability is a cost you budget, not free.

**Where you'll see it:** Prometheus+Grafana, Datadog, Honeycomb, Jaeger; Google's Dapper paper started distributed tracing.

### Logging
**What it is (plain English):** Logging is emitting timestamped records of what a service did. In microservices, logs must be **structured** (machine-parseable JSON, not prose) and **centralized** (shipped to one searchable store), because grepping 50 machines by hand is hopeless.

**The problem it solves:** A user reports "my order failed at 2:03pm." That request crossed Gateway → Orders → Payments → Inventory on different hosts. Plain text logs scattered across machines can't be joined into one story. Structured, correlated logs can.

**How it works (mechanics):** Two rules make logs usable:
- **Structured:** emit `{"ts":"...","level":"ERROR","service":"payments","trace_id":"abc","msg":"card declined","order_id":42}`. Fields are queryable: filter `trace_id=abc` to see the whole request across services.
- **Correlation ID / trace ID:** the gateway generates an ID, passes it in a header (`X-Request-ID`) to every downstream call; each service logs it. Now one query reconstructs the full path.
```
Gateway  trace=abc  "received POST /orders"
Orders   trace=abc  "created order 42"
Payments trace=abc  ERROR "card declined"   ← found it in one query
```
Logs flow to a pipeline (Fluent Bit → Kafka → Elasticsearch/Loki) for search. Use **levels** (DEBUG/INFO/WARN/ERROR) and sample or drop DEBUG in prod to control volume — a busy service can emit millions of lines/min.

**Trade-offs / when NOT to use it:** Logs are the most expensive pillar per byte — verbose logging costs storage and can itself slow the service (synchronous disk writes). Never log secrets/PII. For high-frequency signals, a metric is cheaper than a log line.

**Where you'll see it:** ELK/Elastic, Grafana Loki, Splunk; the correlation-ID pattern is universal in production microservices.

### Microservices Design Patterns & Good Practices
**What it is (plain English):** Recurring, proven solutions to the hard parts of distributed systems: keeping data consistent without cross-service transactions, reading efficiently, and handling cross-cutting concerns without bloating each service.

**The problem it solves:** You can't run a database transaction across services (no shared DB). You can't scatter auth/TLS/retry logic into every service. Patterns give reusable answers.

**How it works (mechanics):** The essentials:
- **Saga:** a business transaction split into local steps across services, coordinated by **events**, with **compensating actions** to undo. `OrderPlaced → reserve inventory → charge card`; if the charge fails, emit a compensating `release inventory`. Replaces the impossible distributed 2-phase commit with eventual consistency.
- **CQRS (Command Query Responsibility Segregation):** separate the write model from a read-optimized model. Writes go to Orders DB; a denormalized read view is updated via events so `GET /order-history` is a single fast lookup instead of a fan-out join.
- **Sidecar:** attach a helper container (proxy) to each service pod to handle networking/retries/TLS uniformly.
- **Service mesh (Istio/Linkerd):** all those sidecars centrally controlled — mTLS, retries, circuit breaking, and traffic-shifting **without changing service code**.
```
[Service] ⇄ [Envoy sidecar] ⇄ network ⇄ [Envoy sidecar] ⇄ [Service]
                     ↑ mesh control plane: mTLS, retries, routing
```
Good practices: one DB per service, async where possible, idempotent handlers, health checks, and API versioning.

**Trade-offs / when NOT to use it:** Sagas trade ACID for eventual consistency and force you to design every compensating action — more code, tricky edge cases. A service mesh adds a proxy hop (~0.5–1 ms) and real operational weight; a 3-service system doesn't need one.

**Where you'll see it:** Uber and Airbnb run sagas for booking/payment flows; Lyft created Envoy; Istio/Linkerd power mesh at many enterprises.

## Comparison Table

| Concern | Mechanism | Answers / Solves | Cost |
|---|---|---|---|
| Packaging | Docker container | Consistent runtime, ~100ms boot | Image build/registry |
| Orchestration | Kubernetes | Placement, healing, rollouts | High complexity |
| Elasticity | HPA / autoscaling | Match capacity to load | Cold-start lag, DB bottleneck |
| Failure isolation | Circuit breaker | Stop cascades, fail fast | Needs tuning + fallback |
| Visibility | Metrics/Traces/Logs | Is it wrong / where / why | Sampling + storage cost |
| Consistency | Saga / CQRS | Cross-service state, fast reads | Eventual consistency, extra code |

**Verdict:** Containers + K8s give you the substrate; circuit breakers and observability keep it alive; sagas/CQRS/mesh handle the distributed-data and cross-cutting hard parts — adopt each only when its problem is real.

## Common Misconceptions
- **Myth:** Containers are lightweight VMs. → **Reality:** Containers share the host kernel (no guest OS), so they boot ~300x faster and add near-zero overhead.
- **Myth:** Autoscaling solves all load problems. → **Reality:** It scales the stateless tier; the database or a downstream service is usually the real ceiling.
- **Myth:** A circuit breaker retries the failed call. → **Reality:** It *stops* calling and fails fast/falls back; retrying is a separate policy (and blind retries make cascades worse).
- **Myth:** Logs and metrics are the same thing. → **Reality:** Metrics are cheap aggregated numbers ("is it wrong?"); logs are expensive discrete events ("why?"); traces show "where."
- **Myth:** A saga is a distributed transaction. → **Reality:** It's a sequence of local transactions with compensations — eventual consistency, not ACID.

## Real-World Case
Netflix's move to the cloud after a 2008 database corruption pushed them to build for failure, not against it. Their engineers wrote Hystrix, a circuit-breaker library, precisely because in a mesh of hundreds of services one slow dependency — say, a bookmarks service — could exhaust caller threads and cascade into a total streaming outage. Hystrix wrapped every remote call: on failure it tripped, served a sensible fallback (e.g., a generic row of titles), and shed load until the dependency recovered. They went further with Chaos Monkey, deliberately killing instances in production to prove the breakers and autoscaling actually worked. The lesson: at scale, failure is constant; the design goal is graceful degradation, and the circuit breaker is the unit that delivers it.

## Self-Test (answers at the bottom)
1. Why does a container start far faster than a VM?
2. HPA target CPU is 60%; you currently run 4 pods averaging 90% CPU. How many pods should HPA scale to?
3. Name the three circuit-breaker states and what triggers each transition.
4. A user's order failed and you have 50 services logging to different machines. What two logging practices let you reconstruct exactly what happened, and how?
5. Design sketch: An order must reserve inventory, charge a card, and notify shipping — across three services with separate databases. You can't use one transaction. Design the flow so a mid-way failure leaves no money charged with no inventory. Name the pattern and one compensating action.

## Interview Soundbites
- "Circuit breakers exist to stop cascading failure — they fail fast and fall back so one slow dependency doesn't exhaust every caller's threads."
- "Metrics tell me *if* something's wrong, traces tell me *where*, logs tell me *why* — I correlate all three by trace ID."
- "You can't run a transaction across services, so I use a saga: local transactions coordinated by events, with a compensating action for every step."

## Mini-Assignment
(~30 min) Take a three-service checkout: Orders, Payments, Shipping. (1) Write the saga as an ordered list of local steps and, for each, its compensating action. (2) Draw the happy path and one failure path (card declined at step 2) showing which compensations fire. (3) Add a circuit breaker around the Payments call: pick a failure threshold, a window, a cooldown, and a fallback, and justify each number. (4) List the exact log fields you'd emit at each hop so one query reconstructs a failed order.

## Recap & Tomorrow
- **Docker & K8s:** containers give consistent, fast-booting units; Kubernetes schedules, heals, and rolls them out with zero downtime.
- **Scaling:** horizontal (HPA) for stateless tiers using `desired = ceil(cur × metric/target)`; watch the DB bottleneck.
- **Circuit breaking:** CLOSED→OPEN→HALF-OPEN to fail fast and stop cascades; needs a fallback and tuned thresholds.
- **Observability:** metrics (is it wrong), traces (where), logs (why) — correlated by trace ID, sampled to control cost.
- **Logging:** structured JSON + correlation IDs, centralized; the most expensive pillar, so control volume.
- **Patterns:** saga (eventual consistency via compensations), CQRS (fast reads), sidecar/service mesh (cross-cutting concerns).

Tomorrow, **Lesson 11 — Rate Limiting**: the algorithms that protect every one of these services — token bucket, leaky bucket, fixed and sliding windows, and how to enforce limits across a distributed fleet.

## Self-Test Answers
1. A container shares the host's kernel and runs as an isolated process, so it only needs to start its own process (~100 ms). A VM boots a full guest operating system on virtualized hardware (~30 s) and carries that OS's memory/CPU overhead.
2. `ceil(4 × 90 / 60) = ceil(6) = 6` pods. HPA scales 4 → 6 so average CPU drops back toward the 60% target.
3. CLOSED (calls flow, counting failures) → OPEN when the failure rate crosses the threshold; OPEN (reject/fallback immediately) → HALF-OPEN after the cooldown elapses; HALF-OPEN (trial calls) → CLOSED on success or back to OPEN on failure.
4. Structured logging (JSON with queryable fields) plus a correlation/trace ID propagated in a header (e.g., `X-Request-ID`) through every service. The gateway generates the ID; each service logs it; centralized log search then filters `trace_id=abc` to return every log line for that one request across all 50 services in order.
5. Use a **saga**. Steps: (1) Orders creates the order, (2) Inventory reserves stock, (3) Payments charges the card, (4) Shipping is notified — coordinated by events. If step 3 (charge) fails, fire the compensating action `ReleaseInventory` to undo step 2 (and cancel the order), so no inventory stays reserved. If a later step fails, `RefundPayment` compensates the charge. The system reaches a consistent end state through compensations rather than a single ACID transaction.
