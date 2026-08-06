# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 6 of 63 — Phase 1: Foundations (Module 6 of 28)
**Module:** Load Balancers
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
A single server tops out — maybe 10,000 concurrent connections, a few thousand requests per second. Every system serving millions of users therefore sits behind a load balancer (LB): the one address the world talks to, quietly fanning traffic across a fleet. Get the LB wrong and you get the classic failures — one hot server melting while others idle, a health check that keeps routing to a dead box, or a cache stampede when you rehash every key. AWS ELB, NGINX, HAProxy, Envoy, and Google's Maglev are all answers to this problem. Every Phase 2 design (URL shortener, chat, video, feed) starts by drawing this box.

## Learning Objectives
By the end of this lesson you can:
- Distinguish round robin, weighted, least-connections, and least-time, and pick one from workload characteristics.
- Explain the concrete difference between a Layer-4 and a Layer-7 load balancer and what each can and cannot see.
- Describe how DNS-based load balancing distributes traffic and why TTL and caching limit it.
- Explain why consistent hashing minimizes key remapping when a node is added or removed, with a real number.
- Design a health-check + failover scheme that ejects a bad backend without flapping.

## The Lesson

### Round Robin
**What it is (plain English):** The simplest policy: hand request 1 to server A, request 2 to B, request 3 to C, request 4 back to A, and so on — a fair rotation, like dealing cards around a table.

**The problem it solves:** You need to spread load evenly with zero state and zero coordination. Round robin requires no knowledge of server load — just a counter modulo N.

**How it works (mechanics):** Keep an index `i`; for each request pick `servers[i % N]` then increment `i`. With 3 servers over 6 requests:
```
req1→A  req2→B  req3→C  req4→A  req5→B  req6→C
```
Each server gets exactly 2. DNS round robin does the same by rotating the order of A-records it returns.

**Trade-offs / when NOT to use it:** It assumes all requests cost the same and all servers are equal. If request 1 is a 5-second report and request 2 is a 5 ms ping, server A gets buried while B breezes through — round robin can't tell. It's also blind to a server that's slow-but-alive. Fine for uniform, stateless workloads; poor for heterogeneous request costs.

**Where you'll see it:** NGINX default upstream policy, AWS Route 53 weighted-round-robin DNS, Kubernetes `kube-proxy` iptables mode. The baseline everyone compares against.

### Weighted & Least-Connections / Least-Time
**What it is (plain English):** Smarter policies that account for server capacity or current load. **Weighted** gives beefier servers a bigger share. **Least-connections** sends the next request to whoever has the fewest open connections. **Least-time** picks the server with the best blend of low active connections and low recent response time.

**The problem it solves:** Real fleets are uneven — a mix of a 32-core box and 8-core boxes, or long-lived connections (websockets) that plain round robin distributes badly. These policies route to where there's actually headroom.

**How it works (mechanics):** *Weighted round robin:* server A weight 3, B weight 1 → over 4 requests A gets 3, B gets 1. *Least-connections* with live counts:
```
A: 12 active   B: 5 active   C: 9 active   → next request → B (fewest)
```
*Least-time* (NGINX Plus / Envoy) ranks by, e.g., `active_conns × avg_response_ms`: A = 12×40=480, B = 5×90=450, C = 9×30=270 → pick C, because although B has fewer connections, its responses are slow.

**Trade-offs / when NOT to use it:** Least-connections needs the LB to track live connection counts — more state, and it can mislead if connections are cheap but CPU-heavy. Weighted requires you to *set* correct weights; stale weights after a hardware change misroute traffic. Overkill for a uniform fleet where round robin already balances well.

**Where you'll see it:** HAProxy `leastconn`, NGINX `least_conn` / `least_time`, AWS ALB's least-outstanding-requests, Envoy's weighted clusters for canary rollouts (send 5% to the new version).

### Layer-4 vs Layer-7
**What it is (plain English):** *Where* in the network stack the LB makes its decision. A **Layer-4** LB routes by IP address and TCP/UDP port — it sees packets, not content. A **Layer-7** LB reads the actual HTTP request — URL path, headers, cookies — and routes on meaning.

