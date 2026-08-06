# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 1 of 63 — Phase 1: Foundations (Module 1 of 28)
**Module:** Foundations of Architecture
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Every system design interview and every production incident traces back to the four ideas in this lesson. When Amazon says "every 100ms of latency cost us 1% in sales," that is this module. When your single database server tips over at 3 AM under Black Friday load, the question "do I buy a bigger box or add more boxes?" is this module. Foundations of Architecture is the vocabulary and the physics on which all of Phase 2 (Lesson 24 Caches, Lesson 31 Distributed Cache, Lesson 40 Load Balancing) is built. Get these numbers wrong and every later design is built on sand.

## Learning Objectives
By the end of this lesson you can:
- Explain from first principles why "just write good code" is not enough and architecture is required at scale.
- Decide between vertical and horizontal scaling using real cost and ceiling numbers, not gut feel.
- Articulate the three hard reasons distributed systems exist despite the pain they cause.
- Define latency, throughput, and availability precisely, and compute "the nines" into minutes of downtime per year.
- Distinguish p50 from p99 latency and explain why tail latency dominates user experience.

## The Lesson

### Why Architecture Is Required
**What it is (plain English):** Architecture is the set of high-level decisions about how a system's parts are split, where they run, and how they talk — decisions that are expensive to reverse later. It is the load-bearing structure, not the paint. A building's architecture decides where the columns go; you can repaint a wall cheaply, but you cannot move a column after the concrete sets.

**The problem it solves:** A single program on a single machine has hard physical ceilings: one CPU's cores, one box's RAM, one disk's IOPS, one NIC's bandwidth. It also has one failure domain — that box dies, the whole service dies. Without deliberate architecture, your system's capacity is whatever one machine can do (~a few thousand QPS for a typical app server) and its availability is whatever one machine's uptime is (~99%, i.e., ~3.65 days down/year).

**How it works (mechanics):** Architecture attacks two axes at once — scale and failure — by decomposition. Worked example: a monolith serving 2,000 QPS on one 8-core box hits 90% CPU. You split it: a stateless app tier behind a load balancer (add boxes to grow), a database tier, and a cache tier.
```
        [Load Balancer]
        /      |      \
   [App1]  [App2]  [App3]   <- stateless, scale by adding boxes
        \      |      /
          [Cache]  [DB]     <- stateful, scaled differently
```
Now 3 app boxes serve ~6,000 QPS, and one dying box drops 1/3 of capacity, not 100%.

**Trade-offs / when NOT to use it:** Architecture adds moving parts — network hops (each adds 0.5–2ms), operational cost, and new failure modes. For a 100-user internal tool, a single box is correct; premature distribution is a classic over-engineering trap.

**Where you'll see it:** Every Phase 2 design starts here. Netflix, Uber, and Amazon all began as monoliths and re-architected only when the numbers forced it.

### Vertical vs Horizontal Scaling
**What it is (plain English):** Vertical scaling (scale-up) means making one machine bigger — more CPU, RAM, faster disk. Horizontal scaling (scale-out) means adding more machines and splitting work across them. Vertical is a bigger truck; horizontal is more trucks.

**The problem it solves:** Both answer "we're out of capacity," but differently. Vertical buys headroom with zero code change. Horizontal removes the single-machine ceiling entirely and, as a bonus, gives you redundancy.

**How it works (mechanics):** Vertical: swap an AWS `m6i.large` (2 vCPU, 8 GB) for an `m6i.16xlarge` (64 vCPU, 256 GB) — 32x the box. But it stops: the largest EC2 instances top out around 448 vCPU / 12 TB RAM, and cost is super-linear — the big box often costs far more than 32 small ones. And it is still one failure domain.
```
 Vertical:   [ 8GB ] -> [ 256GB ]      one box, downtime to resize
 Horizontal: [box][box] -> [box][box][box][box]  add boxes live
```
Horizontal worked number: 4 boxes at 2,000 QPS each = 8,000 QPS; need 12,000? Add 2 boxes. Capacity grows roughly linearly and near-infinitely — *if* the work is stateless or shardable.

**Trade-offs / when NOT to use it:** Horizontal is hard: you need a load balancer, statelessness (or sticky sessions), and data partitioning; distributed bugs appear. Vertical is trivial but has a hard ceiling and no redundancy. Databases are often scaled vertically first because sharding is genuinely painful.

**Where you'll see it:** Stripe scaled Postgres vertically for years before sharding. Google's web tier is horizontal across hundreds of thousands of commodity boxes.

