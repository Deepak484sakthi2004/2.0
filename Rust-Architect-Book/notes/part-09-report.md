# Part 09 report

## SUMMARY.md lines

```markdown
- [Part IX Overview](part-09-collections/README.md)
  - [9.1 Vec<T>: Pointer, Length, Capacity](part-09-collections/ch01-vec.md)
  - [9.2 String and Text Encoding](part-09-collections/ch02-string.md)
  - [9.3 HashMap and HashSet](part-09-collections/ch03-hashmap.md)
  - [9.4 VecDeque, BTreeMap, BinaryHeap — and Why Not LinkedList](part-09-collections/ch04-deque-btree-heap.md)
  - [9.5 Choosing Collections by Cache Behavior](part-09-collections/ch05-cache-behavior.md)
  - [Interlude: The Trade-off Engine — BFS vs DFS, Down to the Stack Page](part-09-collections/interlude-bfs-dfs.md)
  - [Part IX Review](part-09-collections/review.md)
```

Appendix entry (wherever the other answer keys are listed):

```markdown
- [Answers: Part IX](appendix/answers-part-09.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| Vec layout (cap, ptr, len) seen in asm; field order [RUSTC], triple [LANG] | 9.1 | — |
| Vec growth max(2·cap, cap+1), min cap 8/4/1 by element size [LIB], verified + in `grow_amortized` asm | 9.1 | — |
| push fast path (compare/store/inc) vs cold `grow_one`; OOM in push = abort; `try_reserve` | 9.1 | XV (fallible alloc) |
| reserve vs reserve_exact; truncate/clear keep capacity; shrink_to_fit; capacity policies | 9.1 | XIII (buffer pools) |
| collect sizing: exact size_hint = 1 alloc; filter grows; in-place collect = 0 allocs | 9.1 | X (how the specialization works) |
| glibc realloc: in-place for small, mremap for ≥ mmap threshold (inferred, strace to confirm) | 9.1 | XX |
| arrays / slices / Box<[T]> / Vec / SmallVec sizes (verified) | 9.1 | — |
| String building: `s + &t` reuses buffer; `format!("{s}..")` quadratic; write! into buffer; format! capacity heuristic | 9.2 | XX |
| format_args! compile-time parsing; runtime Arguments + dyn Write | 9.2 | XVII/XVIII (macros, builtins) |
| UTF-16: encode_utf16, from_utf16 lone surrogates, from_utf16_lossy; OsString/WTF-8 on Windows | 9.2 | XVI (FFI strings) |
| JNI Modified UTF-8 vs FFM standard UTF-8 | 9.2 | XVI |
| Case mapping allocates, can change length (ß, ﬁ, İ, final sigma); ASCII variants in place; Kelvin sign | 9.2 | — |
| String vs Box<str> vs Arc<str> vs Cow<str> (sizes, clone costs verified); small-string crates | 9.2 | — |
| memchr/memchr3/memmem SIMD search (~10× measured); skip-search only pays for rare targets; 256-entry byte-class table | 9.2 | XX |
| SwissTable/hashbrown: control bytes, h1/h2, SIMD groups, 7/8 load, tombstones | 9.3 | — |
| HashMap capacity sequence, rehash-on-resize counts (verified with counting hasher) | 9.3 | — |
| entry vs get_mut+insert vs hashbrown entry_ref: hashes AND allocations (verified) | 9.3 | — |
| Hasher choice: SipHash (keyed), foldhash/ahash, FxHash; HashDoS (measured quadratic with Fx + crafted keys) | 9.3 | XXI (network input) |
| f64 keys → E0599 (bounds on impl block); OrderedFloat; integer minor units | 9.3 | — |
| Hash/Eq and Ord/Eq contracts = logic errors, not UB | 9.3, 9.4 | XV |
| Concurrent map options (Mutex/RwLock, sharding, ArcSwap, owner thread) | 9.3 | XI |
| VecDeque ring (head+len since 1.67), as_slices, make_contiguous | 9.4 | — |
| BTreeMap B=6, 11 keys/node, linear in-node search; BTree vs HashMap measured | 9.4 | — |
| BinaryHeap max-heap, Reverse, top-k, EDF, lazy deletion; derived Ord = field order | 9.4 | — |
| LinkedList: 1M allocs, 4.7×/47× slower traversal; cursors unstable on 1.98.1 (E0658 verified) | 9.4 | XV (intrusive lists) |
| Memory hierarchy rules: bytes touched, dependent loads, predictability; MLP; throughput ≠ 1/latency | 9.5 | XX |
| AoS vs SoA measured; Vec<Box<T>> in-order vs shuffled measured; lookup structures by n measured | 9.5 | XX |
| rustc field reordering; tuple padding | 9.5 | — |
| False sharing, CachePadded (mentioned) | 9.5 | XIV |
| Recursive frame size measured (224 B debug / 96 B release); overflow verified at 110% of prediction | Interlude | — |
| Guard page + sigaltstack SIGSEGV handler; stack probes (asm verified) | Interlude | XIX (binary/OS) |
| Explicit-stack DFS with (node, edge) frames; mark-on-push is not DFS (verified) | Interlude | — |
| BFS frontier width vs DFS depth (verified); shortest path property | Interlude | — |
| Stack sizes by platform/runtime (2 MiB spawned, 8 MiB Linux main, 1 MiB Windows main, Tokio 2 MiB, JVM 1 MiB, Go growable); RUST_MIN_STACK | Interlude | XII (async recursion Box::pin), XIII |
| serde_json recursion limit 128 as depth-limit example | Interlude | XXII |
| Twelve-criterion decision matrix format | Interlude | every later "X vs Y" |

## Promises to later Parts

- **Part X:** how the in-place `collect` specialization is wired (9.1, 9.5 "Where this goes next").
- **Part XI:** concurrent maps (sharding, `ArcSwap`, owner thread) measured; parallel `chunks_mut`/rayon; level-synchronous parallel BFS; channels as cross-thread queues (9.1, 9.3, 9.4, Interlude).
- **Part XII:** recursive `async fn` needs `Box::pin` (Interlude).
- **Part XIII:** buffer pools with capacity caps (9.1); `spawn_blocking` for CPU-heavy traversal (answers).
- **Part XIV:** false sharing / `CachePadded` measurement (9.1, 9.5).
- **Part XV:** `try_reserve`/fallible allocation (9.1); intrusive linked lists (9.4); `GlobalAlloc` explained (listings).
- **Part XVI:** Java string transfer via FFM vs JNI in the fraud library (9.2).
- **Part XX:** gateway access-log allocation removal p99 (9.2); fraud feature-vector p99 (9.5); THP/huge pages (9.5); order-book redesign benchmark (9.4); allocator contention (9.2, 9.4).

## Promises kept

- Ch 1.1: Vec growth is unspecified [LIB] + reallocation behavior, measured (9.1, with glibc in-place/mremap data).
- Project L1: SipHash default hasher explained, measured against ahash/foldhash/FxHash, HashDoS demonstrated; logstat's `get_mut` + `insert` choice justified with hash + allocation counts (9.3).
- Project L2: `memchr` candidate skipping — measured (ch02-05: memchr ~10× over byte loops, memmem ~8.5× over `str::matches`) and answered: redact's 13-byte candidate class (47% of log bytes are digits) defeats skip-search; use a 256-entry class table, memchr/memmem only for rare triggers (9.2 §6).
- Ch 4.1: retain/drain complexity with measurement (1,900× at n = 50K) (9.1).
- Ch 4.2: entry API internals — one hash verified (10,000 vs 10,099), plus its allocation cost and `entry_ref` (9.3).
- Ch 2.3: f64 not Ord/Eq in collections → E0599 verified; `total_cmp`, `OrderedFloat`, integer minor units (9.3).
- hashbrown/SwissTable internals (9.3).
- Ch 2.4: deep recursion → Interlude (frame measured, overflow verified, guard page + probes).
- Ch 3.6: explicit-stack DFS / cycle detection → Interlude (`(node, edge)` frames; three-color cycle detection as exercise with answer).
- Part III review: order book with `BTreeMap` levels + `VecDeque` FIFO + arena (9.4 production scenario).
- Ch 3.4: String growth "works as for Vec (Part IX)" (9.2, verified String capacities [0, 8, 16, 32, 64]).

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Gateway header vectors | 11–14 headers typical; Vec grew 0→4→8→16 (3 allocs/request, 1.2M allocs/s at 400K req/s); per-connection reused Vec, replaced when capacity > 64 | 9.1 |
| Ingestion batch buffer | replayed backlog → one batch of 2.1M events (~400 MB); clear() kept capacity; OOM kills; fix capacity policy + batch cap | 9.1 |
| Market-data snapshots | design exercise: 40,000 instruments, top 200 = 90% of trades, last 1,000 trades per instrument, 48-byte Trade | 9.1 |
| Gateway access logs | format! per log line → ~5 allocs/request (~2M allocs/s); per-worker String with write!, 4 KiB cap → 0 | 9.2 |
| Fraud JNI name incident | GetStringUTFChars Modified UTF-8 → 0.3% errors for emoji names; from_utf8_lossy "fix" changed hashed feature → 2 weeks of "model drift"; fix GetStringChars/from_utf16 or FFM | 9.2 |
| Ledger statement export | design exercise: ~2M statements overnight, 50–5,000 lines, Greek/Turkish/Japanese | 9.2 |
| Session cache (Rust port) | 2M sessions, 16-byte SessionId, 200-byte Session; inline HashMap ≈ 910 MB vs IndexMap ≈ 490 MB (chosen); foldhash seeded; presized, never shrinks intraday | 9.3 |
| Gateway rate limiter HashDoS | FxHashMap swap for "4% SipHash"; scraper IDs `counter << 32` → quadratic probing, CPU 100%; fix keyed hasher + CI lint `// TRUSTED-KEYS:` | 9.3 |
| Payments idempotency keys | design exercise: ~300M keys/day, 36-char UUIDs, 12 nodes, 24 h retention | 9.3 |
| Matching-engine order book (Rust prototype) | Slab<Order> arena + BTreeMap<i64, VecDeque<usize>> levels; lazy cancel + compaction | 9.4 |
| Settlement batcher EDF incident | BinaryHeap<Reverse<Job>> with derived Ord; field reorder PR → jobs by ID; missed partner cutoffs 2 nights; explicit Ord + property test | 9.4 |
| Gateway metrics time-series | design exercise: 5,000 routes, 10 s interval, 24 h retention | 9.4 |
| Fraud feature vector | HashMap<String, f64> of ~400 features per score → name→index at model load + reused Vec<f64>; ~3% CPU + 400 allocs/score removed (estimate, p99 to verify in XX) | 9.5 |
| Matcher SoA incident | SoA arena for risk report (6× faster) slowed matcher p99; reverted in 2 days; columnar snapshot for report instead | 9.5 |
| Risk-limits service | design exercise: 3M merchants, per-payment counter updates + per-minute 80% scan | 9.5 |
| Fraud linked-accounts traversal | Rust port uses BFS (distance correctness), frontier up to ~80K at hop 2, cap 100K ("too connected"), heap frontier independent of JVM -Xss, reused per worker | Interlude |
| Merchant-onboarding rule engine crash loop | analyst JSON rules, recursive evaluator on Tokio workers; partner rule ~40,000 deep → stack overflow abort → crash loop across replicas; quarantine + depth limit 64 + explicit stack | Interlude |
| Dependency resolver (build tooling, Go today) | design exercise: ~40,000 packages, depth ~15, one ~9,000-deep legacy chain | Interlude |
| Velocity store (fraud) | Part IX review capstone: Java-shaped port; profile ~2,000 events/s, ~150K merchants, ~5M cards/day | review |

