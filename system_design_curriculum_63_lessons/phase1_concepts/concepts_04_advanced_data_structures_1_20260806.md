# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 4 of 63 — Phase 1: Foundations (Module 4 of 28)
**Module:** Advanced Data Structures I
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
Every large system you will design in Phase 2 is, underneath, three or four data structures wearing a trench coat. When Redis evicts your least-used session, that's an LRU cache. When Google Maps or Uber computes a route or an ETA, that's a graph plus Dijkstra (or its cousin A*). If you cannot reason about *why* an LRU get is O(1) or *why* Dijkstra refuses to work with negative edges, you will hand-wave in an interview and the interviewer will know instantly. Today we build the muscle: LRU cache internals, graph representations, and Dijkstra worked out by hand with real numbers.

## Learning Objectives
By the end of this lesson you can:
- Implement an LRU cache with O(1) `get` and `put` and explain exactly why each operation is constant time.
- Choose between an adjacency list and an adjacency matrix using a density calculation, not vibes.
- Trace Dijkstra's algorithm step by step with a priority queue, including how stale entries are handled.
- State the exact time complexity of Dijkstra with a binary heap and why it is `O((V+E) log V)`.
- Explain why Dijkstra breaks on negative edges and name the algorithm you'd use instead.

## The Lesson

### LRU Cache Internals (hashmap + doubly linked list, O(1) get/put)
**What it is (plain English):** An LRU (Least Recently Used) cache is a fixed-size store that, when full, throws out the item nobody has touched in the longest time. Think of a small desk that fits 5 books: to make room for a 6th, you return the book you haven't opened in the longest while.

**The problem it solves:** Memory (RAM) is ~100x faster than SSD and ~100,000x faster than spinning disk, but you can't fit everything in RAM. You want the *hot* subset in memory. LRU is a cheap, effective heuristic for "keep what's likely to be reused."

**How it works (mechanics):** The trick is combining two structures so both lookup and recency-update are O(1):
- A **hashmap** `key → node` gives O(1) lookup of where a key lives.
- A **doubly linked list** orders nodes by recency: most-recently-used at the head, least at the tail. Because it's doubly linked, you can unlink any node in O(1) (you have both neighbors' pointers).

```
HEAD (MRU) <-> [k2] <-> [k5] <-> [k1] <-> TAIL (LRU)
                 ^
   hashmap: {k2:•, k5:•, k1:•}  (• = pointer into list)
```
`get(k5)`: hashmap finds the node in O(1) → unlink it → move to head → return value. `put(k9, v)` when full: create node at head, insert into map; if size > capacity, remove the tail node **and** delete its key from the map. Every step is pointer surgery — no scanning. That's why both are O(1). A common bug: forgetting to delete the evicted key from the hashmap, which leaks memory forever.

**Trade-offs / when NOT to use it:** LRU has no notion of frequency — a single scan of a million cold rows can flush your genuinely hot data ("cache pollution"). For scan-heavy workloads, **LFU** or **ARC** (adaptive) do better. LRU also needs the doubly linked list rewrite on *every* read, which adds write pressure under high concurrency (you need locking or lock-free tricks).

**Where you'll see it:** Redis offers `allkeys-lru` and an approximate LRU (it samples a few keys rather than maintaining a perfect list, to save memory). Your OS page cache and CPU caches use LRU-like policies. In Phase 2, Lesson 31 (Distributed Cache) and Lesson 24 (Caches) build directly on this.

### Graph Data Structure & Representations
**What it is (plain English):** A graph is a set of **vertices** (nodes) connected by **edges**. Edges can be directed (one-way, like Twitter follows) or undirected (mutual, like Facebook friends), and can carry a **weight** (distance, cost, latency).

**The problem it solves:** Any time relationships matter — road networks, social graphs, dependency chains, service call maps — a flat table can't express "reachability" or "shortest path." Graphs make those first-class.

**How it works (mechanics):** Two dominant representations, and picking wrong wastes memory or time.

*Adjacency matrix:* a V×V grid where `M[i][j] = weight` (or 1/0) if an edge exists.
```
     A  B  C
  A [0  4  2]
  B [0  0  0]
  C [0  1  0]     (A→B=4, A→C=2, C→B=1)
```
Edge lookup "is there A→B?" is O(1), but it always costs `V²` space. For V=1,000,000 that's 10¹² cells — impossible.

*Adjacency list:* each vertex stores a list of its neighbors.
```
A -> [(B,4),(C,2)]
B -> []
C -> [(B,1)]
```
Space is O(V+E). Real-world graphs are **sparse** (E ≪ V²) — a social user follows maybe 300 of 400M people — so the list wins overwhelmingly.

