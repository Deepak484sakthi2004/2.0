# Chapter 9.4 — VecDeque, BTreeMap, BinaryHeap — and Why Not LinkedList

> **Where this sits:** Part IX · Collections and Memory · chapter 4 of 5
> **Prerequisites:** Chapters 9.1 and 9.3; the Part III review's order-book redesign ("arena + `BTreeMap` levels").
> **After this chapter you can:** draw a ring buffer and say when `as_slices` returns two pieces; explain why std's ordered
> map is a B-tree and not a red-black tree; build a price-level order book, a top-k filter, and a deadline scheduler;
> argue with measurements why `LinkedList` is almost never the answer; and recognize the one Java habit
> (`PriorityQueue` is a min-heap) that silently inverts a Rust scheduler.

---

## Pass 1 · User level — *Three specialists and one relic*

### 1. Problem

`Vec` and `HashMap` cover most needs. The rest come in three shapes: **queues** (add at one end, remove at the other),
**ordered maps** (range queries, "the best price", "everything between two timestamps"), and **priority queues** ("the
most urgent item next"). The Part III review ended with an order book that used `Rc<RefCell<...>>` everywhere and
promised a redesign using `BTreeMap` and `VecDeque`. This chapter delivers it, measures the ordered map against the hash
map, and settles the linked-list question with numbers rather than folklore.

### 2. Mental model

```text
VecDeque<T>   a Vec used as a ring: (buf, cap, head, len)          32-byte handle, one allocation
              ┌────┬────┬────┬────┬────┬────┬────┬────┐
              │ 9  │ ·  │ 3  │ 4  │ 5  │ 6  │ 7  │ 8  │   head = 2, len = 7, cap = 8
              └────┴────┴────┴────┴────┴────┴────┴────┘
              logical order: 3 4 5 6 7 8 | 9     → as_slices() = ([3..8], [9])

BTreeMap<K,V> a B-tree: nodes of up to 11 sorted keys (and values), internal nodes with up to 12 children
              [ 20 | 50 | 81 ]
              /    |     |    \
          [..] [..27..] [..] [..95..]      height ≈ log₆ n to log₁₂ n; a scan inside each node

BinaryHeap<T> a Vec in heap order: children of i at 2i+1, 2i+2; the MAXIMUM is at index 0
              [9, 5, 8, 3, 1, 2]   ← verified layout for {5, 1, 8, 3, 9, 2}

LinkedList<T> one heap node per element: (next, prev, value)
              [·]⇄[·]⇄[·]⇄[·]      each arrow a pointer to a separate allocation
```

### 3. Rust code

**VecDeque: a ring that wraps** (listing `ch04-01-vecdeque.rs`, verified):

```rust,ignore
let mut q: VecDeque<u32> = VecDeque::with_capacity(8);
for i in 1..=6 { q.push_back(i); }
q.pop_front();
q.pop_front();
for i in 7..=9 { q.push_back(i); }
let (front, back) = q.as_slices();
```

```text
capacity 8, len 7
as_slices: [3, 4, 5, 6, 7, 8] + [9]   <- the ring buffer wrapped
after make_contiguous: [3, 4, 5, 6, 7, 8, 9] + []
rate limiter (3 per 1000 ms): [(0, true), (100, true), (200, true), (300, false), (999, false), (1000, true), (1150, true), (1250, true)]
limiter buffer capacity stayed 3
```

Popping from the front advanced `head` without moving anything. Pushing `9` found the end of the buffer and wrapped to
slot 0. That's why `as_slices` returns two slices: the logical sequence is physically in two pieces. `make_contiguous`
rotates the data into one piece (O(n), once) when an API needs a single `&[T]`, for example to sort or to write to a
socket in one call.

The same listing's sliding-window rate limiter keeps the timestamps of recent events and evicts expired ones from the
front: at most `limit` entries, so the buffer never grows past its initial capacity. That's the standard "last N
events" structure: amortized O(1) per event, one allocation for its whole life.

**BTreeMap: an order book by price level** (listing `ch04-02-btreemap.rs`, verified):

```rust,ignore
/// Price in integer ticks (never f64 keys), each level a FIFO queue of order quantities.
#[derive(Default)]
struct Book {
    bids: BTreeMap<i64, VecDeque<u64>>,
    asks: BTreeMap<i64, VecDeque<u64>>,
}

impl Book {
    fn best_bid(&self) -> Option<i64> {
        self.bids.last_key_value().map(|(p, _)| *p)
    }
    fn best_ask(&self) -> Option<i64> {
        self.asks.first_key_value().map(|(p, _)| *p)
    }
    // depth(): self.bids.iter().rev().take(n) — best bids first
}
```

```text
best bid Some(10050), best ask Some(10055)
top 3 bids: [(10050, 9), (10045, 4), (10040, 5)]
top 3 asks: [(10055, 6), (10060, 3), (10070, 8)]
bid levels in [10040, 10050]: [10040, 10045, 10050]
points in a 5-minute window: [1700000600, 1700000660, 1700000720, 1700000780, 1700000840]
last point at or before t+125s: Some((1700000120, 2.0))
```

Everything an order book needs is one call:

- **Best price:** `last_key_value` for bids, `first_key_value` for asks. That's O(log n), and the path to the
  extreme leaf stays cached in practice.
- **Depth:** `iter().rev().take(n)`.
- **Price-time priority:** each level's `VecDeque` keeps arrival order.
- **Bands:** `range(lo..=hi)`.

The time-series lines show the other classic use. `range(a..b)` returns the points in a window, and
`range(..t).next_back()` answers "the latest value at or before t", the as-of join that market data and metrics
systems do constantly.

**BinaryHeap: top-k and earliest-deadline-first** (listing `ch04-04-binaryheap.rs`, verified):

```rust,ignore
/// Top-k largest with a bounded MIN-heap of size k: O(n log k) time, O(k) memory.
fn top_k(values: impl IntoIterator<Item = u64>, k: usize) -> Vec<u64> {
    let mut heap: BinaryHeap<Reverse<u64>> = BinaryHeap::with_capacity(k + 1);
    for v in values {
        if heap.len() < k {
            heap.push(Reverse(v));
        } else if let Some(&Reverse(smallest)) = heap.peek() {
            if v > smallest {
                heap.pop();
                heap.push(Reverse(v));
            }
        }
    }
    // ...drain and sort descending
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Job {
    deadline_ms: u64, // compared first: field order is the ordering
    id: u32,
}
```

```text
top 3 latencies: [4100, 3000, 2600]
EDF order (deadline, id): [(100, 0), (100, 2), (175, 3), (250, 1)]
heap as stored (level order): [9, 5, 8, 3, 1, 2]
into_sorted_vec: [1, 2, 3, 5, 8, 9]
```

- **`BinaryHeap` is a max-heap.** For "smallest first" (deadlines, Dijkstra distances), wrap elements in
  `std::cmp::Reverse`.
- **Top-k uses the opposite heap.** To keep the k *largest*, keep a *min*-heap of size k and evict its minimum. Memory
  stays O(k) however long the stream, which is how you compute "the 100 slowest requests today" in constant memory.
- **`#[derive(Ord)]` compares fields in declaration order** [LANG]: `deadline_ms` first, then `id` as the tie-break
  (`(100, 0)` before `(100, 2)`). The field order of the struct *is* the scheduling policy. Section 10 is what happens
  when someone forgets that.
- **The stored order is a heap, not a sorted list.** Iterating a `BinaryHeap` (or `as_slice`) gives level order.
  `into_sorted_vec` (O(n log n)) or repeated `pop` gives sorted order.

---

## Pass 2 · Systems level — *Layouts, node sizes, and pointer chases*

### 4. Under the hood

**VecDeque.** [LIB][VERSION] Since Rust 1.67, `VecDeque` is `{ head, len, buf: RawVec }`, and its capacity no longer has
to be a power of two (the older design kept `tail`/`head` indices and a power-of-two capacity). The physical index of
logical element `i` is `head + i`, minus `cap` if that overflows: a compare and a subtraction, no division. When a full
deque grows, the buffer doubles like a `Vec`'s, and then whichever wrapped segment is shorter is copied so the ring is
consistent in the new space. That's O(min(head part, tail part)) extra work on top of the reallocation.

**BTreeMap.** [LIB] std's ordered map is a B-tree with **B = 6**: every node holds up to 11 key-value pairs (at least 5,
except the root), and internal nodes hold up to 12 child pointers. The layout of a leaf for `BTreeMap<u64, u64>`:

