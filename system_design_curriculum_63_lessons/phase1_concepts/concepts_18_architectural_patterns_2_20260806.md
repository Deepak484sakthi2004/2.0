# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 18 of 63 — Phase 1: Foundations (Module 18 of 28)
**Module:** Architectural Patterns II
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Yesterday you learned how boxes talk. Today you learn *where those boxes live* and *who owns them*. The single biggest architecture decision most teams make is how much of the stack to rent versus run — IaaS vs PaaS vs SaaS — and it dictates your cost, speed, and control for years. Then you learn how work gets pushed to the *edge*: CDNs that shave 200ms off every page load, edge devices that decide locally instead of round-tripping to the cloud, and hybrid designs that straddle both. Netflix streams ~15% of global internet traffic almost entirely from CDN edges; understanding this pattern is non-negotiable for Phase 2 designs.

## Learning Objectives
By the end of this lesson you can:
- Place any product on the IaaS/PaaS/SaaS spectrum by asking who manages which layer.
- Explain agent-based architecture and when autonomous local decisions beat central control.
- Justify a hybrid (cloud + on-prem) design with a concrete data-gravity or compliance reason.
- Describe web-based (browser-thin-client) architecture and its trade-offs vs native.
- Explain edge-device architecture and compute a latency saving from local processing.
- Explain how a CDN works, including cache hit ratio math and a real latency number.

## The Lesson

### SaaS vs PaaS vs IaaS
**What it is (plain English):** Three levels of "rent instead of own" for computing. **IaaS** (Infrastructure) rents you raw virtual machines and networks — you install everything above. **PaaS** (Platform) rents you a place to deploy code — the OS, runtime, and scaling are handled. **SaaS** (Software) rents you a finished application — you just log in. Analogy: IaaS is renting land, PaaS is renting a fully-serviced kitchen, SaaS is ordering the meal.

**The problem it solves:** Undifferentiated heavy lifting. Racking servers, patching OSes, and babysitting runtimes add zero business value. Each tier hands more of that toil to the provider so you focus higher up the stack.

**How it works (mechanics):** The dividing line is *who manages each layer*.
```
Layer            On-Prem  IaaS   PaaS   SaaS
Application       you     you    you    PROVIDER
Runtime/Data      you     you    PROV   PROV
OS                you     you    PROV   PROV
Virtualization    you     PROV   PROV   PROV
Servers/Network   you     PROV   PROV   PROV
```
More provider-managed = less control but faster time-to-market.

**Trade-offs / when NOT to use it:** SaaS is fastest but you're locked into their features and pricing; IaaS gives full control but you own patching, scaling, and 2am pages. PaaS can get expensive and opaque at scale (you can't tune what you can't see). Heavy, latency-critical, or compliance-bound workloads sometimes belong on IaaS or on-prem.

**Where you'll see it:** IaaS: AWS EC2, GCP Compute Engine. PaaS: Heroku, AWS Elastic Beanstalk, Google App Engine. SaaS: Gmail, Salesforce, Slack. A startup can go from zero to a live app in a day on PaaS; the same on bare IaaS takes a week.

### Agent-Based Architecture
**What it is (plain English):** Instead of one central brain issuing every command, you deploy many autonomous **agents** — small programs that sense their local environment, make decisions, and act, coordinating loosely with each other or a controller. Think of a swarm rather than a puppeteer.

**The problem it solves:** Central control doesn't scale and doesn't survive network cuts. If every decision must round-trip to a central server, that server is a bottleneck and a single point of failure. Agents keep working locally even when disconnected.

**How it works (mechanics):**
```
[ Control plane ]  <- pushes policy, collects telemetry
    |    |    |
 [agent][agent][agent]  <- each runs on a host, decides locally
   host   host   host
```
A monitoring agent (Datadog, Prometheus node_exporter) lives on each host, samples CPU/memory every ~10–15s, buffers locally, and ships summaries upstream — so a central outage doesn't blind or halt the host. In multi-agent AI systems, each agent owns a sub-task and negotiates results.

