# System Design Mentor — Daily Session
**Date:** 06-Aug-2026
**Lesson:** 35 of 63 — Phase 2: System Design Track (Design 7 of 35)
**Topic:** Search Autocomplete / Typeahead
**Level:** SDE2/SDE3 | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: All foundations were taught in Phase 1. Part 1 is a RECAP, not a re-teach. Part 2 is a rigorous interview: no hand-holding, no filler — but expected answers must still be detailed enough to learn from.

## Opening Brief
Autocomplete is the feature users judge you by without ever thinking about it: type "new y" and expect "new york," "new york times," "new years eve" ranked by popularity, in under ~100ms, on every keystroke. Google serves this at billions of queries/day; Amazon, YouTube, and Elasticsearch's completion suggester all solve variants. It's deceptively hard because you're rendering results *per keystroke* (a 10-char query = up to 10 requests) under a latency budget tighter than normal search, ranking by ever-shifting popularity, and doing it in dozens of languages with typos, personalization, and abuse (you must *not* suggest slurs or a competitor's data breach). The core tension: precompute everything for speed vs. keep it fresh, at read volume 10x a normal search engine.

---

## Part 1 — Prerequisite Recap
*Everything you need today was taught in Phase 1. Refresh it before the interview begins.*

**Tries / prefix trees (taught in Phase 1, Lesson 5):** A trie stores strings by shared prefix; each node is a character, and walking `n` nodes from the root locates all strings with that prefix. Lookup is O(prefix length), independent of dictionary size. The classic optimization is to **precompute and cache the top-k completions at each node** so a prefix query is a single node lookup, not a subtree traversal. This is the beating heart of typeahead.

**Heaps / top-k (taught in Phase 1, Lesson 5):** A min-heap of size k finds the top-k of a stream in O(n log k). We use it offline to compute the top-k most popular completions per trie node, and at query time to merge candidate lists.

**Caching / Redis (taught in Phase 1, Lesson 24):** Sub-millisecond reads, LRU eviction, high hit rates on skewed (Zipfian) key distributions. Autocomplete traffic is *extremely* skewed — a tiny set of prefixes ("a", "th", "fa") covers most requests — so a prefix→top-k cache with >95% hit rate is the primary serving path.

**LRU cache (taught in Phase 1, Lesson 4):** O(1) get/put with a hashmap + doubly linked list. The edge/CDN and in-memory prefix caches use LRU (or LFU) eviction; because traffic is Zipfian, the hot set is small and stable.

**Load balancers (taught in Phase 1, Lesson 6):** L7 LB routes requests; here we care about geo-routing to the nearest edge and consistent routing so a prefix's cache stays warm on one node. Autocomplete is read-dominated, so LB + horizontal read replicas scale it.

**MapReduce / Spark (taught in Phase 1, Lesson 15):** Batch aggregation of query logs into (query, count) and then top-k-per-prefix is a MapReduce/Spark job. The trie is *built offline* from aggregated logs, not mutated per query.

**Bloom filters (taught in Phase 1, Lesson 5):** Space-efficient probabilistic set membership with no false negatives. Used to cheaply reject prefixes we know have no completions, and as a first-pass filter for a blocklist (never-suggest terms) before an authoritative check.

**Kafka / streaming (taught in Phase 1, Lesson 21):** The query-log firehose flows through Kafka; a stream job maintains near-real-time counts (Count-Min Sketch) so trending terms ("earthquake") surface within minutes, not the next daily batch.

> **Recap box:**
> - Trie + **precomputed top-k per node** = O(prefix) lookup, no subtree scan at query time.
> - Min-heap = build top-k-per-node offline; merge candidates online.
> - Redis/edge LRU cache on Zipfian traffic = >95% hit, the real serving path.
> - MapReduce/Spark builds the trie offline from aggregated logs.
> - Kafka + Count-Min Sketch = near-real-time trending without a full rebuild.
> - Bloom filter = cheap "no completions" / blocklist first pass.

---

## Part 2 — The Interview Session
*Where natural, phrase questions as APPLICATIONS of Phase 1 lessons.*

### Warm-Up Questions (Easy)
*Baseline. A good SDE2 answers all without hesitation.*

