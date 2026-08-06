# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 19 of 63 — Phase 1: Foundations (Module 19 of 28)
**Module:** Consistent Hashing in Detail
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
This is one of the two or three ideas that separate a candidate who *has read about* distributed systems from one who *understands* them. The naive way to shard data across N servers — `hash(key) % N` — quietly detonates the moment N changes: adding one server can move nearly *every* key, stampeding your database as caches all miss at once. Consistent hashing is the fix, and it powers Amazon DynamoDB, Cassandra, Discord's message store, and every serious distributed cache. In Phase 2 you will reach for it in almost every design that shards or load-balances. Today we derive it from scratch and prove, with real numbers, exactly what fraction of keys move.

## Learning Objectives
By the end of this lesson you can:
- Explain why `hash(key) % N` fails when N changes and quantify how many keys move.
- Draw the hash ring and map both nodes and keys onto it.
- Explain virtual nodes and why they fix load skew, with a variance number.
- Derive the rebalancing fraction (≈ K/N keys move) and compute it with real numbers.
- Explain how consistent hashing powers distributed caches and load balancers.

## The Lesson

### The Problem with Naive Modulo Hashing
**What it is (plain English):** The obvious way to spread K keys over N servers is `server = hash(key) % N`. It's simple and balanced — until N changes.

**The problem it solves (and then creates):** Modulo gives even distribution, but the mapping depends on N. Change N and almost every key's `% N` result changes, so almost every key relocates.

**How it works (mechanics):** Take 12 keys and 4 servers, then add a 5th.
```
key hash:  0  1  2  3  4  5  6  7  8  9 10 11
% 4:       0  1  2  3  0  1  2  3  0  1  2  3
% 5:       0  1  2  3  4  0  1  2  3  4  0  1
moved?     .  .  .  .  Y  Y  Y  Y  Y  Y  Y  Y   -> 8 of 12 = 67% moved!
```
Going from 4→5 servers moved 8 of 12 keys. In general, ~(N−1)/N of keys move — for 100→101 servers, ~99% relocate. Every moved key is a cache miss; all of them missing at once is a **thundering herd** that can crush the origin database.

**Trade-offs / when NOT to use it:** Modulo is fine only when N is *fixed forever* — a fixed-size in-memory partition you never resize. The instant you autoscale or a node dies, it's a landmine.

**Where you'll see it:** The classic wrong answer in interviews, and the cause of real cache-stampede outages when teams naively `% N` across a memcached fleet.

### The Hash Ring
**What it is (plain English):** Instead of a line of N buckets, imagine a circle of hash values, say 0 to 2³²−1 wrapping around. You place both **servers** and **keys** on this ring using the same hash function. Each key belongs to the first server you meet going clockwise.

**The problem it solves:** It decouples a key's position from N. A key's spot on the ring never changes; only which server is "next clockwise" can change — and only for a small arc when a node joins or leaves.

**How it works (mechanics):** Hash each server (e.g., by its ID/IP) to a point on the ring; hash each key too. To find a key's server, walk clockwise to the next server node.
```
              0 / 2^32
          Sa •           ← key k1 lands here, walks CW → Sa
       /            \
   Sd •              • Sb   ← k2 here → Sb
       \            /
          Sc •
```
Lookups use a sorted structure of server positions (a TreeMap / sorted array) and binary search: **O(log N)** to find the successor. When server **Sb** dies, only keys in the arc between Sa and Sb move — to Sc, the next clockwise node. Every other key stays put.

**Trade-offs / when NOT to use it:** With few nodes, random placement leaves **uneven arcs** — one server may own a 40% arc while another owns 10%, causing hot spots. That's exactly what virtual nodes fix. Also, a departing node dumps all *its* load onto a single successor, not the whole cluster — again fixed by virtual nodes.

**Where you'll see it:** Amazon Dynamo's original design, Cassandra's token ring, Riak, and libketama (the memcached client that popularized it).

### Virtual Nodes (vnodes)
**What it is (plain English):** Instead of placing each physical server once on the ring, place it many times — say 100–200 "virtual" copies at different hashed positions. Each vnode owns a tiny arc, and a physical server owns the union of its vnodes' arcs.

**The problem it solves:** Load skew and lumpy failover. With one point per server and, say, 5 servers, the arcs are wildly uneven and a death dumps everything on one neighbor. Spreading each server across 150 points averages out the arcs and spreads a dead node's load across *many* survivors.

**How it works (mechanics):** For 5 servers × 150 vnodes = 750 points scattered around the ring. Load variance shrinks as 1/√(vnodes): with ~100–200 vnodes per server, each server's share of keys stays within a few percent of the ideal 1/N. When a server dies, its ~150 arcs are each inherited by a *different* clockwise neighbor, so the extra load fans out roughly evenly across the remaining servers instead of hammering one.
```
Physical S1 -> vnodes at ring positions: 12, 88, 140, 203, ...
Physical S2 -> vnodes at: 30, 95, 160, ...
(interleaved, so each server's arcs are spread all around the ring)
```

