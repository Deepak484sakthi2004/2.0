# Appendix A — Answer Key: Part IX

> Model answers. Write yours first. Where several answers are defensible, the key says so. Numbers marked "arithmetic"
> come from layouts, not measurements; measured numbers cite their listing.

---

## Chapter 9.1 — Vec<T>: Pointer, Length, Capacity

### Interview & architecture questions

**1. Drawing a `Vec`.** Three words (capacity 8, pointer, length 5) inline, and a heap buffer of 8 × 8 bytes whose first
five slots are initialized. Guaranteed by the std docs: a `Vec` is a (pointer, capacity, length) triple "and always will
be"; its elements are contiguous and in order; `Vec::new()` doesn't allocate; the pointer is never null; it never shrinks
automatically. **Not** guaranteed: the order of the three fields (rustc currently uses cap, ptr, len, as the asm showed),
the growth strategy ("`Vec` does not guarantee any particular growth strategy"), and the exact capacity after
`reserve`.

**2. Growth on `push`.** [LIB] New capacity = `max(2 × cap, cap + 1)`, and at least 8 for 1-byte elements, 4 for elements
up to 1 KiB, 1 above that; then `realloc`. [LANG] Only amortized O(1) push and "doesn't allocate until needed" are
promised. The factor and the minimums are implementation choices (verified on 1.98.1 in `ch01-01-vec-growth.rs` and in
the `grow_amortized` assembly).

**3. Amortized O(1).** Building n elements by doubling copies at most n/2 + n/4 + … < n elements across all
reallocations, so total work is O(n) and the average per push is O(1). It doesn't bound a *single* push: the push that
reallocates copies the whole buffer (O(n)), which is a latency spike unless the allocator can grow in place or remap.

**4. `Option<Vec<T>>`.** The pointer field is `NonNull`, so the all-zero bit pattern (null) is never a valid `Vec`. The
compiler uses it as the `None` representation (a niche), so no separate discriminant is needed: 24 bytes (verified).

**5. `reserve` vs `reserve_exact`, `truncate` vs `shrink_to_fit`.** `reserve(n)` guarantees room for at least n more and
keeps amortized growth (10 → 20 for `reserve(5)` on a full `Vec` of 10, verified). `reserve_exact(n)` requests exactly
len + n (10 → 15), though the allocator may give more. `truncate` drops elements and keeps capacity; `shrink_to_fit`
reallocates down to about `len`.

**6. `remove` in a loop vs `retain`.** Each `remove(i)` shifts the tail left by one: O(n) per removal, so O(n²) for a
constant fraction of removals. `retain` makes one pass with a read index and a write index, moving each survivor at most
once: O(n). Measured: 56 ms vs 29 µs at n = 50,000.

**7. `collect` allocation.** One allocation when the iterator reports an exact size (`Range`, `map` over a slice
iterator, and so on). Growth when the lower bound is loose (`filter`: 11 allocations for 3,334 survivors, verified).
`vec.into_iter().map(f).collect::<Vec<_>>()` with the same element size and alignment reuses the source buffer: zero
allocations (a [LIB] specialization, verified).

**8. Where the bytes live.** `[T; N]`: all inline (stack or parent). `&[T]`: a 16-byte (ptr, len) view of someone else's
bytes. `Box<[T]>`: 16-byte handle plus exactly len elements on the heap. `Vec<T>`: 24-byte handle plus cap slots on the
heap. `SmallVec<[T; N]>`: up to N inline (48 bytes for N = 4 of `u64`, verified), heap after spilling.

**9. glibc growth.** Small blocks at the end of the heap often grow in place; blocks at or above the mmap threshold
(128 KiB by default) live in their own mappings and grow with `mremap`, which moves pages by editing page tables rather
than copying bytes (the chapter's inference from address changes; confirm with `strace`). Don't design around it:
musl, jemalloc, mimalloc, and Windows' heap all behave differently, and the policy can change with a libc upgrade.

**10. `Vec<u64>` vs `ArrayList<Long>`.** 8 MB vs roughly 4 MB of references plus a million 24-byte `Long` objects (16
with compact headers): ~28 MB, with a pointer chase per element. The analogy holds for the API and the growth model; it
breaks on memory layout, and for `Vec<String>` both designs are pointer-based anyway.

### Debugging exercise (`load_ids`)

1. The input `FF FF FF FF`: `count` = 4,294,967,295, so `with_capacity` asks for 34,359,738,360 bytes. That's below
   `isize::MAX`, so it's not a "capacity overflow" panic. It's an allocation request that fails, and allocation failure
   in `Vec` calls the allocation-error handler, which aborts. (A shorter input panics earlier: `bytes[0..4]` on fewer
   than 4 bytes.)
2. The allocation happens before the loop. The loop's `chunks_exact(8)` over an empty tail pushes nothing, but the
   damage is done.
3. Bound the capacity by what the input can actually contain:
   `let count = count.min(bytes.len().saturating_sub(4) / 8);` (and check `bytes.len() >= 4` first). Honest inputs
   still get exact preallocation. Optionally also reject `count` values that don't match the payload length.

### Selected exercises

- **Beginner.** `Vec<u16>`: [0, 4, 8, 16, 32, 64, 128], 6 allocations for 100 pushes (element size 2 ≤ 1 KiB → minimum
  4). `Vec<[u8; 1500]>`: [0, 1, 2, 4, 8, 16], 5 allocations for 10 pushes (element > 1 KiB → minimum 1).
- **Advanced.** Shrinking at "half full" thrashes: at the boundary, one pop shrinks, the next push grows, and every
  operation reallocates. Shrinking to half when the length falls below a *quarter* leaves the new buffer half full, so
  at least n/4 operations separate any two reallocations, preserving amortized O(1).

### Design exercise (market-data snapshots)

Arithmetic with a 48-byte `Trade`: `with_capacity(1000)` for all 40,000 instruments is 40,000 × 48,000 B ≈ 1.9 GB,
mostly empty. The same holds for a fixed `Box<[Trade; 1000]>` ring per instrument. Grow-on-demand fits the skew: 200 hot
instruments × ~49 KB ≈ 10 MB, plus quiet instruments at a few hundred bytes each ≈ 10–20 MB. But "keep the last 1,000"
needs cheap removal from the front, which `Vec::remove(0)` doesn't have (O(n)), so the good answer is a **`VecDeque`
grown on demand and capped at 1,000** (pop the front when full). The first trade on a quiet instrument costs one small
allocation. When an instrument goes quiet, keep its capacity (cheap for a small deque) or run a periodic
`shrink_to(len)` for deques that haven't grown in a day. Metric: total `capacity × 48` vs total `len × 48` across
instruments, exported, plus RSS.

---

## Chapter 9.2 — String and Text Encoding

### Interview & architecture questions

**1. `+` vs `format!`.** `impl Add<&str> for String` takes the left operand by value and appends to its buffer, so
`s = s + &t` is amortized O(|t|). Java's `s = s + t` builds a new `String` each time, copying the prefix: O(n²) over a
loop. Rust's quadratic trap is `format!("{s}{t}")`, which builds a new `String` containing all of `s` (measured: 80 ms
vs 0.4 ms for 10,000 rows).