```text
LeafNode { parent: *InternalNode, parent_idx: u16, len: u16, keys: [u64; 11], vals: [u64; 11] }
          8 + 2 + 2 (+4 padding) + 88 + 88 ≈ 192 bytes = 3 cache lines, keys contiguous
InternalNode { data: LeafNode, edges: [*Node; 12] }   ≈ 288 bytes
```

Two design choices matter for performance:

1. **Many keys per node.** A red-black tree (Java's `TreeMap`) has one key per node, so a lookup in a million entries
   follows about 20 pointers, each likely a cache miss. A B-tree with ~6–11 keys per node has height ~7 for a million
   entries, and the keys it compares in each node sit in one or two cache lines.
2. **Linear search inside a node.** [LIB] std scans a node's keys left to right rather than binary-searching them. For
   11 keys in contiguous memory, a linear scan is branch-predictor-friendly and at least as fast as binary search. It
   also means `Ord::cmp` is called up to 11 times per level, so an expensive comparison (long `String` keys) costs more
   in a `BTreeMap` than its height suggests.

**BinaryHeap.** [LIB] A `Vec<T>` with the heap property. `push` appends and sifts up: O(log n) comparisons and moves.
`pop` swaps the last element into the root and sifts down. std uses the "sift down to the bottom, then sift up"
variant, which saves comparisons on average because the element moved to the root usually belongs near the bottom.
`BinaryHeap::from(vec)` heapifies in O(n), faster than n pushes. `peek_mut` lets you modify the top element in place,
and the heap re-sifts when the guard drops. There's no decrease-key operation: algorithms like Dijkstra push a new
entry and skip stale ones when popped ("lazy deletion").

**LinkedList.** [LIB] A doubly linked list with a 24-byte handle (head, tail, len) and one heap node per element holding
`next`, `prev`, and the value. The stable API supports pushing and popping at both ends, `append` (O(1) splice of a
whole list), `split_off` (O(n) walk to the position), iteration, and `contains`. What it **doesn't** support on stable is
the one operation that justifies linked lists: removing or inserting at a position you're already holding. That's the
cursor API, and it's still feature-gated (listing `ch04-06-linkedlist-cursor.rs`, verified on 1.98.1):

```text
error[E0658]: use of unstable library feature `linked_list_cursors`
 --> src/main.rs:8:29
  |
8 |     let mut cursor = orders.cursor_front_mut();
  |                             ^^^^^^^^^^^^^^^^
  |
  = note: see issue #58533 <https://github.com/rust-lang/rust/issues/58533> for more information
```

### 5. Memory

**Handle sizes** (listing `ch01-06-sizes.rs`, verified): `VecDeque<u64>` 32 bytes, `BinaryHeap<u64>` 24,
`LinkedList<u64>` 24, `BTreeMap<u64, u64>` 24, `HashMap<u64, u64>` 48.

**Allocations for a million `u64`s** (listing `ch04-05-linkedlist-vs-vec.rs`, counting allocator, exact):

```text
Vec<u64>:              1 allocations for 1000000 elements
VecDeque<u64>:         1 allocations
LinkedList<u64>: 1000000 allocations (one node = prev + next + value = 24 bytes each)
```

`collect` from a `Range` knows the exact size, so `Vec` and `VecDeque` allocate once. The list allocates a million
24-byte nodes. With glibc malloc each one occupies a 32-byte chunk (we'll see that 32-byte spacing measured directly
in Chapter 9.5), so the list takes 32 MB for 8 MB of data, and a million `free` calls when it's dropped.

**BTreeMap memory.** Nodes are between half-full and full, so a `BTreeMap<u64, u64>` uses roughly 16 bytes of payload
per entry divided by the fill factor, plus node headers and child pointers. That's comparable to a `HashMap` at
average load, and it grows smoothly: one node at a time, no 2× steps.

**Ownership.** All four own their elements. Dropping a `BTreeMap` or `LinkedList` walks every node to free it. For a
huge structure that's a measurable pause at drop time (Chapter 3.1's route-table dropper thread applies). A `VecDeque` or
`BinaryHeap` frees one buffer (after running element destructors, if any).

