# Chapter 9.5 — Choosing Collections by Cache Behavior

> **Where this sits:** Part IX · Collections and Memory · chapter 5 of 5
> **Prerequisites:** Chapters 9.1–9.4.
> **After this chapter you can:** predict which of two O(1) or O(log n) structures is faster from the bytes they touch
> and the loads they chain; lay out data as arrays-of-structs or structs-of-arrays on purpose; explain why
> `Vec<Box<T>>` is Java's memory model and when you want it anyway; choose a lookup structure by size and density; and
> tell a latency argument from a throughput argument.

---

## Pass 1 · User level — *Big-O ties; the memory hierarchy breaks them*

### 1. Problem

Every measurement in this Part so far had the same shape: two structures with the same Big-O, and constant factors
that differed by 3×, 18×, or 47×. A `Vec` and a `LinkedList` both traverse in O(n). A sorted `Vec` and a `BTreeMap`
both look up in O(log n). The difference is always the same thing: **how many bytes each operation pulls through the
memory hierarchy, and whether the CPU can fetch them before it needs them**. This chapter makes that the explicit basis
for choosing collections, with three more measurements and a way of reasoning you can apply to structures this book
never measures.

### 2. Mental model

**The memory hierarchy, order-of-magnitude** [CPU] (typical x86-64 server parts; the exact numbers vary by generation,
so treat these as scales, not specs):

```text
 level        size (per core unless shared)   latency           unit of transfer
 registers    ~16 general + vector registers  0 cycles
 L1 data      32–48 KiB                       ~4–5 cycles  ~1 ns
 L2           0.5–2 MiB                       ~12–16 cycles ~4 ns
 L3 (shared)  tens to hundreds of MiB         ~40–70 cycles ~10–20 ns
 DRAM         GBs                             ~200–400 cycles ~60–120 ns
                                              cache line = 64 bytes on x86-64 (Apple M-series cores use 128)
 TLB          ~1.5–3K entries                 a miss = a page-table walk; each entry covers a 4 KiB page (or 2 MiB huge page)
```

Three rules follow, and every result in this Part is an instance of one of them:

1. **Bytes touched.** Memory moves in 64-byte lines. If you need 8 bytes from each line you fetch, you're paying for 64.
   (AoS vs SoA below: 8× the traffic.)
2. **Dependent vs independent loads.** If the next address comes from the current load (a linked list, a tree, a binary
   search), misses happen one after another. If addresses are known in advance (an array, a batch of independent
   lookups), the CPU overlaps many misses at once. That's memory-level parallelism, typically 10+ outstanding misses
   per core.
3. **Predictability.** Hardware prefetchers detect sequential and constant-stride streams and fetch ahead. Random
   addresses defeat them, and random addresses across many pages also defeat the TLB.

### 3. Rust code

**Array of structs vs struct of arrays** (listing `ch05-01-aos-soa.rs`, release, 2 million orders; one run on a shared
machine, so compare ratios):

```rust,ignore
/// 64 bytes: exactly one cache line per order.
struct Order { id: u64, account: u64, price: i64, qty: u64, ts: u64, venue: u64, flags: u64, side: u64 }

/// The same data, one Vec per field ("struct of arrays").
struct Orders { price: Vec<i64>, qty: Vec<u64> /* ...the other six columns */ }

let s1: i64 = aos.iter().map(|o| o.price).sum();           // AoS
let s2: i64 = soa.price.iter().sum();                       // SoA
```

```text
size_of::<Order>() = 64
AoS bytes touched per scan: 122 MB; SoA price column: 15 MB
round 0: sum(price) AoS    5.96ms SoA  466.11µs | sum(price*qty) AoS    3.84ms SoA    1.01ms
round 1: sum(price) AoS    5.96ms SoA  471.23µs | sum(price*qty) AoS    3.77ms SoA    1.01ms
round 2: sum(price) AoS    5.98ms SoA  490.87µs | sum(price*qty) AoS    3.79ms SoA  996.17µs
```