**Decision rule with numbers:** density d = E / V². If d is high (dense, d → 1, e.g., a small complete graph), the matrix is fine and cache-friendly. If E ≈ V or E ≈ 10V (sparse), use a list. Example: V=10,000, E=50,000 → matrix = 100,000,000 cells; list ≈ 60,000 entries. The list is ~1,600x smaller.

**Trade-offs / when NOT to use it:** Matrix wastes space on sparse graphs but gives O(1) edge checks and trivial matrix-math (good for algorithms like Floyd-Warshall). Lists are compact but "does edge A→B exist?" is O(degree). 

**Where you'll see it:** Google Maps stores the road network as an adjacency list (Lesson 53). LinkedIn's degrees-of-connection (Lesson 46) is graph traversal. Kubernetes and build tools model dependencies as DAGs.

### Dijkstra's Algorithm (with a fully worked example)
**What it is (plain English):** Dijkstra finds the shortest path from one source vertex to all others in a graph with **non-negative** edge weights. It's a greedy algorithm: repeatedly finalize the closest not-yet-finalized vertex, because with no negative edges, once a vertex is the closest unvisited one, no cheaper path to it can exist.

**The problem it solves:** "Fastest route from my location to everywhere" — routing, network packet paths (OSPF uses it), ETA computation.

**How it works (mechanics):** Maintain `dist[]` (best known distance, init source=0, rest=∞) and a **min-priority-queue** keyed by distance. Repeatedly pop the smallest, and **relax** each outgoing edge: if `dist[u] + w(u,v) < dist[v]`, update `dist[v]` and push `(new_dist, v)`.

Graph (directed, source = **A**):
```
A→B=4   A→C=2   C→B=1   B→D=5
C→D=8   C→E=10  D→E=2   D→F=6   E→F=3
```
Trace (PQ shown; ✗ = stale/skipped duplicate):

| Step | Pop (dist) | Relaxations | dist after |
|------|-----------|-------------|------------|
| init | —         | —           | A0 B∞ C∞ D∞ E∞ F∞ |
| 1 | A(0) | B=4, C=2 | A0 B4 C2 D∞ E∞ F∞ |
| 2 | C(2) | B: 2+1=3<4 ✓, D=10, E=12 | A0 B3 C2 D10 E12 F∞ |
| 3 | B(3) | D: 3+5=8<10 ✓ | A0 B3 C2 D8 E12 F∞ |
| 4 | B(4)✗ | already finalized at 3 → skip | — |
| 5 | D(8) | E: 8+2=10<12 ✓, F: 8+6=14 | A0 B3 C2 D8 E10 F14 |
| 6 | E(10) | F: 10+3=13<14 ✓ | A0 B3 C2 D8 E10 F13 |
| 7 | F(13) | no out-edges | done |

**Final shortest distances from A:** A=0, C=2, B=3, D=8, E=10, F=13.
**Path to F:** A→C→B→D→E→F = 2+1+5+2+3 = **13** (beats A→C→B→D→F = 14).

**Complexity:** With a binary heap, each of E edges may cause one push (O(log V)) and each of V vertices one pop (O(log V)): total **O((V+E) log V)**. A Fibonacci heap gives O(E + V log V) in theory but is rarely used in practice due to constants.

**Trade-offs / when NOT to use it:** Dijkstra **fails with negative edges** — its greedy "finalize the closest" assumption breaks, because a later negative edge could make a "finalized" vertex cheaper. Use **Bellman-Ford** (O(V·E)) for negative edges, or **A\*** (Dijkstra + a heuristic) when you have one target and a good distance estimate (Maps uses A*). For all-pairs on dense graphs, **Floyd-Warshall** (O(V³)).

**Where you'll see it:** OSPF/IS-IS routing protocols, Google Maps' base layer before heuristics, network latency-aware load balancing.

## Comparison Table

| Dimension | Adjacency Matrix | Adjacency List |
|---|---|---|
| Space | O(V²) | O(V + E) |
| Edge exists? (A→B) | O(1) | O(degree(A)) |
| Iterate neighbors of A | O(V) | O(degree(A)) |
| Best for | Dense graphs, matrix algos | Sparse graphs (most real ones) |

**Verdict:** Real systems are sparse — default to the adjacency list; reach for the matrix only when the graph is small-and-dense or the algorithm needs matrix math.

