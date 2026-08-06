# System Design Mentor — Daily Lesson
**Date:** 06-Aug-2026
**Lesson:** 5 of 63 — Phase 1: Foundations (Module 5 of 28)
**Module:** Advanced Data Structures II
**Level:** Newbie → SDE2/SDE3 track | 60–150 LPA
**Mentor:** Arjun Mehta (40+ YOE)

---

## NOTE: The student is a newbie. Teach every concept from first principles — technical and detailed, but explained so the student truly understands it and can apply it later in the System Design Track.

## Why This Module Matters
The structures today are the ones that quietly power the internet's biggest systems. When you type "sys" into Google and it suggests "system design" in under 50 ms, that's a trie. When Cassandra decides *not* to hit disk for a key that doesn't exist, that's a Bloom filter. When Redis Sorted Sets rank a billion-row leaderboard, that's a skip list. When Git or Bitcoin verifies that a 100 GB dataset hasn't been tampered with by comparing one 32-byte hash, that's a Merkle tree. Miss these and half of Phase 2 (search, storage engines, blockchains, sync) will feel like magic instead of engineering.

## Learning Objectives
By the end of this lesson you can:
- Explain why a trie gives O(L) prefix lookup independent of how many words it stores, and when its memory cost bites.
- Use a heap to answer "top-K of a stream" in O(N log K) instead of O(N log N), and justify the K vs N-K heap choice.
- Compute a Bloom filter's false-positive rate for a target size and hash count, and state why it never gives false negatives.
- Describe how a skip list achieves O(log N) search with probabilistic levels, and why Redis chose it over a balanced tree.
- Explain how a Merkle tree localizes "which block differs" in O(log N) hashes.

## The Lesson

### Tries (Prefix Trees)
**What it is (plain English):** A trie is a tree where the *path from the root spells the key*, one character per edge. All words sharing a prefix share the same path until they diverge. Think of a phone directory tree: everything under "S → Y → S" is a prefix of "system," "syslog," "syntax."

**The problem it solves:** A hashmap answers "is this exact word present?" but cannot cheaply answer "give me all words starting with 'sys'." Scanning a million words for a prefix is O(N·L). A trie makes prefix queries O(L) where L is the query length — independent of dictionary size.

**How it works (mechanics):** Each node holds up to 26 child pointers (for a–z) and an `isEnd` flag. Inserting "cat," "car," "can":
```
        (root)
          |c
        [ c ]
          |a
        [ a ]
       /  |  \
      t   r   n
   (end)(end)(end)
```
Lookup "car": follow c→a→r, check `isEnd`. Three hops = O(L). Autocomplete "ca": walk to the "a" node, then DFS the subtree to collect "cat, car, can."

**Trade-offs / when NOT to use it:** Memory is the enemy. A naive node = 26 pointers × 8 bytes = 208 bytes even for one child. For 1M words averaging 8 chars that's easily hundreds of MB. Fixes: **radix/Patricia tries** compress single-child chains; **ternary search trees** use 3 pointers per node. Not worth it if you never do prefix queries — use a hashmap.

**Where you'll see it:** Google/IDE autocomplete, IP routing tables (longest-prefix match uses a trie), Elasticsearch's FST-based term dictionary. Phase 2 Lesson 42 (Typeahead/Search) builds on this.

### Heaps & Top-K
**What it is (plain English):** A binary heap is a complete tree where every parent is ≤ (min-heap) or ≥ (max-heap) its children. It's stored as a flat array — no pointers — so it's cache-friendly. It gives you the smallest/largest element in O(1) and insert/remove in O(log N).

**The problem it solves:** "What are the top 10 trending hashtags out of 500 million tweets?" Sorting everything is O(N log N) and needs all N in memory. A heap of size K answers it in O(N log K) with only K elements held.

**How it works (mechanics):** To find the **top-K largest**, keep a **min-heap of size K**. For each incoming element:
1. If heap has < K items, push it.
2. Else if element > heap.min (the root), pop the min and push the element.

