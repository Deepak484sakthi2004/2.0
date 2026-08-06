# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 17 of 63 — Phase 1: Foundations (Module 17 of 28)
**Module:** Architectural Patterns I
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Before you can design a URL shortener or a payments ledger in Phase 2, you need a vocabulary for *how the boxes talk to each other*. Every architecture diagram you will ever draw is an assembly of a handful of primitive patterns: client-server, proxies, layers, brokers, and coordination via leader election. Netflix, Uber, and Kafka-based pipelines are not magic — they are these six patterns wired together at scale. If you cannot say precisely why a *reverse* proxy differs from a *forward* proxy, or why a cluster needs exactly one leader, an interviewer will spot the gap in ten seconds. Today we build that vocabulary rigorously.

## Learning Objectives
By the end of this lesson you can:
- Draw the client-server request/response cycle and name what state lives where.
- Distinguish a forward proxy from a reverse proxy by *who they hide* and give a real product for each.
- Explain layered architecture and why a strict layer boundary both helps and hurts.
- Map an HTTP request through MVC and name each component's job.
- Describe the broker pattern and how it decouples producers from consumers with a real throughput number.
- Explain why distributed systems elect a leader and trace a quorum-based election.

## The Lesson

### Client-Server
**What it is (plain English):** One machine (the *server*) owns a resource or service and waits; many machines (the *clients*) send requests and get responses. It is the request/response contract that underpins nearly the entire web. Your browser is a client; the machine answering at `api.stripe.com` is a server.

**The problem it solves:** Centralizing data and logic. Instead of every user holding a copy of the truth (which would diverge instantly), one authoritative server holds it and mediates access, so all clients see a consistent, controlled view.

**How it works (mechanics):** The client opens a connection (TCP handshake, ~1 round trip; add TLS for ~1–2 more), sends a request (e.g., `GET /orders/42`), the server processes it and returns a response with a status code.
```
CLIENT                          SERVER
  |  --- TCP SYN/ACK ------->    |
  |  --- GET /orders/42 ---->    |  look up order 42 in DB
  |  <-- 200 OK {json} ------    |
```
The server is typically *stateless per request* for HTTP: it authenticates each request rather than trusting a prior one. Session state, if any, lives in a store (Redis) — not in server memory — so any of N servers behind a load balancer can serve you.

**Trade-offs / when NOT to use it:** The server is a central bottleneck and single point of failure; you scale it horizontally and replicate. For massively parallel, low-trust, or offline-first workloads (BitTorrent, blockchains), **peer-to-peer** beats client-server because there is no central authority to overload.

**Where you'll see it:** Every REST/gRPC API, databases (Postgres server + psql client), DNS, SMTP. A single modern web server (nginx) handles ~10,000+ concurrent connections per node.

### Forward Proxy vs Reverse Proxy
**What it is (plain English):** A proxy is a middleman that relays traffic. A **forward proxy** sits in front of *clients* and speaks to the internet on their behalf (it hides the clients). A **reverse proxy** sits in front of *servers* and receives internet traffic on their behalf (it hides the servers). Same word, opposite direction.

**The problem it solves:** Forward proxies give an organization one controlled exit point — filtering, caching, and anonymizing outbound traffic. Reverse proxies give a fleet of backend servers one controlled entry point — load balancing, TLS termination, and hiding internal topology.

**How it works (mechanics):**
```
FORWARD:  [clients] -> [forward proxy] -> ( internet ) -> server
                        hides the clients

REVERSE:  client -> ( internet ) -> [reverse proxy] -> [servers]
                                     hides the servers
```
A reverse proxy like nginx terminates TLS once (saving each backend the ~1–2ms crypto cost), picks a backend by round-robin or least-connections, and can cache responses so a hot page never touches the app tier. A forward proxy (Squid) enforces "no facebook.com at work" and caches OS updates so 500 laptops download once.

**Trade-offs / when NOT to use it:** Both add a network hop (~0.5–2ms) and become a choke point you must make highly available. A reverse proxy that isn't replicated is a single point of failure for *every* backend behind it.

**Where you'll see it:** Reverse: nginx, HAProxy, AWS ALB, Cloudflare. Forward: corporate Squid proxies, VPN egress gateways.

### Layered Architecture
**What it is (plain English):** Organize the system into horizontal layers where each layer only talks to the one directly below it: presentation → business logic → data access → database. It is the "n-tier" pattern that most enterprise apps still use.

**The problem it solves:** Uncontrolled coupling. Without layers, UI code queries the database directly and business rules leak everywhere; one schema change breaks fifty files. Layers create seams so you can change the database without touching the UI.

**How it works (mechanics):**
```
[ Presentation ]   <- controllers, HTTP handling
        |
[ Business Logic ] <- rules: "can this user refund?"
        |
[ Data Access ]    <- repositories, SQL
        |
[ Database ]       <- Postgres
```
A request enters the top and flows straight down; the response flows back up. In *strict* layering, the presentation layer may not skip past business logic to hit the database — that rule is what preserves the seams.