## Common Misconceptions
- **Myth:** LRU eviction requires scanning to find the oldest item. → **Reality:** The tail of the doubly linked list *is* the oldest; removal is O(1).
- **Myth:** A hashmap alone can build an LRU cache. → **Reality:** A plain hashmap has no ordering; you need the linked list (or an ordered map) to know recency. Java's `LinkedHashMap` bundles both.
- **Myth:** Dijkstra works on any weighted graph. → **Reality:** Only non-negative weights. Negative edges require Bellman-Ford.
- **Myth:** You must decrease-key in the heap. → **Reality:** The common "lazy" version just pushes a new entry and skips stale pops — simpler and fast enough.
- **Myth:** Adjacency matrices are always wasteful. → **Reality:** For a dense graph or one needing O(1) edge checks (e.g., Floyd-Warshall), the matrix is the right call.

## Real-World Case
In 2015, engineers debugging a large-scale caching tier found a service intermittently thrashing: a nightly batch job scanned millions of rarely-accessed rows, and because the cache used strict LRU, that one-time scan evicted the genuinely hot working set — every morning latency spiked as the cache slowly re-warmed. The fix wasn't more RAM; it was policy. They moved scan traffic to a bypass path (don't cache single-touch scans) and, for the main tier, adopted a frequency-aware policy so a single sweep couldn't dethrone hot keys. Lesson: the *eviction policy* is a design decision, not a default. Redis later shipped an `lfu` mode for exactly this class of workload.

## Self-Test (answers at the bottom)
1. Why is `get` on a well-implemented LRU cache O(1) and not O(n)?
2. For a graph with V = 100,000 and E = 400,000, which representation do you pick and roughly how much space does each cost?
3. In the Dijkstra trace above, why was the entry B(4) skipped at step 4?
4. You add an edge with weight −3 to the example graph. What breaks, and which algorithm do you switch to?
5. Design sketch: You're building a session cache for 50M daily users, ~2KB per session, holding 10M hot sessions in RAM. Which structure and eviction policy, and roughly how much RAM? What one change would you make if traffic is scan-heavy analytics reads?

## Interview Soundbites
- "LRU is O(1) both ways because a hashmap gives O(1) lookup and a doubly linked list gives O(1) reordering and O(1) tail eviction — neither operation scans."
- "Real graphs are sparse, so I default to an adjacency list at O(V+E); I only use a matrix when the graph is dense or I need O(1) edge existence checks."
- "Dijkstra is greedy and correct only for non-negative weights; the moment there's a negative edge I reach for Bellman-Ford, and if I have a single target with a good heuristic, A*."

## Mini-Assignment
On paper (~30 min): (1) Draw the example graph and re-run Dijkstra from source **C** instead of A. List final distances to every vertex and the path to F. (2) Then write, in ~15 lines of pseudocode, an LRU cache class with `get(key)` and `put(key, value)` using a hashmap + doubly linked list, and annotate each line with its time complexity. Confirm every line is O(1).

## Recap & Tomorrow
- **LRU cache:** hashmap (O(1) find) + doubly linked list (O(1) reorder/evict) = O(1) get/put; pick the eviction policy deliberately.
- **Graph representations:** adjacency list O(V+E) for sparse (default); matrix O(V²) for dense / O(1) edge checks.
- **Dijkstra:** greedy shortest path with a min-heap, O((V+E) log V), non-negative weights only; Bellman-Ford for negatives, A* with a heuristic.

Tomorrow, **Lesson 5 — Advanced Data Structures II**: tries, heaps & top-K, Bloom filters, skip lists, and Merkle trees — the structures nearly every Phase 2 design leans on.

## Self-Test Answers
1. Because both the lookup (hashmap → node pointer) and the recency update (unlink the node and move it to the head of the doubly linked list) are constant-time pointer operations; nothing is scanned.
2. Adjacency list. It's sparse (E ≈ 4V). List ≈ 500,000 entries (V+E), whereas a matrix is 100,000² = 10¹⁰ cells — utterly infeasible. Pick the list.
3. Because B was already popped and finalized at distance 3 in step 3. The (4, B) entry is a stale leftover from the earlier relaxation via A; the algorithm sees B's final distance (3) is already ≤ 4 and skips it.
4. Dijkstra's greedy assumption breaks: a vertex "finalized" early could later be reached more cheaply through the negative edge, but Dijkstra never revisits it, yielding a wrong answer. Switch to Bellman-Ford (O(V·E)), which relaxes all edges V−1 times and can also detect negative cycles.
5. Hashmap + doubly linked list LRU (or Redis with `allkeys-lru`). 10M sessions × 2KB ≈ 20 GB of value data plus overhead for pointers/keys (~budget 30–40 GB). For scan-heavy analytics traffic, switch the main tier to a frequency-aware policy (LFU) or route scans through a no-cache bypass so a single sweep can't evict the hot working set.