Q1. You learned tries in Lesson 5. Why is a plain trie *not enough* to serve autocomplete, and what's the standard fix?
> **What a strong answer covers:** A plain trie locates the prefix node in O(prefix), but the completions live in the entire subtree below it — for prefix "a" that could be millions of strings, and you must then rank them by popularity. Traversing + sorting the subtree per keystroke blows the latency budget. Fix: **precompute the top-k (e.g., top 5–10) completions at every node** and store them on the node (or in a separate `prefix → [top-k]` map). Query becomes a single lookup. Trade-off: extra storage (each node caches k entries) and staleness (top-k is only as fresh as the last build).
> **Common weak answer:** "Use a trie, walk down to the node, DFS the subtree and return the words." No mention of ranking, of the subtree explosion for short prefixes, or of precomputation — this design times out on "a".
> **Mentor follow-up if they answer well:** Where do you store the top-k — on the trie node itself, or in a separate flat hashmap keyed by prefix string? (Flat `prefix → top-k` map, often just materialized into Redis, is what most production systems serve from; the trie is the build-time structure. Storing on nodes couples build and serve.)

Q2. Estimate the read QPS and the serving-data size for a search box with 500M DAU, each running 8 searches/day, average query 15 characters typed.
> **What a strong answer covers:** Searches/day = 500M × 8 = 4B. But autocomplete fires per keystroke (debounced): assume ~6 requests per query after debouncing 15 keystrokes → 24B autocomplete requests/day ≈ **278k req/sec average**, peak ~5x → ~1.4M req/sec. This is why autocomplete QPS dwarfs actual search QPS (~46k/sec). Serving data: suppose 100M distinct popular prefixes worth caching, each with top-10 completions ×~30 bytes = ~300 bytes → ~30GB of prefix→top-k data — fits in a Redis cluster / in memory. The full trie over, say, a 500M-query vocabulary is larger but the *hot serving set* is small because traffic is Zipfian.
> **Mentor follow-up:** Given 1.4M req/sec peak, what's your read path and where does it terminate? (Edge/CDN cache → regional Redis prefix cache → suggestion service reading precomputed store; >95% should never reach the service. The number forces you to make the cache the design, not an add-on.)

Q3. Should autocomplete return results with strong consistency (always the absolute latest popularity) or is eventual/stale acceptable? Justify.
> **What a strong answer covers:** Stale is not just acceptable, it's *correct*. Nobody notices if "taylor swift" is ranked #2 vs #1 for a prefix, or if a term that started trending 5 minutes ago isn't there yet. Autocomplete is a **read-optimized, precomputed, eventually-consistent** system: freshness on the order of minutes (streaming trends) to hours (batch rebuild) is fine, and this staleness is exactly what buys you the sub-100ms latency and massive read scale. The exceptions are *safety* (blocklist updates must apply fast) and *legal takedowns* (remove-now), which are handled by a fast override layer, not by making the whole system strongly consistent.
> **Red flag answer:** "We need the latest counts every keystroke, so query a live analytics DB per request." That's a real-time aggregation on the hot path at 1.4M QPS — guaranteed to be slow and expensive, and it buys correctness nobody needs.

---

### High-Level Design (Medium)
*Candidate drives. Components, data flows, protocols.*

