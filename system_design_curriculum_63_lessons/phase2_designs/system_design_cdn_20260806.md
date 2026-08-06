# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 43 of 63 — Phase 2: System Design Track (Design 15 of 35)
**Topic:** CDN (Content Delivery Network)
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
A CDN is how the internet stays fast: instead of every user in Mumbai, São Paulo, and Berlin fetching a video or webpage from one origin server in Virginia (200ms+ RTT, and one server melting under global load), the content is cached at hundreds of **edge points-of-presence (PoPs)** physically near users, so a request travels tens of kilometers instead of thousands. Cloudflare, Akamai, Fastly, CloudFront, and Google's edge serve the majority of internet traffic this way. The genius is deceptively simple — "cache things close to users" — but the engineering is brutal: how do you route a user to the *nearest healthy* PoP (anycast, GeoDNS), keep a 95%+ cache hit ratio across a two-tier hierarchy, invalidate a stale object across 300 PoPs in seconds, and protect a small origin from a global cache-miss stampede — all while absorbing multi-Tbps DDoS attacks. This is the physics-meets-distributed-systems problem: you can't beat the speed of light, so you move the bytes closer.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**SaaS/PaaS/IaaS/CDN (taught in Phase 1, Lesson 18):** The CDN concept was introduced here — edge caching, origin offload, PoPs. Today we build its internals: cache hierarchy, routing, invalidation.

**Networking / DNS / OSI (taught in Phase 1, Lesson 7):** The routing layer. **GeoDNS** and **Anycast** are how a user is directed to the nearest PoP; understanding DNS resolution, TTLs, and BGP anycast (same IP announced from many locations, BGP routes to the topologically nearest) is essential. TCP/TLS handshake RTT is what the CDN minimizes.

**Caches / Redis / Memcached / LRU (taught in Phase 1, Lessons 24, 4):** Each edge is a giant cache. Eviction (LRU/LFU/S3-FIFO), TTLs, cache-control semantics, hit ratio math — all directly applied. The edge cache is the heart of the system.

**Load balancers (taught in Phase 1, Lesson 6):** Inside a PoP, requests are LB'd across edge servers; anycast + consistent hashing routes an object to the same edge server to maximize hit ratio.

**Consistent hashing (taught in Phase 1, Lesson 19):** Within a PoP, which edge server caches which object — consistent hashing (CARP / rendezvous hashing) so adding/removing a server only remaps ~1/N of objects, preserving cache warmth.

**HA / replication / DR (taught in Phase 1, Lessons 3, 14):** PoPs fail; anycast + health checks reroute to the next-nearest PoP. Origin shielding and multi-origin failover.

**TLS / WAF / Zero Trust / rate limiting (taught in Phase 1, Lessons 25, 11):** The CDN terminates TLS at the edge (offloading origin), runs a WAF, and is the front line for DDoS mitigation and rate limiting — the edge is your security perimeter.