The heap always holds the K biggest seen so far; its root is the "cutoff." Worked example, top-3 of stream [5, 1, 9, 3, 7, 2, 8]:
```
push 5,1,9 → heap{1,5,9}, min=1
3 > 1? yes → drop 1, push 3 → {3,5,9}, min=3
7 > 3? yes → drop 3, push 7 → {5,7,9}, min=5
2 > 5? no  → skip
8 > 5? yes → drop 5, push 8 → {7,8,9}
```
Result: top-3 = {7,8,9}. We touched 7 elements, heap never exceeded 3. Cost ≈ 7 × log 3.

**Trade-offs / when NOT to use it:** A heap gives you the top-K *set* but not fully sorted order (you sort the final K, O(K log K), cheap). It's not for range queries or "kth smallest with deletes" at scale — use a balanced BST or quickselect (O(N) average) for a one-shot kth element.

**Where you'll see it:** Dijkstra's priority queue (Lesson 4), Kafka/Spark top-N aggregations, OS process schedulers, Twitter trending.

### Bloom Filters
**What it is (plain English):** A Bloom filter is a compact bit array that answers "have I *probably* seen this element?" It can say "definitely no" or "maybe yes" — it never misses a real member (no false negatives) but occasionally lies "yes" for absent ones (false positives).

**The problem it solves:** Before an expensive lookup — a disk read, a network call — you want a cheap gate. If the filter says "definitely not present," you skip the costly operation entirely. It trades a little accuracy for enormous memory savings vs. storing the real keys.

**How it works (mechanics):** An m-bit array and k independent hash functions. **Insert x:** set bits h1(x), h2(x), …, hk(x) to 1. **Query x:** if *any* of those k bits is 0 → definitely absent; if *all* are 1 → probably present.
```
m=16 bits, k=3
insert "cat": h→ {2, 7, 11}   → set bits 2,7,11
query "dog": h→ {2, 7, 5}     → bit 5 is 0 → DEFINITELY NOT present
query "car": h→ {2, 11, 7}    → all 1 → "maybe" (collision! false positive)
```
False-positive rate ≈ (1 − e^(−kn/m))^k. For n=1M items, m=9.6M bits (~1.2 MB) and k=7, FP ≈ **1%**. Storing 1M actual 16-byte keys would cost 16 MB — the filter is ~13x smaller.

**Trade-offs / when NOT to use it:** You cannot delete (unsetting a bit could break another element) — use a **Counting Bloom filter** or **Cuckoo filter** if deletes matter. You can't retrieve elements, only test membership. Undersize it and FP rate explodes.

**Where you'll see it:** Cassandra/HBase/RocksDB use one per SSTable to skip disk reads for absent keys. Chrome's Safe Browsing, Bitcoin SPV wallets, CDN cache filters.

### Skip Lists
**What it is (plain English):** A skip list is a sorted linked list with express lanes. The bottom layer links every node; higher layers link a random subset, letting you "skip" ahead. It gives O(log N) search like a balanced tree, but with simple linked-list code and no rotations.

**The problem it solves:** A plain sorted linked list is O(N) to search. Balanced trees (red-black, AVL) give O(log N) but rotations are fiddly and hard to make concurrent. Skip lists get O(log N) probabilistically with far simpler, lock-friendly code.

**How it works (mechanics):** Each inserted node is promoted to the next level up with probability p=0.5 (coin flips). So ~1/2 the nodes reach level 1, ~1/4 reach level 2, etc. — roughly log₂N levels. Search starts top-left and drops down when the next node overshoots.
```
L2: 1 --------------------> 9
L1: 1 ------> 4 ----------> 9
L0: 1 -> 2 -> 4 -> 6 -> 7 -> 9   (search 6: 1→9? too big, drop; 1→4→9? too big, drop; 4→6 ✓)
```
Searching 6 among these visits ~3 nodes instead of 5. At N=1M, average search ≈ log₂(1M) ≈ 20 comparisons.