**2. `format!` phases.** Compile time: `format_args!` (a compiler built-in) parses and validates the format string, and
the literal pieces become static data. Run time: an `fmt::Arguments` with a reference and a formatting-function pointer
per argument, driven through `&mut dyn fmt::Write`. One shared engine keeps binary size down; the price is indirect calls
and no per-call-site specialization.

**3. `from_utf8_lossy`.** It allocates only when the input is invalid (then it returns `Cow::Owned` with U+FFFD
inserted); valid input is borrowed (verified). Reject it in any path where the text is hashed, compared, stored,
signed, or used as an identifier. Accept it for display and logs.

**4. UTF-16 and lone surrogates.** A Java `String` is a sequence of UTF-16 code units and may contain unpaired
surrogates, which aren't Unicode scalar values, so they can't appear in a Rust `String`: `from_utf16` returns `Err`
(verified). On Windows, `OsString` stores WTF-8, which can encode lone surrogates, and converts losslessly via
`encode_wide` and `from_wide`.

**5. Case mapping.** The result's length isn't known in advance and can differ from the input, so a new `String` is
needed. ASCII mapping changes only `A`–`Z`, one byte to one byte, so it's in place. Length changes: `ß` → `SS` (one char
to two), `ﬁ` → `FI` (3 bytes to 2), `İ` → `i` + U+0307 (2 bytes to 3). Verified in `ch02-03-case-mapping.rs`.

**6. Modified UTF-8.** Java's internal serialization form: U+0000 as `C0 80`, and supplementary characters as two
3-byte surrogate encodings (CESU-8 style). You meet it in JNI's `GetStringUTFChars`/`NewStringUTF` and in
`DataOutputStream.writeUTF`/`DataInput.readUTF`. Rust's `from_utf8` rejects it for any emoji or NUL.

**7. String types.** `String` 24 B, heap = capacity, clone allocates. `Box<str>` 16 B, heap = exact length, clone
allocates. `Arc<str>` 16 B, one heap block (two counters plus bytes), clone = atomic increment (1 allocation for 1,000
clones' worth of `Vec`, verified). `Cow<str>` 24 B, borrowed or owned, clone depends on the variant.

**8. Currency codes.** An enum (`#[repr(u8)]`) if the set is closed and known at compile time, or a `[u8; 3]` if it must
accept new codes. 50M × `String` ≈ 50M × (24 + 32-byte heap chunk) ≈ 2.8 GB (arithmetic); `[u8; 3]` is 150 MB, an enum
50 MB, and neither allocates.

**9. Small strings.** When you have millions of short strings (most under ~23 bytes), and especially when they're
cloned often. Not when strings are usually long (every access pays the inline/heap branch for nothing) or when a
closed set allows an enum or an ID.

**10. `memchr`.** It speeds up searches for one to three distinct bytes (or a substring, via `memmem`) by checking 16–32
bytes per SIMD step: ~10× over byte loops in `ch02-05-memchr.rs`. Skip-searching pays off only when the targets are
rare. "Any digit" is ten bytes (more than `memchr3` handles) and, in logs, about half of all bytes (47% measured), so
there's nothing to skip. Use a 256-entry class table, or SIMD class matching, and keep `memchr`/`memmem` for the rare
triggers (`@`, `"Bearer "`).

### Debugging exercise (`is_hop_by_hop`)

1. One allocation per call (`to_lowercase`), and it's called once per header: ~10–15 allocations per request, on every
   request.
2. `to_lowercase` maps the Kelvin sign U+212A to ASCII `k`, so a header name containing it (for example
   `"\u{212A}eep-Alive"`) matches `keep-alive`. HTTP field names are ASCII tokens, so a strict parser must reject that
   name. Two components that disagree on whether a header is hop-by-hop is the raw material of request-smuggling bugs.
3. ```rust,ignore
   fn is_hop_by_hop(name: &str) -> bool {
       ["connection", "keep-alive", "transfer-encoding", "upgrade"]
           .iter()
           .any(|h| name.eq_ignore_ascii_case(h))
   }
   ```
   And normalize in place with `make_ascii_lowercase` where an owned name is already available (or return
   `Cow<str>`, borrowing when the name is already lowercase).

### Selected exercises

- **Beginner.** Make the key structured: `HashMap<(u32, u64), V>` with `cache.get(&(tenant, user_id))`: no
  allocation, and it's also a smaller key. If the key must be a string (it's shared with another system), keep a
  per-thread `String` buffer, `clear()` it, `write!` the key into it, and look up with `&buf`.
