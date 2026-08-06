# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 28 of 63 — Phase 1: Foundations (Module 28 of 28)
**Module:** Capacity Planning & Estimation
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
This is the last lesson of Phase 1, and it's the one that makes every design *concrete*. When someone asks "do we need 200 servers or 300?", "will this fit in RAM?", or "how much bandwidth does the CDN need?", the answer isn't a shrug — it's arithmetic. Every real capacity plan, every AWS bill defense, and every strong interview answer runs on back-of-envelope math you can do on a whiteboard in 90 seconds. Companies overspend millions by guessing high or fall over by guessing low. Today you learn to compute the numbers that turn a hand-wavy design into a defensible one — the exact skill Phase 2 leans on in every single lesson.

## Learning Objectives
By the end of this lesson you can:
- Do back-of-envelope math with rounded powers of 10 and the latency numbers every engineer should know.
- Convert a QPS target into a concrete server count using per-server throughput.
- Size RAM, SSD, and disk for a given dataset and access pattern.
- Compute cache size from the working set and a hit-rate target.
- Estimate bandwidth in and out, and answer "200 or 300 servers?" with a margin.

## The Lesson

### Back-of-Envelope Math
**What it is (plain English):** Fast approximate calculation using round numbers to size a system in your head. You trade precision for speed — being within 2× is a win; the goal is the right *order of magnitude*, not decimals.
**The problem it solves:** You can't provision infrastructure or defend a design without numbers, but you also can't run a load test on a whiteboard. Back-of-envelope math gets you a defensible estimate in under two minutes.
**How it works (mechanics):** Memorize a few anchors and round aggressively:
```
Time:   1 day ≈ 86,400 s ≈ 10^5 s (round up).  1 month ≈ 2.5M s.
Powers: KB=10^3, MB=10^6, GB=10^9, TB=10^12, PB=10^15
Latency you must know (Jeff Dean's numbers):
   L1 cache ref .......... 0.5 ns
   Main memory ref ....... 100 ns
   SSD random read ....... 16 µs (~150K IOPS)
   Network round trip DC .. 0.5 ms
   Disk seek (HDD) ....... 10 ms
   Cross-continent RTT ... 150 ms
```
Worked: 100M requests/day ÷ 10^5 s ≈ **1,000 req/sec** average. Peak is typically 2×–3× average, so plan for ~2,500–3,000 req/sec. Every estimate starts by turning a daily total into per-second.
**Trade-offs / when NOT to use it:** It's an *estimate* — never present it as exact. The failure mode is a hidden 1000× error (e.g., forgetting a 100:1 read:write ratio), so always sanity-check the order of magnitude aloud.
**Where you'll see it:** Every system-design interview, every capacity doc, every "can this fit on one box?" Slack thread.

### QPS → Number of Servers
**What it is (plain English):** Turning a request rate (queries per second) into how many machines you need, by dividing total QPS by what one server can handle. This is the "200 or 300 servers" question in its purest form.
**The problem it solves:** Underprovision and you fall over at peak; overprovision and you burn budget. This math sets the fleet size with a deliberate safety margin.
**How it works (mechanics):** Server throughput comes from per-request CPU time (or a load test):
```
Given: peak 230,000 QPS. One server handles ~5,000 QPS
       (e.g., request takes ~10 ms of a core; 8 cores → 800 req/s/core-ms...
        or measured: 5K QPS/server from a load test).
Servers (bare) = 230,000 / 5,000 = 46 servers
Add headroom: never run at 100%. Target 70% utilization:
       46 / 0.70 ≈ 66 servers
Add redundancy (survive losing 1 AZ of 3 → lose 33%):
       66 / (1 − 0.33) ≈ 99 → round to ~100 servers
```
So the honest answer is "~100," not "46" — the margin between them *is* the engineering.
**Trade-offs / when NOT to use it:** The whole thing hinges on the per-server QPS figure; guess it wrong and everything's off. Measure it if you can. Also account for non-uniform load — one hot shard can need more servers than the average implies.
**Where you'll see it:** Autoscaling group sizing, Kubernetes HPA targets, every "how many pods?" decision.