Q4. Design the full autocomplete system: the offline build pipeline and the online serving path.
> **Key components expected:** Client (with debounce + local cache); CDN/edge cache; L7 LB + geo-routing; suggestion service (stateless, reads precomputed store); prefix→top-k store (Redis + backing trie/DB); **offline pipeline**: query-log collection → Kafka → aggregation (Spark/MapReduce, daily) → top-k-per-prefix build → trie/store publish; **near-real-time layer**: Kafka stream + Count-Min Sketch for trending, merged at serve time; blocklist/moderation service; personalization/ranking layer.
> **Architecture diagram (text):**
```
 ONLINE (read path, ~1.4M req/s peak)
  Client (debounce 100–200ms, local LRU)
     │ GET /suggest?q=new+y
     ▼
  CDN / Edge cache ──hit──▶ return top-k         (hottest prefixes)
     │ miss
     ▼
  L7 LB (geo-route) ──▶ Suggestion Service (stateless, autoscaled)
     │                        │  read prefix→top-k
     │                        ▼
     │                  Redis prefix cache ──miss──▶ Precomputed store (trie/KV)
     │                        │
     │                  merge: batch top-k  +  trending(CMS)  +  personalization
     │                        │  filter: blocklist (Bloom → authoritative)
     ▼                        ▼
   response (top 5–10, <100ms)

 OFFLINE (build path)
  Query logs ─▶ Kafka ─▶ Spark daily aggregation (query,count)
             │                     │
             │                     ▼  top-k per prefix (min-heap)
             │              Build trie / prefix→top-k  ─▶ publish (versioned) ─▶ Redis/store
             ▼
  Kafka stream ─▶ Count-Min Sketch (trending, minutes) ─▶ trending overlay
```
> **What separates SDE2 from SDE3 here:** An SDE2 builds one trie and serves from it. An SDE3 separates **build from serve** (the online path never mutates the trie), makes the serving store a flat `prefix→top-k` cache (the trie is a build artifact), **versions and atomically swaps** the published index (blue-green trie so a bad build can roll back), and layers a **streaming trending overlay** on top of the batch index so freshness doesn't require a full rebuild. They also budget latency per hop and put the debounce + client cache first because the cheapest request is the one never sent.

Q5. Trace what happens as a user types "n", "ne", "new", "new ", "new y" — five keystrokes.
> **Expected trace:**
> 1. Client **debounces** (~150ms): rapid typing "n","ne","new" in <150ms fires only one request for "new" — you do NOT send 5 requests for 5 keystrokes.
> 2. Client checks its **local cache**: if "new y" completions were fetched earlier this session, serve locally, zero network.
> 3. Request `GET /suggest?q=new+y` hits the **edge cache**; "new y" is extremely popular → cache hit, returns `["new york","new york times","new years eve",...]`.
> 4. On miss, suggestion service reads Redis `prefix:"new y" → top-k`. Hit → return.
> 5. On Redis miss (rare, cold prefix), service consults the precomputed store / walks the published trie to the "new y" node, reads its cached top-k, backfills Redis, returns.
> 6. Service merges batch top-k with the **trending overlay** (if "new years eve" is spiking in December, CMS bumps it) and applies the **blocklist filter** before responding.
> **Tricky part:** Candidates forget the **debounce + client cache**, so they design for 1.4M QPS when good client behavior cuts it dramatically; and they forget that "new " (with trailing space / word boundary) and typo prefixes ("nwe") need normalization/fuzzy handling, not exact prefix match.

Q6. Design the suggestion API and discuss what must be in the query params.
> **Expected API design:**
> ```
> GET /v1/suggest?q=<prefix>&lang=en&region=US&limit=10&session=<id>
>   → 200 { query:"new y",
>           suggestions:[ {text:"new york", score:0.98, type:"popular"},
>                         {text:"new york times", score:0.81}, ... ],
>           version:"idx-2026-08-06" }
> ```
> **What to push on:** **Latency & caching headers** — short `Cache-Control` so the CDN can cache per (q,lang,region) but not too long (trending). **`lang`/`region`** are cache-key dimensions (autocomplete is per-locale). **`limit`** small (5–10). **No pagination** — autocomplete is top-k only, never page 2. **Debounce is client-side**, but the API should be idempotent and cheap (pure read, GET, cacheable). **`session`/user** only if personalizing, and personalization must not destroy cacheability — usually applied as a light re-rank on top of the shared cached list, not a per-user cache. **Return the index `version`** for debuggability and cache-busting on rebuild.

---

### Data Modeling (Medium–Hard)