**Trade-offs / when NOT to use it:** Distributed autonomy is hard to reason about — emergent behavior, version drift across thousands of agents, and eventual (not immediate) consistency of the global view. For a small system where a central controller easily keeps up, agents are needless complexity.

**Where you'll see it:** Observability agents (Datadog, Prometheus, Fluentd), configuration agents (Chef/Puppet/Salt), Kubernetes' kubelet on each node, and modern multi-agent LLM orchestration.

### Hybrid Architecture
**What it is (plain English):** A deliberate split where part of the system runs in the public cloud and part runs on-premises (or in a private cloud), connected over a secure link. You keep some workloads home and burst the rest to the cloud.

**The problem it solves:** "All-in cloud" isn't always legal, cheap, or fast. Regulated data may be required to stay on-prem; a 50-year-old mainframe can't be lifted overnight; and moving petabytes to the cloud is slow and expensive ("data gravity"). Hybrid lets you modernize incrementally.

**How it works (mechanics):**
```
[ On-Prem DC ]======VPN / Direct Connect======[ Public Cloud ]
  sensitive DB, mainframe          elastic web tier, analytics
     (data stays home)            (scales up for traffic spikes)
```
A common pattern is *cloud bursting*: run baseline load on-prem, and when demand exceeds capacity, spin up extra cloud instances. AWS Direct Connect gives a dedicated ~1–10 Gbps private link with more stable latency than public internet.

**Trade-offs / when NOT to use it:** You now operate two environments, two security models, and a fragile link between them — more operational surface, not less. Latency across the link (often 5–50ms) can hurt chatty workloads. If you have no compliance, legacy, or data-gravity constraint, a single cloud is simpler.

**Where you'll see it:** Banks and hospitals (data residency), retailers bursting for Black Friday, AWS Outposts / Azure Arc / Google Anthos which extend cloud control planes on-prem.

### Web-Based Architecture
**What it is (plain English):** The application lives on servers and is delivered to a *thin client* — the browser — over HTTP. The user installs nothing; the browser downloads HTML/CSS/JS on demand and renders the UI. Updates ship instantly to everyone.

**The problem it solves:** Distribution and update pain of native apps. Without the web model, every feature means shipping a new binary to every OS and waiting for users to update. Web apps update the moment you refresh.

**How it works (mechanics):**
```
Browser --HTTP GET--> Web server --> App tier --> DB
   ^                                          |
   |<---- HTML + JS bundle (~200KB–2MB) ------|
   then AJAX/fetch calls for JSON as you click
```
Two flavors: **server-side rendering** (server returns finished HTML — fast first paint) and **single-page apps** (browser downloads a JS bundle once, then fetches JSON — snappy after load). Time-to-interactive is dominated by bundle size and round trips; a 1MB JS bundle on a 4G link (~5 Mbps) adds ~1.6s just to download.

**Trade-offs / when NOT to use it:** The browser sandbox limits hardware access and offline capability; heavy computation or true native performance (high-end games, pro video editing) still favors native. Large JS bundles hurt low-end devices.

**Where you'll see it:** Gmail, Google Docs, Figma, Notion, virtually all SaaS dashboards. Progressive Web Apps (PWAs) blur the line by caching for offline use.

### Edge-Device Architecture
**What it is (plain English):** Push computation *out to where the data is generated* — sensors, cameras, phones, IoT gateways — instead of sending everything to a central cloud. The device decides locally and only forwards what matters.

**The problem it solves:** Latency, bandwidth, and privacy. Round-tripping every camera frame to the cloud for inference is slow (~50–200ms each way) and floods the network. Local processing responds in milliseconds and sends only results.

**How it works (mechanics):** Consider a factory camera doing defect detection at 30 frames/sec.
```
CLOUD-ONLY:  frame -> internet (100ms) -> cloud inference -> back (100ms) = ~200ms/decision
EDGE:        frame -> on-device model (~15ms) -> decision, send only alerts upstream
```
Bandwidth math: streaming raw 1080p video ~5 Mbps continuously vs sending only a 2KB alert when a defect appears — often a **1000x** reduction in uplink traffic. The edge tier runs a smaller model; the cloud does the heavy training and aggregation.