### Why We Need Distributed Systems
**What it is (plain English):** A distributed system is multiple computers, connected by a network, cooperating to look like one system to the user. We accept its enormous complexity only because three things force us to.

**The problem it solves:** Three hard limits of a single machine: (1) **Scale** — no single box holds 500 TB in RAM or serves 1M QPS. (2) **Availability** — one box has one power supply, one kernel; to survive a machine death you need a second machine, which is by definition distributed. (3) **Geography** — a user in Mumbai talking to a server in Virginia eats ~180ms round-trip from the speed of light alone; you need servers near users.

**How it works (mechanics):** Split state and computation across nodes and coordinate over the network. Latency-of-light worked number: light in fiber travels ~200,000 km/s. Mumbai↔Virginia is ~13,000 km one way, so ~65ms one way, ~130ms round trip *minimum*, before any processing. Put a replica in Mumbai and that user's read drops to single-digit ms.
```
 One region:  Mumbai user --130ms RTT--> [US server]
 Distributed: Mumbai user --~5ms--> [Mumbai replica]  <--async--> [US replica]
```

**Trade-offs / when NOT to use it:** You inherit the network's brutal realities — partial failures, message loss, and the CAP theorem (Lesson 2). Debugging is far harder. If one region and one box meet your scale, availability, and latency needs, stay simple.

**Where you'll see it:** Every large system — Cassandra, DynamoDB, Kafka, Google Spanner — is distributed precisely for scale, availability, and geography.

### Latency, Throughput, Availability & "The Nines"
**What it is (plain English):** Latency is how long one request takes (time). Throughput is how many requests you handle per second (rate). Availability is the fraction of time the system is up. They are independent — a system can have high throughput and terrible latency.

**The problem it solves:** These are the units you design and get graded in. You cannot say "make it fast" — you say "p99 read latency under 50ms at 20,000 QPS with 99.99% availability." That is measurable and testable.

**How it works (mechanics):** Latency is a distribution, not a number. Report percentiles: **p50** (median), **p99** (99th percentile — worst 1%). A p50 of 10ms with a p99 of 900ms means 1 in 100 users waits nearly a second. Since a page makes many calls, tail latency dominates: with 100 backend calls per page, the odds *none* hits the p99 are 0.99¹⁰⁰ ≈ 37% — so ~63% of pages hit at least one slow call. **Availability** as nines, mapped to downtime/year:

| Availability | Downtime/year | Downtime/day |
|---|---|---|
| 99% (two 9s) | 3.65 days | 14.4 min |
| 99.9% (three 9s) | 8.77 hours | 1.44 min |
| 99.99% (four 9s) | 52.6 min | 8.6 sec |
| 99.999% (five 9s) | 5.26 min | 0.86 sec |

Math: (1 − 0.9999) × 365 × 24 × 60 = 52.6 minutes.

**Trade-offs / when NOT to use it:** Each extra nine costs exponentially more (redundancy, on-call, automation). Five nines for a blog is waste; four nines for payments is table stakes.

**Where you'll see it:** AWS S3 targets 99.99% availability; SLA credits kick in when it's missed. Amazon's "100ms = 1% sales" and Google's latency-ranking studies are this module in the wild.

## Comparison Table

| Dimension | Vertical Scaling (scale-up) | Horizontal Scaling (scale-out) |
|---|---|---|
| How | Bigger single machine | More machines |
| Ceiling | Hard (~448 vCPU / 12 TB) | Near-unlimited |
| Redundancy | None (one failure domain) | Built-in |
| Code changes | ~None | LB, statelessness, sharding |
| Cost curve | Super-linear | Roughly linear |
| Best for | Databases early, simple apps | Stateless web tiers, scale |

**Verdict:** Scale up first for simplicity; scale out when you hit the ceiling or need redundancy — most mature systems end up doing both (big DB boxes, many app boxes).

## Common Misconceptions
- **Myth:** More powerful hardware always fixes scale. → **Reality:** Vertical scaling has a hard ceiling and gives zero redundancy; past a point you must scale out.
- **Myth:** Latency and throughput are the same thing. → **Reality:** Latency is time-per-request; throughput is requests-per-second. Batching can raise throughput while *worsening* latency.
- **Myth:** Average latency describes user experience. → **Reality:** Averages hide the tail; p99/p999 is what users feel, especially when a page fans out to many services.
- **Myth:** 99.9% availability is basically always up. → **Reality:** That's still 8.77 hours of downtime a year.
- **Myth:** Distributed systems are strictly better. → **Reality:** They trade simplicity for scale/availability and introduce partial failure — only adopt when the numbers demand it.

