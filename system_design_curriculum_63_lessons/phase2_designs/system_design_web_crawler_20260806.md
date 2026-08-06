# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 36 of 63 — Phase 2: System Design Track (Design 8 of 35)
**Topic:** Web Crawler
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
A web crawler is a BFS over the largest, most hostile graph in existence — the web has ~50+ billion indexed pages, no schema, adversarial content (spider traps, cloaking, malware), and every host is a rate-limited stranger you must be polite to. Googlebot, Bingbot, and the Common Crawl foundation run this at planetary scale; the same machinery powers search indexing, price scrapers, archive.org, and LLM training-data collection. It's hard because you must be *polite* (respect robots.txt and per-host crawl delays), *fresh* (re-crawl news hourly, static pages monthly), *complete* (don't miss pages), *efficient* (don't re-fetch unchanged pages), and *robust* (a single malicious site generating infinite URLs must not consume your whole fleet) — all while deduplicating billions of URLs and near-duplicate documents.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Graphs / BFS traversal (taught in Phase 1, Lesson 4):** The web is a directed graph of pages (nodes) and hyperlinks (edges); crawling is a traversal, typically BFS from a seed set, using a frontier (queue) of URLs to visit and a visited set to avoid revisiting. BFS gives good coverage of high-value, well-linked pages first. The frontier *is* the crawler's core data structure.

**Bloom filters (taught in Phase 1, Lesson 5):** Space-efficient probabilistic set membership, no false negatives, tunable false-positive rate. With 50B+ URLs, an exact "have I seen this URL?" set is huge; a Bloom filter answers "definitely new" vs "probably seen" in a few bits per URL — the standard URL-dedup primitive.

**Consistent hashing (taught in Phase 1, Lesson 19):** Maps keys to nodes on a ring with vnodes so scaling reshuffles only ~1/N of keys. We shard the frontier and the seen-set by **hostname** so all URLs of one host land on one worker — essential for enforcing per-host politeness in one place.

**Kafka / queues (taught in Phase 1, Lesson 21):** Durable, partitioned queues decouple stages. The multi-stage pipeline (fetch → parse → extract → dedup → enqueue) is a series of Kafka topics; partitioning and consumer groups give parallelism and backpressure via lag.

**Rate limiting (taught in Phase 1, Lesson 11):** Token bucket / delay-based throttling. Politeness = per-host rate limiting: at most 1 request every N seconds per host (from robots.txt `Crawl-delay` or a default), regardless of how many URLs you have for that host.

**GFS / distributed storage / MapReduce (taught in Phase 1, Lesson 15):** Crawled content is stored in a distributed blob store (GFS/HDFS/S3), and offline jobs (MapReduce/Spark) compute link graphs, dedup, and PageRank-style priorities over the corpus.

**Caching / DNS (taught in Phase 1, Lessons 24 & 7):** DNS resolution is a per-host cost; at billions of fetches, uncached DNS lookups become a bottleneck and can DoS resolvers — so DNS is aggressively cached. Content-addressable caching (hashes) dedups documents.

**SQL/NoSQL, LSM (taught in Phase 1, Lesson 23):** The URL metadata store (last-crawled, etag, next-crawl-time, status) is a massive write-heavy key-value workload → LSM-based wide-column store (Bigtable/Cassandra/RocksDB), not a B-tree RDBMS.

> **Recap box:**
> - Crawl = BFS over the web graph; the **frontier** (URL queue) is the core structure.
> - Bloom filter = "seen this URL?" at 50B scale in a few bits each.
> - Consistent hashing by **hostname** → per-host politeness enforced in one place.
> - Kafka topics = the fetch→parse→extract→dedup pipeline with backpressure.
> - Token-bucket per-host rate limiting = politeness (respect robots `Crawl-delay`).
> - GFS/S3 + MapReduce = store content, compute link graph & priorities offline.
> - DNS caching = avoid re-resolving hosts billions of times.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. You learned BFS in Lesson 4. A crawler is BFS over the web graph — what are the two data structures the traversal needs, and why is each hard at web scale?
> **What a strong answer covers:** (1) The **frontier** — the queue of URLs to fetch (BFS uses a FIFO-ish queue, but a real crawler makes it a *priority* queue by page importance/freshness). Hard because it holds billions of URLs, must be persistent (survive restarts), distributed, and must enforce politeness (not just "next URL" but "next URL for a host we're allowed to hit now"). (2) The **seen/visited set** — dedup so you don't re-crawl. Hard because exact membership over 50B+ URLs is huge → Bloom filter + backing store. Bonus: mention the web graph is unbounded and adversarial, so pure BFS needs bounds (depth limits, per-host caps).
> **Common weak answer:** "A queue and a HashSet." Correct in a textbook, but no acknowledgment of scale (billions), persistence, politeness, or the Bloom-filter approximation — this design OOMs on day one.
> **Mentor follow-up if they answer well:** Pure FIFO BFS treats a spammer's 10M auto-generated URLs the same as the NYT homepage. How do you prioritize? (Priority frontier keyed by PageRank/importance + freshness need; per-host caps so one host can't dominate.)

