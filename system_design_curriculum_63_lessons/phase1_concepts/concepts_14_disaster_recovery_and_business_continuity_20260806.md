# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 14 of 63 — Phase 1: Foundations (Module 14 of 28)
**Module:** Disaster Recovery & Business Continuity
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Yesterday you learned the topologies; today you learn how to *survive the day the topology fails*. Disaster Recovery (DR) and Business Continuity are where architecture meets money and lawyers. When an AWS region browns out, when a datacenter floods, when someone fat-fingers `DELETE FROM users`, the question is not "will it happen" but "how much data and time will we lose, and did we design for it on purpose?" Netflix, banks, and airlines spend real budget here. Every senior interview eventually asks "what's your RTO and RPO?" — if you can't put numbers on it, you're an SDE1. Today we make those numbers rigorous.

## Learning Objectives
By the end of this lesson you can:
- Define RTO and RPO precisely and translate a business requirement into target numbers.
- Compare the four DR patterns (backup-restore, pilot light, warm standby, active-active) by cost and RTO.
- Design a backup strategy using the 3-2-1 rule and explain full vs incremental vs WAL archiving with real sizes.
- Explain how multi-region DR actually fails over (DNS, health checks, data replication) and its pitfalls.
- Justify why untested DR is worthless and describe a failover drill / game day.

## The Lesson