## Real-World Case
In 2018, an AWS engineer's back-of-envelope math saved a launch. A team wanted five-nines (99.999%) availability for an internal reporting dashboard used by ~200 analysts during business hours. Building it meant multi-region active-active, automated failover, and a 24/7 on-call rotation — roughly a 3x cost and months of work. The principal engineer asked one question: "What's the actual cost of 52 minutes of downtime a year for a dashboard people check twice a day?" The honest answer was "near zero." They shipped single-region 99.9%, redirected the saved engineering months to the customer-facing checkout path (where 100ms genuinely moved revenue), and were done in two weeks. The lesson: nines are a business decision priced in engineering effort, not a badge of honor.

## Self-Test (answers at the bottom)
1. Define latency and throughput, and give an example where one improves while the other worsens.
2. Convert 99.99% availability into downtime per year and per day.
3. Your single DB box is at 95% CPU serving 4,000 QPS. Walk through when you'd scale vertically vs horizontally, with the trade-off of each.
4. A page makes 50 independent backend calls, each with a p99 of 100ms. Roughly what fraction of pages will experience at least one 100ms+ call, and what does that tell you about tail latency?
5. Design sketch: You're launching a photo-sharing app expecting 500 QPS at launch, growing 10x/year, users in India + US, targeting 99.9% availability. Describe your scaling approach for the app tier and why, and one thing you'd change if you needed 99.99%.

## Interview Soundbites
- "Latency is a distribution, not a number — I design to p99, because at scale the tail is the user experience."
- "I scale up for simplicity and scale out for headroom and redundancy; vertical has a hard ceiling and one failure domain, horizontal removes both but demands statelessness and sharding."
- "We adopt distributed systems for exactly three reasons — scale, availability, geography — and we pay for it in partial failure and CAP trade-offs."

## Mini-Assignment
On paper (~30 min): (1) Build the nines table yourself — for 99%, 99.9%, 99.99%, 99.999%, compute downtime per year, per month, and per day from first principles ((1−A) × time). (2) Pick a system you use daily (say, a chat app). Estimate its QPS at peak, decide vertical vs horizontal for each tier (app, DB, cache), and pick a target availability with a one-line business justification. Write the target as a single SLA sentence: "p99 X ms at Y QPS with Z% availability."

## Recap & Tomorrow
- **Why architecture:** single machines have hard capacity and failure ceilings; architecture decomposes to beat both — but don't over-engineer small systems.
- **Vertical vs horizontal:** scale-up is simple with a hard ceiling and no redundancy; scale-out is near-unlimited and redundant but needs statelessness and sharding.
- **Why distributed:** scale, availability, and geography force it, despite partial-failure pain.
- **Latency/throughput/availability:** distinct units; design to p99; each extra nine costs exponentially (99.99% = 52.6 min/year).

Tomorrow, **Lesson 2 — CAP Theorem & ACID Properties**: what actually happens to your data during a network partition, why you can't have consistency and availability at the same time, and the ACID/BASE/PACELC trade-offs every database forces on you.

## Self-Test Answers
1. Latency = time for one request (e.g., 20ms); throughput = requests served per second (e.g., 5,000 QPS). Batching writes raises throughput (fewer disk flushes) but each request now waits for the batch, worsening latency — they trade off.
2. (1 − 0.9999) × 365.25 × 24 × 60 ≈ 52.6 minutes/year, ≈ 8.6 seconds/day.
3. Scale vertically first if a bigger box (more CPU/RAM/IOPS) buys headroom with no code change — fast and simple, but there's a hard ceiling and still one failure domain. Scale horizontally (read replicas, then sharding) when you hit that ceiling or need redundancy — near-unlimited capacity and fault tolerance, but you pay in partitioning complexity and consistency challenges.
4. P(no slow call) = 0.99⁵⁰ ≈ 0.605, so ~39.5% of pages hit at least one 100ms+ call. Tail latency compounds under fan-out: even a rare slow call becomes common across many parallel calls, so you must attack p99, not the average.
5. Stateless app tier behind a load balancer, scaled horizontally — start with ~2 boxes for redundancy, add boxes as QPS grows 10x/year. Deploy a replica/region in India and one in the US for geography (cut cross-ocean 130ms RTT). 99.9% is single-region-per-geo with a standby. For 99.99%, add automated multi-AZ failover and remove single points of failure (redundant LBs, multi-AZ DB), accepting the added cost and operational load.
