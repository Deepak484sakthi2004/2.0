# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 27 of 63 — Phase 1: Foundations (Module 27 of 28)
**Module:** The Architecture Approach Framework
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Knowing every component (Modules 1–26) is worthless if you freeze when handed a blank whiteboard and "Design Twitter." The single biggest reason strong engineers fail system-design interviews isn't lack of knowledge — it's lack of *structure*: they jump straight to databases, forget to ask about scale, and ramble. This lesson gives you a repeatable eight-step framework you'll run on *every* Phase 2 design (Lessons 29–63) and in every real design review. Master it and you'll always know your next sentence. It is the scaffolding that turns scattered knowledge into a coherent, senior-sounding design.

## Learning Objectives
By the end of this lesson you can:
- Separate functional requirements (what it does) from non-functional (how well) and drive the design from both.
- Run an eight-step framework — requirements → estimation → API → data → HLD → deep-dive → scale → operate — in order, on the clock.
- Budget a 45-minute interview across the steps so you never run out of time on scale.
- Ask the three or four clarifying questions that reframe an ambiguous prompt.
- Communicate a design to an interviewer or review board so they can follow your reasoning, not just your diagram.

## The Lesson

### Capturing Functional & Non-Functional Requirements
**What it is (plain English):** Functional requirements (FRs) are the features — the verbs the system performs ("post a tweet," "shorten a URL," "book a seat"). Non-functional requirements (NFRs) are the qualities — scale, latency, availability, consistency, durability. FRs tell you *what to build*; NFRs tell you *what shape it must take*.
**The problem it solves:** Skip FRs and you design the wrong system. Skip NFRs and you design a toy — a URL shortener for 100 users and one for 100M are architecturally different systems. NFRs are what force caching, sharding, and replication into the design.
**How it works (mechanics):** Spend the first ~5 minutes here. Ask, then write two lists:
```
FUNCTIONAL (features, in scope):        NON-FUNCTIONAL (qualities, with numbers):
- create short URL                      - 100M writes/day, 10B reads/day (100:1 read-heavy)
- redirect short→long                   - redirect p99 < 100 ms
- custom alias (optional)               - 99.99% availability (~52 min/yr down)
OUT OF SCOPE: analytics, auth           - links immutable, ~5-yr retention, eventual-consistent OK
```
The NFR numbers directly drive Step 2. "Read-heavy 100:1" → cache + read replicas. "p99 < 100 ms" → in-memory lookup. "99.99%" → multi-AZ, no SPOF.
**Trade-offs / when NOT to use it:** Don't over-elaborate — 3–5 FRs and 4–5 NFRs is plenty; listing 20 features burns your clock. Guessing numbers silently is the failure mode; state your assumptions aloud so the interviewer can correct them.
**Where you'll see it:** Every Phase 2 lesson opens exactly this way; every real PRD and design doc has these two sections.