Q7. Design the data structures/stores: the serving store and the offline aggregation store.
> **Expected schema:**
```
-- Serving store (Redis) — flat, precomputed
KEY   suggest:{lang}:{region}:{prefix}      VALUE  [ {text,score}, ... top-k ]   (TTL/versioned)
-- Backing precomputed trie (published as an immutable artifact, versioned)
--   node: char, children map, top_k:[ (completion, score) ]

-- Offline aggregation (columnar / warehouse — e.g., BigQuery/Hive)
CREATE TABLE query_counts (
  query        string,
  lang         string,
  region       string,
  count_day    bigint,
  count_7d     bigint,        -- rolling window for recency-weighted popularity
  last_seen    date
);
-- derived
CREATE TABLE prefix_topk (
  prefix       string,
  lang         string,
  region       string,
  top_k        array<struct<text:string, score:double>>,
  index_version string
);
```
> **Index choices and why:** Serving is a **hash lookup** on the full prefix string (`prefix→top-k`) — O(1), no traversal — because we precomputed top-k. The trie exists at *build* time to compute each node's top-k by merging children's top-k bottom-up (a node's top-k ⊆ union of its children's top-k + terminal words), which is far cheaper than re-aggregating the subtree per prefix. `prefix_topk` is keyed by `(prefix, lang, region)`.
> **Partitioning key and why:** Partition the serving cache by **prefix** (hash) with consistent hashing so hot prefixes spread and a prefix's data is co-located. Partition the offline `query_counts` by `(lang, region, date)` for pruned aggregation. Crucially, **shard by prefix, not by user** — the data is shared across users; personalization is a thin overlay.

Q8. How do you rank completions — and how do you make "recently trending" beat "historically popular" without rebuilding the whole trie constantly?
> **Expected answer:** Base score = recency-weighted popularity, e.g., `score = α·log(count_7d) + (1-α)·log(count_all_time)` with time decay so last week matters more than last year. Compute offline into `prefix_topk`. For **trending**, run a Kafka stream job maintaining a **Count-Min Sketch** of recent query counts (minutes-scale window); at serve time, merge the batch top-k with the trending candidates and re-rank — a term spiking now (an earthquake, a product launch) is injected without waiting for the daily build. Optionally boost by CTR (which suggestion users actually click) and personalization (user's own history) as re-rank signals on the shared list.
> **Trap:** Ranking purely by all-time count → "facebook" always beats a breaking-news term; the box feels dead. Or the opposite: rebuilding the full trie every few minutes to stay fresh → enormous compute and index churn. The right answer is **batch base + streaming overlay**, not one or the other.

Q9. Autocomplete is eventually consistent by design (Q3). Name the two places where eventual consistency is *dangerous* and how you handle them.
> **Expected answer:** (1) **Blocklist / safety takedowns** — if a term must never be suggested (slurs, a leaked-data query, a court-ordered removal), you cannot wait for the next daily build. Handle via a **fast override layer**: a small, strongly-consistent blocklist/denylist checked at serve time (Bloom filter first pass → authoritative set), pushed globally in seconds via a config/feature-flag channel, applied *after* candidate generation so it overrides any stale index. (2) **Personalization / privacy** — a user's private queries must not leak into the *global* shared suggestions (one user typing their password or SSN must never become a public completion). Handle by never feeding raw per-user logs into the global index without a **popularity threshold** (a query must be issued by ≥N distinct users before it's suggestable — k-anonymity), which also prevents PII leakage.
> **Mentor pushback:** "Your daily build is eventually consistent, fine — but a journalist publishes at 9 AM that your box suggests a slur for a public figure. Your next build is at 2 AM." Answer: that's exactly why safety is *not* on the eventual-consistency path — the override layer removes it in seconds independent of the index build. The build being stale is fine; safety being stale is a headline.

---

### Low-Level Design (Hard)

Q10. The hardest sub-problem: computing top-k completions for *every* prefix efficiently over a 500M-query vocabulary, and serving it in <5ms. Design the build and the lookup.
> **Problem statement:** For each of ~hundreds of millions of prefixes, precompute the top-k most popular completions, cheaply enough to rebuild daily, and serve any prefix in a single fast lookup.
> **Naive solution:** For each prefix, scan all queries starting with it, sort by count, take top-k. O(prefixes × vocabulary) — quadratic, impossible.
> **Why naive fails at scale:** 500M queries × up to 15 prefixes each × sorting = astronomically expensive; and short prefixes ("a") each scan hundreds of millions of candidates. It won't finish in a day.
> **Expected optimal approach:** Build a trie once from the aggregated `(query, count)` set, then compute top-k **bottom-up in one post-order traversal**: a node's top-k is the k-way merge (via a size-k min-heap) of its children's top-k lists plus any word terminating at this node. Because a node's completions are exactly the union of its descendants', the child top-k lists already contain every candidate that could be in the parent's top-k — so merging k-sized lists (not full subtrees) suffices. Total cost ≈ O(total nodes × k log(children)). Then flatten `node → top-k` into a `prefix→top-k` KV map published to Redis. Serving is a single O(prefix) hash lookup / trie walk.
> **Pseudo-code or class diagram:**
```
def build_topk(node):                       # post-order over the trie
    heap = MinHeap(size=k)
    if node.is_word:
        heap.push((node.word, node.count))
    for child in node.children.values():
        child_topk = build_topk(child)       # each already ≤ k, sorted
        for (word, cnt) in child_topk:        # merge candidates
            heap.push_if_better((word, cnt))  # keeps only top-k
    node.top_k = heap.sorted_desc()
    return node.top_k

# serve
def suggest(prefix):
    node = walk(root, prefix)                 # O(len(prefix))
    if node is None: return []                # Bloom filter can short-circuit
    return node.top_k                         # precomputed, O(1)
```
Key point: correctness relies on the **monotonic containment** — the parent's top-k is drawn only from its children's top-k unions, so we never re-scan subtrees.