Q2. Estimate the scale: crawl 10B pages/month with a target refresh, average page 100KB. Give fetch rate, bandwidth, and storage.
> **What a strong answer covers:** 10B pages/month ÷ (30×86400 s) ≈ **3,860 pages/sec average**, peak ~2x → ~8k/sec. Bandwidth: 3,860 × 100KB = **~386 MB/sec ≈ 3.1 Gbps** sustained ingest (before compression). Storage: 10B × 100KB = **1 PB/month** raw; with gzip (~5x on HTML) ≈ 200 TB/month stored; keep multiple months → petabytes. URL metadata: 10B URLs × ~200 bytes ≈ 2 TB just for the frontier/seen metadata per month, but the cumulative URL universe (including uncrawled discovered links, ~10x fetched) is ~100B+ URLs → a Bloom filter of 100B URLs at ~10 bits each ≈ 125 GB (fits in RAM sharded).
> **Mentor follow-up:** Given ~4k fetches/sec and per-host politeness of 1 req/sec, how many *distinct hosts* must you be actively crawling to sustain that rate? (At least ~4,000 hosts in-flight concurrently — this is why the frontier must be organized *by host*, and why crawling few large sites can't saturate your fleet.)

Q3. A candidate says "just use a HashSet of visited URLs in memory." What's wrong, and what's the standard structure?
> **What a strong answer covers:** A HashSet of 100B URL strings (avg ~70 bytes each) is ~7 TB+ — won't fit in one machine's RAM, and exact strings are wasteful. Standard: normalize the URL, then a **Bloom filter** (a few bits/URL, ~125 GB for 100B at 1% FPR) as the fast "definitely new / probably seen" filter, backed by a durable KV store (Bigtable/RocksDB) for authoritative checks and metadata. False positives (Bloom says "seen" when new) mean occasionally skipping a genuinely new URL — acceptable at web scale; false negatives never happen, so you never double-crawl due to the filter.
> **Red flag answer:** "In-memory HashSet, scale it with more RAM." Ignores the 7TB reality and offers no persistence — a restart loses the entire visited set and re-crawls the web.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the crawler pipeline end to end — from seed URLs to stored, deduplicated, indexed content.
> **Key components expected:** Seed set; **URL Frontier** (priority + politeness queues, sharded by host); DNS resolver + cache; **Fetcher** workers (HTTP, robots.txt aware); **robots.txt** cache/service; **Parser/Extractor** (extract links + content); **URL normalizer + dedup** (Bloom filter + seen store); **content dedup** (checksum + SimHash/MinHash near-dup); **content store** (S3/GFS); **URL metadata store** (Bigtable — last-crawled, etag, next-crawl); **scheduler/prioritizer** (re-injects URLs by freshness/importance); offline link-graph / PageRank job.
> **Architecture diagram (text):**
```
  Seeds ─┐
         ▼
   ┌───────────────── URL FRONTIER ─────────────────┐
   │  front queues (by priority)  →  back queues     │  sharded by host
   │  (per-host politeness: host→queue, next-fetch t)│  (consistent hashing)
   └───────────────┬─────────────────────────────────┘
                   │ pop URL (host allowed now)
                   ▼
             DNS cache ──▶ resolve host
                   │
                   ▼
        robots.txt cache ──▶ allowed?  ──no──▶ drop
                   │ yes
                   ▼
     ┌──────────── FETCHER pool ────────────┐  HTTP GET, conditional (If-None-Match/If-Modified-Since)
     │  429/503 → backoff; 304 → skip body  │
     └───────────────┬──────────────────────┘
                     │ raw page → Kafka(content)
                     ▼
              PARSER / EXTRACTOR
         ┌───────────┴────────────┐
   extract links            extract content
         │                        │
   normalize + Bloom          content hash + SimHash
   dedup (seen?)              (exact + near-dup?)
         │ new                     │ new
         ▼                         ▼
   enqueue to Frontier       Content Store (S3) + metadata (Bigtable)
                                   │
                                   ▼  offline: link graph, PageRank → priorities
```
> **What separates SDE2 from SDE3 here:** An SDE2 draws fetch→parse→store. An SDE3 designs the **Frontier as a two-tier structure (front queues for priority, back queues for per-host politeness)** — the Mercator design — so prioritization and politeness are decoupled; adds **conditional GETs (ETag/If-Modified-Since → 304)** so re-crawls are cheap; treats **robots.txt and DNS as cached services** (not per-fetch calls); and separates **URL dedup from content dedup** (two different problems — same content under many URLs, and same URL). They also make the frontier **persistent** so a fleet restart doesn't lose billions of queued URLs.

Q5. Trace fetching one URL — `https://example.com/article` — from the moment it's popped off the frontier to its links being enqueued.
> **Expected trace:**
> 1. Frontier pops the URL from `example.com`'s back-queue *only if* `now ≥ next_allowed_fetch[example.com]`; else it stays queued (politeness).
> 2. **DNS**: resolve `example.com` via the DNS cache (TTL-respecting); cache miss → resolver, then cache.
> 3. **robots.txt**: check cached robots for `example.com`; if `/article` is `Disallow`, drop. Respect `Crawl-delay`.
> 4. **Conditional fetch**: HTTP GET with `If-None-Match: <etag>` / `If-Modified-Since` from the metadata store. If `304 Not Modified`, skip the body, update `last_crawled`, reschedule, done. If `429/503`, exponential backoff and requeue.
> 5. On `200`: stream body (with a size cap to avoid a 2GB tarpit), compute a **content hash**; if hash unchanged, treat as unmodified. Store body in S3 (content-addressed), update metadata (etag, last_crawled, next_crawl based on change frequency).
> 6. **Parse**: extract links, **normalize** each (lowercase host, strip fragments, resolve relative → absolute, sort query params, remove session IDs).
> 7. For each normalized link: Bloom-filter check → if "probably seen," verify against seen store; if genuinely new, **enqueue to frontier** with a priority and set `next_allowed_fetch` for its host.
> 8. Set `example.com`'s `next_allowed_fetch = now + crawl_delay`.
> **Tricky part:** Candidates skip the **conditional GET / 304** path (so they re-download unchanged pages, wasting the bulk of bandwidth) and skip **URL normalization** (so `example.com/a?utm=x`, `example.com/a?utm=y`, and `example.com/a#top` are treated as three URLs → frontier explosion and infinite dedup misses).

Q6. Define the internal API / interface between the Frontier and the Fetchers, and how a fetcher asks for "the next URL I'm allowed to crawl."
> **Expected API design:**
> ```
> // Frontier service
> GET  /frontier/next?worker_id=W&n=100
>   → [ {url, host, priority, etag?, last_modified?}, ... ]   // only hosts whose politeness window is open
> POST /frontier/add
>   body: { urls:[{url, priority, discovered_from}], ... }    // enqueue newly discovered links
> POST /frontier/complete
>   body: { url, status, next_crawl_at, etag, content_hash }  // fetch result → reschedule/metadata
> // robots + dns as cached services
> GET  /robots/allowed?host=example.com&path=/article  → {allowed, crawl_delay}
> ```
> **What to push on:** The frontier hands out URLs in **batches per worker** (amortize round-trips at 8k fetches/sec) and must only return URLs whose **host politeness window is open** — the politeness logic lives in the frontier, not the fetcher, so no coordination is needed among fetchers. **Idempotency**: `/complete` must be idempotent (a fetcher may crash and a URL be re-handed-out — completing twice must not corrupt metadata). **Backpressure**: if all hosts are in cooldown, `next` returns fewer/zero URLs rather than politeness-violating ones. Discovered links are added with dedup at enqueue time.

---

### Data Modeling (Medium–Hard)

Q7. Design the URL metadata store and the frontier's persistent state.
> **Expected schema:**
```sql
-- URL metadata (Bigtable/Cassandra — write-heavy, keyed by URL hash)
CREATE TABLE url_meta (
  url_hash        blob,          -- e.g. 128-bit hash of normalized URL  (partition key)
  url             text,
  host            text,
  last_crawled    timestamp,
  next_crawl_at   timestamp,     -- adaptive: news hourly, static monthly
  etag            text,
  last_modified   timestamp,
  content_hash    blob,          -- to detect unchanged content
  simhash         bigint,        -- 64-bit near-duplicate fingerprint
  http_status     int,
  crawl_depth     int,
  priority        float,         -- PageRank-ish importance
  fail_count      int,
  PRIMARY KEY (url_hash)
);

-- Per-host state (for politeness) — keyed by host, colocated with that host's frontier shard
CREATE TABLE host_state (
  host              text,
  next_allowed_at   timestamp,   -- now + crawl_delay
  crawl_delay_ms    int,         -- from robots.txt or default
  robots_txt        text,
  robots_fetched_at timestamp,
  ip                text,        -- cached DNS
  PRIMARY KEY (host)
);

-- Content store: S3/GFS, key = content_hash (content-addressed, natural dedup)
```
> **Index choices and why:** `url_meta` is keyed by **`url_hash`** (128-bit hash of the normalized URL) so lookups are O(1) point reads and the key is fixed-width (URLs vary wildly in length). A secondary index / scan on `next_crawl_at` (or a separate scheduler queue) drives re-crawl selection. `content_hash` enables exact-dup detection; `simhash` enables near-dup. `host_state` keyed by `host` so politeness state is a single point read colocated with the host's frontier shard.
> **Partitioning key and why:** `url_meta` partitions by `url_hash` (uniform spread, no host hot spots). The **frontier and host_state partition by `host`** via consistent hashing — this is the critical decision: all URLs of one host, plus its politeness/robots/DNS state, live on one shard, so per-host rate limiting needs no cross-shard coordination.

Q8. How do you decide *when* to re-crawl a page? A naive "re-crawl everything every N days" wastes most of your fetch budget. Design the freshness strategy.
> **Expected answer:** **Adaptive re-crawl** based on observed change frequency. Track per-URL change history (via `content_hash` / `Last-Modified`); estimate each page's change rate λ and schedule `next_crawl_at` accordingly — a Poisson-process model (as in Cho & Garcia-Molina's freshness work): frequently-changing, high-importance pages (news homepages) re-crawled minutes–hours; static pages monthly. Weight by importance (PageRank) so you spend the budget where it matters. Use **conditional GETs** (ETag/If-Modified-Since) so even a re-crawl of an unchanged page costs a 304 (headers only), not a full download. Sitemaps and `<lastmod>` hints, plus HTTP `Cache-Control`/`Expires`, refine the estimate.
> **Trap:** Fixed-interval re-crawl (everything every 7 days) wastes ~90% of fetches on unchanged low-value pages while missing fast-changing important ones — poor freshness *and* poor efficiency simultaneously. Also naive: re-downloading full bodies to check for changes instead of conditional GET / content-hash comparison.