**Trade-offs / when NOT to use it:** O(log N) is *expected*, not guaranteed — a pathological run of coin flips could degrade it (astronomically unlikely). Extra pointers cost memory (~2 pointers per node average). For strict worst-case guarantees or ordered scans on disk, a B-tree is better.

**Where you'll see it:** Redis Sorted Sets (ZSET) use a skip list + hashmap for leaderboards. LevelDB's MemTable, Lucene's posting lists, java.util.concurrent's `ConcurrentSkipListMap`.

### Merkle Trees
**What it is (plain English):** A Merkle tree is a binary tree of hashes. Leaves are hashes of data blocks; each parent is the hash of its two children concatenated. The single root hash fingerprints the entire dataset — change one byte anywhere and the root changes.

**The problem it solves:** Comparing two large datasets (or verifying a download) by comparing them byte-for-byte is O(N) and needs both copies. A Merkle root lets two parties confirm "we're identical" by exchanging 32 bytes, and *locate* a difference in O(log N).

**How it works (mechanics):** 4 blocks D1–D4:
```
                Root = H(H12 + H34)
               /                    \
        H12 = H(H1+H2)        H34 = H(H3+H4)
         /        \             /        \
     H1=H(D1)  H2=H(D2)     H3=H(D3)  H4=H(D4)
```
If two nodes disagree on Root, compare H12 vs H34 → say H34 differs → compare H3 vs H4 → find D4 changed. For 1M blocks you find the bad block in log₂(1M) ≈ **20 hash comparisons**, not 1M. Anti-entropy repair (Cassandra) uses exactly this to sync replicas cheaply.

**Trade-offs / when NOT to use it:** Building the tree costs O(N) hashing up front; for tiny datasets a plain checksum is simpler. Rebalancing on inserts is awkward — best for fixed/append-mostly blocks. Security depends on a collision-resistant hash (SHA-256, not MD5).

**Where you'll see it:** Git commits (a commit is a Merkle DAG), Bitcoin/Ethereum block verification, Cassandra/DynamoDB anti-entropy repair, IPFS, BitTorrent piece verification.

## Comparison Table

| Dimension | Trie | Heap (top-K) | Bloom Filter | Skip List | Merkle Tree |
|---|---|---|---|---|---|
| Core question | prefix match | K largest/smallest | probable membership | sorted search | data equality/diff |
| Key op cost | O(L) | O(N log K) | O(k) per op | O(log N) exp. | O(log N) to locate diff |
| Space | high (pointers) | O(K) | tiny (bits) | O(N) + pointers | O(N) hashes |
| Exactness | exact | exact | approximate (FP only) | exact | exact (probabilistic hash) |

**Verdict:** Reach for the one whose *core question* matches yours — prefix, ranking, membership, ordered lookup, or integrity — not the one you happen to know best.

## Common Misconceptions
- **Myth:** Bloom filters can give false negatives. → **Reality:** Never. If a bit is 0, the element was never inserted. Only false *positives* happen.
- **Myth:** A trie is always faster than a hashmap. → **Reality:** For *exact* lookup a hashmap is O(1) vs the trie's O(L); the trie only wins for prefix/range queries.
- **Myth:** For top-K largest you use a max-heap. → **Reality:** You use a **min-heap of size K** so the smallest of your current top-K sits at the root as the cheap cutoff.
- **Myth:** Skip lists guarantee O(log N). → **Reality:** It's *expected* O(log N) from randomization; worst case is O(N) but vanishingly improbable.
- **Myth:** Merkle trees encrypt data. → **Reality:** They only *verify integrity* via hashing; they provide no confidentiality.

## Real-World Case
Cassandra's read path is a masterclass in Bloom filters. Each on-disk SSTable carries a Bloom filter in RAM. When a read for key `user:42` arrives, Cassandra checks each SSTable's filter *before* touching disk. If the filter says "definitely absent," that SSTable is skipped entirely — no disk seek. With dozens of SSTables per table and disk seeks at ~5–10 ms each, this turns a potential 10-seek read into 1. Operators who tuned `bloom_filter_fp_chance` too high (say 0.1) saw read latency balloon because the filters lied "maybe present" too often, forcing needless disk reads. Dropping it to 0.01 cost a bit more RAM but slashed p99 read latency. The lesson: a probabilistic structure's *parameter* is a real latency knob, not a default to ignore.