### RTO and RPO
**What it is (plain English):** Two numbers that define "how bad can it get." **RPO (Recovery Point Objective)** = how much *data* you can afford to lose, measured in time. **RTO (Recovery Time Objective)** = how long you can afford to be *down*, measured in time. RPO looks backward (to your last safe copy); RTO looks forward (to when you're back up).

**The problem it solves:** They turn vague fear ("we can't go down!") into an engineering budget. Zero RTO/RPO costs a fortune; being honest about what the business truly needs saves millions.

**How it works (mechanics):** Picture the disaster on a timeline:
```
   last good copy        DISASTER            back online
────────┼──────────────────┼───────────────────┼────────▶ time
        │◀──── RPO ───────▶│◀───── RTO ───────▶│
     (data lost)        (moment)          (downtime)
```
Worked example: you back up every 6 hours and the crash hits 5 hours after the last backup → **RPO experienced = 5 hours of data lost**. If rebuilding takes 90 minutes → **RTO = 90 min of downtime**. If the business says "we can lose at most 5 minutes of orders," a 6-hour backup interval *cannot* meet it — you need continuous replication (RPO ≈ seconds).

**Trade-offs / when NOT to use it:** Driving both to zero is exponentially expensive: RPO=0 needs synchronous replication (latency cost on every write); RTO≈0 needs a hot standby running 24/7 (double the infra bill). Not everything deserves it — an internal analytics dashboard can tolerate RPO=24h; a payments ledger cannot.

**Where you'll see it:** Every SLA and DR runbook; cloud provider tiers are literally sold on RTO/RPO. In Phase 2, capacity and cost estimates hang off these two numbers.

### Backup Strategies (3-2-1, full / incremental / WAL)
**What it is (plain English):** Systematic copies of your data so you can rebuild after loss or corruption. The gold standard is the **3-2-1 rule**: keep **3** copies of data, on **2** different media types, with **1** copy offsite.

**The problem it solves:** Replication protects against hardware failure but faithfully copies logical disasters (a bad `DELETE`, ransomware). Backups give you a *point in the past* to roll back to — the only defense against corruption and human error.

**How it works (mechanics):** Three backup types trade storage for restore speed:
- **Full:** complete copy every time. Simple restore, huge storage. A 1 TB DB backed up daily = 30 TB/month.
- **Incremental:** only changes since the last backup. Tiny (e.g. 20 GB/day of deltas) but restore = last full + *every* increment in order — slow and fragile if one link is missing.
- **WAL / continuous archiving:** ship the write-ahead log continuously → point-in-time recovery to any second (RPO ≈ seconds).
```
Sun[FULL 1TB] Mon[+20G] Tue[+18G] Wed[+22G] ...   restore Wed = FULL + Mon + Tue + Wed
```
Worked number: full weekly (1 TB) + daily incrementals (~20 GB) = ~1.14 TB/week stored vs 7 TB for daily fulls — ~6x cheaper, at the cost of a multi-step restore.

**Trade-offs / when NOT to use it:** A backup nobody has *restored* is Schrödinger's backup — simultaneously working and broken until you test it. Incremental chains fail if any segment corrupts. Backups also must be *immutable/offline* or ransomware encrypts them too.

**Where you'll see it:** Postgres `pg_basebackup` + WAL archiving, MySQL binlog, AWS RDS automated snapshots + point-in-time recovery, the tape/Glacier tier from Lesson 13.

### Multi-Region Disaster Recovery
**What it is (plain English):** Running your system in more than one geographic region so an entire-region outage (power, network, natural disaster) doesn't kill the product. It spans four patterns from cheap-and-slow to expensive-and-instant.

**The problem it solves:** A single region is a big SPOF. Whole AWS regions have gone dark (us-east-1 has had multi-hour outages). Multi-region DR converts "region down = company down" into "region down = failover in minutes."

**How it works (mechanics):** The four patterns, ordered by RTO and cost:
```
Pattern         Infra running in DR    Typical RTO      Cost
Backup & Restore  nothing (just data)   hours            $
Pilot Light       core DB replicating   ~10s of minutes  $$
Warm Standby      scaled-down full stack minutes          $$$
Active-Active     full stack, live traffic ~0             $$$$
```
Failover mechanics: data is continuously replicated to the DR region; **health checks** detect the primary is dead; **DNS** (e.g. Route 53) or global load balancer repoints traffic to the DR region; the DR stack scales up. Worked example: warm standby running at 10% capacity replicating with 2 s lag → on failover, RPO ≈ 2 s, RTO ≈ time to scale from 10%→100% + DNS TTL (say 60 s TTL) ≈ a few minutes.

**Trade-offs / when NOT to use it:** Cross-region replication adds latency (Mumbai↔Virginia RTT ≈ 180–220 ms), so synchronous cross-region writes are painful — most go async, accepting RPO > 0. Active-active is costly and re-introduces the master-master conflict problem. DNS TTLs and client caching can stretch RTO beyond your plan.

**Where you'll see it:** Netflix runs active-active across AWS regions; banks run warm standby; most SaaS start at pilot light. Deep-dived again in Phase 2's global systems lessons.

### Failover Drills (Game Days / Chaos Engineering)
**What it is (plain English):** Deliberately breaking things in a controlled way to prove your DR actually works — before a real disaster does it for you. A fire drill for infrastructure. Netflix's **Chaos Monkey** randomly kills production instances on purpose.

**The problem it solves:** DR plans rot. Configs drift, replication silently breaks, the runbook references a person who left. The *only* way to know your RTO/RPO are real is to trigger a failover and measure. GitLab (Lesson 13) discovered all five backup methods were broken — during the actual incident, the worst possible time.

**How it works (mechanics):** A structured "game day":
```
1. Announce blast radius + rollback plan.
2. Inject failure: kill the primary DB / block a region / drop a node.
3. Observe: did health checks fire? did failover trigger automatically?
4. MEASURE actual RTO and RPO with a stopwatch.
5. Compare to targets; file the gaps; fix; repeat.
```
Worked example: target RTO = 5 min. Drill reveals DNS TTL was 300 s and the standby took 4 min to warm → measured RTO = 9 min. You failed the target — but you found out on a Tuesday afternoon, not during a real outage. Lower the TTL to 30 s, keep the standby warmer, re-drill → 3 min.

**Trade-offs / when NOT to use it:** Chaos in production needs guardrails (blast-radius limits, business-hours only, instant rollback) or you cause the outage you feared. Start in staging. But a DR plan never drilled is fiction.

**Where you'll see it:** Netflix's Simian Army, AWS/Google internal "GameDays" and DiRT exercises, financial regulators that *mandate* periodic failover tests.

### How an Architect Guarantees Continuity
**What it is (plain English):** Business Continuity is the whole discipline of keeping the *business* running through disaster — not just the database. It's the architect's synthesis of everything above into a defensible guarantee with numbers, owners, and proof.

**The problem it solves:** Individual tactics (backups, replicas, regions) don't add up to a guarantee on their own. Continuity is the promise, backed by evidence, that the business survives defined disaster scenarios within agreed RTO/RPO.

**How it works (mechanics):** The architect's checklist:
```
1. Classify data/services by criticality → assign RTO/RPO tiers.
2. Map SPOFs → eliminate or accept explicitly (redundancy N+1).
3. Choose topology + DR pattern per tier (from cost budget).
4. Write the runbook: who does what, in what order, with what command.
5. Automate failover + health checks (humans are slow at 3 AM).
6. Drill quarterly; measure; close gaps.
7. Monitor RPO continuously (alert if replication lag > threshold).
```
Worked example tiering: Tier-0 payments (RTO 1 min / RPO 0, active-active + sync), Tier-1 user data (RTO 15 min / RPO 5 min, warm standby), Tier-2 analytics (RTO 24 h / RPO 24 h, daily backup). Each tier gets a proportional budget instead of gold-plating everything.

**Trade-offs / when NOT to use it:** Over-engineering continuity for low-value data burns money; under-engineering it for critical data burns the company. The skill is *tiering* — spending where loss actually hurts.

**Where you'll see it:** Bank DR programs audited by regulators (RBI, SEC), SOC 2 / ISO 22301 certifications, and the "non-functional requirements" section of every serious design doc.

## Comparison Table

| Dimension | Backup & Restore | Pilot Light | Warm Standby | Active-Active |
|---|---|---|---|---|
| RTO | Hours | 10s of minutes | Minutes | ~0 |
| RPO | Last backup | Seconds (replicating DB) | Seconds | ~0 |
| DR infra running | None | Core data only | Scaled-down stack | Full stack live |
| Relative cost | $ | $$ | $$$ | $$$$ |
| Conflict risk | None | None | None | Master-master conflicts |

**Verdict:** Match the pattern to the data tier — pay for active-active only where downtime costs more than the standby bill.

## Common Misconceptions
- **Myth:** RTO and RPO are the same thing. → **Reality:** RPO = data loss (backward); RTO = downtime (forward). You can have great RPO and terrible RTO, or vice versa.
- **Myth:** Replication is disaster recovery. → **Reality:** It doesn't protect against logical corruption or human error — those replicate instantly. You still need backups.
- **Myth:** We have backups, so we're covered. → **Reality:** Not until you've restored one end-to-end and timed it. Untested backups fail exactly when needed.
- **Myth:** Multi-region means zero RPO automatically. → **Reality:** Cross-region replication is usually async (latency), so RPO > 0 unless you pay for sync and its write penalty.
- **Myth:** DR is an ops problem, not an architecture problem. → **Reality:** RTO/RPO tiers shape topology, cost, and consistency choices from day one.

## Real-World Case
On 20 February 2017, an Amazon S3 engineer running a routine debugging playbook in the us-east-1 region mistyped a command, taking far more capacity offline than intended. S3's index and placement subsystems required a full restart — something that hadn't been done in years — and the restart took ~4 hours. Because a huge slice of the internet stored assets and even *status dashboards* in us-east-1 (including AWS's own health dashboard, which couldn't update), sites across the web broke: Slack, Trello, Quora, IoT devices. The lesson companies took: don't put all your eggs — or your status page — in one region, and design human-error blast radii into your tooling. Firms with multi-region assets rode it out; single-region ones simply went dark.

## Self-Test (answers at the bottom)
1. In one sentence each, define RTO and RPO and say which direction on the timeline each points.
2. You take full backups weekly and incrementals daily. To restore Thursday's state, exactly which files do you need and in what order?
3. Your business requires losing no more than 30 seconds of data across a full region failure. Which DR pattern(s) can meet this and which cannot? Why?
4. A failover drill shows measured RTO of 9 minutes against a 5-minute target, with 5 of those minutes spent waiting on DNS. Name two concrete fixes.
5. Design sketch: Tier a food-delivery platform's data (live orders, user profiles, historical analytics) into RTO/RPO classes and assign a DR pattern and rough monthly-cost tier to each. Justify why you don't give analytics the same guarantee as live orders.

## Interview Soundbites
- "RPO is how much data I can lose; RTO is how long I can be down — I put a number on both before I choose a topology, because those numbers *are* the architecture."
- "Replication is not backup: a bad `DELETE` replicates in milliseconds, so I keep 3-2-1 backups with at least one immutable offsite copy."
- "An untested DR plan is fiction — I run quarterly game days and measure actual RTO/RPO with a stopwatch, because GitLab and everyone else learns the hard way that backups silently rot."

## Mini-Assignment
On paper (~30 min): For a system with a 2 TB primary database: (1) Design a backup schedule meeting RPO = 5 minutes and compute rough monthly storage for full-weekly + WAL archiving (assume 40 GB/day of WAL). (2) Write a 6-step failover runbook for a region outage, and for each step estimate its time contribution, then sum to a predicted RTO. (3) Identify the single biggest contributor to that RTO and propose one change to halve it.

## Recap & Tomorrow
- **RTO/RPO:** downtime tolerance vs data-loss tolerance — the two numbers that drive every DR decision.
- **Backup strategies:** 3-2-1; full vs incremental vs WAL trade storage for restore speed; test your restores.
- **Multi-region DR:** four patterns from backup-restore to active-active, priced by RTO.
- **Failover drills:** chaos/game days prove your numbers are real, on your schedule not the disaster's.
- **Guaranteeing continuity:** tier data by criticality and spend proportionally; automate and drill.

Tomorrow, **Lesson 15 — Cluster & Big Data Architectures**: how many commodity machines cooperate as one — GFS, Hadoop, MapReduce, and Spark — the substrate under every analytics platform.

## Self-Test Answers
1. RTO is the maximum acceptable downtime — how long until service is restored (points *forward* from the disaster). RPO is the maximum acceptable data loss measured in time — how far *back* to the last safe copy.
2. Last Sunday's full backup first, then Monday, Tuesday, Wednesday, and Thursday's incrementals applied in chronological order. Missing or corrupt any one increment breaks the chain.
3. Warm standby and active-active (continuous/near-continuous replication, RPO in seconds) can meet 30 s; active-active best. Backup-and-restore (RPO = last backup, often hours) and pilot light with infrequent snapshots cannot, because their recovery point is too far back.
4. Lower the DNS TTL (e.g. from 300 s to 30 s) so clients repoint faster, and/or use a health-check-driven global load balancer instead of DNS failover; also pre-warm the standby so scale-up isn't on the critical path.
5. Live orders → RTO ~1 min / RPO ~seconds, active-active or warm standby with sync-ish replication ($$$–$$$$) because lost/duplicated orders cost money and trust. User profiles → RTO ~15 min / RPO ~5 min, warm standby ($$$). Historical analytics → RTO ~24 h / RPO ~24 h, daily backups ($). Analytics gets a weaker guarantee because a day-old rebuild causes no customer-facing harm, so paying for active-active there is wasted money.