**Trade-offs / when NOT to use it:** Edge devices are constrained (CPU, memory, power) and hard to update/secure at fleet scale — you may run thousands of them in the field. Models must be shrunk (quantized), losing some accuracy. If latency isn't critical and data volume is small, centralize instead.

**Where you'll see it:** Tesla's on-car inference, Apple's on-device Face ID, AWS IoT Greengrass, Cloudflare Workers (edge compute), industrial predictive-maintenance sensors.

### CDN (Content Delivery Architecture)
**What it is (plain English):** A **Content Delivery Network** is a globally distributed fleet of cache servers (edge PoPs — points of presence) that store copies of your content close to users. When someone in Mumbai requests an image, they get it from a Mumbai edge, not your origin server in Virginia.

**The problem it solves:** The speed of light. A round trip Mumbai↔Virginia is ~250ms minimum on physics alone; multiply by the dozens of assets on a page and it's crippling. CDNs cut that to a local ~10–30ms and offload traffic from your origin.

**How it works (mechanics):** On a cache miss, the edge fetches from origin, stores it (respecting `Cache-Control`/TTL), and serves subsequent requests locally.
```
User(Mumbai) -> Edge PoP (Mumbai)
                  |-- HIT  -> serve in ~15ms, origin untouched
                  |-- MISS -> fetch from origin(Virginia ~250ms), cache, serve
```
**Hit-ratio math:** with a 95% hit ratio and 1,000,000 requests, only 50,000 reach origin — a **20x** load reduction. Effective average latency ≈ 0.95×15ms + 0.05×250ms ≈ **26.75ms** versus 250ms without a CDN. That's roughly a 9x speedup.

**Trade-offs / when NOT to use it:** Caching stale content is the classic pitfall — you need careful TTLs and cache invalidation ("one of the two hard problems in CS"). CDNs cache *static/cacheable* content well; highly personalized, per-user dynamic responses often can't be cached (though edge compute is closing that gap). CDNs add cost and a layer to debug.

**Where you'll see it:** Cloudflare, Akamai, AWS CloudFront, Fastly. Netflix runs its own CDN (Open Connect) with appliances inside ISP networks; YouTube, Facebook, and every large site front static assets with a CDN.

## Comparison Table

| Dimension | IaaS | PaaS | SaaS |
|---|---|---|---|
| You manage | OS, runtime, app, data | App + data only | Nothing (just use it) |
| Provider manages | Hardware, virtualization | + OS, runtime, scaling | Entire stack |
| Control | Highest | Medium | Lowest |
| Time-to-market | Slowest | Fast | Instant |
| Example | AWS EC2 | Heroku, App Engine | Gmail, Salesforce |

**Verdict:** Move up the stack (toward SaaS) for speed and less toil; move down (toward IaaS) when you need control, tuning, or compliance.

## Common Misconceptions
- **Myth:** The cloud is always cheaper than on-prem. → **Reality:** At steady, predictable, high utilization, owned hardware can be far cheaper — cloud wins on elasticity and speed, not always raw cost.
- **Myth:** A CDN speeds up everything. → **Reality:** It accelerates cacheable content; uncacheable per-user dynamic responses see little benefit without edge compute.
- **Myth:** Edge computing replaces the cloud. → **Reality:** They're complementary — edge does fast local inference, cloud does training and aggregation.
- **Myth:** PaaS means you never think about scaling. → **Reality:** You still design for it; PaaS automates *mechanism*, not your bad data model.
- **Myth:** Hybrid is just "some servers, some cloud." → **Reality:** The hard part is the secure, low-latency link and a unified security/identity model across both.

## Real-World Case
When Netflix moved to streaming, sending every bit from central AWS regions would have saturated ISP links and buckled under peak-hour demand (Netflix can be ~15% of global downstream internet traffic). Their answer was **Open Connect**: purpose-built CDN appliances placed *inside* ISP data centers, pre-loaded overnight with the shows likely to be watched that evening based on prediction models. At peak, your stream comes from a box a few milliseconds away inside your own ISP, not from Virginia. The result: dramatically lower latency, near-zero rebuffering, and massively reduced backbone traffic — a textbook edge/CDN win that also saved ISPs and Netflix money. The architectural lesson: move the *data* to the user, and predict demand so the cache is warm before the request arrives.