**Trade-offs / when NOT to use it:** More vnodes = more metadata and a bigger ring to search/store; extremely high counts add memory and bookkeeping overhead. There's a sweet spot (Cassandra historically defaulted to 256 tokens/node, later tuned down to ~16 with better allocation). Non-uniform hardware also wants weighted vnodes (a beefier box gets more).

**Where you'll see it:** Cassandra "num_tokens", DynamoDB partitioning, Discord's Cassandra/ScyllaDB clusters, and every production consistent-hash implementation — nobody runs one-point-per-node in prod.

### Rebalancing Math — What Fraction of Keys Move
**What it is (plain English):** The headline promise of consistent hashing: when you add or remove a server, only about **1/N** of the keys move, not almost all of them. Let's prove it with numbers.

**The problem it solves:** It bounds the disruption of scaling. Adding capacity should be a gentle event, not a cluster-wide cache stampede.

**How it works (worked example with real numbers):** Suppose K = 1,000,000 keys spread evenly over N = 10 servers → 100,000 keys each. Now add an 11th server.
- A new server slots into the ring and, thanks to vnodes, steals a fair share of arcs from the *existing* servers.
- Fair share for the new node = K / (N+1) = 1,000,000 / 11 ≈ **90,909 keys** move.
- That's ~9.1% of keys — versus ~91% (10/11) with naive modulo. A **~10x** reduction in data movement.

General formula: adding the (N+1)th node moves **K/(N+1)** keys; removing one of N nodes moves **K/N** keys. Contrast with modulo, which moves ~K·(N−1)/N. For a 100-node cluster: consistent hashing moves ~1% on a scale event; modulo moves ~99%.

```
             keys moved when adding a node
Modulo:      ~ (N-1)/N   -> 10 nodes: 90.9%
Consistent:  ~ 1/(N+1)   -> 10 nodes:  9.1%
```

**Trade-offs / when NOT to use it:** The 1/N guarantee assumes good vnode spread; with too few vnodes the moved fraction still averages 1/N but individual servers can be lopsided. And moving even 1/N of a huge dataset is real I/O — you still throttle and stream it.

**Where you'll see it:** This exact math is why Cassandra/Dynamo can add nodes under live traffic, and why a well-configured memcached fleet survives an autoscale event without a stampede.

### How It Powers Distributed Caches & Load Balancers
**What it is (plain English):** Consistent hashing is the routing brain that decides *which* of N cache servers or backend servers owns a given key or session — while surviving node churn gracefully.

**The problem it solves:** In a cache fleet, you want a key to reliably hit the *same* server so it stays cached (high hit ratio); in a load balancer, you may want the *same client/session* to hit the *same* backend (sticky sessions). Both need a mapping that barely changes when servers come and go.

**How it works (mechanics):** *Distributed cache:* the client hashes the key onto the ring and talks directly to the owning cache node — no central coordinator. When a node dies, only its ~1/N of keys miss and re-warm elsewhere; the other ~90%+ stay hot. *Load balancer:* "consistent hashing" balancing (e.g., in HAProxy/Envoy/nginx `hash $arg_user consistent`) routes a given key/IP to a stable backend, so adding a server reshuffles only ~1/N of flows instead of resetting every connection.
```
client --hash(user_id)--> ring --> backend B3 (stable across scale events)
```

**Trade-offs / when NOT to use it:** Sticky/consistent routing can create hot spots if one key is disproportionately popular (a celebrity user); you may add "bounded loads" or fall back to round-robin for even CPU spread. For stateless backends where any server will do, plain round-robin/least-connections is simpler.

**Where you'll see it:** memcached (libketama), Redis Cluster's hash slots (a fixed 16,384-slot variant), Envoy/HAProxy ring-hash load balancing, DynamoDB, Cassandra, CDN request routing.

## Comparison Table

| Dimension | Modulo `hash % N` | Consistent Hashing (+ vnodes) |
|---|---|---|
| Keys moved on adding a node | ~(N−1)/N (≈91% at N=10) | ~1/(N+1) (≈9% at N=10) |
| Lookup cost | O(1) | O(log N) via sorted ring |
| Load balance | Even (while N fixed) | Even with enough vnodes |
| Failover | Rehashes ~everything | Only dead node's ~1/N of keys |
| Handles autoscaling | No — stampede | Yes — gentle |

**Verdict:** Use modulo only for a truly fixed N; the moment nodes join or leave under live traffic, consistent hashing with virtual nodes is the standard.