Q9. Consistency vs availability for the seen-set / frontier: two fetchers on different shards might both discover and enqueue the same URL. Is strong dedup required? Where is eventual consistency fine?
> **Expected answer:** Perfect dedup is **not** required — the crawler is availability/throughput-oriented. Occasionally crawling the same URL twice wastes one fetch; that's cheap compared to the cost of strong global coordination on every enqueue at 8k/sec. So the Bloom filter + seen store can be **eventually consistent**; a small double-crawl rate is acceptable. Where you *do* want more care: (1) **politeness must be effectively strong per host** — if two fetchers both think they can hit `example.com` now, you violate robots and risk an IP ban; solved structurally by routing a host to *one* shard (consistent hashing by host), so its politeness state is single-owner, not by distributed locks. (2) The **content store is idempotent** (content-addressed by hash), so double-writes are harmless.
> **Mentor pushback:** "You said double-crawl is fine — but a spider trap generates infinite unique URLs, and your eventually-consistent dedup can't catch them because they're all genuinely new." Correct — dedup doesn't solve traps; you need **per-host URL caps, crawl-depth limits, and URL-pattern anomaly detection** (a host emitting millions of URLs with monotonic query params) as a separate defense. Consistency isn't the tool for traps.

---

### Low-Level Design (Hard)