## Self-Test (answers at the bottom)
1. Name the layer that differs between IaaS and PaaS in terms of who manages it.
2. Give one concrete reason a bank would choose a hybrid architecture over pure cloud.
3. A page pulls 40 assets; a round trip to origin is 250ms and to a local CDN edge is 20ms. Roughly how much time does the CDN save if assets load sequentially? (illustrative)
4. A factory camera runs at 30 fps. Why does edge inference beat cloud-only, and estimate the per-decision latency difference.
5. Design sketch: You're launching a global photo-sharing app. Which service model do you start on, where do you put images for fast global delivery, and what hit ratio would you target — show the origin-load reduction math.

## Interview Soundbites
- "The IaaS/PaaS/SaaS choice is really 'how much undifferentiated heavy lifting do I want to rent away' — move up the stack for speed, down for control and compliance."
- "A CDN turns a physics problem into a local one: a 95% hit ratio cuts origin load 20x and average latency from ~250ms to ~27ms."
- "Edge computing is about latency, bandwidth, and privacy — decide locally in ~15ms and send only the 2KB result, not the 5 Mbps raw stream."

## Mini-Assignment
On paper (~30 min): Design the delivery layer for a news site with readers on five continents. (1) Choose a service model and justify it in two sentences. (2) Draw the CDN topology: origin + at least three edge PoPs, and label which content is cacheable (article HTML, images) vs not (logged-in user's personalized feed). (3) Compute effective average latency for a 90% and a 99% hit ratio, given origin=220ms and edge=25ms, and state how many of 2,000,000 daily requests reach origin in each case. (4) Note one cache-invalidation strategy for breaking-news updates.

## Recap & Tomorrow
- **IaaS/PaaS/SaaS:** rent more of the stack as you go up — trade control for speed; decided by "who manages which layer."
- **Agent-based:** autonomous local agents beat a central brain for scale and partition-tolerance; harder to reason about.
- **Hybrid:** cloud + on-prem for compliance, legacy, and data gravity — at the cost of two environments and a fragile link.
- **Web-based:** thin browser client, instant updates, sandbox limits; SSR vs SPA trade first-paint for post-load snappiness.
- **Edge-device:** compute where data is born — ~15ms local decisions, 1000x less uplink, constrained hardware.
- **CDN:** cache near users; 95% hit ratio → 20x less origin load and ~9x faster; watch cache invalidation.

Tomorrow, **Lesson 19 — Consistent Hashing in Detail**: the hash ring, virtual nodes, and the exact rebalancing math that lets distributed caches and load balancers add or remove nodes while moving only a small fraction of the keys.

## Self-Test Answers
1. The OS (and runtime) layer: in IaaS you install and patch the OS/runtime yourself; in PaaS the provider manages the OS, runtime, and scaling, leaving you only the application and data.
2. Regulatory/data-residency rules may require sensitive customer or transaction data to stay on-premises, while the bank still uses the cloud's elasticity for the web tier or analytics — hence a hybrid split with the sensitive DB kept home.
3. Sequentially, origin ≈ 40 × 250ms = 10,000ms; CDN ≈ 40 × 20ms = 800ms — a saving of ~9,200ms (about 9.2s). (Real browsers parallelize, but this shows the order-of-magnitude win.)
4. Cloud-only round-trips each frame (~100ms each way ≈ 200ms per decision) and floods bandwidth; edge runs a local model (~15ms) and sends only alerts. Difference ≈ 200ms vs 15ms per decision — over 10x faster and orders of magnitude less uplink.
5. Start on PaaS/managed services for speed (or IaaS if you need tuning). Put images behind a CDN (CloudFront/Cloudflare) so users fetch from a nearby edge. Target ~95%+ hit ratio: with 10,000,000 requests, only ~500,000 reach origin — a 20x reduction — and average latency drops toward the edge's ~20–30ms.