**Bloom filters (taught in Phase 1, Lesson 5):** Used in cache admission (don't cache one-hit-wonders) and to check "have we seen this object" cheaply.

> **Recap box:**
> - CDN = cache content at edge PoPs near users to cut RTT and offload origin.
> - Anycast + GeoDNS route users to the nearest healthy PoP (BGP/DNS).
> - Two-tier cache (edge → regional/shield → origin) maximizes hit ratio and shields origin.
> - Consistent/rendezvous hashing places objects on edge servers within a PoP.
> - Invalidation across hundreds of PoPs in seconds is the hard part; so is the cache-miss stampede.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. Why does a CDN make things faster? Give the two independent reasons, with a latency number.
> **What a strong answer covers:** (1) **Physical proximity / speed of light:** RTT is bounded by distance — Mumbai→Virginia is ~200ms round trip; Mumbai→Mumbai-PoP is ~5–10ms. Every TCP handshake, TLS handshake, and byte transfer is that much faster, and short RTT also lets TCP congestion windows grow faster (throughput ∝ 1/RTT). (2) **Origin offload:** a 95% cache hit ratio means the origin sees 20x less traffic, so it doesn't melt and stays responsive for the 5% that do reach it. Proximity cuts *latency*; caching cuts *origin load*. Both independently matter.
> **Common weak answer:** "It caches stuff so it's faster" — misses that proximity (RTT/speed-of-light) is a separate, physics-level win even for the first byte, and misses the origin-protection angle.
> **Mentor follow-up if they answer well:** For *dynamic*, uncacheable content (a personalized API response), does a CDN still help? (Yes — TLS/TCP termination at the edge + a warm keep-alive backbone connection to origin cuts handshake RTTs; "dynamic acceleration" / route optimization over the CDN's private backbone beats the public internet path.)

Q2. Estimate: a site serves 1 PB/day of mostly-static content to users globally at a 95% cache hit ratio. What's the origin egress, and roughly how many edge PoPs/servers?
> **What a strong answer covers:** Origin egress = 5% miss × 1 PB = **50 TB/day** ≈ 4.6 Gbps average origin egress (vs 92 Gbps if no CDN) — a 20x reduction, the whole point. Peak is higher (say 3–5x average → ~15–25 Gbps origin). Edge side: 1 PB/day ≈ 92 Gbps average delivered, peaking maybe 3x → ~280 Gbps, spread over, say, 50–100 PoPs globally = a few Gbps/PoP, each PoP a handful of servers with 100 GbE NICs. Cache storage: the "working set" (hot content) must fit in edge SSD/RAM; if hot set is 10 TB, each PoP holds it on NVMe. The key ratio to state: **hit ratio directly sets origin cost** — 95%→99% halves origin egress again.
> **Mentor follow-up:** Your CFO wants origin egress cut further. Bigger edge caches, or a mid-tier shield? (A regional shield tier: edge misses hit a regional cache, not origin, so origin sees misses-of-misses — pushes effective hit ratio toward 99%+ and collapses the miss fan-in.)

Q3. GeoDNS vs Anycast for routing users to the nearest PoP — what's the difference and which is better?
> **What a strong answer covers:** **GeoDNS:** the authoritative DNS server returns a different IP based on the resolver's location (or EDNS Client Subnet), pointing the user at a nearby PoP. Simple, works everywhere, but routing granularity is the *resolver's* location (can be wrong if using 8.8.8.8), and failover is bounded by DNS TTL (stale clients keep hitting a dead PoP for the TTL duration). **Anycast:** the *same* IP is announced via BGP from every PoP; the internet's routing fabric delivers packets to the topologically nearest PoP automatically. Faster failover (BGP reconverges, no DNS TTL wait), naturally absorbs/distributes DDoS across PoPs, but you get less explicit control and TCP-connection stability depends on stable BGP paths. Modern CDNs use **both**: anycast for the front IPs + GeoDNS/steering for coarse geo and origin selection.
> **Red flag answer:** "Just use round-robin DNS" — ignores geography entirely, sends users to random (possibly distant/dead) PoPs, and gives no proximity benefit.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the high-level architecture of a CDN. Draw the tiers and the routing layer.
> **Key components expected:** A **routing layer** (Anycast + GeoDNS + health-based steering), **edge PoPs** (each a cluster of caching servers behind an in-PoP LB, consistent-hashed by object), a **mid-tier / regional shield** cache, the customer **origin(s)** (with origin shielding), a **control plane** (config, cache-purge distribution, TLS certs), and **telemetry/logging** (hit ratios, per-PoP health). TLS termination + WAF + DDoS scrubbing at the edge.
> **Architecture diagram (text):**
```
                 User ──DNS──▶ GeoDNS/Anycast steering ──▶ nearest healthy PoP IP
                                                                │
     ┌─────────────────────────── EDGE PoP (many, global) ───────────────────────────┐
     │  Anycast VIP → in-PoP LB → [Edge Cache Servers] (consistent-hash by object key) │
     │  TLS termination │ WAF │ DDoS/rate-limit │ cache lookup (RAM→NVMe→disk tiers)   │
     └───────────────────────────────────┬───────────────────────────────────────────┘
                                          │  cache MISS (only 5%)
                                          ▼
                         REGIONAL SHIELD / MID-TIER CACHE (few per region)
                         (aggregates edge misses; origin sees miss-of-miss)
                                          │  shield MISS (only ~1%)
                                          ▼
                                Origin Shield (1 designated PoP per origin)
                                          │
                                          ▼
                                   Customer Origin(s)  ◀── multi-origin failover
     Control plane: config push, cache PURGE fan-out (pub/sub), cert mgmt, steering maps
     Telemetry: per-PoP hit ratio, health checks (feed steering), access logs → analytics
```
> **What separates SDE2 from SDE3 here:** SDE2 draws "edge caches + origin." SDE3 adds the **multi-tier hierarchy with origin shielding** — the single most important design choice for origin protection and hit ratio — and explains *why*: without a shield, N edge PoPs each independently miss on a cold object and all N hammer origin simultaneously (an N-way stampede); with a designated shield/origin-shield PoP, only *one* request reaches origin per object and fans back out. They also separate the **data plane** (serve bytes, must be dumb-fast) from the **control plane** (config/purge, eventually consistent, must not block serving).

Q5. Trace a request for `https://cdn.site.com/video/chunk_42.ts` from a user in Mumbai, on both a cache hit and a cache miss.
> **Expected trace (hit):**
> 1. DNS resolves `cdn.site.com` → anycast VIP; BGP routes the user's packets to the Mumbai PoP.
> 2. In-PoP LB hashes the object key → the edge server that owns `chunk_42.ts` (consistent hashing, so it's a deterministic, cache-warm server).
> 3. TLS handshake terminates at edge (session resumption if returning). Edge checks cache: RAM → NVMe. **Hit** → serve bytes directly. RTT ~5–10ms, no origin involved.
> **Expected trace (miss):**
> 1–2 as above; edge cache **miss**.
> 3. Edge requests from the **regional shield** (not origin directly). Shield miss → **origin shield PoP** → origin over a warm keep-alive backbone connection.
> 4. Origin returns the object with `Cache-Control`/`ETag`/`max-age`; each tier stores it per TTL on the way back; edge serves the user and now holds it for the next request.
> **Tricky part:** Candidates route the edge miss straight to origin (missing the shield tier → stampede risk), and forget **request coalescing** — if 10,000 Mumbai users request `chunk_42.ts` in the same second on a cold cache, the edge must send **one** request upstream and have the other 9,999 wait on it (single-flight), not 10,000 origin requests. Also forgetting that range requests / byte-range caching matter hugely for video.

Q6. Design the cache-control contract and the purge (invalidation) API. How does a customer say "cache this" and "this changed, drop it everywhere"?
> **Expected API design:**
> - **Caching directives** (origin response headers): `Cache-Control: public, max-age=86400`, `ETag`, `Last-Modified`, `Vary`, `Surrogate-Control`/`Surrogate-Key` (CDN-specific, lets edge TTL differ from browser TTL and tag objects for group purges).
> - **Purge API:** `POST /v1/purge` with either `{urls: [...]}` (purge by exact URL), `{tags: ["product-123"]}` (purge by surrogate key — invalidate all objects tagged with a product), or `{purgeAll: true}` (nuclear, rare). Returns a purge job ID; propagation is async across PoPs.
> - **Prefetch/warm API:** `POST /v1/prefetch {urls}` to pre-populate edges before a launch.
> **What to push on:** **Tag-based / surrogate-key purge** is the senior answer — purging by URL doesn't scale when one data change affects thousands of URLs; tags let "invalidate everything about product-123" be one call. **Versioned URLs** (`/style.a1b2c3.css`) as an alternative to purging entirely — change the URL, never invalidate (immutable caching, the best practice for static assets). **Soft purge** (mark stale, revalidate) vs **hard purge** (evict). **Idempotency** on purge. **Propagation SLA** — how fast is "everywhere"? (Seconds for good CDNs.)

---

### Data Modeling (Medium–Hard)
Q7. Model the cache entry and the routing/steering data. What does an edge server store per object?
> **Expected schema:**
```
# EDGE CACHE ENTRY (per object, in-memory index → NVMe/disk body)
CacheObject {
  key            = hash(host + path + vary_headers)   # cache key
  body_location  = {tier: RAM|NVMe|DISK, offset, size}
  content_type, content_length
  etag, last_modified
  expires_at     = fetch_time + max_age               # TTL
  stale_until    = expires_at + stale_while_revalidate # SWR window
  surrogate_keys = ["product-123", "catalog"]         # for tag purge
  cache_control  = {public, s-maxage, no-store...}
  hit_count, last_access                              # for LRU/LFU eviction
}
# EDGE INDEX: hashmap key -> metadata; LRU/LFU/S3-FIFO list for eviction

# ROUTING / STEERING (control plane, pushed to DNS + PoP health)
PoP { id, region, anycast_vip, capacity_gbps, health, geo_coords }
SteeringMap { client_geo/subnet -> ranked_PoP_list }   # GeoDNS answers
PurgeLog { purge_id, type: url|tag|all, keys[], issued_at, propagated_pops[] }
```
> **Index choices and why:** In-memory **hashmap** on cache key for O(1) lookup; an **eviction structure** (LRU list, or LFU/S3-FIFO for better hit ratio on skewed workloads — TinyLFU with a Bloom/Count-Min sketch for admission). A **reverse index** `surrogate_key -> [cache keys]` so a tag purge is O(objects with that tag), not a full scan.
> **Partitioning key and why:** Within a PoP, objects are partitioned across edge servers by **consistent/rendezvous hashing on the cache key** so each object has a deterministic home server (maximizes hit ratio, and adding/removing a server remaps only ~1/N). Across PoPs, partitioning is *geographic* (by user proximity), not by key — every PoP can hold any object.

Q8. How does a tag/surrogate-key purge actually reach and execute across 300 PoPs efficiently?
> **Expected answer:** The purge is a **control-plane fan-out**, not a data-plane operation. (1) Customer calls `POST /purge {tags:["product-123"]}`. (2) The control plane publishes the purge to a global **pub/sub / gossip distribution network** that reaches all PoPs in seconds (this is the hard infra — a reliable, fast, global broadcast). (3) Each edge server, on receiving the purge, uses its **reverse index** (`surrogate_key -> cache keys`) to find and evict/mark-stale matching objects — O(matching objects), no scan. (4) For efficiency at scale, prefer **soft purge** (mark stale → revalidate with origin via `If-None-Match` on next request; a `304 Not Modified` is cheap) over hard eviction, so a mass purge doesn't cause a mass cold-miss stampede. (5) A **generation number / versioned tag** trick: instead of evicting, bump a version associated with the tag so old entries are logically invalid without touching each object.
> **Trap:** Purging by iterating every URL, or doing a full cache scan per purge (O(cache size) per purge — deadly). Or hard-purging everything at once → every PoP goes cold → origin stampede. The reverse index + soft purge + fast broadcast is the correct triple.

Q9. Consistency vs availability: a customer updates a product image but keeps the same URL. How stale can the CDN be, and what do you trade?
> **Expected answer:** A CDN is fundamentally **AP / eventually consistent** — after a purge, different PoPs go stale-then-fresh at slightly different times (propagation is seconds, not instant/atomic), and clients holding a `max-age` response stay stale until it expires regardless. You trade **strong global consistency for availability and speed**: you cannot atomically flip 300 PoPs + millions of browser caches simultaneously. The honest guarantees: (a) *browser* staleness is bounded by `max-age`; (b) *edge* staleness after a purge is bounded by propagation SLA (seconds). The right engineering answer is **don't rely on invalidation for correctness — use versioned/immutable URLs** (`img.v42.jpg` with `max-age=1yr, immutable`), so a "change" is a new URL that's fresh by construction and old URLs simply age out. Invalidation is for the cases you can't version.
> **Mentor pushback:** "I purged, so it's consistent now." Scenario: a browser fetched the object 10 minutes ago with `max-age=3600` — your edge purge doesn't touch that browser's cache; the user sees the old image for up to an hour. Fix: short/zero browser `max-age` + long edge `s-maxage` (revalidate cheaply at edge), or versioned URLs. Purge affects edges, not already-delivered client caches — a subtlety many miss.

---

### Low-Level Design (Hard)
Q10. Hardest sub-problem: a viral event makes 500,000 users worldwide request a brand-new, uncached object in the same 2 seconds. Protect the origin (the "cache stampede" / "thundering herd").
> **Problem statement:** A cold object suddenly gets massive concurrent demand across many PoPs. Naively, every one of those requests misses and forwards upstream, and the small origin gets 500k concurrent requests and dies — the CDN *amplifies* the failure it was meant to prevent.
> **Naive solution:** Each edge server independently forwards every miss to origin; each PoP independently misses to origin.
> **Why naive fails at scale:** Two levels of fan-in: within a PoP, thousands of concurrent misses for the same key each open an upstream fetch; across PoPs, every PoP independently misses. 500k requests → potentially 500k origin hits. The origin (sized for 5% steady traffic) collapses; then *nothing* is cached and it stays down (metastable failure).
> **Expected optimal approach:** **Request coalescing (single-flight) + tiered shielding + SWR.** (1) **Single-flight per edge server:** the first miss for a key acquires a per-key lock and does the one upstream fetch; all concurrent requests for that key *wait on the same in-flight fetch* and share the response — thousands of misses → one upstream request per server. (2) **Consistent hashing within the PoP** ensures all requests for that key hit the *same* edge server, so coalescing is effective (not spread across servers). (3) **Origin shield / mid-tier:** all PoPs' misses funnel through a designated shield PoP that *also* single-flights, so origin sees **exactly one** request globally per object, which fans back out. (4) **Stale-while-revalidate:** if any stale copy exists, serve it instantly and revalidate in the background — never make the user wait on the origin fetch. (5) For genuinely new content, **prefetch/warm** edges before the known-viral launch.
> **Pseudo-code or class diagram:**
```
inflight = {}          # key -> Future  (per edge server, single-flight)
locks    = KeyedLock()

def get(key):
    obj = cache.lookup(key)
    if obj and not obj.expired():      return obj                    # hit
    if obj and obj.in_swr_window():                                  # stale-while-revalidate
        async_revalidate(key);         return obj                    # serve stale NOW
    with locks.acquire(key):           # only ONE fetcher per key
        if key in inflight: return inflight[key].wait()  # coalesce: share the fetch
        fut = inflight[key] = fetch_from_upstream(key)   # upstream = shield, not origin
    try:    obj = fut.result(); cache.store(key, obj); return obj
    finally: inflight.pop(key, None)

# upstream chain: edge -> regional shield (also single-flights) -> origin shield -> origin
# net effect: 500k user requests -> ~1 origin request per object
```

Q11. Concurrency / race: two requests for the same key miss simultaneously on one edge server and both start upstream fetches; worse, origin returns two slightly different versions. Fix.
> **Scenario:** Without single-flight, request A and B both miss `chunk_42`, both fetch; if origin content changed between the two fetches, they cache-race and the "winner" is nondeterministic; and you've doubled origin load per key.
> **Expected fix:** The **per-key single-flight lock** (Q10) is the primary fix — the second request finds the in-flight future and *waits* rather than starting its own fetch, so there's exactly one upstream fetch and one cached result, no race. For correctness of *which* version wins, honor `ETag`/`Last-Modified` and cache the response the single fetch returned (deterministic). Use a **short lock with a timeout** so a hung upstream fetch doesn't block waiters forever — on fetch timeout, fail the waiters (or serve stale) rather than holding the lock indefinitely.
> **Follow-up (what if the fetch-holder — the single-flight leader — dies mid-fetch?):** The lock must have a lease/timeout so a crashed leader's lock is released and a waiter promotes to a new leader and retries; without a lease you'd deadlock all waiters on that key. This is the same fencing/lease reasoning as a distributed lock (Lesson 41/17) — the lock protects a resource (origin), and a dead holder must not freeze it forever.

Q12. Failure deep-dive: an entire PoP (or the origin) goes down. Trace both, and how users keep getting served.
> **Scenario A — PoP failure:** The Mumbai PoP loses power/network.
> **Expected handling:** **Anycast** self-heals: BGP withdraws the failed PoP's route, and users' packets reconverge to the next-nearest PoP (Delhi/Singapore) within seconds — no DNS TTL wait. Health checks feed the steering system to stop directing traffic there. Users see slightly higher latency (farther PoP) and a cold-ish cache at the new PoP (mitigated because the shield tier is still warm). Capacity headroom (N+1 PoPs) absorbs the shifted load.
> **Scenario B — origin failure:** The customer origin is down.
> **Expected handling:** **Serve stale (stale-if-error):** the CDN keeps serving the last-cached copy past its TTL when the origin errors — availability over freshness, exactly right for an outage. Configure `stale-if-error` windows. Multi-origin failover routes to a healthy origin replica. For uncacheable content, a graceful custom error page from the edge. The principle: the CDN is a **shock absorber** — it should keep the site *up on stale bytes* while the origin recovers. Circuit-break origin fetches so the edge isn't hammering a dead origin. DLQ isn't relevant; the key patterns are stale-if-error, multi-origin, and circuit breaking.

---

### Scaling to 10x / 100x (Hard)
Q13. At 10x traffic (say 10 PB/day, plus a 5 Tbps DDoS), where does it break first?
> **Expected answer:** Three walls: (1) **Origin egress on cache misses** — even 3% misses of 10 PB = 300 TB/day; the shield tier and hit-ratio optimization are what keep origin alive. (2) **Per-PoP capacity / NIC saturation and cache storage** — the hot working set may exceed edge NVMe, tanking hit ratio; scale by adding PoPs and bigger caches, and by cache-admission policies (don't cache one-hit-wonders — Bloom/TinyLFU admission, Lesson 5). (3) **DDoS absorption** — 5 Tbps must be soaked at the edge; anycast naturally *distributes* a volumetric attack across all PoPs (each PoP eats its geographic slice), and L7 filtering/WAF/rate-limiting drops junk before it reaches origin. The first hard failure under a stampede is usually **origin**, which is why shielding + coalescing (Q10) is the load-bearing design.
> **Numbers to ground the answer:** 10 PB/day ≈ 925 Gbps average delivered, peaking ~3 Tbps → needs hundreds of PoPs. 3% miss → ~300 TB/day origin = ~28 Gbps origin egress (survivable only *with* a shield collapsing the fan-in). DDoS: 5 Tbps ÷ 200 PoPs = 25 Gbps/PoP — absorbable per-PoP; the whole point of anycast for DDoS.

Q14. Sharding / placement: how do you decide which object lives on which edge server, and what's the hot spot?
> **Expected sharding strategy:** Within a PoP, place objects on edge servers via **consistent hashing or rendezvous (HRW) hashing on the cache key** — deterministic home per object maximizes hit ratio (requests for the same key always hit the same warm server) and adding/removing a server remaps only ~1/N of keys (Lesson 19), preserving cache warmth. Across PoPs, placement is by **geography** (users pull to the nearest PoP), and any PoP can cache any object on demand.
> **Hot spot problem:** A single **mega-popular object** (a viral video chunk, a launch-day asset) hashes to *one* edge server, which then saturates its NIC while peers idle — the classic hot-key problem. **Detect** via per-server/per-key request rate. **Fix:** replicate the hot key across *multiple* servers in the PoP (relax consistent hashing for hot keys — hash to a small set and load-balance among them), or promote the hottest objects into a shared **RAM tier** replicated on every server in the PoP. This is the same hot-key mitigation as a distributed cache (Lesson 24): detect skew, replicate the hot slice rather than forcing it onto one owner.

Q15. Caching strategy — the eviction, TTL, and admission policy at the edge. Where's the subtlety?
> **Expected layered cache design:**
> - **Tiered storage within an edge:** hottest objects in **RAM**, warm on **NVMe SSD**, cold on **disk** — promote/demote by access frequency. Serve from the fastest tier that has it.
> - **Eviction:** plain LRU is beaten by **LFU / TinyLFU / S3-FIFO** on CDN workloads because access is heavily skewed (Zipfian) and LRU is polluted by scans/one-hit-wonders. TinyLFU uses a Count-Min sketch (small, Lesson 5) to admit only objects more frequent than the one they'd evict.
> - **Admission control:** don't cache **one-hit-wonders** (objects requested once) — a Bloom filter "have I seen this key before?" gates admission; only cache on the *second* request, saving cache space for genuinely hot content (a real Akamai/CDN optimization).
> - **TTL:** driven by origin `Cache-Control`/`s-maxage`; use **stale-while-revalidate** to serve instantly while refreshing.
> **Cache invalidation trap:** The subtle one is that **eviction ≠ invalidation** — LRU eviction is a *capacity* decision (silent, per-server), while purge is a *correctness* decision (must be global and reliable). Conflating them means you assume "it'll fall out of cache eventually" for a *correctness*-critical update (wrong image showing) — but a hot object never falls out of LRU. Correctness needs explicit purge or versioned URLs, never reliance on eviction. Second trap: a mass purge causing a cold-cache stampede — mitigate with soft-purge/SWR (Q8).

Q16. Cost/efficiency at scale.
> **Expected answer:** (1) **Maximize hit ratio** — it's the dominant cost lever: every 1% of hit ratio is a chunk of expensive origin egress avoided; shield tier + admission control + big edge caches. (2) **Peering / private backbone** — deliver over the CDN's own network and settlement-free peering to cut transit/egress costs; keep bytes off expensive paths. (3) **Compression + modern codecs** (brotli, AVIF/WebP image transcoding at the edge, adaptive bitrate for video) to move fewer bytes. (4) **Tiered storage** (RAM/NVMe/disk) so you're not buying all-flash for cold content. (5) **Cache-admission** to not waste space/IO on one-hit-wonders. (6) **Collapse origin fetches** (coalescing + shield) so you pay origin egress once per object, not per PoP. (7) **Immutable/versioned URLs** with year-long TTLs → near-100% hit ratio for static assets, minimal revalidation traffic.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)
**H1.** Deep internals: explain anycast's interaction with TCP/TLS. What happens to a long-lived connection if BGP reroutes mid-connection, and how do CDNs cope? (Direction: anycast is per-*packet* routed; if a BGP path change mid-connection sends packets to a different PoP that has no state for that TCP connection, the connection breaks (RST). In practice BGP paths are stable enough that this is rare, and CDNs mitigate with connection-oriented handling, keeping stateful flows sticky, using anycast mainly for the initial routing + stable within a session, and QUIC/HTTP3's connection IDs which survive path changes better than TCP's 4-tuple. This is why some designs use anycast for DNS/initial steering but unicast VIPs for the actual long-lived data connection.)

**H2.** Cross-cutting security: the edge is your DDoS front line and TLS terminator. How do you absorb a 5 Tbps volumetric attack and an L7 (HTTP flood) attack differently, and handle TLS keys across 300 PoPs? (Direction: volumetric (L3/L4) is absorbed by **anycast dispersion** (attack splits across all PoPs) + upstream scrubbing + SYN cookies + dropping at the edge NIC/eBPF before it costs CPU. L7 floods need application-aware detection: rate limiting, JS challenges/CAPTCHA, behavioral fingerprinting, WAF rules — you can't just soak them, you must *identify* bad requests. TLS: private keys distributed to edges is a huge attack surface — use short-lived certs, "keyless SSL" (the private key stays in the customer's HSM and only signing is delegated), and per-PoP cert rotation. Zero-trust to origin via mTLS.)

**H3.** Operational: how do you push a config or software change to 300 PoPs without breaking the internet, and roll back in seconds? (Direction: staged/canary rollout — deploy to 1 PoP, watch error/hit-ratio SLOs, then a region, then global; automated rollback on SLO breach. Config distribution via a versioned control plane with atomic apply per PoP and last-known-good fallback (data plane never blocks on control plane). The infamous risk: a bad global config push (a bad regex, as in real Cloudflare/Fastly outages) took the whole edge down — so config changes need validation, canarying, and a fast global rollback path. Health-checked, drain-then-deploy per PoP.)

**H4.** Observability: what do you instrument for a CDN, and what pages you? (Direction: **cache hit ratio** per PoP/customer (the north-star; a drop = origin about to get hammered), **origin egress/offload ratio**, **edge→origin fetch latency and error rate**, **per-PoP health/capacity/NIC saturation**, **purge propagation latency** (SLA compliance), **RUM (real user monitoring)** for actual end-user latency by geo, and **DDoS/attack traffic** dashboards. Page on: hit-ratio cliff (stampede incoming), origin error rate (serving stale-if-error), PoP down/steering failover, purge propagation SLA breach, cert expiry.)

**H5.** 'Undo a bad decision': you launched with a flat single-tier edge (edges fetch directly from origin, no shield) and every product launch melts the origin with cross-PoP miss stampedes. Migrate to a shielded hierarchy without downtime. (Direction: introduce a **regional shield / origin-shield tier** and reconfigure edge origin-fetch to route through the shield instead of directly to origin — a control-plane config change, no data migration; roll out shield-by-region canary-style watching origin egress drop. Add single-flight coalescing at both edge and shield. Verify the N-way stampede collapses to ~1 origin fetch/object via origin-egress metrics before/after. Because the cache data plane is stateless (rebuilds from origin), this is a routing reconfiguration, reversible instantly by pointing edges back at origin.)

---

### Mentor's Closing Notes
**Top 3 things most candidates get wrong on this topic:**
1. **Forgetting the cache-miss stampede / no origin shielding** — they draw edges fetching straight from origin and don't realize a cold viral object turns the CDN into a DDoS *against* the origin. Single-flight coalescing + a shield tier is the load-bearing answer.
2. **Treating invalidation as instant and global** — assuming a purge atomically updates everything, ignoring propagation delay and (crucially) already-delivered browser caches bounded by `max-age`. Seniors reach for **versioned/immutable URLs** so they rarely need to invalidate at all.
3. **Only citing caching, not proximity + routing** — they miss that the RTT/speed-of-light win is independent of caching (helps even dynamic content), and they hand-wave routing instead of naming anycast + GeoDNS and their failover trade-offs.

**The one insight that makes an answer truly impressive:**
Recognizing a CDN as a **hierarchical, request-coalescing shock absorber** whose job is to make the origin see *exactly one request per object regardless of global demand*, and that **correctness (invalidation) and capacity (eviction) are different problems** — you version URLs for correctness, use LFU/TinyLFU + admission for capacity, and never confuse "it'll evict eventually" with "it's invalidated." Add the physics framing (you can't beat the speed of light, so the entire architecture exists to shorten the distance bytes travel and to collapse N global misses into 1 origin fetch), plus knowing the real-world footgun that a bad global config push is the CDN's most catastrophic failure mode — and you're clearly operating at staff level.

**Suggested follow-up reading:**
- "The Akamai Network: A Platform for High-Performance Internet Applications" (Nygren et al.) — the canonical CDN architecture paper; and Cloudflare's engineering blog on anycast + origin shielding.
- Facebook's "An Analysis of Facebook Photo Caching" and the TinyLFU / S3-FIFO cache-admission papers (Lesson 5 sketches applied to real cache hit-ratio wins).

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate — especially Lessons 7 (DNS/anycast), 24 (caches/LRU), 19 (consistent hashing), 18 (CDN intro), 25 (TLS/WAF/DDoS).
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to a CDN. Expected answers include real algorithms/mechanisms (anycast/BGP, GeoDNS, consistent/rendezvous hashing, single-flight coalescing, TinyLFU/S3-FIFO admission, stale-while-revalidate, surrogate-key purge), specific failure modes (cross-PoP miss stampede, bad global config push, browser-cache staleness, hot-key NIC saturation), and real numbers. Cross-referenced Phase 1 lessons throughout.