Q10. The hardest sub-problem: the **URL Frontier** that simultaneously (a) prioritizes important/fresh URLs and (b) never violates per-host politeness, at billions of URLs. Design it.
> **Problem statement:** Serve fetchers a stream of URLs such that high-priority URLs are crawled sooner AND no host is hit faster than its `crawl_delay`, with billions of queued URLs, persistent across restarts.
> **Naive solution:** One global priority queue by importance; fetchers pop the top. Or one FIFO BFS queue.
> **Why naive fails at scale:** A single global priority queue ignores politeness — the top 1,000 URLs might all be from `reddit.com`, so you'd hammer one host and get IP-banned while starving every other host. A single FIFO ignores priority (crawls junk before the NYT). Neither is distributable without a global lock at 8k/sec.
> **Expected optimal approach:** The **Mercator two-tier frontier**: (1) **Front queues** — F priority queues (e.g., 1–10); a prioritizer assigns each URL a priority (by PageRank/importance/freshness) and enqueues into the matching front queue. (2) **Back queues** — B back queues, each holding URLs for *exactly one host at a time* (a `host → back-queue` map ensures all of one host's URLs funnel to one back queue → politeness enforced by construction). A **min-heap keyed by `next_allowed_fetch` time** picks which back queue is ready. A router moves URLs front→back maintaining the invariant "one host per back queue." Fetchers pull from ready back queues. Politeness = a back queue isn't served until its host's cooldown expires. Priority is preserved because front→back routing pulls from higher-priority front queues more often. The whole thing is sharded by host (consistent hashing) and persisted (RocksDB/Kafka) so it survives restarts.
> **Pseudo-code or class diagram:**
```
class Frontier:
    front_queues: List[PriorityQueue]     # F queues by priority tier
    back_queues:  List[Queue]             # B queues, each ~1 host
    host_to_back: Dict[host, int]         # host → its back queue index
    heap: MinHeap[(next_allowed_at, back_queue_id)]

    def add(url):
        p = prioritizer.score(url)        # PageRank + freshness
        front_queues[p].push(url)

    def route():                          # front → back, keep 1-host-per-backqueue
        url = pick_from_front()           # weighted by priority tier
        h = host(url)
        if h in host_to_back:
            back_queues[host_to_back[h]].push(url)
        else:
            bq = pick_empty_back_queue()
            host_to_back[h] = bq
            back_queues[bq].push(url)
            heap.push((now, bq))

    def next():                           # fetcher asks for work
        t, bq = heap.peek()
        if now < t: return None           # nothing polite to serve yet
        heap.pop()
        url = back_queues[bq].pop()
        schedule_next(bq, now + crawl_delay(host(url)))   # re-heap
        return url
```

Q11. Concurrency / race: a fetcher pulls a URL, starts a 30-second fetch, then **crashes**. The URL is neither completed nor re-queued — it's lost. Meanwhile if you re-hand it out too eagerly, two fetchers crawl it at once (politeness violation). Design the fix.
> **Scenario:** URL is "in-flight" with a dead owner. Lose it → coverage gap. Re-hand-out immediately → double fetch of the same host inside the crawl-delay window.
> **Expected fix:** Treat handed-out URLs as **leases with a visibility timeout** (like SQS): when `next()` hands a URL to worker W, mark it in-flight with a lease deadline (e.g., 2× expected fetch time). If `/complete` arrives before the deadline, finalize. If the lease expires (worker died), the URL becomes eligible for re-assignment — but re-assignment still goes through the *same host back queue*, so politeness is preserved (it won't be re-served until the host's `next_allowed_at`). Make `/complete` **idempotent** (keyed by url_hash + attempt) so a slow-but-alive worker completing after its lease expired doesn't corrupt state. Cap re-leases with `fail_count` → after N failures, park the URL (dead/permanently-erroring) rather than looping forever.
> **Follow-up — what if the lock/lease holder is alive but stuck (GC pause, slow host)?** The lease expires and the URL may be re-served, causing a duplicate fetch — accepted (dedup at content store makes the write idempotent; the wasted fetch is cheap). The invariant that matters — never violate per-host politeness — holds because re-service still respects the host's cooldown. This is at-least-once crawling with idempotent storage, mirroring the notification-system lesson.

Q12. Edge case / failure deep-dive: **spider traps and duplicate content**. A site auto-generates infinite URLs (`/calendar?date=...` forever) and mirrors the same content across millions of URLs. What breaks and how do you defend?
> **Scenario:** Infinite-URL trap → frontier grows without bound, one host consumes your fleet, and you index millions of near-identical pages (crawl budget wasted, index polluted).
> **Expected handling:** Layered defenses: (1) **Crawl-depth limit** and **per-host URL cap** — no single host may contribute more than X URLs or depth D; a spammer can't monopolize. (2) **URL pattern / anomaly detection** — a host emitting many URLs differing only by a monotonic parameter (`?page=1..∞`, `?date=...`) is flagged and rate-throttled or blocked. (3) **Content near-dup detection with SimHash** — compute a 64-bit SimHash of each page; if a new page's SimHash is within Hamming distance ~3 of an already-crawled page, it's a near-duplicate → don't index, deprioritize the source. (4) **Budget per host** proportional to its importance (PageRank), so low-value hosts get few fetches. (5) Respect `robots.txt` and `<meta name="robots" content="noindex,nofollow">` and canonical tags (`<link rel="canonical">`) which explicitly tell you the duplicate story. (6) Size/time caps per fetch to avoid tarpits (a URL that streams forever).

---

### Scaling to 10x / 100x (Hard)

Q13. You go from 10B to 1T pages and 4k → 400k fetches/sec target. Where does it break first, and what's the number?
> **Expected answer:** Two things break before compute: **(1) outbound bandwidth & polite host coverage**, and **(2) DNS**. At 400k fetches/sec × 100KB = **40 GB/sec ≈ 320 Gbps** egress — a serious network-provisioning problem requiring many machines across many network egress points/regions. And with 1 req/sec/host politeness, sustaining 400k fetches/sec needs **≥400,000 distinct hosts actively in-flight** — the frontier must efficiently juggle hundreds of thousands of open host windows; if your crawl is skewed toward few large hosts, politeness caps your throughput no matter how much compute you add. **DNS** becomes a bottleneck: 400k fetches/sec with poor caching could mean hundreds of thousands of resolutions/sec — you'd DoS your own resolvers, so you run a large distributed DNS cache with prefetching. Signals: frontier "ready host" count vs target fetch rate (if fewer hosts are polite-ready than needed, throughput is host-bound), and DNS cache hit rate. Storage grows to petabytes/month → 1T pages × ~20KB compressed = ~20 PB.
> **Numbers to ground the answer:** 320 Gbps egress; ≥400k concurrent polite hosts; DNS hit-rate must be >99% or resolvers melt; ~20 PB/month stored compressed.

Q14. How do you shard the frontier and the seen-set across the fleet, and where's the hot spot?
> **Expected sharding strategy:** **Consistent hashing by hostname** (Lesson 19) with vnodes: all URLs and politeness/robots/DNS state for a host live on one shard, so per-host politeness is enforced locally with no cross-shard coordination, and scaling reshuffles only ~1/N of hosts. The seen-set / Bloom filter is sharded the same way (by host, or by url_hash for the metadata store).
> **Hot spot problem:** A few mega-hosts (youtube.com, wikipedia.org, a giant e-commerce site) have vastly more URLs than the average host, so their shard holds a huge back queue and does more work — but politeness *caps* how fast you can crawl any single host anyway (1 req/sec), so a mega-host doesn't cause a *throughput* hot spot, it causes a *storage/queue-size* imbalance on one shard. Detect via per-shard queue depth. Fix: allow a large host to be **split across multiple back queues by sub-path or by resolved IP** (many big sites have many IPs/CDNs, so per-IP politeness lets you crawl them faster safely), and cap per-host URL retention so an enormous but low-value host doesn't hoard frontier memory. Rebalance vnodes if a shard is persistently hot.

Q15. Design the caching layers and name the invalidation trap unique to crawling.
> **Expected layered cache design:** (1) **DNS cache** (respect TTLs, prefetch popular hosts) — avoids re-resolving billions of times. (2) **robots.txt cache** per host with a refresh TTL (hours) — you must not fetch robots.txt before every page. (3) **Content-addressed store** (key = content hash) — natural dedup cache; if you've stored this exact content, don't re-store. (4) **In-memory Bloom filter** for seen-URLs as the front cache over the durable seen store. (5) HTTP-level conditional caching via **ETag/Last-Modified** so the *origin server* helps you cache (304s).
> **Cache invalidation trap:** **robots.txt staleness cuts both ways.** If you cache robots.txt too long and a site *adds* a `Disallow`, you keep crawling disallowed paths → a politeness/legal violation and possible ban. If you refresh it too aggressively, you waste fetches and add latency to every crawl. The senior answer: cache robots with a bounded TTL (e.g., 24h) but treat a `403/429` or a robots change as an immediate signal to re-fetch robots and back off — and always re-check robots on cache expiry *before* crawling that host again. The DNS cache has the mirror trap: stale IP after a site migrates → you crawl a dead or wrong server.

Q16. Cost/efficiency at scale. Where's the money and how do you cut it?
> **Expected answer:** Cost is (1) **bandwidth/egress** (the biggest — hundreds of Gbps), (2) **storage** (petabytes), (3) **compute** for parsing/dedup. Cuts: **conditional GETs + content-hash comparison** so unchanged pages cost a 304 (headers only) — since most re-crawls find no change, this saves the majority of bandwidth. **Adaptive re-crawl** so you don't waste fetches on static/low-value pages (Q8). **Compression** (gzip HTML ~5x, columnar for extracted structured data) and **tiered storage** — hot recent crawls on fast storage, older on S3 Glacier/cold. **Near-dup elimination (SimHash)** so you don't store/index millions of mirror pages. **Prioritize by importance** so the budget goes to high-value pages. **Regional crawling** — crawl from a region near the target hosts to cut latency and cross-region egress. **Skip non-useful content types** (don't download huge binaries/videos unless needed; check Content-Length/Content-Type first).

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Deep internals — near-duplicate detection: explain SimHash vs MinHash, why SimHash's Hamming distance correlates with cosine similarity, and how you'd find near-dups among *billions* of 64-bit fingerprints in sub-linear time. (Expect: SimHash maps a document's weighted feature vector to a 64-bit fingerprint where Hamming distance ∝ angular/cosine distance; MinHash estimates Jaccard set similarity via min-of-permutation hashes — better for set overlap, SimHash cheaper for text. For billion-scale lookup within Hamming distance k, use the **Charikar/Manku "HTTP-scale" table trick**: store the fingerprints in several sorted tables permuted so that a small-Hamming-distance match shares a high-order block, giving near-O(1) probes — the classic Google near-dup paper.)

**H2.** Cross-cutting — legal/ethical: robots.txt is advisory, not law; some sites forbid crawling in ToS; GDPR "right to be forgotten" and copyright/paywalls apply; and you must not become a DDoS. How do these reshape the design? (Expect: honor robots.txt + `noindex` + `nofollow` + canonical as hard rules; per-host and per-IP rate caps well below the host's capacity; identify yourself via `User-Agent` with a contact URL; honor `Retry-After`; support takedown/opt-out lists checked at index time; region-aware crawling for data-residency; back off hard on 429/503 to avoid being an accidental DoS.)

**H3.** Operational — how do you deploy a new parser or re-prioritize the frontier across a running 10,000-machine crawl without losing frontier state or double-crawling? (Expect: frontier state persisted (RocksDB/Kafka) independent of stateless fetcher/parser workers → rolling-restart workers freely; version the parser and canary it on a % of hosts; leases + idempotent complete make in-flight loss safe; re-prioritization is a config/scoring change applied to the front-queue scorer, not a data migration; blue-green the frontier service reading the same persistent store.)

**H4.** Observability — what tells you the crawl is *unhealthy* (not just "fetchers are up")? (Expect: fetch success rate by HTTP status (spike in 429/403 = you're being throttled/banned), fetch freshness lag vs target per priority tier, frontier size trend (unbounded growth = trap), unique-hosts-in-flight vs fetch rate (politeness-bound throughput), dedup/near-dup rate, DNS/robots cache hit rates, bandwidth per fetch, per-host ban detection. Alert on ban-rate and frontier explosion, not just liveness.)

**H5.** 'Undo a bad decision' — you originally sharded the frontier **by url_hash** (uniform, simple) and now per-host politeness requires expensive cross-shard coordination (a host's URLs are scattered everywhere). Migrate to shard-by-host live. (Expect: recognize url_hash sharding scatters a host across all shards → politeness needs a global per-host lock/coordinator, which throttles throughput and is fragile; migrate to consistent-hashing-by-host so each host is single-owner; do it by draining/rehashing shard-by-shard while a routing layer sends new discoveries to the new scheme and old shards finish their in-flight work; run both mappings during transition, verify no politeness violations via ban-rate metrics, then cut over. Emphasize reversibility and measuring the ban rate before/after.)

---

### Mentor's Closing Notes

**Top 3 things most candidates get wrong on this topic:**
1. **Ignoring politeness as a first-class constraint.** They build a fast fetcher and a priority queue and never structure the frontier *by host*, so their design would hammer big sites, get IP-banned, and can't actually sustain its target throughput (which is host-bound, not compute-bound).
2. **Re-downloading unchanged pages.** They design a re-crawl loop with no conditional GET / ETag / content-hash, wasting the majority of bandwidth on pages that didn't change — the single biggest efficiency miss.
3. **Conflating URL dedup with content dedup, and forgetting traps.** They Bloom-filter URLs and call dedup done, missing that (a) the same content lives under millions of URLs (needs SimHash near-dup) and (b) spider traps generate infinite *genuinely-new* URLs that dedup can't catch (needs depth/host caps + anomaly detection).

**The one insight that makes an answer truly impressive:**
Recognize that **crawler throughput is fundamentally politeness-bound, not compute-bound** — your ceiling is (number of distinct hosts you can keep in polite rotation) × (per-host rate), so the entire architecture (shard-by-host, two-tier Mercator frontier, per-IP politeness for multi-IP hosts) exists to maximize the count of hosts you can serve concurrently while never violating any single host's crawl-delay. Candidates who frame it this way have understood the actual problem.

**Suggested follow-up reading:**
- Manku, Jain, Das Sarma — "Detecting Near-Duplicates for Web Crawling" (SimHash at Google scale).
- The Mercator paper (Heydon & Najork, "Mercator: A Scalable, Extensible Web Crawler") and Cho & Garcia-Molina on crawl freshness/refresh policies; the Common Crawl architecture posts.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