- **Advanced.** Scan for `b'A'..=b'Z'`; if none, return `Cow::Borrowed(s)`. Otherwise `let mut o = s.to_owned();
  o.make_ascii_lowercase(); Cow::Owned(o)`: exactly one allocation.

### Design exercise (statement export)

A strong answer: take text from the ledger over a boundary with a defined encoding (FFM with standard UTF-8, or
Protobuf/JSON, which are UTF-8 by specification, never JNI modified UTF-8); borrow row text from the decoded batch buffer
while rendering; format each line with `write!` into a per-worker `String` (or directly into a `BufWriter`) with a
capacity cap; render amounts from integer minor units with explicit per-currency decimal places; match merchant names
with Unicode case *folding* plus NFC normalization (not `to_lowercase` equality), and use ICU4X where a locale matters
(Turkish dotted and dotless i). The budget: allocations per statement ≈ a constant (the output buffer) rather than
proportional to the line count. Test with golden files per locale, including Turkish `İ/ı`, Greek final sigma, and
supplementary-plane characters.

---

## Chapter 9.3 — HashMap and HashSet

### Interview & architecture questions

**1. SwissTable.** One allocation: buckets of `(K, V)` and one control byte per bucket (EMPTY, DELETED, or FULL with 7
bits of hash). `h1` (low bits) picks the starting group; `h2` (top 7 bits) is stored in the control byte. A lookup loads
16 control bytes, compares all of them with `h2` in one SIMD compare (`pcmpeqb`) and turns the result into a bitmask
(`pmovmskb`), checks the keys of the matching slots only, and stops at any EMPTY byte; otherwise it probes the next
group.

**2. Capacity 1,792.** 1,000 × 8/7 = 1,142.9 → 1,143 buckets needed → rounded to a power of two, 2,048 → usable
capacity 2,048 × 7/8 = 1,792.

**3. Rehashing.** hashbrown doesn't store hashes (saving memory per bucket), so moving an entry to a bigger table means
recomputing its hash. Java stores `hash` in each `Node` and splits each bin by one extra bit on resize. It matters when
hashing is expensive (SipHash over long strings) and when a big map resizes on a latency-sensitive path: presize.
Measured: 2,788 hashes for 1,000 inserts from empty, 1,000 when presized.

**4. Hashes and allocations** (`ch03-04-entry-hash-count.rs`, 10,000 lookups, 100 distinct keys):

| Pattern | Hashes | Allocations |
|---|---|---|
| `get_mut` + `insert` | 1 per hit, 2 per miss (10,099) | 1 per miss (100) |
| `entry(k.to_string())` | 1 per call (10,000) | 1 per call (10,000) |
| hashbrown `entry_ref(&k)` | 1 per call (10,000) | 1 per miss (100) |