## Self-Test (answers at the bottom)
1. Why does a trie lookup cost O(L) regardless of how many words are stored?
2. To find the top-100 scores in a stream of 10 million, what heap (type and size) do you use, and what's the complexity?
3. A Bloom filter says "maybe present" for a key you never inserted. What is this called, and can it ever say "not present" for a key you *did* insert?
4. In a skip list of 1M elements with p=0.5, roughly how many levels and how many comparisons for a search?
5. Design sketch: Two data centers each hold 1 billion 4 KB records and must detect which records differ after a network partition, using minimal bandwidth. What structure, and roughly how many hash comparisons to isolate one differing record?

## Interview Soundbites
- "A Bloom filter is a fast, memory-cheap gate: it can be wrong only in the safe direction — false positives, never false negatives — so I use it to skip expensive lookups."
- "For top-K of a huge stream I keep a min-heap of size K, so I get O(N log K) time and O(K) space instead of sorting everything."
- "Redis picked a skip list for sorted sets because it gets O(log N) with lock-friendly, rotation-free code — simpler to make concurrent than a balanced tree."

## Mini-Assignment
On paper (~30 min): (1) Insert the words `["ten","tea","ted","ax","axe"]` into a trie, draw it, and mark every `isEnd`. Then trace the autocomplete for prefix `"te"`. (2) Compute a Bloom filter sizing: you need to hold 5,000,000 URLs at a 1% false-positive rate. Using m/n ≈ 9.6 bits/element and k=7, how many bits and megabytes does the filter need? Compare that to storing 5M raw 40-byte URLs.

## Recap & Tomorrow
- **Tries:** O(L) prefix lookup independent of dictionary size; watch the pointer memory (use radix/Patricia to compress).
- **Heaps & top-K:** min-heap of size K gives O(N log K) streaming top-K with O(K) memory.
- **Bloom filters:** tiny bit-array membership test; false positives only, never false negatives; tune the FP rate.
- **Skip lists:** probabilistic express lanes give expected O(log N) with simple, concurrent-friendly code (Redis ZSET).
- **Merkle trees:** hash tree fingerprints data; locate a differing block in O(log N) (Git, blockchains, anti-entropy repair).

Tomorrow, **Lesson 6 — Load Balancers**: round robin, weighted and least-connections, Layer-4 vs Layer-7, DNS load balancing, consistent-hashing LBs, and health checks — how one entry point fans traffic across a fleet without falling over.

## Self-Test Answers
1. Because you walk exactly L edges — one per character of the query — and each hop is a constant-time pointer follow. The number of *other* words in the trie doesn't add to the path length of your specific key, so it's O(L), not O(N) or O(N·L).
2. A **min-heap of size 100**. For each of 10M elements you compare against the root (O(1)) and possibly push/pop (O(log 100)). Total ≈ O(N log K) = 10M × log₂(100) ≈ 10M × 7 operations, with only 100 elements in memory.
3. That's a **false positive** — allowed and expected. It can *never* say "not present" for something you inserted, because insertion set all k of that key's bits to 1 and bits are never cleared; a query only reports absent if it finds a 0 bit. So no false negatives.
4. About log₂(1,000,000) ≈ 20 levels, and roughly the same order of comparisons — around 20 (a small constant factor times log N) to locate an element. Space adds ~2 pointers per node on average.
5. Build a **Merkle tree** over the 1 billion records in each DC and exchange root hashes. If roots differ, recursively compare child hashes down the tree. To isolate one differing record among 1 billion leaves takes log₂(10⁹) ≈ **30 hash comparisons**, sending only ~30 × 32-byte hashes rather than shipping terabytes. This is exactly Cassandra/Dynamo anti-entropy repair.