## Verification

- `tools/verify.ps1 listings/part-09`: **32 files, 40 checks, all PASS** on rustc 1.98.1, edition 2024 (final run saved to scratchpad `part-09/verify-final.txt`). Includes: 2 intended compile errors (E0599 f64 key; E0658 LinkedList cursors), 2 verified crashes (`overflowed its stack`, debug and release), 3 `build` listings for asm.
- Compiler artifacts (via `tools/emit.ps1`, release asm, quoted trimmed with comments added): `Vec::push` fast/slow path + `grow_amortized` + `finish_grow` (ch01-05); recursive `dfs` prologue, 96-byte frame (intl-01, `-CrateType bin`); inline stack probes vs red-zone leaf (intl-05). Label numbers kept as emitted; comments added are marked with `;`.
- Timing listings are one run each on the shared Playground machine and labelled noisy in the text; allocation counts are exact.
- Arithmetic (not measured) estimates are labelled: HashMap memory per entry, session-cache 910/520/490 MB, velocity-store budget, fraud feature-vector CPU estimate, Java object-size comparisons (hedged by JDK/flags).
- Unverified-here, labelled with how to check: glibc mremap inference (`strace -e trace=mmap,mremap,brk`); fxhash-on-strings hypothesis (Systems exercise); small-string crate inline sizes ("per their docs"); Valhalla status (hedged).