## Common Misconceptions
- **Myth:** Consistent hashing means no keys move when you add a server. → **Reality:** About 1/N move — the win is that it's ~1/N, not ~all.
- **Myth:** One ring point per server is enough. → **Reality:** That causes severe load skew and lumpy failover; you need ~100–200 virtual nodes per server.
- **Myth:** It requires a central coordinator. → **Reality:** Clients can compute the owner locally from the ring; that's the whole point of the decentralized Dynamo design.
- **Myth:** Redis Cluster uses a hash ring. → **Reality:** It uses a fixed 16,384 hash-slot variant of the same idea, not a continuous ring — but the "minimal movement" property is the same.
- **Myth:** More virtual nodes are always better. → **Reality:** Beyond a point they add metadata and lookup overhead for diminishing balance gains.

## Real-World Case
Discord stores billions of messages and outgrew their original database, migrating to Cassandra (and later ScyllaDB). Both rely on consistent hashing over a token ring with virtual nodes to shard messages by channel. The payoff is operational: Discord can add nodes to absorb growth, and when a node fails, only that node's ~1/N slice of partitions needs to be served by replicas and re-streamed — the rest of the cluster keeps humming. Their well-known pain point was *hot partitions* (a single wildly-active channel overwhelming one shard), which is the classic consistent-hashing caveat: the fraction-moved math is beautiful, but a single super-popular key still lands on one node. The fix is application-level: split hot partitions and add bounded-load routing — proof that the algorithm handles *scale* but you must still handle *skew*.

## Self-Test (answers at the bottom)
1. With `hash % N`, roughly what fraction of keys move when you grow from 4 to 5 servers? Show it.
2. On a hash ring, how do you find which server owns a given key, and what's the lookup complexity?
3. Why does one ring position per physical server cause problems, and how do virtual nodes fix both load skew and failover?
4. You run 20 cache servers holding 2,000,000 keys and add a 21st. Roughly how many keys move with consistent hashing, and how many would move with modulo?
5. Design sketch: You're building a distributed session cache expected to autoscale between 8 and 40 nodes during the day. Which routing scheme, how many virtual nodes per server, and what happens to hit ratio when the fleet scales from 20→21 nodes?

## Interview Soundbites
- "`hash % N` is a landmine: change N and ~(N−1)/N of your keys move at once — a guaranteed cache stampede. Consistent hashing moves only ~1/N."
- "Virtual nodes are non-negotiable — one point per server gives lumpy arcs and dumps a dead node's whole load on one neighbor; 150 vnodes spread both the keys and the failover."
- "For N=10, adding a node moves ~9% of keys with a hash ring versus ~91% with modulo — that 10x difference is why every distributed cache uses it."

## Mini-Assignment
On paper (~30 min): (1) Draw a hash ring with 4 servers (Sa, Sb, Sc, Sd) at positions 10, 90, 170, 250 on a 0–255 ring, and place keys hashing to 5, 95, 200, 255 — assign each to its clockwise owner. (2) Now kill Sb; list exactly which keys move and to which server, and confirm the rest don't move. (3) Compute the rebalancing fraction for a real cluster: K=5,000,000 keys, N=25 servers, you add one — how many keys move with consistent hashing vs modulo? (4) In two sentences, explain why you'd configure ~150 virtual nodes per server rather than 1.

## Recap & Tomorrow
- **Modulo hashing:** even but brittle — changing N moves ~(N−1)/N of keys; a stampede waiting to happen.
- **The hash ring:** place servers and keys on a circle; a key belongs to the next clockwise server; O(log N) lookup.
- **Virtual nodes:** ~100–200 points per server to even out arcs and fan out failover load.
- **Rebalancing math:** adding a node moves ~K/(N+1) keys (≈9% at N=10) — ~10x less than modulo.
- **Powers caches & LBs:** stable key→server mapping keeps hit ratios high and sessions sticky through churn; watch for hot keys.

Tomorrow, **Lesson 20 — Cloud & AWS Basics**: EC2 instances, security groups and firewalls, CloudWatch monitoring, and the VPC / subnet / internet-and-NAT-gateway networking model that every cloud design sits on.

## Self-Test Answers
1. About 8 of 12 in the worked example, i.e., ~(N−1)/N = 4/5 = 80% of keys move (the general rate; the specific example showed 8/12 ≈ 67% due to small-sample rounding). The point stands: most keys relocate.
2. Hash the key onto the ring and walk clockwise to the first server position (the "successor"), found by binary search over a sorted list of server positions — O(log N).
3. One point per server gives uneven arcs (hot spots) and makes a dead node dump all its keys on a single neighbor. Virtual nodes place each server at ~150 spots so arcs even out (variance shrinks ~1/√vnodes) and a dead node's arcs are inherited by many different survivors.
4. Consistent hashing: ~K/(N+1) = 2,000,000/21 ≈ 95,238 keys (~4.8%). Modulo: ~(N−1)/N = 20/21 ≈ 95% → ~1,900,000 keys. Roughly a 20x difference.
5. Use consistent hashing with virtual nodes (~100–200 per server) so clients route keys to stable owners. On a 20→21 scale-up, only ~1/21 ≈ 4.8% of sessions move; those miss and re-warm while ~95% stay hot, so the hit ratio dips only slightly instead of collapsing.