(The listing's "MB" is MiB: 2M × 64 bytes = 122 MiB.) Summing one field of a 64-byte struct pulls every byte of every
order through the hierarchy. The SoA column is 8× smaller, sequential, and vectorizable, and it ran about 12× faster.
Summing `price * qty` needs two columns, so SoA touches 2/8 of the data and wins by about 3.8×.

One oddity is worth flagging rather than hiding: in this run, the AoS loop that reads *two* fields was faster than the
one that reads *one*. Both touch every cache line, so the difference must come from code generation, not memory. We
haven't inspected it; the Systems exercise asks you to emit both loops' assembly and explain it.

**Boxes: Java's memory model, built in Rust** (listing `ch05-02-scattered-boxes.rs`, release, 4 million `u64`s):

```rust,ignore
let flat: Vec<u64> = (0..n as u64).collect();
// Java's ArrayList<Long>: an array of references to separately allocated boxes.
let boxed: Vec<Box<u64>> = (0..n as u64).map(Box::new).collect();
// Same boxes, shuffled: the references no longer follow allocation order.
let mut shuffled: Vec<Box<u64>> = /* ... Fisher–Yates shuffle ... */;
```

```text
adjacent Box<u64> allocations are 32 bytes apart (for 8 bytes of payload)
round 0: Vec<u64>    1.09ms | Vec<Box<u64>> in order    5.22ms | shuffled   37.23ms
round 1: Vec<u64>    1.01ms | Vec<Box<u64>> in order    5.31ms | shuffled   37.35ms
round 2: Vec<u64>    1.04ms | Vec<Box<u64>> in order    5.52ms | shuffled   37.35ms
```

- **Boxing in allocation order costs ~5×.** Each 8-byte value occupies a 32-byte allocator chunk (verified: adjacent
  boxes are 32 bytes apart; glibc adds an 8-byte header and rounds to 16), and we also read the 8-byte pointers. That's
  5× the bytes. The addresses are still sequential, so the prefetcher keeps up.
- **Shuffled boxes cost ~37×.** Same data, same count, but the pointers now lead to random places across ~128 MB. The
  prefetcher can't predict them, most loads miss cache, and many miss the TLB. The loads are *independent* (each box's
  address comes from the pointer array, not from the previous box), so the CPU overlaps several misses at once. That's
  why it's ~9 ns per element rather than a full DRAM latency.

**Lookup structures by size** (listing `ch05-03-lookup-structures.rs`, release; 1 million random lookups of existing
keys; keys are sparse with stride 3; one noisy run):

```text
n =      1000: sorted Vec + binary_search    8.88ms | HashMap    7.81ms | BTreeMap   34.52ms | dense Vec index    1.25ms
n =    100000: sorted Vec + binary_search   34.64ms | HashMap   15.18ms | BTreeMap   76.39ms | dense Vec index    2.05ms
n =   1000000: sorted Vec + binary_search  215.14ms | HashMap   73.72ms | BTreeMap  196.31ms | dense Vec index    7.24ms
```

| Structure | Per lookup at n = 1M (this run) | Mechanism |
|---|---|---|
| Dense `Vec<Option<u32>>` indexed by key | ~7 ns | one load at a computed address; lookups independent, so misses overlap |
| `HashMap` (SipHash) | ~74 ns | hash computation + ~1–2 cache lines |
| `BTreeMap` | ~196 ns | ~7 levels, a linear scan in each; dependent loads |
| Sorted `Vec` + `binary_search` | ~215 ns | ~20 dependent loads; the last ~10 miss cache; ~50% of its branches mispredict |

At n = 1,000 everything fits in L1/L2, and the ranking is about computation: binary search's ~10 unpredictable
branches and SipHash's ~20 ns of arithmetic come out about even. At n = 1M, memory dominates, and the structures that
chain loads (binary search, B-tree) fall behind the hash map.