### Estimation (Back-of-Envelope)
**What it is (plain English):** Rough capacity math done in your head/on the board to size the system — QPS, storage, bandwidth, cache. It converts the NFR numbers into infrastructure reality *before* you draw boxes. (Lesson 28 drills the arithmetic; here it's the framework step.)
**The problem it solves:** Without it you can't justify "this needs sharding" or "this fits on one box." Estimation is what separates "I'd use a database" from "at 12K writes/sec and 36 TB/year, I need to shard across ~10 nodes."
**How it works (mechanics):** Convert daily totals to per-second and multiply by per-item cost:
```
Writes: 100M/day ÷ 86,400 s ≈ 1,160 writes/sec (call it ~1.2K)
Reads : 10B/day  ÷ 86,400 s ≈ 115,700 reads/sec (~116K, peak ~2× = 230K)
Storage/yr: 100M/day × 365 × 500 B ≈ 18.25 TB/yr
Cache: hot 20% of daily reads → 20M URLs × 500 B ≈ 10 GB (fits in RAM)
```
Round aggressively (86,400 ≈ 100,000; use powers of 10). The output feeds every later step: 230K read QPS → cache + many read replicas; 18 TB/yr → sharding plan.
**Trade-offs / when NOT to use it:** Precision is a trap — being off by 2× is fine; being off by 1000× (forgetting read:write ratio) is fatal. Don't spend 15 minutes here; ~5 minutes and move on with the key numbers written down.
**Where you'll see it:** Capacity-planning docs, on-call runbooks, and every interview scorecard's "quantitative reasoning" line.

### API Design
**What it is (plain English):** Define the contract — the endpoints/methods clients call, their inputs, and outputs — before internals. It's the interface that pins down exactly what the system does, turning fuzzy FRs into concrete operations.
**The problem it solves:** The API forces clarity: it exposes hidden requirements (pagination? idempotency? auth?) and gives you a spine to hang the data model and components on. Design internals first and you often build things no client actually needs.
**How it works (mechanics):** One line per functional requirement, with request/response shape:
```
POST /urls            {longUrl, customAlias?}     → 201 {shortUrl}
GET  /{shortCode}                                 → 302 Location: longUrl
Idempotency: POST includes a client key so retries don't create duplicates.
Auth: API key in header; rate-limit 100 req/min/key.
```
Choose the style deliberately: REST for resources, gRPC for internal low-latency service-to-service, GraphQL when clients need flexible field selection. Note status codes (302 vs 301 matters for a shortener — 301 is cached forever by browsers, killing your analytics).
**Trade-offs / when NOT to use it:** Don't design 25 endpoints — cover the core FRs. Over-detailing request schemas (every field type) wastes time; sketch enough to be unambiguous.
**Where you'll see it:** Every microservice starts as an API spec (OpenAPI/Protobuf); interviewers explicitly look for a clean contract.

### Data Model & Storage
**What it is (plain English):** Decide what you store, in what schema, and in which kind of store (SQL, NoSQL, blob, cache). This is where consistency, access patterns, and scale collide into a concrete choice.
**The problem it solves:** The data model determines what queries are cheap and what's impossible. Pick wrong and no amount of caching saves you — you can't retrofit a good access pattern onto a bad schema at 100M rows/day.
**How it works (mechanics):** Derive the schema from the API's access patterns, then pick the store:
```
Table urls: shortCode (PK) | longUrl | createdAt | userId
Access pattern: point lookup by shortCode (99% of traffic) → key-value shines
Store choice: shortCode→longUrl is a pure KV lookup → DynamoDB/Cassandra,
              partition/shard by shortCode hash (even distribution).
Reads 230K QPS → front with Redis; write path SQL only if you need transactions.
```
Ask: point lookups or ranges? Strong or eventual consistency? Read:write ratio? Those answers pick SQL vs NoSQL vs KV and the shard key. State the shard key explicitly — it's the single most important scaling decision.
**Trade-offs / when NOT to use it:** Defaulting to "Postgres for everything" or "NoSQL because scale" without matching access patterns is the classic mistake. A wrong shard key causes hotspots that no scaling fixes.
**Where you'll see it:** Every design doc's "Data Model" section; the shard-key debate is a real recurring production conversation.

### High-Level Design (HLD)
**What it is (plain English):** The box-and-arrow diagram: the major components and how a request flows through them. This is the moment you draw the system — client, load balancer, services, caches, databases, queues — and trace the happy path.
**The problem it solves:** It gives everyone (you, interviewer, review board) a shared mental model. Without it, deep-dives have no context; with it, you can point and say "here's where we scale."
**How it works (mechanics):** Draw the request path end to end:
```
[Client] → [DNS/CDN] → [Load Balancer L7] → [API Gateway: auth, rate-limit]
   → [URL Service (stateless, N replicas)] → [Redis cache] --miss--> [KV Store (sharded)]
   Write path: → [ID/keygen] → [KV Store] → async → [Analytics queue → warehouse]
```
Walk one read and one write aloud through the boxes. Keep it to ~6–10 components — enough to be complete, not so many it's noise. Every box should trace back to an FR or NFR.
**Trade-offs / when NOT to use it:** Don't prematurely add Kafka, Spark, and five caches "because real systems have them" — justify each box by a requirement. An over-decorated diagram signals cargo-culting.
**Where you'll see it:** The centerpiece of every architecture review deck and interview whiteboard.

### Deep-Dive
**What it is (plain English):** Zoom into the one or two components that are hard or interesting and design them in detail. This is where senior signal lives — anyone can draw boxes; showing you understand *how the tricky box actually works* is what earns the SDE3 grade.
**The problem it solves:** The HLD hides the hard parts. The deep-dive proves you can handle the real engineering: the key-generation scheme, the cache-invalidation strategy, the consistency mechanism, the hotspot mitigation.
**How it works (mechanics):** Pick the crux and drill:
```
Deep-dive: unique short-code generation at 1.2K writes/sec, no collisions.
Option A: hash(longUrl) → base62, take 7 chars → 62^7 ≈ 3.5×10^12 space
          but collisions need a check-and-retry (extra read).
Option B: a distributed counter (Redis INCR / range-allocated blocks per node)
          → base62 encode the integer → guaranteed unique, no collision check.
Chosen: B with per-node pre-allocated ID ranges (e.g., 10K IDs/block) → no
        per-request coordination, survives node restarts.
```
Name the algorithm, the number, the failure it avoids. Let the interviewer steer which component to open.
**Trade-offs / when NOT to use it:** Don't deep-dive everything — you'll run out of time. One or two well-chosen dives beat five shallow ones. Diving into a trivial component wastes your senior signal.
**Where you'll see it:** The "detailed design" section of an RFC; the part of the interview where the grade is decided.

### Scale
**What it is (plain English):** Address what breaks as load grows 10×–100×: bottlenecks, single points of failure, hotspots, and the fixes — sharding, replication, caching layers, async processing, CDNs. It's stress-testing your own design out loud.
**The problem it solves:** A design that works at 1K QPS may collapse at 230K. Explicitly walking the bottlenecks shows you can operate at scale, not just draw it.
**How it works (mechanics):** Walk each tier and name the bottleneck + fix with a number:
```
Tier          Bottleneck at 230K QPS         Fix
DB reads      single node ~10K QPS          → read replicas + Redis (95% hit → ~11K to DB)
DB writes     hot shard on popular key      → better shard key / consistent hashing
Stateless svc CPU-bound                     → horizontal autoscale behind LB
Redirects     global latency               → CDN / edge caching of hot codes
SPOF          one LB, one cache node        → multi-AZ, replicated cache, no SPOF
```
Do the cache-hit math: 230K reads × 95% hit → only ~11.5K reach the DB, which replicas can serve. That number *is* your scaling argument.
**Trade-offs / when NOT to use it:** Don't add scaling machinery the requirements don't demand — over-engineering for 100M users when the NFR says 100K is a red flag too. Scale to the stated numbers, not to impress.
**Where you'll see it:** Every capacity review; the interviewer's "what happens at 10×?" follow-up.

### Operate
**What it is (plain English):** How you run the system in production: monitoring, alerting, logging, deployment, failure handling, and trade-offs you're consciously accepting. It signals you've actually operated systems, not just designed them on paper.
**The problem it solves:** A design nobody can observe or safely deploy is a liability. This step covers the "day 2" concerns that separate a real architect from a whiteboard artist.
**How it works (mechanics):** Name the golden signals and the failure story:
```
Monitor: the 4 golden signals — latency (p50/p99), traffic (QPS), errors (%), saturation (CPU/mem)
Alert:   page if redirect p99 > 100 ms or error rate > 1% for 5 min
Deploy:  canary 1% → 10% → 100%, auto-rollback on error spike
Failure: cache down → serve from DB (degraded, slower) not outage; DB shard down
         → that key range fails, rest serves (blast radius contained)
Trade-off stated: we chose eventual consistency for analytics → counts lag ~seconds (acceptable)
```
End by naming the trade-offs you deliberately accepted — that honesty is a strong senior signal.
**Trade-offs / when NOT to use it:** In a tight 45-minute interview this is often 2–3 minutes at the end; don't let it crowd out the deep-dive. But never skip it entirely — a design with no observability story is incomplete.
**Where you'll see it:** SRE runbooks, on-call rotations, the "operational readiness review" before any launch.

### How to Communicate a Design Clearly
**What it is (plain English):** The meta-skill: narrating your design so a listener follows your reasoning in real time. A brilliant design explained as a chaotic stream of consciousness scores worse than a good design explained cleanly. You're being graded on communication as much as content.
**The problem it solves:** Interviewers and review boards can't read your mind. If they can't follow *why* you chose each thing, they assume you got lucky. Structure makes your competence legible.
**How it works (mechanics):** A few concrete techniques:
```
1. Signpost: "I'll go requirements → estimation → API → data → HLD → deep-dive → scale."
2. Think aloud + state assumptions: "I'll assume 100:1 read:write — stop me if that's wrong."
3. Drive the diagram: point as you talk; trace one request end to end.
4. Time-box: ~45 min → 5 req, 5 est, 5 API/data, 10 HLD, 15 deep-dive+scale, 5 operate.
5. Name trade-offs proactively: "I chose eventual consistency here; the cost is stale counts."
6. Invite steering: "Want me to go deeper on keygen or on the cache?"
```
The interviewer is a collaborator, not an examiner — pull them in.
**Trade-offs / when NOT to use it:** Over-signposting ("now I will do step 4b") is robotic; keep it natural. Silence is worse than imperfect narration — never think for 60 seconds without talking.
**Where you'll see it:** Every interview, every architecture review board, every design doc walkthrough with a staff engineer.

## Comparison Table

| Step | Time (of ~45 min) | Question it answers | Output |
|---|---|---|---|
| 1. Requirements | ~5 min | What & how well? | FR list + NFR numbers |
| 2. Estimation | ~5 min | How big? | QPS, storage, cache, bandwidth |
| 3. API | ~3 min | What's the contract? | Endpoints + I/O |
| 4. Data model | ~4 min | What & where stored? | Schema + store + shard key |
| 5. HLD | ~8 min | How does a request flow? | Box-and-arrow diagram |
| 6. Deep-dive | ~10 min | How does the hard part work? | Detailed component design |
| 7. Scale | ~7 min | What breaks at 10–100×? | Bottlenecks + fixes |
| 8. Operate | ~3 min | How do we run it? | Monitoring, deploy, failure story |

**Verdict:** Run them in order — each step's output is the next step's input. Requirements without numbers make estimation impossible; skipping estimation makes scaling arguments hand-wavy. The framework is a pipeline, not a menu.

## Common Misconceptions
- **Myth:** Start with the database — that's the "real" design. → **Reality:** Start with requirements and estimation; the database falls out of the access patterns and numbers, not the other way round.
- **Myth:** More components = more impressive. → **Reality:** Every unjustified box (Kafka, Spark, five caches) is a red flag; each component must trace to an FR or NFR.
- **Myth:** Non-functional requirements are optional polish. → **Reality:** NFR numbers (QPS, latency, availability) are what *force* the interesting architecture — they're the whole game.
- **Myth:** The best design silently speaks for itself. → **Reality:** You're graded on communication; an unexplained good design scores below a clearly narrated decent one.
- **Myth:** You must finish all eight steps perfectly. → **Reality:** A well-time-boxed design that nails requirements, HLD, and one deep-dive beats a rushed sprint through all eight.

## Real-World Case
A senior candidate I once interviewed was handed "design a ride-hailing dispatch." He immediately started drawing microservices and databases — no requirements, no numbers. Twenty minutes in, he realized he'd designed for strong global consistency on driver locations, which at millions of GPS updates per second is absurdly expensive and unnecessary (a location 2 seconds stale is fine). He'd skipped Steps 1–2, so his entire data layer was wrong, and he had no time to fix it. A weaker-on-paper candidate who spent five minutes on requirements ("location updates are high-volume, eventual consistency is fine; matching needs a geospatial index; ~1M concurrent drivers") built a cleaner design and scored higher. The lesson: the framework isn't bureaucracy — those first ten minutes on requirements and estimation are what make every later decision correct.

## Self-Test (answers at the bottom)
1. Give one functional and one non-functional requirement for a chat app, and say which one forces sharding.
2. Why must estimation come before high-level design, not after?
3. You have 15 minutes left after the HLD. Do you add three more features or deep-dive the ID generator? Justify with the grading logic.
4. An interviewer says "assume 500M daily active users." Which two later steps does that number most change, and how?
5. Design sketch: Apply the eight-step framework, in order, to "design a pastebin." Write one line per step with at least one real number in Steps 1, 2, and 7.

## Interview Soundbites
- "I'll drive this requirements → estimation → API → data → HLD → deep-dive → scale → operate, and I'll state my assumptions out loud so you can correct me early."
- "The non-functional numbers are the whole game — 100:1 read-heavy at 230K QPS is what forces the cache and read replicas, not personal preference."
- "I'd rather nail requirements, the HLD, and one deep-dive cleanly than sprint through eight shallow steps — depth on the hard part is where the real signal is."

## Mini-Assignment
Pick any well-known product (WhatsApp, Google Drive, Uber). On paper with a timer (~30 min), run all eight steps in order, out loud as if to an interviewer, and hold yourself to the time budget in the comparison table. Force yourself to: (1) write at least three NFRs *with numbers*, (2) do the QPS and storage math, (3) draw a 6–10 box HLD, (4) pick exactly one component to deep-dive, and (5) name one trade-off you consciously accepted. Record yourself; play it back and mark every place you rambled or skipped a step.

## Recap & Tomorrow
- **Functional vs non-functional:** features vs qualities-with-numbers; NFRs force the architecture.
- **Estimation:** convert NFR numbers to QPS/storage/cache before drawing boxes.
- **API:** the contract that pins down what the system does, one line per FR.
- **Data model:** schema + store + shard key derived from access patterns.
- **HLD:** 6–10 justified boxes; trace one read and one write.
- **Deep-dive:** one or two hard components in real detail — where senior signal lives.
- **Scale:** name each tier's bottleneck and fix with a number.
- **Operate:** golden signals, deploy strategy, failure story, stated trade-offs.
- **Communicate:** signpost, think aloud, time-box, invite steering.

Tomorrow, **Lesson 28 — Capacity Planning & Estimation**: we drill Step 2 to the bone — back-of-envelope QPS→servers math, RAM/SSD/disk sizing, cache and bandwidth calculations, and how to actually answer "do we need 200 servers or 300?"

## Self-Test Answers
1. Functional: "send/receive messages in a 1:1 chat." Non-functional: "support 500M DAU sending 40 messages/day → ~230K messages/sec at peak." The non-functional scale number forces sharding — a single database can't hold or serve that volume, so you shard messages (e.g., by conversation ID).
2. Estimation produces the numbers (QPS, storage, cache size) that determine the *shape* of the HLD — whether you need a cache, read replicas, sharding, or a CDN. Draw the HLD first and you're guessing at components; the estimation tells you which boxes are actually required and why.
3. Deep-dive the ID generator. Interviews grade on depth and senior signal, not feature count — three more shallow features add nothing an SDE1 couldn't list, whereas showing you can design collision-free ID generation at scale (counter blocks vs hashing, with numbers) is exactly the SDE2/SDE3 differentiator the scorecard rewards.
4. It most changes (a) Estimation — 500M DAU drives your QPS, storage/year, and cache-size math, turning "fits on one box" into "must shard across N nodes," and (b) Scale — it dictates the bottlenecks and fixes (read replicas, sharding strategy, CDN, no SPOF across AZs). Downstream, it also reshapes the data model's shard key.
5. Requirements: FR create/read a paste; NFR ~10M writes/day (~116/sec), reads ~100M/day (~1.2K/sec), p99 read < 200 ms, eventual OK. Estimation: 10M/day × 10 KB × 365 ≈ 36.5 TB/yr; cache hot 10% ≈ 1M pastes. API: POST /pastes {content,ttl} → {id}; GET /{id} → content. Data: id (PK) → content in blob/KV store (S3 for content, KV for metadata), shard by id hash. HLD: Client → CDN → LB → Paste service → Redis → KV/S3. Deep-dive: unique id via base62 counter blocks, no collision check. Scale: reads 1.2K QPS × 90% cache hit → ~120 QPS to store; shard writes by id to avoid hotspots; CDN for hot pastes. Operate: golden signals, canary deploys, cache-down → serve from store degraded.