## Word count

| File | Words |
|---|---|
| README.md | 630 |
| ch01-vec.md | 4,528 |
| ch02-string.md | 4,077 |
| ch03-hashmap.md | 4,477 |
| ch04-deque-btree-heap.md | 3,862 |
| ch05-cache-behavior.md | 3,851 |
| interlude-bfs-dfs.md | 5,023 |
| review.md | 1,475 |
| answers-part-09.md | 6,705 |
| **Total** | **34,628** |

## Tooling notes

- Thread-overflow depth test pattern: measure frame size in-process (address of a local at depth 0 and 1000), then run at 90% (ok) and 110% (crash) of `stack / frame` on an explicitly sized `thread::Builder` thread. Deterministic per compiler version; works for both profiles in one listing each.
- On 1.98.1 the overflow message for an unnamed thread is `thread '<unknown>' (NN) has overflowed its stack` (thread id included); the `crash overflowed its stack` needle matches it.
- `emit.ps1 -CrateType bin` works for pulling a single function out of a binary listing (search the output for `playground::fn_name:`).
- A counting `BuildHasher` (wrapping `std::hash::RandomState`, counting `finish()` in a thread_local) is a reusable instrument for "how many hashes" claims; hashbrown skips hashing on an empty table (10,099 not 10,100).
- `memchr`/`memmem` are on the Playground and make a good SIMD-vs-scalar measurement; `filter().count()` on bytes did NOT beat memchr here (5.65 ms vs 0.46 ms).