### 6. CPU / OS

**Traversal: contiguous vs linked** (same listing, release, one run on a shared machine; noisy, and the ratios are the
point):

```text
round 0: sum Vec  301.86µs | VecDeque  269.78µs | LinkedList fresh    1.28ms | LinkedList aged   12.93ms
round 1: sum Vec  274.04µs | VecDeque  267.20µs | LinkedList fresh    1.28ms | LinkedList aged   12.84ms
```

- **`Vec` and `VecDeque` are the same speed**: both are sequential scans of one buffer, vectorizable [CPU].
- **A freshly built list is ~4.7× slower.** Its nodes were allocated one after another, so they happen to sit at
  consecutive 32-byte addresses and the prefetcher still helps. But each step is a *dependent load*: the address of
  the next node isn't known until the current one arrives, so there's no memory-level parallelism and no
  vectorization.
- **An "aged" list is ~47× slower.** The listing interleaved each node's allocation with a random-size allocation that
  stays alive, the way nodes are scattered in a long-running server. Now every step is likely a cache miss, and some are
  TLB misses. This is the realistic number for a list that lives in a busy process.

**BTreeMap vs HashMap** (listing `ch04-03-btree-vs-hash.rs`, release, 1M random `u64` keys; one noisy run):

```text
build     HashMap   50.63ms
build    BTreeMap   58.71ms
1M gets   HashMap   66.45ms  (4964fd655d986771)
1M gets  BTreeMap  218.49ms  (4964fd655d986771)
sorted scan BTreeMap    1.13ms  (1000000 keys)
sorted scan HashMap    20.48ms  (collect + sort_unstable)
range     BTreeMap    7.35µs  (1000 keys)
range      HashMap    2.09ms  (1000 keys, full scan)
```