**The problem it solves:** L4 gives raw speed and protocol-agnostic forwarding. L7 gives smart, content-aware routing: send `/api/*` to one fleet, `/images/*` to another, sticky-session by cookie, TLS termination, and per-path retries.

**How it works (mechanics):**
```
L4 LB:  sees [src IP:port → dst IP:port], forwards TCP stream, never opens payload
        Decision: hash(client IP) or round robin → backend. ~microsecond overhead.

L7 LB:  terminates TCP, parses:  GET /checkout HTTP/1.1  Host: shop.com  Cookie: sid=abc
        Decision: path /checkout → checkout-fleet; sid=abc → same backend (sticky).
```
L4 forwards millions of packets/sec with tiny CPU because it never parses HTTP. L7 costs more CPU per request (it decrypts TLS and parses headers) but unlocks routing, rewriting, and observability. Typical: L4 adds tens of microseconds; L7 adds a fraction of a millisecond plus TLS cost.

**Trade-offs / when NOT to use it:** L4 can't do path routing, can't inspect a request, can't terminate TLS meaningfully. L7 is more CPU-hungry and a bigger attack/parsing surface. Many designs stack them: L4 at the edge for volume, L7 behind it for smarts.

**Where you'll see it:** AWS **NLB** (L4) vs **ALB** (L7); HAProxy in `mode tcp` vs `mode http`; Envoy and NGINX as L7; Google Maglev as an L4 tier feeding L7 proxies.

### DNS Load Balancing
**What it is (plain English):** Balancing at name-resolution time. When a client asks DNS for `example.com`, the DNS server hands back one of several IP addresses — spreading users across servers or whole data centers before a single packet is sent.

**The problem it solves:** It's the *first* and cheapest layer of distribution and the only one that can steer users to a nearby region (geo-DNS). It needs no in-path hardware — the resolver does the spreading.

**How it works (mechanics):** The zone has multiple A-records; the DNS server rotates or geo-selects them:
```
dig example.com  → 203.0.113.10 (US-East)
(next client)    → 198.51.100.20 (EU-West)   ← geo/latency based
```
TTL controls how long resolvers cache the answer. TTL=60s means clients re-resolve each minute; TTL=3600s means an hour of stickiness.

**Trade-offs / when NOT to use it:** DNS is *coarse and sticky*. Resolvers and browsers cache aggressively and often ignore low TTLs, so you cannot rely on DNS to drain a failing server quickly — a dead IP may keep getting traffic for minutes after you pull it. It also balances by *resolver*, not by request, so one corporate resolver can funnel thousands of users to one IP. Use DNS for coarse geo/DC steering, then a real LB inside for fine control.

**Where you'll see it:** AWS Route 53 latency/geo routing, Cloudflare and Akamai global traffic management, GeoDNS in front of multi-region deployments.

### Consistent-Hashing Load Balancing
**What it is (plain English):** A way to map requests (or keys) to servers so that when servers join or leave, *almost all* keys stay put. Servers and keys are placed on a conceptual ring (0 … 2³²); a key goes to the first server clockwise from its hash.

**The problem it solves:** Naive `hash(key) % N` remaps nearly *every* key when N changes — catastrophic for caches (mass misses) and for sticky routing. Consistent hashing changes only ~1/N of keys when one node is added or removed.

**How it works (mechanics):** Ring of size 2³²; each server hashes to points on it. A key hashes to a point and walks clockwise to the next server.
```
Ring:  [0]---S1(hash 100)---key(150)→S1---S2(400)---S3(800)---[2^32]
```
Add S4 at 250: only keys between 100 and 250 (previously going to S2) move to S4 — the rest are untouched. **Numbers:** with 4 servers and 1,000,000 keys, `% N` rehash on scaling to 5 moves ~800,000 keys; consistent hashing moves ~200,000 (1/N). **Virtual nodes** (each server placed at, say, 150 ring points) smooth out uneven splits so no server gets a lopsided arc.