### RAM / SSD / Disk Sizing
**What it is (plain English):** Deciding how much of each storage tier you need and which data lives where. RAM is fastest and smallest/most expensive; SSD is the middle; HDD is cheapest and largest/slowest. You place data by how hot it is.
**The problem it solves:** Put everything in RAM and the bill is absurd; put hot data on HDD and latency dies (10 ms seeks vs 100 ns RAM = 100,000×). Sizing each tier right is a cost-vs-latency optimization.
**How it works (mechanics):** Size from row size × row count, tier by access pattern:
```
Dataset: 500M users × 1 KB profile = 500 GB total.
Hot (10% accessed often): 50 GB → RAM/cache.
Warm (full dataset, point lookups): 500 GB → SSD (16 µs reads, 150K IOPS).
Cold (logs, backups, 5 yr): 100M events/day × 1 KB × 365 × 5 ≈ 182 TB → HDD/object storage.
Rough cost intuition: RAM ~$5/GB/mo, SSD ~$0.10/GB/mo, HDD/object ~$0.02/GB/mo.
  → keeping 500 GB in RAM ≈ $2,500/mo vs on SSD ≈ $50/mo. Tier deliberately.
```
Add ~20–30% overhead for indexes, replication (3× copies!), and fragmentation. Replication alone triples storage: 500 GB logical → 1.5 TB physical.
**Trade-offs / when NOT to use it:** Forgetting the 3× replication factor is the classic undercount. Also, "everything in RAM" is sometimes right (Redis-only architectures) when latency is worth the money — know when.
**Where you'll see it:** Database instance sizing (RAM for buffer pool), Redis capacity, S3 vs EBS decisions.

### Cache Sizing
**What it is (plain English):** Computing how much memory your cache needs to hold the "working set" — the subset of data that's accessed often enough to be worth keeping in RAM — to hit a target hit rate.
**The problem it solves:** Too small a cache thrashes (low hit rate, constant DB hits); too big wastes expensive RAM. The right size captures the hot set and stops there, exploiting the 80/20 (Pareto) access skew.
**How it works (mechanics):** Cache the hot fraction, then verify the DB survives the misses:
```
Reads: 230,000 QPS. Data: 500M items × 1 KB.
Working set: assume 20% of items get 80% of traffic → cache 100M × 1 KB = 100 GB.
Fits across a few cache nodes (e.g., 4 × 32 GB Redis).
Hit-rate check: target 95% hit.
  Misses to DB = 230,000 × (1 − 0.95) = 11,500 QPS
  → if one DB node does ~10K QPS, 2 replicas comfortably serve the misses.
Compare 90% hit: misses = 23,000 QPS → need ~3 DB nodes. The last 5% of
  hit rate literally halves your database fleet — that's why cache sizing matters.
```
Add TTL and eviction (LRU) so stale/cold entries free space automatically.
**Trade-offs / when NOT to use it:** Bigger caches have diminishing returns — going 95%→99% may need 3× the RAM for the long tail. Cache invalidation complexity rises with size. Don't cache write-heavy or strongly-consistent data blindly.
**Where you'll see it:** Redis/Memcached cluster sizing, CDN cache tuning, every read-heavy design in Phase 2.

### Bandwidth
**What it is (plain English):** How much data flows in and out per second — ingress (uploads/writes) and egress (downloads/reads). It's QPS multiplied by payload size, and it's often the *real* bottleneck and the biggest line on a cloud bill.
**The problem it solves:** A design can be fine on CPU and storage but saturate the network. Egress especially costs real money (~$0.05–0.09/GB on public clouds) and caps throughput.
**How it works (mechanics):** Bandwidth = QPS × average payload size:
```
Read-heavy API: 230,000 reads/sec × 2 KB response = 460 MB/sec ≈ 3.7 Gbps egress.
Write: 1,200 writes/sec × 2 KB = 2.4 MB/sec ≈ 19 Mbps ingress.
Video example: 1M concurrent viewers × 5 Mbps (1080p) = 5 Tbps → impossible
   from origin → you MUST use a CDN to push bytes from edge PoPs.
Monthly egress cost: 460 MB/s × 2.5M s/mo = 1.15 PB/mo × $0.05/GB ≈ $57,500/mo
   → cache/CDN to cut origin egress by 90% → ~$5,750/mo. That's the business case.
```
**Trade-offs / when NOT to use it:** Peak vs average matters hugely for bandwidth (a viral event can spike 10×). Under-provisioned network links cause packet loss and latency long before CPU maxes out. CDNs cut egress but add per-request and storage cost.
**Where you'll see it:** CDN sizing, NIC/instance network limits, cloud egress bill optimization, video/streaming design.