| Operation | Winner | Ratio in this run | Mechanism |
|---|---|---|---|
| Build 1M entries | tie | 1.2× | both are dominated by random memory writes |
| 1M point lookups | `HashMap` | 3.3× | ~1 probe vs ~7 levels of node scanning |
| Full scan in key order | `BTreeMap` | 18× | the B-tree is already sorted; the hash map must collect and sort |
| 1,000-key range | `BTreeMap` | ~280× | O(log n + k) vs a full O(n) scan |

(Even SipHash, the slow hasher of Chapter 9.3, left the hash map 3× ahead on point lookups.)

**Concurrency.** None of these types synchronize internally. The cross-thread versions of a queue are channels
(`std::sync::mpsc`, `crossbeam-channel`) and lock-free queues (`crossbeam::queue`), covered in Part XI. There's no
concurrent ordered map in std. Java's `ConcurrentSkipListMap` has ecosystem equivalents (`crossbeam-skiplist`), or you
use one owner thread for the structure, which is what matching engines usually do anyway.

---

## Pass 3 · Architect level — *Picking the specialist*

### 7. Trade-offs

**`Vec` vs `LinkedList`** (SPEC comparison row, with this chapter's measurements):

| Criterion | `Vec<T>` / `VecDeque<T>` | `LinkedList<T>` |
|---|---|---|
| Memory | 8 bytes per `u64` + spare capacity | 24-byte node per element, 32 bytes after allocator rounding |
| CPU | sequential, vectorizable | one dependent load per element |
| Latency | occasional reallocation spike | an allocator call on *every* push |
| Throughput | measured 4.7× (fresh) to 47× (aged) faster traversal | see left |
| Contention | none built in | none built in; a million allocations stress a shared allocator (Part XX) |
| Cache | prefetcher-friendly | fresh: partly; aged: a miss per node |
| Allocation | 1 (presized) or ~log₂ n | n |
| Complexity | index arithmetic | pointer surgery, mostly hidden |
| Safety | fully safe | fully safe API, but the useful cursor API is unstable |
| Maintainability | universally understood | readers ask "why a linked list?" |
| Failure modes | middle insert/remove is O(n) | memory 4× the data; drop walks n nodes |
| Operational | predictable RSS | fragmentation; long drops |

When is `LinkedList` right? When you need O(1) `append` of whole lists and never traverse, or when you need stable
element addresses and cursor-based removal badly enough to use nightly or write your own intrusive list (Part XV). For
nearly everything else, including LRU caches, the Chapter 3.6 design (an index-linked list *inside* a `Vec`, plus a
`HashMap`) gives O(1) operations with contiguous memory. The `slab` crate packages the "stable index into a `Vec`"
idea.

> **Why not LinkedList for insert-in-the-middle?** Because you first have to *find* the middle. Finding position k is an
> O(k) pointer walk, the same asymptotic cost as a `Vec`'s `memmove` of the tail, and the walk's constant factor is
> dozens of times worse (the aged measurement). Bjarne Stroustrup made this argument with measurements in his
> GoingNative 2012 keynote for C++'s `vector` vs `list`, and the mechanism is the same in Rust. A linked list wins only
> when you already hold the position, and on stable Rust you can't hold one.

**Queue choice:** `VecDeque` for FIFO work queues, sliding windows, and BFS frontiers (the Interlude). A `Vec` used as a
stack when LIFO is fine, since it's slightly simpler than a deque.

**Ordered-map choice:** `BTreeMap` when you need order, ranges, or min/max. A sorted `Vec` plus `binary_search` when the
data is built once and read many times (Chapter 9.5 measures it). `HashMap` otherwise.

**Priority choice:** `BinaryHeap` for "next most urgent". A `BTreeMap<(priority, seq), T>` when you also need to cancel
or reprioritize arbitrary entries (O(log n) removal by key, which a heap can't do).

### 8. Java comparison

| Java | Rust | Notes |
|---|---|---|
| `ArrayDeque` | `VecDeque` | Same design: a circular array. Java's own docs say `ArrayDeque` is likely faster than `LinkedList` as a queue. |
| `LinkedList` (as `List` or `Deque`) | `LinkedList`, rarely | Java's version is also a node per element, plus the boxed element. The same advice applies in both languages. |
| `TreeMap` (red-black tree) | `BTreeMap` | One node object per entry (~40 bytes plus boxed keys) and ~20 levels for 1M entries in Java, versus 11 keys per node and ~7 levels in Rust. |
| `NavigableMap.floorEntry(k)` | `range(..=k).next_back()` | Rust composes it from `range`. |
| `PriorityQueue` (**min-heap** by natural order) | `BinaryHeap` (**max-heap**) | **The trap.** Porting a Java scheduler without `Reverse` runs the *latest* deadline first. |
| `Comparator.comparing(Job::deadline).thenComparing(Job::id)` | `#[derive(Ord)]` field order, or a manual `impl Ord` | In Java the ordering is visible at the construction site. In Rust it can hide in field order. |
| `ConcurrentSkipListMap` | no std equivalent | Part XI. |

> **Analogy limit.** Java's collections all hold references, so `TreeMap<Long, Order>` and `ArrayDeque<Order>` are
> both webs of pointers to separately allocated `Order` objects; switching between them changes the algorithm but not
> the memory model. In Rust, the collection *contains* the values, so switching collections changes the memory layout
> too. That's why Rust makes collection choice a performance decision in a way it rarely is in Java.

### 9. Production scenario

**The order-book redesign, delivered.** The Part III review ended with a Java-shaped order book (`Rc<RefCell<Order>>`
everywhere) and sketched a redesign. Meridian's matching-engine prototype now has:

```rust,ignore
struct Book {
    orders: slab::Slab<Order>,                  // arena: stable usize keys, contiguous storage
    bids: BTreeMap<i64, VecDeque<usize>>,       // price (ticks) → FIFO of slab keys
    asks: BTreeMap<i64, VecDeque<usize>>,
}
```

- **Add order:** insert into the slab (O(1) amortized), then push its key onto the level's `VecDeque`: `entry(price)
  .or_default().push_back(key)`.
- **Match:** take the best level with `first_entry`/`last_entry`, pop from the front of its queue, and remove the level
  when it's empty.
- **Cancel:** mark the order cancelled in the slab (O(1)) and leave its key in the queue. The matcher skips cancelled
  keys when it pops them (lazy deletion, the same idea as a heap without decrease-key). A background compaction pass
  rebuilds any level whose queue is more than half tombstones.

The design keeps all orders in one contiguous arena, keeps the levels ordered with an O(log L) best-price lookup (L,
the number of price levels, is usually in the hundreds), and has no reference counting and no `RefCell`. Its
performance claims are hypotheses until Part XX's benchmark; the structure is what Part III promised.

### 10. Failure scenario

**The scheduler that ran jobs by ID.** Meridian's settlement batcher uses an earliest-deadline-first queue:
`BinaryHeap<Reverse<Job>>` with `#[derive(Ord)] struct Job { deadline_ms: u64, id: u32, ... }`. A cleanup PR titled
"group identity fields first" reordered the struct to `{ id: u32, deadline_ms: u64, ... }`. It compiled and passed
review, and the unit tests passed, because every test used IDs in deadline order.

In production, the heap ordered jobs by ID. Old jobs with low IDs but late deadlines ran first, and urgent jobs
queued behind them. Settlement files for one partner missed their cutoff two nights in a row.

- **Detection:** the "deadline miss" counter, which had been zero for months, started ticking. A trace showed a job
  with a 5-minute deadline waiting behind a job with a 6-hour deadline.
- **Root cause:** [LANG] derived `Ord` is lexicographic in field declaration order. Reordering fields changed the
  scheduling policy, and nothing in the diff said so.
- **Fix:** replace the derive with an explicit `impl Ord for Job` that compares `(deadline_ms, id)` and says so in a
  comment, and add a property test that shuffles jobs and asserts pops come out in deadline order.
- **Lesson:** when ordering is *policy*, write it down explicitly. Derived `Ord` is for types whose natural order is
  obvious and stable (IDs, versions), not for business rules.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part IX).*

1. Draw a `VecDeque` whose contents wrap around. What does `as_slices` return, and when would you call
   `make_contiguous`?
2. Why is std's ordered map a B-tree and not a red-black tree? What does linear search inside a node cost and save?
3. Give the Big-O and the mechanism behind each winner: point lookup, full ordered scan, and range query in `BTreeMap`
   vs `HashMap`.
4. How do you get a min-heap from `BinaryHeap`? How do you keep the top k of a stream in O(k) memory?
5. Why doesn't `BinaryHeap` have decrease-key, and how does Dijkstra work around it?
6. Why is a freshly built `LinkedList` "only" ~5× slower to traverse than a `Vec`, while an aged one is ~50× slower?
7. What does the stable `LinkedList` API lack, and why does that matter for the usual argument in favor of linked lists?
8. What replaces a linked list for an LRU cache in Rust?
9. Where does the ordering of a `#[derive(Ord)]` struct come from, and when is that dangerous?

### 12. Exercises

- **Beginner.** Implement a bounded "last 100 latencies" buffer with `VecDeque` that never reallocates after
  construction. Assert it with the counting allocator.
- **Intermediate.** Add `cancel(order_key)` and `match_market(side, qty)` to the order-book scenario using lazy
  deletion. Write a test in which a cancelled order at the best price is skipped.
- **Advanced.** Implement Dijkstra on a 1,000 × 1,000 grid with random weights using `BinaryHeap<Reverse<(u32, u32)>>`
  and lazy deletion. Count how many stale entries were popped, and compare with the number of edges.
- **Systems.** Extend `ch04-03-btree-vs-hash.rs` with 32-byte `String` keys. Predict from the mechanism in Section 4 how
  the lookup ratio changes, then measure.
- **Architecture.** Find a `LinkedList` or `LinkedList`-shaped structure (Java `LinkedList`, a hand-rolled list) in a
  codebase you know. Write the replacement and the measurement that would justify the change.

### 13. Debugging exercise

```rust,ignore
use std::collections::BinaryHeap;

struct Task { priority: u8, name: String }
// (Ord implemented by priority only; higher = more urgent)

fn run_all(mut tasks: BinaryHeap<Task>) {
    for task in tasks.iter() {
        execute(task);
    }
    tasks.clear();
}
```

1. Tasks run in an order that looks random. Why?
2. Give two fixes, one of which consumes the heap and one of which doesn't.
3. The `Ord` impl compares only `priority`, while `PartialEq` is derived (compares both fields). What contract does that
   break, and what could go wrong?

### 14. Design exercise

**Time-series store for gateway metrics.** Meridian wants a small in-process store for per-route metrics: 5,000 routes,
one data point per route every 10 seconds, 24 hours of retention, queries of "route X between t1 and t2" and "the latest
value of all routes".

Compare three designs: `HashMap<RouteId, BTreeMap<Timestamp, Point>>`, `HashMap<RouteId, VecDeque<Point>>` (with
timestamps implied by position), and one `BTreeMap<(RouteId, Timestamp), Point>`. For each, compute the memory for 24
hours, the cost of inserting a point, of expiring old points, and of each query. Pick one and name the access pattern
that would change your choice.