**Trade-offs / when NOT to use it:** Every request pays for traversing all layers, adding latency and boilerplate ("sinkhole" anti-pattern: a request passes through a layer that does nothing but forward). For very high-throughput or event-driven systems, layered architecture is too rigid — an event-driven or hexagonal design fits better.

**Where you'll see it:** The default Spring Boot / Django / .NET enterprise app. Most CRUD backends at banks and insurers are three-tier layered systems.

### MVC (Model-View-Controller)
**What it is (plain English):** A pattern that splits a UI-facing application into three roles: the **Model** (data + rules), the **View** (what the user sees), and the **Controller** (the traffic cop that takes input and coordinates the other two). It keeps rendering separate from logic.

**The problem it solves:** Tangling display code with business logic. Without MVC, an HTML template contains SQL and tax calculations, making it untestable and unchangeable. MVC isolates each concern so you can test the Model without a browser.

**How it works (mechanics):** Trace `POST /login`:
```
1. Router -> Controller.login(request)
2. Controller asks Model: User.authenticate(email, pw)
3. Model checks DB, returns user or error
4. Controller picks a View: success -> dashboard, else -> login page
5. View renders HTML; Controller returns the response
```
The Controller never renders HTML itself and the View never queries the database. The Model is the only place that knows business rules.

**Trade-offs / when NOT to use it:** For rich, interactive frontends the "View" got so heavy that MVC gave way to MVVM/component models (React). Fat controllers or fat models are the classic decay mode. MVC is overkill for a tiny script with no UI.

**Where you'll see it:** Ruby on Rails, Django, Spring MVC, ASP.NET MVC, Laravel — the backbone of server-rendered web apps for 20 years.

### Broker Architecture
**What it is (plain English):** Instead of producers calling consumers directly, they hand messages to a **broker** (a message middleman) that stores and routes them. Producers and consumers never know about each other — they only know the broker. It is the heart of event-driven systems.

**The problem it solves:** Tight coupling and load spikes. If service A calls service B synchronously, A fails when B is down and A must wait at B's speed. A broker lets A fire-and-forget: the message sits in a durable queue until B is ready, absorbing bursts.

**How it works (mechanics):**
```
Producer --> [ BROKER: topic "orders" ]  --> Consumer group
   (fire)      partition 0 [m1 m2 m3]         (pull at own pace)
               partition 1 [m4 m5]
```
Kafka partitions a topic across brokers; each message gets an offset; consumers track their own offset and pull. This decouples throughput: producers can write while consumers lag. A single Kafka cluster routinely handles **millions of messages per second** (LinkedIn's original cluster processed ~1 trillion messages/day).

**Trade-offs / when NOT to use it:** The broker adds latency (a hop plus persistence) and becomes critical infrastructure you must replicate and monitor. For a simple, low-latency, request/response call where you need an immediate answer, a synchronous call is simpler — don't insert a broker just to feel modern.

**Where you'll see it:** Apache Kafka, RabbitMQ, AWS SQS/SNS, Google Pub/Sub. Uber, Netflix, and virtually every large event pipeline run on brokers.

### Leader Election
**What it is (plain English):** In a cluster of identical nodes, you often need exactly *one* to be "in charge" — to accept writes, assign work, or hold a lock. Leader election is the protocol by which the nodes agree on that single leader, and re-agree quickly when it dies.

**The problem it solves:** Split-brain and duplicated work. If two nodes both think they're the primary database, they accept conflicting writes and corrupt data. Election guarantees at most one leader at a time (safety) and that a new one emerges fast after failure (liveness).

**How it works (mechanics):** Consensus algorithms like **Raft** use quorum voting. Each node has a term number; a candidate requests votes; a node grants at most one vote per term; a candidate that collects a **majority** (⌊N/2⌋+1) becomes leader.
```
5 nodes, majority = 3
Node C times out -> becomes candidate, term=7
Asks A,B,D,E for votes
Gets A,B,D -> 3 votes -> C is leader for term 7
```
Requiring a majority is what prevents two leaders: two disjoint majorities in a 5-node cluster are impossible.

**Trade-offs / when NOT to use it:** Elections cost round trips and a brief unavailability window (typically 150–300ms of election timeout in Raft) during failover. You need an odd node count to avoid tie-splits, and if you can't form a majority (network partition), the cluster stops accepting writes to stay safe. For truly stateless, leaderless designs (Dynamo-style), skip it.

**Where you'll see it:** ZooKeeper, etcd (Kubernetes' brain), Kafka's controller, Redis Sentinel, and every Raft/Paxos-based database (CockroachDB, Consul).

## Comparison Table

| Dimension | Forward Proxy | Reverse Proxy |
|---|---|---|
| Sits in front of | Clients | Servers |
| Hides | The client's identity | The server topology |
| Primary jobs | Filtering, egress control, outbound cache | Load balancing, TLS termination, response cache |
| Example | Squid, corporate VPN egress | nginx, HAProxy, Cloudflare, AWS ALB |
| Who configures it | The client's organization | The server's operator |

**Verdict:** Ask "whose side is the middleman on?" — protect clients with a forward proxy, protect servers with a reverse proxy.