### Answering "Do We Need 200 Servers or 300?"
**What it is (plain English):** The synthesis question — combining QPS, per-server throughput, utilization headroom, redundancy, and growth into one defensible fleet number with a stated margin. The answer is never a bare number; it's a number *plus its justification*.
**The problem it solves:** "About 250, I guess" fails an interview and a budget review. A structured derivation — with each multiplier named — is how you defend the spend or the risk.
**How it works (mechanics):** Chain the multipliers explicitly:
```
1. Peak QPS:              300,000
2. Per-server capacity:   ÷ 2,000 QPS   → 150 servers (raw)
3. Utilization headroom:  ÷ 0.70        → 214 servers (run at 70%, not 100%)
4. Redundancy (N+1 AZ):   ÷ 0.67        → 320 servers (survive losing 1 of 3 AZs)
5. Growth buffer (6 mo):  × 1.10        → 352 → round to 350–360 servers
```
So "200 vs 300?" → **neither**: ~350, and here's every factor. If per-server capacity were 3,000 QPS instead, step 2 gives 100 → final ~235, and you'd say ~250. The sensitivity to that one input is exactly why you *measure* per-server throughput rather than guess.
**Trade-offs / when NOT to use it:** Don't gold-plate the margins — 3× headroom "to be safe" doubles the bill for no reason. State your assumptions so reviewers can push back on the per-server number or the growth rate. Autoscaling lets you provision for average and burst to peak, changing the math entirely.
**Where you'll see it:** Every capacity review, cloud cost negotiation, and the climactic follow-up in a scaling interview.

## Comparison Table

| Quantity | Formula | Worked example | Result |
|---|---|---|---|
| Avg QPS | daily ÷ 86,400 | 100M/day ÷ 10^5 | ~1,000/sec |
| Peak QPS | avg × 2–3 | 1,000 × 2.5 | ~2,500/sec |
| Servers | peak QPS ÷ per-server ÷ util ÷ redundancy | 300K ÷ 2K ÷ 0.7 ÷ 0.67 | ~320 |
| Storage/yr | daily items × size × 365 × replication | 100M × 1KB × 365 × 3 | ~110 TB |
| Cache size | working-set fraction × dataset | 20% × 500 GB | 100 GB |
| DB QPS after cache | reads × (1 − hit rate) | 230K × 0.05 | ~11.5K/sec |
| Egress | read QPS × payload | 230K × 2 KB | ~3.7 Gbps |

**Verdict:** Every capacity number is one small formula times a rounded input. Memorize the seven above, always apply utilization + redundancy margins to server counts, and always multiply storage by the replication factor — those two omissions cause most estimation errors.

## Common Misconceptions
- **Myth:** You should compute exact numbers. → **Reality:** Order of magnitude is the goal; round 86,400 to 10^5 and move on. Being within 2× is success.
- **Myth:** Server count = QPS ÷ per-server QPS. → **Reality:** That's the *raw* count; you must divide by utilization target (~0.7) and redundancy (~0.67), then add growth — often ~2× the raw number.
- **Myth:** Storage = data size. → **Reality:** Multiply by the replication factor (usually 3×) and add ~20–30% for indexes/overhead.
- **Myth:** A bigger cache always helps. → **Reality:** Diminishing returns on the long tail; 95%→99% hit can cost 3× the RAM. Size to the working set.
- **Myth:** CPU is the usual bottleneck. → **Reality:** For read-heavy or media systems, bandwidth (and its egress bill) saturates first — 1M viewers × 5 Mbps = 5 Tbps forces a CDN.

## Real-World Case
When Disney+ launched in November 2019, it hit ~10M sign-ups on day one — far above internal projections. The parts that held were the ones with honest capacity math and elastic, CDN-fronted delivery: video bytes were served from edge PoPs, not origin, so a 5-Tbps-class egress problem never touched their servers. The parts that strained were stateful services sized to a lower QPS estimate, which had to be scaled hard in the first hours. The takeaway architects repeat: get your peak-QPS and bandwidth estimates right *before* launch, apply generous-but-justified margins to the stateful tiers, and push all high-volume egress to a CDN — because the difference between "200 servers" and "350 servers" is the difference between a smooth launch and a trending-for-the-wrong-reasons outage.