---

## Pass 2 · Systems level — *Why each result came out the way it did*

### 4. Under the hood

**AoS vs SoA in the machine.** [CPU][RUSTC] The SoA sum is a loop over contiguous `i64`s. LLVM vectorizes it with
AVX2 or SSE2 (4 or 2 lanes per instruction, unrolled), and the hardware prefetcher streams the column from memory
ahead of use. The AoS loop reads one 8-byte field per 64-byte line. It *can* be vectorized with gathers or by loading
full lines and shuffling, but the memory traffic is fixed at 8× regardless: at 122 MiB per scan, it's a
memory-bandwidth measurement. Roughly 122 MiB in ~6 ms is ~20 GB/s, a plausible single-core streaming rate.

**Dependent loads, precisely.** Compare three loops that each touch a million or so scattered addresses:

```text
 shuffled Vec<Box<u64>>     for p in ptrs { sum += *p }        addresses known in advance → misses overlap
 aged LinkedList (9.4)      node = node.next                   next address unknown until the load returns
 binary search              mid = if key < a[mid] {..} else {..} next address depends on this comparison
```

An out-of-order core keeps many independent loads in flight (limited by its reorder buffer and miss-handling
registers), so the first loop runs at a fraction of DRAM latency per element. The second and third can't start the next
load until the current one returns. That's why "pointer chasing" is the phrase for the worst case: it serializes the
latency. It's also why *batching* independent lookups (processing many keys per loop iteration) helps hash maps and
binary searches alike.