Q11. Concurrency / consistency during an index rebuild: the daily build produces a new trie while millions of queries are being served from the old one. How do you swap without serving partial/corrupt results or a latency spike?
> **Scenario:** If you mutate the live trie in place, a reader mid-walk can see a half-updated node (some children new, some old) → garbage or missing suggestions; and rebuild write-locks would stall reads at 1.4M QPS.
> **Expected fix:** **Immutable, versioned, atomic swap (blue-green index).** Build the new index entirely off to the side as `idx-2026-08-06`, publish it fully, then atomically flip a single pointer/alias (`current_index → new`) that serving nodes read — an O(1) pointer swap, no reader ever sees a partial index. Load the new index into serving-node memory / a new Redis keyspace *before* the flip (warm it), so there's no cold-cache latency cliff at swap time. Keep the previous version live for a grace period so in-flight requests finish and you can **instantly roll back** if the new build is bad (e.g., a bug dropped half the vocabulary). This is copy-on-write / read-copy-update semantics at the index level.
> **Follow-up — what if the new index build is subtly wrong and you don't notice until after the swap?** Validation gates before swap: compare index size/coverage vs previous (alert if vocabulary dropped >X%), run a golden-set of canary prefixes and assert expected suggestions, and keep the prior version hot for instant rollback via the alias flip.

Q12. Edge case / failure deep-dive: fuzzy matching and typos. The user types "nwe yrk" (transposed) — a strict prefix trie returns nothing. How do you handle typos without destroying latency?
> **Scenario:** ~10–15% of queries contain typos. A pure prefix match returns empty for "nwe", a terrible experience; but full edit-distance search over the vocabulary per keystroke is far too slow for the latency budget.
> **Expected handling:** Don't do live edit-distance over the whole trie. Options, layered: (1) **Precompute common misspellings** offline — mine the query logs for `typo → correction` pairs (users who typed "nwe york" then "new york" within the session reveal corrections) and add high-value typo prefixes into the serving map. (2) **Bounded fuzzy match** at serve time using a **BK-tree** or a **Levenshtein automaton** (as Elasticsearch/Lucene's `FuzzySuggester` does) limited to edit distance ≤1–2 and only when the strict prefix returns too few results — kept fast because the candidate set is pruned by prefix. (3) **Phonetic / keyboard-adjacency** normalization for the first character to avoid empty results. The governing principle: strict prefix is the fast common path; fuzzy is a bounded fallback triggered only on sparse results, never on every keystroke.

---

### Scaling to 10x / 100x (Hard)

Q13. You 100x to ~14M autocomplete req/sec peak. Where does it break first, and what's the number?
> **Expected answer:** Not the trie or the DB — those are read-only and cacheable. It breaks at the **cache/serving fan-out and the network edge**: 14M req/sec of tiny requests is a *connection and request-rate* problem, dominated by per-request overhead, not data volume. The fix is to push the hit ratio as close to 100% at the **edge/CDN** as possible (the Zipfian distribution means a few million hot (prefix,lang,region) keys serve the vast majority) and to make the origin path trivial. Signal: edge cache hit-rate and origin QPS — if origin QPS climbs above your provisioned Redis/service capacity, your hot-set caching is insufficient. Secondary break: the **offline build** — as vocabulary and languages grow, the daily Spark build may not finish in its window; you shard the build by (lang, prefix-range) and parallelize. Grounding: with a 97% edge hit rate, 14M req/sec → only ~420k req/sec reach origin — comfortably servable by a sharded Redis; at 90% hit rate it's 1.4M and painful. The **cache hit rate is the whole ballgame.**
> **Numbers to ground the answer:** hot set ~few million keys × ~300B = ~1GB per edge PoP (trivially cacheable); origin QPS = total × (1 − hit_rate) — this single equation drives capacity.