## Self-Test (answers at the bottom)
1. Convert 500M requests/day to average QPS, then estimate peak.
2. Your dataset is 200M rows × 2 KB, stored with 3× replication and 25% index overhead. How much physical storage?
3. Peak is 400K QPS, one server does 2,500 QPS, you target 70% utilization and must survive losing 1 of 4 AZs. How many servers?
4. Reads are 300K QPS. Cache hit rate 96%; one DB replica serves 12K QPS. How many replicas do the misses need, and how does it change at 92% hit?
5. Design sketch: A photo-sharing app has 50M DAU, each viewing 40 photos/day (avg 300 KB each) and uploading 2 photos/day. Compute read QPS, egress bandwidth, daily upload storage (with 3× replication), and roughly how many read-serving servers if one handles 5,000 QPS at 70% utilization with N+1 across 3 AZs. State where a CDN changes the answer.

## Interview Soundbites
- "Raw server count is QPS over per-server throughput, but the real number divides by ~0.7 utilization and ~0.67 redundancy and adds growth — so 150 raw becomes ~320."
- "The last five points of cache hit rate literally halve the database fleet: 95% versus 90% is 11K versus 23K QPS hitting the DB."
- "For read-heavy or media systems, bandwidth saturates before CPU — a million 1080p viewers is five terabits per second, which is a CDN problem, not a server problem."

## Mini-Assignment
Pick a system with public scale numbers (Twitter/X ~500M posts/day, or YouTube ~1B hours watched/day). On paper (~30 min): (1) convert the headline number to average and peak QPS; (2) size total yearly storage with a realistic per-item size and 3× replication; (3) compute egress bandwidth for the read path; (4) derive a server count with explicit utilization, redundancy, and growth multipliers; and (5) write one sentence on where a CDN or cache changes each number by an order of magnitude. Show every multiplier — the goal is a derivation a review board couldn't poke a hole in.

## Recap & Tomorrow
- **Back-of-envelope:** round to powers of 10, know the latency anchors, turn daily totals into per-second.
- **QPS → servers:** divide by per-server throughput, then by utilization and redundancy, add growth.
- **RAM/SSD/disk:** tier by heat; size by row × count; never forget the 3× replication factor.
- **Cache sizing:** capture the ~20% working set; verify the DB survives the miss traffic.
- **Bandwidth:** QPS × payload; egress is the bill and often the real bottleneck; CDN for media.
- **"200 or 300?":** neither in a vacuum — chain the multipliers and state every assumption.

That closes **Phase 1: Foundations** — 28 modules of the components, trade-offs, framework, and math that every real system is built from. Tomorrow we start **Phase 2, Lesson 29 — Design a URL Shortener (TinyURL/bit.ly)**: your very first end-to-end design, where you'll run the eight-step framework from Lesson 27 and the capacity math from today for real — computing 100:1 read:write QPS, sizing the key space (why 7 base62 chars = 3.5 trillion URLs), choosing the ID-generation scheme, and defending the whole thing to a review board. Bring your envelope.

## Self-Test Answers
1. 500M ÷ 86,400 ≈ 500M ÷ 10^5 = 5,000 QPS average. Peak ≈ 2–3× → ~10,000–15,000 QPS. Always state the peak multiplier you assumed.
2. Logical = 200M × 2 KB = 400 GB. With 3× replication = 1.2 TB. Add 25% index overhead → 1.2 TB × 1.25 = **1.5 TB** physical.
3. Raw = 400,000 ÷ 2,500 = 160 servers. Utilization: 160 ÷ 0.70 ≈ 229. Redundancy (survive 1 of 4 AZs → lose 25%): 229 ÷ 0.75 ≈ 305 → round to **~305–310 servers**.
4. At 96% hit: misses = 300,000 × 0.04 = 12,000 QPS → exactly 1 replica (at its 12K limit), so realistically **2** for headroom/redundancy. At 92% hit: misses = 300,000 × 0.08 = 24,000 QPS → 24,000 ÷ 12,000 = 2 raw, **3 with headroom** — dropping 4 points of hit rate adds a whole DB replica.
5. Reads: 50M × 40 = 2B/day ÷ 10^5 ≈ 20,000 QPS avg, peak ~50,000 QPS. Egress: 20,000 × 300 KB = 6 GB/sec ≈ 48 Gbps avg (peak ~120 Gbps) — clearly a CDN job. Uploads: 50M × 2 = 100M photos/day × 300 KB = 30 TB/day logical × 3 replication = **90 TB/day**. Read servers on peak 50K QPS: 50,000 ÷ 5,000 = 10 raw ÷ 0.70 ≈ 15 ÷ 0.67 (N+1 of 3 AZs) ≈ **~22 servers** — but a CDN serves ~95% of photo bytes from the edge, so origin read QPS and the 48 Gbps egress both drop ~20×, cutting both the server count and the bandwidth bill by an order of magnitude.