**Trade-offs / when NOT to use it:** Without virtual nodes, load can be lumpy (one server owns a big arc). It adds complexity vs plain round robin, and it doesn't by itself account for server load — it accounts for *stability*. Skip it if your backends are stateless and you don't need affinity.

**Where you'll see it:** Amazon DynamoDB and Cassandra partitioning, Discord's session routing, Google Maglev, memcached client sharding, CDN edge selection.

### Health Checks & Failure Handling
**What it is (plain English):** The LB continuously pings each backend and routes only to healthy ones. When a server fails checks it's *ejected* from rotation; when it recovers it's added back — so a dead box stops receiving traffic within seconds.

**The problem it solves:** Without health checks, the LB happily forwards requests to a crashed or overloaded server, and users get errors. Health checks turn a partial failure into a non-event by rerouting around it.

**How it works (mechanics):** Two kinds. **Active:** the LB probes an endpoint (e.g., `GET /healthz` every 5 s; mark unhealthy after 3 consecutive failures, healthy after 2 successes). **Passive:** the LB watches real traffic and ejects a backend that returns errors/timeouts (Envoy "outlier detection").
```
t=0s  /healthz → 200  (healthy)
t=5s  → timeout (fail 1)
t=10s → timeout (fail 2)
t=15s → timeout (fail 3) → EJECT B, reroute B's share to A,C
t=…   → 200,200 → RE-ADD B
```
With a 5 s interval and 3-failure threshold, a dead server is drained in ~15 s. **Anti-flapping:** require multiple consecutive successes before re-adding, and cap how many backends can be ejected at once (don't eject the whole fleet on a shared dependency blip). **Cascading failure** guard: pair with connection draining and circuit breakers.

**Trade-offs / when NOT to use it:** Too aggressive (1 failure = eject) causes flapping under transient blips; too lax (30 s interval) leaves users hitting a dead box for half a minute. A *shallow* health check (TCP connect) can call a broken app "healthy"; a *deep* check (hits the DB) can eject the whole fleet if the DB hiccups. Tune the depth deliberately.

**Where you'll see it:** AWS Target Group health checks, Kubernetes liveness/readiness probes, Envoy outlier detection, HAProxy `check inter 5s fall 3 rise 2`.

## Comparison Table

| Dimension | Round Robin | Weighted | Least-Connections | Least-Time |
|---|---|---|---|---|
| State needed | counter only | preset weights | live conn counts | conns + response times |
| Handles uneven servers | no | yes | somewhat | yes |
| Handles uneven request cost | no | no | yes | yes (best) |
| Overhead | lowest | low | medium | highest |

**Verdict:** Start with round robin for a uniform, stateless fleet; move to least-connections when request costs vary, weighted when server sizes vary, and least-time when latency matters most.

## Common Misconceptions
- **Myth:** A load balancer is just round robin. → **Reality:** Policy is one axis; layer (L4/L7), health checking, TLS, and hashing matter just as much.
- **Myth:** DNS load balancing can fail traffic over instantly. → **Reality:** Resolver/browser caching ignores low TTLs; drains take minutes, so DNS is coarse steering, not fast failover.
- **Myth:** `hash(key) % N` is a fine way to shard. → **Reality:** Changing N remaps ~all keys; use consistent hashing to move only ~1/N.
- **Myth:** L7 is always better than L4. → **Reality:** L7 costs CPU and can't match L4's packet throughput; big designs use both in tiers.
- **Myth:** Health checks make you immune to failure. → **Reality:** A deep check tied to a shared dependency can eject the *entire* fleet at once — checks need thresholds and ejection caps.

## Real-World Case
Google built **Maglev**, a software Layer-4 load balancer, because commodity servers running it were cheaper and more flexible than hardware LB appliances. A key design choice: consistent hashing with a twist, so that when a Maglev node is added or removed, existing TCP connections mostly stay pinned to the same backend — critical because breaking a live connection means a user-visible reset. Each Maglev machine handles millions of packets per second and the fleet fronts Google's user-facing services. Their published paper reported handling the equivalent of a large fraction of Google's traffic on a modest number of machines. The lesson for you: even at Google scale, the LB tier is *software* running standard hashing and health-checking ideas — the same primitives in this lesson, just tuned relentlessly.

## Self-Test (answers at the bottom)
1. In plain round robin with 4 servers, which server handles the 10th request?
2. Give one workload where least-connections clearly beats round robin, and say why.
3. What can a Layer-7 LB do that a Layer-4 LB fundamentally cannot?
4. You shard 1,000,000 cache keys across 10 servers and add an 11th. Roughly how many keys move under `hash % N` vs consistent hashing?
5. Design sketch: A chat service uses long-lived websocket connections across 20 servers behind an LB, and you must add capacity during peak without dropping users. Which policy and hashing scheme, what health-check settings, and how do you add servers without a mass reconnect storm?

## Interview Soundbites
- "Round robin is fair only when requests and servers are uniform; the moment costs vary I move to least-connections or least-time."
- "L4 routes on IP and port at packet speed; L7 reads the HTTP request and routes on path, cookie, and header — so I stack an L4 edge in front of L7 proxies."
- "I never shard with `hash % N` in production — one node change remaps everything; consistent hashing with virtual nodes moves only about 1/N of keys."

## Mini-Assignment
On paper (~30 min): (1) Draw a consistent-hashing ring with 4 servers and 8 keys (pick hash values). Remove one server and list exactly which keys move and which don't. Then add virtual nodes (3 per server) and note how the arcs even out. (2) Write a health-check spec for a backend fleet: probe path, interval, unhealthy threshold, healthy threshold, and one anti-flapping rule. Compute how many seconds a dead server keeps receiving traffic under your settings.

## Recap & Tomorrow
- **Round robin:** fair rotation, zero state; assumes uniform servers and requests.
- **Weighted / least-connections / least-time:** account for capacity, live load, and latency respectively.
- **L4 vs L7:** packet-speed IP/port routing vs content-aware HTTP routing (path, cookie, TLS); often stacked.
- **DNS LB:** coarse, cache-sticky geo/DC steering — not fast failover.
- **Consistent hashing:** moves only ~1/N keys on scaling; virtual nodes smooth the load.
- **Health checks:** active + passive probes eject bad backends in seconds; threshold and cap ejections to avoid flapping and fleet-wide outages.

Tomorrow, **Lesson 7 — Networking & Data Centers I**: the OSI model, DNS servers, NAT, how routers and switches actually move packets, Layer-2 vs Layer-3 traffic, and spine-leaf data-center fabric — the plumbing every distributed system rides on.

## Self-Test Answers
1. Requests cycle A,B,C,D,A,B,C,D,A,B… The 10th request is index 9 (0-based) → 9 mod 4 = 1 → the **2nd server (B)**.
2. Long-lived connections of uneven duration — e.g., websockets or streaming downloads. Round robin keeps assigning new connections evenly by *count of dealt requests*, but some servers accumulate many still-open long connections while others' connections finished. Least-connections routes to whoever actually has the fewest *open* connections, matching real load.
3. Read and route on the *content* of the request — URL path, HTTP headers, cookies — enabling path-based routing, cookie stickiness, header rewriting, and TLS termination. An L4 LB only sees IP addresses and ports, never the HTTP payload, so it can't do any of that.
4. Under `hash % N` (10→11), the modulus changes so almost every key's target changes — roughly **900,000+ of 1,000,000 move**. Under consistent hashing, only the keys in the arc now owned by the new server move — about 1/11 ≈ **~91,000 keys**; the rest stay put.
5. Use **consistent hashing with virtual nodes** to pin each websocket session to a server so existing connections aren't disturbed, and **least-connections** for new sessions. Health checks: `GET /healthz` every 5 s, unhealthy after 3 fails, healthy after 2 — draining a dead node in ~15 s, with an ejection cap so a shared blip can't drop the fleet. Add capacity by inserting new virtual nodes on the ring: only ~1/N of *future* sessions route to the new servers, and existing connections stay pinned, so there's no mass reconnect storm.