Q14. How do you shard the serving store, and where's the hot spot?
> **Expected sharding strategy:** **Hash by (lang, region, prefix)** with consistent hashing + vnodes (Lesson 19) so adding capacity reshuffles ~1/N of keys and each prefix's top-k is on one shard → single-shard reads, no scatter-gather. Sharding by prefix (not user) is correct because data is shared.
> **Hot spot problem:** Short/common prefixes — "a", "th", "s" — are requested orders of magnitude more than long ones, so their shard gets hammered (a single-key hot spot). Detect via per-key/per-shard QPS metrics. Fix: (1) these hottest keys should essentially always be served from the **edge/CDN and client cache**, never reaching a shard; (2) **replicate hot keys** across multiple shards / all serving nodes (they're tiny and read-only — replicate the top few thousand prefixes everywhere); (3) since the value is immutable between builds, aggressive multi-layer caching makes the hot spot harmless. This is a read-only, replicate-the-hot-set problem, not a write-contention one.

Q15. Design the caching layers and name the invalidation trap unique to autocomplete.
> **Expected layered cache design:** **L0** client-side local cache + debounce (the cheapest request is the one not sent). **L1** CDN/edge cache keyed by (q, lang, region), short TTL (seconds–minutes) so trending stays fresh. **L2** in-process LRU on serving nodes for the hottest prefixes. **L3** Redis prefix cache (sharded). **L4** the published immutable trie/store as backstop. Warm L1–L3 for the hot set right after each index swap.
> **Cache invalidation trap:** Autocomplete caches are keyed by *prefix*, but a single vocabulary change (a term's popularity jumps, or a term gets blocklisted) affects the top-k of **many prefixes at once** — "taylor" newly appearing changes top-k for "t", "ta", "tay", "tayl"... So invalidation isn't one key; it's a *family* of prefixes. Handle it by **versioning the whole index** and swapping atomically (invalidate by bumping the index version in the cache key / flipping the alias) rather than trying to surgically evict individual affected prefixes — which is error-prone and would miss some. For the safety/blocklist case, the override layer applies *after* the cache read, so you don't need to invalidate the cache at all to suppress a term.

Q16. Cost/efficiency at scale. Where's the spend and how do you cut it?
> **Expected answer:** Cost is dominated by (1) **edge/CDN + serving fleet** for the massive read QPS and (2) the **offline build** compute. Cuts: maximize **cache hit rate** (every extra point of hit rate directly removes origin cost — client debounce, edge caching, hot-key replication); **compress** the serving payload (top-k of short strings; store scores as quantized ints, share a string dictionary); **prune the vocabulary** — you don't need top-k for prefixes nobody types, so only materialize prefixes with count above a threshold (long-tail prefixes fall back to a live trie walk, rare). For the build: **incremental builds** — most of the vocabulary is stable day to day, so rebuild only changed subtrees rather than the whole trie; shard the Spark job by prefix-range. **Tier by language/region** — don't hold every locale hot in every PoP; serve rare locales from origin. Autoscale serving on QPS (diurnal), and let the CDN absorb the peak.

---

### Mentor's 5 Hardest Questions (SDE3+ Differentiators)

**H1.** Deep internals — Count-Min Sketch for trending: explain why you'd use a CMS over an exact hashmap of counts for the streaming layer, its error bounds, and how you'd combine it with the batch top-k without double-counting. (Expect: CMS gives sublinear memory for counting a firehose with bounded over-estimate ε with probability 1−δ, tunable via width/depth = ceil(e/ε) × ceil(ln(1/δ)); never under-counts, so it's safe for "is this trending"; you use it to *detect* spikes and inject candidates, then let the next batch build absorb them — the overlay is a delta, and you avoid double-count by treating batch as base truth and CMS as a bounded-window recency signal, not summing them.)

**H2.** Cross-cutting — internationalization: CJK languages have no spaces and huge character sets; Arabic is RTL; some languages need transliteration ("namaste" → Devanagari). How does this reshape the trie and tokenization? (Expect: per-locale indices and tokenizers; CJK needs character/segment-level tries and often input-method-editor awareness (romaji→kanji); normalization (Unicode NFC, case/diacritic folding) before trie insertion; separate ranking per locale; the "prefix" concept differs — you may index on transliterated forms too.)

**H3.** Operational — rolling out a new ranking model or index format without downtime and with the ability to A/B test. (Expect: versioned indices + alias flip (from Q11); serve N% of traffic from the new ranking via a feature flag / traffic split keyed by session; measure CTR / query-abandonment as the online metric; blue-green the serving fleet; instant rollback by flipping the alias back; shadow-serve the new index and diff results before going live.)

**H4.** Observability — what tells you autocomplete quality regressed (not just that it's up)? (Expect: online quality metrics — suggestion CTR, mean reciprocal rank of the clicked suggestion, "no results" rate, query-abandonment/refinement rate; latency p50/p95/p99 per hop; cache hit rate per layer; index coverage/size per build with alerts on drops; per-locale dashboards; canary golden-prefix assertions; distributed tracing. A latency graph alone won't catch "we started returning garbage.")

**H5.** 'Undo a bad decision' — you originally personalized by keeping a **per-user autocomplete cache**, and it's destroyed your cache hit rate and blown up memory. Migrate to shared-cache + re-rank without regressing personalization. (Expect: recognize per-user cache defeats the Zipfian shared-cache win — hit rate craters; migrate to a single shared `prefix→top-k` cache plus a *lightweight per-user re-rank* applied at serve time using a small per-user signal (recent history in a compact store); run both in parallel, compare CTR to prove personalization quality holds, then cut over; measure cache-hit-rate recovery. Emphasize: personalization belongs as a thin re-rank over shared data, never as a cache-key dimension.)

---

### Mentor's Closing Notes

**Top 3 things most candidates get wrong on this topic:**
1. **Traversing the subtree at query time.** They build a trie and DFS below the prefix node per keystroke — forgetting to precompute top-k per node. This times out on short prefixes and misses that ranking, not lookup, is the hard part.
2. **Designing for raw keystroke QPS.** They never mention client-side debounce or client/edge caching, so they over-engineer the backend for 10x the traffic that good client behavior eliminates. The cheapest request is the one never sent.
3. **Conflating freshness with consistency.** They either serve stale forever (dead-feeling box, no trending) or try to be real-time on every keystroke (a live analytics query at 1.4M QPS). The right model is batch base + streaming overlay + a fast *safety* override — and they forget the safety override entirely.

**The one insight that makes an answer truly impressive:**
Separate the three time-scales explicitly: **batch** (hours) for the popularity base index, **streaming** (minutes) for trending via Count-Min Sketch, and **instant** (seconds) for the safety/blocklist override — each on its own path, merged at serve time. Most candidates pick one clock for the whole system; the senior move is recognizing autocomplete needs three, and that safety must never sit on the slow one.

**Suggested follow-up reading:**
- Lucene/Elasticsearch completion suggester internals (FST-based suggesters) and the `FuzzySuggester`.
- Google's research/blog on query autocompletion and privacy (k-anonymity thresholds for suggestions); Cormode & Muthukrishnan's Count-Min Sketch paper.

---

## How to Use This Session
1. **Recap first:** Read Part 1; revisit any Phase 1 lesson you can't restate.
2. **Solo mode:** Answer each Part 2 section, then read the expected answer. Grade yourself.
3. **Interactive mode:** Paste into a new Claude chat: 'You are Arjun Mehta. I am your student. Start with Q1, don't reveal expected answers — ask one at a time, push back on weak answers.'
4. **Mock interview mode:** Timer on. Answer Q4–Q15 in 45 minutes, then review.

---
QUALITY BAR: questions specific and non-generic — tailored exactly to THIS system. Expected answers include real algorithms, data structures, specific failure modes, real numbers. Cross-reference Phase 1 lesson numbers. Write as Arjun Mehta — direct, rigorous, no fluff.