For 99% hits: `get_mut` + `insert` with std, or `entry_ref` with hashbrown. `entry` is right when the key is already
owned (you'd move it in anyway) or cheap to construct (`u64`).

**5. `HashMap<f64, V>`.** `HashMap::new` has no bounds; `insert` lives in an `impl<K: Eq + Hash, V, S: BuildHasher>`
block, so the method exists but its bounds fail: E0599. Fixes: integer minor units (money, prices: almost always right),
or `OrderedFloat<f64>` when float identity is really the key (for example, memoizing a function of a float), with its
NaN-equals-NaN and `-0.0 == 0.0` semantics understood.

**6. HashDoS.** An attacker who can predict the hash sends keys that collide, turning a hash table into a list. Java
treeifies bins at 8 entries (O(log n) worst case, for `Comparable` keys); Rust uses a keyed hash so collisions can't
be computed offline. hashbrown with an unkeyed hasher and crafted keys goes quadratic: measured ×4 time per doubling of
n with FxHash and keys `i << 32` (7.5 → 30 → 132 ms), while SipHash with the same keys stayed linear.

**7. FxHash.** Appropriate for keys an attacker can't choose: compiler-internal IDs, indices you allocate, keys
generated inside your trust boundary. A vulnerability when keys come from requests, files, or partners, or when a
"harmless" upstream format lets outsiders influence them.

**8. `HashMap<[u8; 16], [u8; 200]>`, 2M entries.** Buckets = next_pow2(2M × 8/7) = 4,194,304; × (216 + 1) ≈ 910 MB
(arithmetic). Alternatives: box the value (~105 MB of table + ~416 MB of values ≈ 520 MB, and a pointer chase per
access), `IndexMap` (~448 MB dense entries + ~38 MB index ≈ 490 MB, and insertion order for free), or a `Vec` of values
plus a `HashMap<[u8; 16], u32>` index (≈ 400 MB + 4,194,304 × 21 B ≈ 90 MB, and stable u32 handles, but deletion needs
a free list).

**9. Lying `Hash`.** std's map is written so that its `unsafe` internals never depend on `Hash`/`Eq` being correct for
memory safety: probing stays in bounds and every slot it reads is initialized regardless of what the hash says. A
wrong hash only makes it look in the wrong place, which gives wrong answers, not UB. std documents such misuse as a
logic error with unspecified (but safe) behavior.

**10. Concurrent maps.** `Mutex`/`RwLock<HashMap>`; sharding by key hash (`dashmap` packages it); `ArcSwap<HashMap>`
snapshots; or one owner thread with messages. For a read-mostly config table: `ArcSwap<HashMap<..>>`, where readers load
a snapshot without locking and writers build a new map and swap it in (Chapter 3.1's route table).

### Debugging exercise (`CacheKey`)

1. `request_id` is unique per request, so no lookup ever finds an earlier entry (0% hits), and every request inserts a
   new entry that's never hit again. The map grows until OOM.
2. ```rust,ignore
   impl PartialEq for CacheKey {
       fn eq(&self, o: &Self) -> bool { self.tenant == o.tenant && self.path == o.path }
   }
   impl Eq for CacheKey {}
   impl Hash for CacheKey {
       fn hash<H: Hasher>(&self, h: &mut H) { self.tenant.hash(h); self.path.hash(h); }
   }
   ```
   Invariant: `a == b` ⇒ `hash(a) == hash(b)`. Both impls must ignore exactly the same fields. Better: take
   `request_id` out of the key entirely and log it elsewhere.
3. Options: intern paths to `u32` at startup and key by `(u32 tenant, u32 path_id)` (a `Copy` key, no allocation); or
   nest maps, `HashMap<u32, HashMap<String, Response>>`, and query `.get(&tenant)?.get(path)` with `&str`, which
   `Borrow<str>` allows; or use hashbrown's `Equivalent` trait to query a `(u32, String)`-keyed map with a borrowed
   `(u32, &str)`-shaped type.

### Selected exercises

- **Beginner.** `HashSet<u32>`, 100 inserts: capacities [0, 3, 7, 14, 28, 56, 112], 6 allocations (the same
  bucket-count rule; element size doesn't change it).
- **Advanced.** An identity hasher is safe only when key bits are uniformly random and *unpredictable to clients*:
  server-generated 128-bit random IDs qualify (take the low 64 bits). If IDs become client-chosen, sequential, or
  time-based (UUIDv7's leading timestamp bits), the low bits are no longer random and the map degrades, or can be
  attacked.

### Design exercise (idempotency keys)

Per node: 300M/12 = 25M keys per day. Represent keys as 16-byte binary UUIDs (parse the 36 characters once; reject
malformed input). Value: a `u64` response reference plus a `u32` minute timestamp. Entry ≈ 32 bytes after padding.
Table: next_pow2(25M × 8/7) = 33,554,432 buckets × 33 B ≈ 1.1 GB (arithmetic). Hasher: keyed (clients choose the keys).
Expiry without scanning: 24 hourly maps in a `VecDeque<HashMap<..>>`; every hour, drop the oldest map (one deallocation
cascade, ideally on a background thread) and push a new presized one. Lookups check up to 24 maps, newest first, which is
the price of scan-free expiry. The alternative is one map plus an expiry queue of keys, with tombstones. Java's
`ConcurrentHashMap<String, Long>` with the same data: roughly `String` (24) + its `byte[]` (~56) + `Long` (24) + `Node`
(32) + table slot ≈ 140 B per entry, ≈ 3.5 GB per node. Measure first: the resident memory of the chosen layout filled
with 25M synthetic keys, and the p99 of lookups during an hourly rotation.

---

## Chapter 9.4 — VecDeque, BTreeMap, BinaryHeap — and Why Not LinkedList

### Interview & architecture questions

**1. Wrapped `VecDeque`.** With cap 8, head at 2, and 7 elements, the logical sequence occupies slots 2–7 then 0:
`as_slices` returns `([3..8], [9])` (verified). Call `make_contiguous` when you need one `&[T]`: sorting, binary
search, or a single `write_all`.

**2. B-tree, not red-black.** Up to 11 keys per node means ~6–7 levels for a million entries, each level one or two
cache lines of contiguous keys, instead of ~20 dependent pointer loads. The linear scan inside a node is
branch-predictable and fast for 11 small keys, but it calls `Ord::cmp` up to 11 times per level, which hurts with
expensive comparisons (long strings).

**3. BTreeMap vs HashMap** (`ch04-03-btree-vs-hash.rs`): point lookup, HashMap wins (O(1), ~1 probe vs ~7 levels; 3.3×);
full ordered scan, BTreeMap wins (already sorted, O(n), vs collect + sort O(n log n); 18×); range query, BTreeMap wins
(O(log n + k) vs O(n) full scan; ~280× for 1,000 of 1M keys).

**4. Min-heap and top-k.** Wrap elements in `Reverse`. Top k largest: keep a `BinaryHeap<Reverse<T>>` of size k; for
each new item larger than the heap's minimum, pop and push. O(n log k) time, O(k) memory.

**5. No decrease-key.** A binary heap has no handle to find an arbitrary element's position (that would need an extra
index map updated on every swap). Dijkstra pushes a new `(distance, node)` whenever it finds a shorter distance, and on
pop skips entries whose distance is worse than the best known (lazy deletion). The heap holds up to O(E) entries.

**6. Fresh vs aged list.** Fresh nodes were allocated consecutively, so they sit at increasing, nearby addresses: the
prefetcher partly hides the misses, and only the dependency chain costs (~4.7×). Aged nodes are scattered among other
live allocations: most steps miss the cache, some miss the TLB, and the dependent loads serialize those misses (~47×).

**7. Stable `LinkedList` API.** No cursors (feature-gated on 1.98.1, verified E0658), so no O(1) insert or remove at a
position you hold. The classic argument for linked lists ("O(1) removal in the middle") therefore doesn't apply on
stable Rust at all.

**8. LRU without a linked list.** An index-linked list threaded through a `Vec` (an arena of entries with `prev`/`next`
indices) plus a `HashMap<K, index>` (Chapter 3.6), or the `lru` crate. O(1) operations, contiguous memory, no `unsafe`
in your code.

**9. Derived `Ord`.** Lexicographic in field declaration order [LANG]. Dangerous when the order encodes a business rule
(priority, deadline): reordering fields for readability silently changes the policy (Section 10's incident).

### Debugging exercise (`run_all`)

1. `BinaryHeap::iter` visits the backing `Vec` in heap (level) order, which is only partially ordered.
2. Consuming: `while let Some(t) = tasks.pop() { execute(&t) }` (priority order), or `tasks.into_sorted_vec()` iterated
   in reverse. Non-consuming: `let mut v: Vec<&Task> = tasks.iter().collect(); v.sort_by(|a, b| b.cmp(a));` then
   iterate.
3. `Ord` must agree with `PartialEq`: `a.cmp(b) == Equal` if and only if `a == b`. Here two tasks with the same priority
   and different names compare `Equal` but aren't `==`. That's a logic error: sorting, `dedup`, `BTreeSet` membership,
   and binary search can give inconsistent answers (never UB). Derive both, or implement both on the same fields.

### Selected exercises

- **Beginner.** `VecDeque::with_capacity(100)`; before each `push_back`, if `len() == 100`, `pop_front()`. The counting
  allocator shows zero allocations after construction.
- **Advanced.** Expect stale pops to be a significant fraction of pushes on dense random-weight grids; each edge
  relaxation can push once, so the heap is bounded by E, not V. The count, not the prediction, is the answer.

### Design exercise (time-series store)

5,000 routes × 8,640 points per day (every 10 s) = 43.2M points. (a) `HashMap<RouteId, BTreeMap<Ts, Point>>`: per point
~(8 + 16)/fill + node overhead ≈ 35–45 B → ~1.5–2 GB; insert O(log n); expiry needs per-route `pop_first` loops; range
queries O(log n + k). (b) `HashMap<RouteId, VecDeque<Point>>` with implied timestamps (a fixed interval, sentinel values
for gaps): 16 B × 43.2M ≈ 690 MB (8-byte values alone ≈ 345 MB); insert and expire O(1); range query by index
arithmetic, O(k); latest O(1). (c) One `BTreeMap<(RouteId, Ts), Point>`: similar memory to (a), range per route works
(`range((r, t1)..(r, t2))`), "latest of all routes" needs 5,000 `range(..).next_back()` calls, and expiry is a scan.
Choose (b) for fixed-interval metrics. Irregular timestamps or sparse series push you to (a).

---

## Chapter 9.5 — Choosing Collections by Cache Behavior

### Interview & architecture questions

**1. The three rules.** Bytes touched: AoS vs SoA (12× for a one-field sum). Dependent loads: aged `LinkedList` (47×)
and binary search losing to a hash map at 1M keys. Predictability: shuffled vs in-order boxes (37× vs 5×).

**2. 12× vs 3.8×.** A one-field sum touches 1/8 of AoS's bytes in SoA form and vectorizes; a two-field product touches
2/8, so the traffic ratio is 4×, close to the measured 3.8×. (The AoS one-field loop being slower than the AoS two-field
loop is a codegen anomaly the chapter leaves as an exercise.)

**3. Boxes.** In order: 32-byte allocator chunks at sequential addresses, so the prefetcher streams them; the cost is
~5× the bytes (8-byte pointer + 32-byte chunk per 8 bytes of data). Shuffled: random addresses across ~128 MB defeat the
prefetcher and the TLB. The loads are independent, so misses overlap, but most still go far down the hierarchy.

**4. Binary search vs HashMap.** At 1M keys, binary search makes ~20 *dependent* loads per lookup, the last ~10 missing
cache, and mispredicts about half its branches; the hash map computes a hash and touches 1–2 lines. At 1,000 keys,
everything is in L1/L2, so it's computation: ~10 mispredicted branches vs ~20 ns of SipHash, about even (8.9 vs 7.8 ms).

**5. "7 ns per lookup".** It's throughput: the benchmark loop issues independent lookups, and the CPU overlaps their
misses. A single lookup on a request's critical path, against cold memory, sees the full miss latency (tens to a
hundred-plus ns per miss).

**6. Dense index.** When keys are small, dense integers (sequential IDs, codes, enum discriminants). Memory is
proportional to the key *range*, not the key count: stride-3 keys cost 24 bytes per real entry in the listing, and a
sparse 64-bit key space makes it impossible. Map sparse IDs to dense indices once, at the boundary.

**7. GC and locality.** Copying and compacting collectors move live objects together, often in an order that follows
references, which can restore locality after churn. Rust allocators never move objects, so fragmentation persists
until you rebuild the structure yourself.

**8. `Vec<Box<T>>` anyway.** Large `T` that's moved around often (moving 8 bytes instead of kilobytes), stable addresses
across `Vec` growth, trait objects (`Box<dyn Trait>`), and recursive types.

**9. SoA is wrong** when the hot path reads whole records (each record touches one line per column), when records are
inserted and removed individually at high rates (every column must be updated), and when collections are small enough
to fit in cache anyway.

### Debugging exercise (`Vec<Box<Session>>`)

1. The pointers' order no longer matches memory order. `swap_remove` moves the last pointer into each gap, and new
   sessions are allocated wherever the allocator has free chunks, so after a week the scan follows random addresses:
   cache and TLB misses instead of a sequential stream.
2. A compacting GC periodically relocates live `Session` objects next to each other, restoring locality (and Java's
   `ArrayList<Session>` would have the same pointer-array shape, but the objects would get moved back together).
3. Keep the boxes and compact manually: periodically (or when scan time exceeds a threshold) rebuild with fresh boxes
   allocated in `Vec` order (an O(n) pass with n allocations). Or remove the boxes: `Vec<Session>` inline, so
   `swap_remove` moves a whole `Session` (still O(1)), the scan is sequential forever, and you lose stable addresses (use
   indices or a `slab`).

### Selected exercises

- **Beginner.** `struct A { a: u8, b: u64, c: u8 }` → 16 (rustc reorders to `b, a, c`, plus padding to 8).
  `(u64, u32)` → 16. `[(u64, u32); 4]` → 64.
- **Intermediate.** Expect a linear scan of contiguous `(u64, u32)` pairs to win at small n (a few dozen entries, since
  it's branch-predictable and vectorizable) and lose as n grows. The measured crossover on your machine is the answer.

### Design exercise (merchant risk counters)

AoS per merchant: volume `i64`, count `u32`, last seen `u32` (seconds from an epoch offset), limit `i64` = 24 B → 3M ×
24 ≈ 72 MB (arithmetic). The payment path reads and writes all four fields of one merchant: AoS gives one cache line.
The scan reads volume and limit (16 of 24 bytes): AoS is still efficient. SoA buys little for the scan and costs the
payment path. Merchant IDs from outside are sparse, so map them to dense `u32` indices at onboarding (a `HashMap` lookup
per payment, or carry the index in the request after the first lookup). Keep the scan off the payment path: better,
flag "crossed 80%" on the payment path itself (one comparison), so no scan is needed. If you do scan, run it on its own
core, knowing that streaming 72 MB will evict L3 lines the payment path uses. Falsifier: the payment p99 during the
scan vs outside it.

---

## Interlude — The Trade-off Engine: BFS vs DFS

### Interview & architecture questions

**1. The architect's "why BFS".** Choose the frontier *order* by the answer (FIFO when distance matters: shortest
unweighted paths, "within k hops"; LIFO when DFS structure matters: post-order, cycles). Choose its *location* (call
stack: fixed, abort on overflow; heap: growable). Know its *bound* (DFS: depth; BFS: widest level) and whether input
controls that bound.

**2. Measuring a frame.** Record a local's address at two depths and divide the difference by the depth difference
(`intl-01-frame-size.rs`: 224 B debug, 96 B release). Debug keeps every local and temporary in its own stack slot;
release keeps values in registers and saves only the callee-saved registers it needs.

**3. Overflow, step by step.** The next frame's first write (or a stack probe) hits the guard page → page fault →
`SIGSEGV` → std's handler runs on the alternate signal stack → it finds the fault address in the thread's guard range →
prints "thread '…' has overflowed its stack" and "fatal runtime error: stack overflow, aborting" → `abort`.

**4. Stack probes.** Frames larger than a page touch each page in order (`sub rsp, 4096; mov qword ptr [rsp], 0`,
verified). Without them, a large frame could move `rsp` past the guard page and write into whatever is mapped below
(a "stack clash"): silent memory corruption instead of a clean abort.

**5. Mark-on-push isn't DFS.** It discovers nodes in a different tree (verified: node 3's parent is 0 instead of 2) and
has no post-order, so it can't produce a topological order or distinguish back edges (cycles) from cross edges. It's
fine for plain reachability.

**6. Peak frontiers.** Binary tree of depth 20: BFS 2²⁰ = 1,048,576 entries (4 MiB of `u32`); explicit DFS 21 frames
(168 B with `(node, edge)` frames). Chain of 1M: BFS 1 entry; `(node, edge)` DFS 1M frames = 7.8 MiB (verified); a
recursive DFS would need ~92 MiB of stack at 96 B per frame.

**7. Depth-limited DFS and "within k hops".** DFS marks a node visited the first time it reaches it, possibly through a
long path, and then prunes; a shorter path found later is ignored, so nodes within k hops can be reported at the wrong
distance or cut off by the limit.

**8. Stack sizes.** Rust spawned threads 2 MiB; Linux main typically 8 MiB; Windows main 1 MiB; macOS main 8 MiB;
Tokio workers 2 MiB; HotSpot threads 1 MiB. Consequence: the same function passes on one thread or OS and aborts on
another, and `RUST_MIN_STACK` in one environment changes behavior without a code change.

**9. `StackOverflowError`.** It can be thrown in the middle of any method, leaving data structures half-updated and
locks in unexpected states; recovery code may itself overflow. Catching it rarely produces a correct program. The real
fix, bounding depth, is the same in both languages.

**10. Recursion is right** when depth is bounded by construction (balanced trees: O(log n); grammars with enforced
depth limits; your own data structures) and clarity matters, or when input depth is validated against a limit well
under the thread's stack.

### Debugging exercise (Windows overflow)

1. In drop glue: dropping the head `Box<Node>` drops its `next: Option<Box<Node>>`, which drops the next node, and so
   on: 30,000 nested drop calls (Chapter 3.5's glue, applied recursively).
2. The main thread's stack is typically 8 MiB on Linux and 1 MiB on Windows.
3. Implement `Drop` iteratively: `let mut cur = self.head.take(); while let Some(mut node) = cur { cur =
   node.next.take(); }`. Each node is dropped with its `next` already detached, so every drop is one level deep: constant
   stack, on every platform.

### Selected exercises

- **Beginner.** Expect about 96 + 128 = 224 bytes in release (the array may also change register saves, so measure), so
  the 2 MiB limit falls to roughly 9,000 levels.
- **Intermediate.** Use three states: unvisited, on stack (gray), done (black). An edge to a gray node is a back edge:
  a cycle, and the frames on the stack from that node to the top *are* the cycle's path. Mark black when the frame
  pops.

### Design exercise (dependency resolver)

DFS with `(node, edge)` frames on the heap, three-color marking, post-order for the resolution order, and cycle errors
that print the frame stack's path. The frontier is at most the longest path (≤ 40,000 frames × 8 B = 320 KB), so the
library behaves the same on a 2 MiB Tokio worker and a CLI main thread. (For CPU-heavy resolution on the build server,
run it via `spawn_blocking`, Part XIII.) Kahn's algorithm (BFS over in-degrees) gives the order without recursion too,
but reporting the cycle's path is harder. Recursive DFS is fine at depth 15 and at risk on the 9,000-deep legacy chain:
the Interlude's small `dfs` already topped out near 9,400 levels on a 2 MiB stack in debug, and a real resolver's frames
are larger. The first measurement: maximum depth and peak frontier on the real package graph.

---

## Part IX Review — the velocity store

### Task A — issues

1. **`LinkedList<Event>` per merchant**: one allocation per event, ~96-byte nodes, pointer chasing on every expiry
   (9.4). Use a `VecDeque`.
2. **Expiry runs only for the merchant being recorded.** Quiet merchants keep stale events forever: unbounded memory and
   wrong counts. *Visible in the output:* `coffee-42`'s two events are an hour old but still count, so "top count 2".
3. **`amounts_by_merchant` is never trimmed**: totals include expired events and grow forever. *Visible:* `books-7`
   totals 20.25, though only the 8.25 event is inside the window.
4. **`Vec<Box<f64>>`**: an allocation per amount and 40 bytes per 8 (9.5). Keep a running sum instead.
5. **`f64` for money** (2.3): use `i64` minor units.
6. **`format!("{}", e.merchant)`**: an allocation to copy a `String` (9.2); and it shouldn't be copied at all.
7. **`entry(key.clone())`** allocates an owned key on every event even for known merchants (9.3); plus a second lookup
   with `get_mut(&e.merchant).unwrap()` (an extra hash).
8. **`e.clone()`** copies two `String`s per event when `e` could be moved; the merchant name ends up stored three
   times per event (map key, event, graph).
9. **`card_graph`** appends a merchant `String` per *event*, not per distinct pair: duplicates without bound, never
   expired.
10. **`hot_merchants` is rebuilt on every event**: 150,000 `String` clones and an O(M log M) sort per event, 2,000
    times a second. That's about 300 million allocations per second: the service can't keep up. Maintain counts and
    compute the top 10 on demand (or every second) with a bounded `BinaryHeap`.
11. **Ties in `hot_merchants`** come out in `HashMap` iteration order, which differs per map instance: nondeterministic
    output across pods and runs.
12. **`linked_merchants` recurses** with input-controlled depth (fraud rings are exactly long chains): a stack overflow
    abort takes down the process (Interlude).
13. **`seen: Vec` with `contains`**: O(n²) membership.
14. **The inner loop scans all of `card_graph`** for every merchant visited: O(M × C × L) with 5 million cards. It needs
    a reverse index (merchant → cards) and a BFS with a hop limit and a frontier cap.
15. **`card_hash` as a 64-character hex `String`**: 24 + 80 bytes (heap chunk) per copy instead of `[u8; 32]` or an
    interned `u32`.
16. No capacity policies and no bounds anywhere; nothing limits merchants, cards, or events.

### Task B — memory budget (arithmetic, order-of-magnitude)

7.2M events per hour. Per event: a list node (~80 B → 96-byte chunk) + the merchant name's heap copy (~32 B) + the card
hash (~80 B) ≈ 210 B; amounts ≈ 44 B (pointer + 32-byte box + `Vec` slack); graph ≈ 60 B (a `String` in a `Vec` +
heap). After **one hour**: events ≈ 1.5 GB, amounts ≈ 0.3 GB, graph ≈ 0.45 GB, card keys ≈ 0.2 GB: **~2.5 GB**. After
**one day**: events stay near the window (plus stale events for quiet merchants), but amounts (172.8M × 44 B ≈ 7.6 GB)
and the graph (≈ 10 GB, plus ~1 GB of card keys) grow without bound: **~20 GB**, so OOM within hours. In practice, the
CPU cost of Task A #10 stops the service first.

### Task C — a redesign

```rust,ignore
pub struct VelocityStore {
    merchant_ids: HashMap<Box<str>, u32>,      // interner (keyed hasher: names come from outside)
    merchant_names: Vec<Box<str>>,
    card_ids: HashMap<[u8; 32], u32>,          // binary card hash; rebuilt daily (generational)
    window: VecDeque<(u32 /*ts_s*/, u32 /*merchant*/, i64 /*cents*/)>, // global, time-ordered
    counts: Vec<u32>,                          // per merchant, in window
    sums: Vec<i64>,                            // per merchant, in window, minor units
    merchant_cards: Vec<SmallVec<[u32; 4]>>,   // merchant -> distinct cards
    card_merchants: Vec<SmallVec<[u32; 4]>>,   // card -> distinct merchants
    frontier: VecDeque<(u32, u8)>,             // reused BFS state
    visited: Vec<u32>,                         // generation-stamped, reused
    generation: u32,
}

impl VelocityStore {
    pub fn record(&mut self, merchant: &str, card: [u8; 32], cents: i64, ts_s: u32) {
        let m = self.intern_merchant(merchant);   // get(&str) first; allocate only for a new merchant
        let c = self.intern_card(card);
        self.window.push_back((ts_s, m, cents));
        self.counts[m as usize] += 1;
        self.sums[m as usize] += cents;
        while let Some(&(t, om, oc)) = self.window.front() {
            if t + 3600 > ts_s { break; }
            self.window.pop_front();              // expires EVERY merchant's old events
            self.counts[om as usize] -= 1;
            self.sums[om as usize] -= oc;
        }
        self.link(m, c);                          // push to both SmallVecs only if the pair is new
    }
}
```

The top 10 is computed on demand with a size-10 `BinaryHeap<Reverse<(u32, u32)>>` over `counts` (O(M log 10), or cached
per second), with ties broken by merchant ID for determinism. The linked search is a BFS over the bipartite graph with
`max_hops` and `max_frontier`, using the reused `frontier` and generation-stamped `visited`. This assumes events arrive
in time order; with out-of-order arrivals, use per-merchant `VecDeque`s or a small reordering buffer.

Budget (arithmetic): window 7.2M × 16 B ≈ 115 MB (presize 8M; up to 2× if it grows); per-merchant vectors ≈ 2 MB;
interner ≈ 10–15 MB; cards ≈ 5M × ~60 B ≈ 300 MB per day with a daily generational rebuild; graph edges ≈ distinct
pairs × 8 B. That's **~0.5 GB, bounded by retention**. Allocations per `record` in steady state: zero, amortized (new
merchants, cards, and edges excepted).

### Task D — window structure (condensed)

| Criterion | `LinkedList<Event>` per merchant | `VecDeque<(ts, cents)>` per merchant | one global `VecDeque` + counters |
|---|---|---|---|
| Memory | ~210 B/event | 16 B/event + a deque per merchant | 16 B/event, one buffer |
| CPU | a pointer chase per step | sequential | sequential |
| Latency | an allocator call per event | occasional growth per merchant | occasional growth, one buffer |
| Throughput | allocator-bound | good | best |
| Contention | allocator | none | none (single owner) |
| Cache | misses per node | contiguous per merchant | contiguous |
| Allocation | per event | ~log per merchant | ~log total |
| Complexity | low | low | low; needs time-ordered input |
| Safety | safe | safe | safe |
| Maintainability | clear, wrong expiry | clear | clear |
| Failure modes | memory growth, stale merchants | stale quiet merchants unless swept | out-of-order events break expiry |
| Operational | RSS grows with churn | RSS ∝ merchants × window | RSS ∝ events in window |

Choose the global `VecDeque` if the input is time-ordered (it expires everything in O(1) amortized); otherwise use
per-merchant deques plus a periodic sweep.

---

## Interview mode — model answers (short)

1. See Chapter 9.1 Q1–Q3: compare len with cap, store, increment; `grow_one` → `grow_amortized` (max(2·cap, cap+1),
   minimum 4 for `u64`) → `finish_grow` (overflow check against `isize::MAX`, `realloc` or `alloc`, abort on failure).
   Guaranteed: the triple, contiguity, amortized O(1). Not guaranteed: field order and growth factor.
2. Retained capacity: a `Vec`, `String`, `HashMap`, or `VecDeque` that was `clear()`ed after a spike. Confirm by
   exporting `capacity()` (or `capacity × size_of`) for long-lived buffers, and fix with a capacity policy.
3. Each `format!` builds its own `String` (≥1 allocation, ~1.9 each for short rows because of the capacity heuristic).
   Write with `write!` into one buffer.
4. JNI `GetStringUTFChars` gives Modified UTF-8 (fails `from_utf8` for emoji and NUL); lone surrogates can't become
   Rust strings; `lossy` corrupts silently. Use `GetStringChars` + `from_utf16`, or FFM's standard UTF-8.
5. Chapter 9.3 Q1.
6. Replace SipHash when profiling shows hashing matters and keys are trusted (FxHash) or when a keyed fast hasher's
   guarantees suffice (foldhash, ahash). The PR must answer: "Can anyone outside our trust boundary influence these
   keys?"
7. Chapter 9.3 Q4 table.
8. Chapter 9.4 Q2.
9. Chapter 9.4: 1M allocations vs 1; 4.7× to 47× slower traversal; no stable cursors. Remaining use: O(1) splicing of
   whole lists (`append`) with no traversal.
10. At 1,000 keys they tied; at 1M the hash map won by ~3× (74 vs 215 ns per lookup), because binary search's ~20 loads
    are dependent and its branches mispredict. Locality favors the hash map's 1–2 lines per lookup.
11. Throughput under overlap, not single-lookup latency (Chapter 9.5 Q5).
12. Build profile (224 vs 96 B frames), thread type (main vs spawned vs Tokio worker), OS (1 MiB Windows main),
    `RUST_MIN_STACK`, `ulimit -s`, inlining changes from code edits, input depth.
13. Guard page + `SIGSEGV` handler on an alternate stack + stack probes for large frames (Interlude Q3–Q4).
14. Interlude Q1.
15. Use the Interlude's twelve criteria: Memory, CPU, Latency, Throughput, Contention, Cache, Allocation, Complexity,
    Safety, Maintainability, Failure modes, Operational implications, each filled with this Part's numbers.