**Binary search's branch problem.** Each comparison in a binary search goes left or right with ~50% probability, so the
branch predictor is wrong half the time. That's ~10–20 cycles lost per level [CPU]. Branchless formulations (a
conditional move instead of a branch) and cache-friendly layouts (the Eytzinger layout, which stores the implicit tree
in BFS order so the next few levels can be prefetched) are known remedies. Khuong and Morin (2017, "Array Layouts for
Comparison-Based Searching") reported large speedups from them. That's an exercise here, not a measured claim.

### 5. Memory

**Bytes per entry, by structure** (from layouts in this Part; `u64` keys with `u32` values unless stated; arithmetic,
not measured):

| Structure | Bytes per entry | Notes |
|---|---|---|
| Dense `Vec<Option<u32>>` | 8 per *possible* key | wins only when keys are dense; here, with stride 3, 24 bytes per actual entry |
| Sorted `Vec<(u64, u32)>` | 16 (+ spare capacity) | padding: `(u64, u32)` is 16 bytes, not 12 |
| `HashMap<u64, u32>` | 17 per bucket / load (44–87%) → ~19–39 | Chapter 9.3's formula |
| `BTreeMap<u64, u32>` | ~12 per slot / fill (50–100%) + node headers → ~15–30 | |
| `Vec<Box<u64>>` | 8 (pointer) + 32 (chunk) = 40 | vs 8 for `Vec<u64>` |
| `LinkedList<u64>` | 32 (node chunk) | vs 8 |

Two layout tools shrink these numbers:

- **Field order and padding.** [RUSTC] rustc reorders the fields of a default-repr struct to minimize padding, so a
  struct with fields `u8, u64, u8` is 16 bytes, not 24. Tuples are reordered the same way, but a `(u64, u32)` still
  pads to 16 because its size must be a multiple of its 8-byte alignment. Two parallel `Vec`s (`keys: Vec<u64>`,
  `vals: Vec<u32>`) store the same data in 12 bytes per entry. That's SoA again.
- **Narrower types.** A `u32` index instead of a `usize`, a `u16` price-level offset instead of an `i64` price, a
  1-byte enum instead of a `String`. In a structure with a hundred million entries, every byte saved per entry is
  100 MB.

### 6. CPU / OS

**Throughput is not 1/latency.** The dense index answered 1M lookups in 7 ms: 7 ns each, far below DRAM latency. That's
not because each lookup was fast. It's because the loop issued independent lookups and the CPU overlapped them. The
same structure answering one request's single lookup would see the full miss latency. When someone quotes "ns per
lookup" from a benchmark loop, ask whether production issues lookups in batches (throughput) or one at a time on a
request's critical path (latency).

**The TLB is part of the cache story.** [OS][CPU] With 4 KiB pages, a 128 MiB random-access working set spans 32,768
pages, far more than the TLB holds, so random access pays for page-table walks too. Transparent huge pages (2 MiB) can
cut that sharply for large heaps. On Linux, `madvise(MADV_HUGEPAGE)` or the system THP setting controls it, and some
allocators (jemalloc, mimalloc) have options for it. It's a deployment-level lever; Part XX measures it.

**Concurrency and cache lines.** [CPU] Two threads writing to different variables in the *same* cache line slow each
other down, because the line bounces between their caches (false sharing). A `Vec<AtomicU64>` of per-thread counters is
the textbook case. The fix is padding each element to a cache line (`crossbeam_utils::CachePadded`, which pads to 128
bytes on x86-64 to account for the adjacent-line prefetcher). Part XIV measures it. For collections, the rule is: a
structure written by many threads should be sharded so each shard sits in its own lines.

---

## Pass 3 · Architect level — *A selection method*

### 7. Trade-offs

**A selection method.** For any hot collection, answer four questions in order:

1. **What is the access pattern?** Scan all, scan one field, point lookups, range queries, FIFO, priority, or
   insert/remove at positions.
2. **How big, relative to cache?** Fits in L1/L2 (compute-bound: branches and hashing dominate), fits in L3, or beyond
   (memory-bound: bytes and dependent loads dominate).
3. **Are the keys dense?** Small integer ranges, enums, and codes can be indices, which beats every map.
4. **Is the access latency-critical or throughput-bound?** One lookup on a request path, or millions in a batch job.

Then pick:

| Access pattern | Default | Consider | Avoid |
|---|---|---|---|
| Scan everything | `Vec<T>` | SoA if scans read few fields | `LinkedList`, `Vec<Box<T>>` |
| Point lookup, sparse keys | `HashMap` | sorted `Vec` for small, static sets; a faster keyed hasher | `BTreeMap` if you never need order |
| Point lookup, dense keys | `Vec<Option<T>>` / `Vec<T>` indexed by key | a bitmap for sets | any map |
| Range / ordered / min-max | `BTreeMap` | sorted `Vec` if built once, read many | `HashMap` plus sort per query |
| FIFO / sliding window | `VecDeque` | a fixed-size ring (`Box<[T]>` plus indices) | `LinkedList` |
| Priority | `BinaryHeap` | `BTreeMap<(prio, seq), T>` if you must cancel | a sorted `Vec` with inserts |
| Stable handles, deletions | `slab` / an arena `Vec` with indices | `HashMap<Id, T>` | `Rc<RefCell<T>>` webs |
| Insertion-ordered map | `IndexMap` | a `Vec` plus a `HashMap<K, usize>` | `LinkedList` + map |
| Many small collections | `Vec` | `SmallVec` if most stay under N | a `Vec` per item when a flat `Vec` plus ranges would do |

**`Box<T>` vs `Vec<T>` for many objects** (SPEC comparison row, read as "one allocation per object vs one for all"):

| Criterion | `Vec<Box<T>>` (object per allocation) | `Vec<T>` (inline) |
|---|---|---|
| Memory | pointer + chunk overhead per element (40 B for a `u64`) | `size_of::<T>()` per element |
| CPU | a pointer load per access | direct |
| Latency | a miss per element once scattered | sequential |
| Throughput | measured 5× (in order) to 37× (shuffled) slower scans | see left |
| Contention | n allocator calls under concurrency | ~log n allocations |
| Cache | depends on allocation history | contiguous |
| Allocation | n | 1 or ~log₂ n |
| Complexity | none | none |
| Safety | both fully safe | same |
| Maintainability | stable addresses: elements don't move on growth | elements move when the `Vec` grows (borrows prevent misuse) |
| Failure modes | fragmentation that never heals | large reallocation copies (if `T` is big) |
| Operational | RSS grows with churn | predictable RSS |

`Vec<Box<T>>` is right when elements are large and moved around often (moving a pointer is cheaper than moving 1 KB),
when you need stable addresses (a `Box`'s contents don't move when the `Vec` grows), or for trait objects
(`Vec<Box<dyn Handler>>`, Part VI). Otherwise, store values inline.

### 8. Java comparison

| Java reality | Rust reality |
|---|---|
| Every non-primitive element is a reference to a separate object (`ArrayList<Order>` is `Vec<Box<Order>>` plus headers) | Elements are inline unless you box them |
| Copying/compacting collectors (G1, ZGC, Shenandoah) move objects and can restore locality, since survivors are copied together | Allocators never move objects: fragmentation after churn is permanent unless you rebuild the structure |
| Escape analysis can scalar-replace short-lived objects, but not objects stored in collections | No boxing to remove in the first place |
| High-performance Java writes SoA by hand with primitive arrays (`long[] prices`) or off-heap buffers (Agrona, Chronicle) | SoA is ordinary `Vec`s of plain types |
| Project Valhalla value classes aim to flatten objects into arrays (preview/early-access as of this writing; check your JDK) | Available today by default |

> **Analogy limit.** A Java engineer's intuition is "the GC handles layout". It's partly right: compacting collectors
> do repair locality that Rust's allocators never repair. After a million inserts and deletes, a Java `TreeMap` may
> have *better* locality than a Rust `Vec<Box<T>>` built the same way. What Rust gives you is the ability to not need
> repair: choose inline storage and the question disappears. When you do box in Rust, rebuild long-lived structures
> occasionally (collect into a new `Vec`), which is a manual compaction.

> **Why not SoA everywhere?** Because SoA optimizes for scans of a few fields and penalizes access to one whole record:
> reading all eight fields of order i now touches eight cache lines (one per column) instead of one. Section 10 is that
> mistake in production.

### 9. Production scenario

**The fraud feature vector.** Meridian's fraud scoring (50K scores/s, p99 < 5 ms, ~3 ms CPU per score) was ported from
Java with its data model intact: each score builds a `HashMap<String, f64>` of about 400 named features, then the model
reads them back by name.

Arithmetic for one score (with this Part's measurements as the unit costs, so this is an estimate to verify, not a
measurement):

```text
build:  400 × String key allocation + 400 × SipHash insert on short strings, 8 table allocations (444 rehashes)
read:   400 × lookup by &str                    ≈ 400 × ~130 ns (Chapter 9.3's short-String lookup)
total:  ≈ 400+ allocations and roughly 100 µs of CPU per score: ~3% of the 3 ms budget
```

The redesign turns names into indices **once**, when the model loads: a `HashMap<&'static str, usize>` from feature
name to position. Each score then fills a `Vec<f64>` of length 400 (reused per worker thread), and the model reads
`features[i]`. That's zero allocations per score after warm-up, and reads become one indexed load.

The team was explicit about the expected win: about 3% of CPU, which is useful but not transformative, and it removed
400 allocations per score from a process whose p99 was sensitive to allocator contention across 120 cores. They
recorded the hypothesis (p99 improves) and the measurement plan (Part XX's allocator and latency profiling), rather
than claiming the p99 win in advance.

### 10. Failure scenario

**The SoA rewrite that slowed the matcher.** After a talk on data-oriented design, a Meridian engineer converted the
matching engine's order arena from `Slab<Order>` (AoS) to eight parallel `Vec`s (SoA), because the end-of-day risk
report, which sums exposure across all orders, got 6× faster in a benchmark.

The matching hot path doesn't scan. For each incoming order it touches a handful of resting orders, and for each of
those it reads nearly every field: price, quantity, account, timestamp, flags. With AoS, that was one 64-byte cache line
per resting order. With SoA, it's eight lines at eight different addresses. The matcher's p99 latency in the
staging load test rose noticeably, and the rewrite was reverted two days later.

- **Root cause:** the layout was chosen for the batch workload (field scans) and imposed on the latency-critical
  workload (whole-record access).
- **Fix:** keep AoS in the matcher. The risk report reads a *snapshot*: at the end of the day, the arena is copied into
  columnar form (a one-off O(n) pass that the report job can afford).
- **Lesson:** layout is a property of the access pattern, not of the data. When two workloads disagree, give each its
  own layout and pay for the conversion where latency doesn't matter.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part IX).*

1. State the three rules (bytes touched, dependent loads, predictability) and give one measurement from this Part that
   illustrates each.
2. Why did SoA win by ~12× for a one-field sum but only ~3.8× for a two-field product?
3. Why are shuffled boxes ~37× slower than a flat `Vec`, but boxes in allocation order only ~5×?
4. Why did the sorted-`Vec` binary search lose to `HashMap` at 1M keys but roughly tie at 1,000?
5. What is the difference between "7 ns per lookup" in a benchmark loop and the latency of one lookup on a request path?
6. When is a dense `Vec` indexed by key the right "map"? What does it cost when keys are sparse?
7. How do compacting garbage collectors change the locality story compared with Rust's allocators?
8. When is `Vec<Box<T>>` the right choice despite this chapter's numbers?
9. When is SoA the wrong choice?

### 12. Exercises

- **Beginner.** Compute `size_of` for `struct A { a: u8, b: u64, c: u8 }`, for `(u64, u32)`, and for
  `[(u64, u32); 4]`. Predict first, then check with a listing.
- **Intermediate.** Extend `ch05-03-lookup-structures.rs` with n = 16 and n = 64, and add a linear scan of a
  `Vec<(u64, u32)>`. Find the crossover point where linear scan stops winning, and explain it.
- **Advanced.** Implement an Eytzinger-layout search (the sorted keys stored in BFS order of the implicit binary tree)
  and compare it with `binary_search` at n = 1M. Then add a software prefetch of the node four levels down
  (`core::arch::x86_64::_mm_prefetch`) and measure again.
- **Systems.** Emit the release assembly for the two AoS loops in `ch05-01-aos-soa.rs`
  (`tools/emit.ps1 ... -Target asm -Mode release`, with each loop in an `#[inline(never)]` function). Explain why
  the two-field loop ran faster than the one-field loop.
- **Architecture.** Take one hot data structure in a system you know. Answer the four selection questions from Section
  7, and write down whether its current layout matches the answers.

### 13. Debugging exercise

A Meridian service keeps `Vec<Box<Session>>` for its active sessions and scans them every second to expire idle ones.
The scan took 3 ms at startup. After a week of uptime (sessions constantly created and expired, with `swap_remove` for
deletions), it takes 40 ms, with the same number of sessions.

1. What changed in memory, given that the number of sessions didn't?
2. Why would a Java version of this service likely *not* show the same degradation?
3. Give two fixes: one that keeps the boxes, and one that removes them. What does each cost?

### 14. Design exercise

**Per-merchant risk counters.** Meridian's risk-limits service (100K ops/s, p99 < 1 ms) keeps, for each of 3 million
merchants, a running total of today's volume, a count of transactions, a last-seen timestamp, and a limit. Every
payment reads the limit and updates the three counters of one merchant. Every minute, a reporting job scans all
merchants for those above 80% of their limit.

Choose the layout (AoS vs SoA, `HashMap<MerchantId, State>` vs a dense index behind an ID-mapping table), justify it
against both workloads, and estimate the memory. Say how you'd keep the per-minute scan from interfering with the
payment path, and name the measurement that would falsify your design.