## Common Misconceptions
- **Myth:** A proxy and a load balancer are different things. → **Reality:** A load balancer *is* a specialized reverse proxy; the reverse proxy is the general category.
- **Myth:** MVC is a web-only pattern. → **Reality:** It originated in desktop GUIs (Smalltalk, 1979); the web borrowed it.
- **Myth:** A broker guarantees messages are never lost. → **Reality:** Only with the right durability settings (replication factor ≥ 3, acks=all); a misconfigured broker drops data on crash.
- **Myth:** The leader does all the work, so the cluster is only as fast as one node. → **Reality:** In many systems the leader only coordinates writes; reads scale across followers.
- **Myth:** Layered architecture means physical servers per layer. → **Reality:** Layers are a *logical* separation; all four can run in one process.

## Real-World Case
In October 2021, Facebook (Meta) went globally dark for ~6 hours. The root cause was a BGP configuration change that withdrew the routes to Facebook's DNS servers — effectively removing the "servers" that their entire reverse-proxy and client-server fabric pointed at. Worse, their internal tools and even physical badge readers depended on the same infrastructure, so engineers couldn't easily get into the data centers to fix it. The lesson for architects: your control plane (the thing that fixes outages) must not depend on the data plane it manages. It is the same principle as leader election needing an *independent* coordination service (ZooKeeper/etcd) rather than trusting the very nodes that might be failing.

## Self-Test (answers at the bottom)
1. In one sentence, what does a forward proxy hide and what does a reverse proxy hide?
2. In strict layered architecture, can the presentation layer query the database directly? Why is that rule there?
3. Trace `POST /checkout` through MVC: name what the Controller, Model, and View each do.
4. A 5-node Raft cluster suffers a network partition splitting it 2 | 3. Which side keeps accepting writes, and why can't the other side elect its own leader?
5. Design sketch: You have an order-taking API that spikes 20x during flash sales and a slower fulfillment service. Which pattern decouples them, how do you size for the burst, and what durability setting prevents lost orders?

## Interview Soundbites
- "A forward proxy protects clients and hides them from the internet; a reverse proxy protects servers and hides them from clients — a load balancer is just a reverse proxy with a scheduling policy."
- "Leader election exists to prevent split-brain: requiring a majority quorum makes two simultaneous leaders mathematically impossible, at the cost of a short failover window."
- "I reach for a broker when producers and consumers have different speeds or availability — it turns a fragile synchronous call into a durable, buffered handoff."

## Mini-Assignment
On paper (~30 min): Draw an end-to-end architecture for a food-delivery app. Place (1) a reverse proxy at the edge, (2) a layered/MVC backend, (3) a Kafka broker between "order placed" and the "notify restaurant / assign driver" consumers, and (4) an etcd-based leader for the dispatch coordinator. For each component, write one sentence on what breaks if it fails and how you make it highly available. Then compute: if orders arrive at 500/sec during a promo and each consumer processes 50/sec, how many consumers do you need, and how deep must the queue be to absorb a 30-second consumer outage?

## Recap & Tomorrow
- **Client-server:** authoritative server, many clients, request/response; keep session state in a store so any node can serve.
- **Forward vs reverse proxy:** forward hides clients (egress control); reverse hides servers (load balancing, TLS).
- **Layered architecture:** logical n-tier seams; helps decoupling, costs traversal latency.
- **MVC:** Controller coordinates, Model owns rules/data, View renders — never cross those roles.
- **Broker:** durable middleman that decouples producer and consumer speed; Kafka does millions/sec.
- **Leader election:** majority-quorum consensus (Raft) gives exactly one leader and safe, fast failover.

Tomorrow, **Lesson 18 — Architectural Patterns II**: cloud service models (IaaS/PaaS/SaaS), agent-based, hybrid, web-based, edge-device architectures, and how a CDN delivers content close to the user.

## Self-Test Answers
1. A forward proxy hides the clients (it speaks to the internet on their behalf); a reverse proxy hides the servers (it receives internet traffic on their behalf).
2. No — in strict layering the presentation layer may only call the layer directly below it (business logic). The rule preserves seams: it keeps SQL and business rules out of the UI so you can change the database or logic without touching presentation code.
3. Controller receives the request and orchestrates; Model runs the business rules (validate cart, charge payment, decrement inventory) and hits the DB; View renders the confirmation or error page. The Controller picks which View; it never renders HTML or runs business logic itself.
4. The 3-node side keeps accepting writes because it can form a majority (3 ≥ ⌊5/2⌋+1 = 3). The 2-node side cannot reach 3 votes, so it can never elect a leader and correctly refuses writes — this is what prevents split-brain.
5. Insert a broker (Kafka/SQS) between the order API and fulfillment: the API fire-and-forgets so the 20x spike lands in a durable queue instead of overwhelming fulfillment. Size the queue for peak_rate × outage_window and add consumers to drain the backlog. Set replication factor ≥ 3 and acks=all so an order is durably persisted before the API returns success — no lost orders on a broker crash.
