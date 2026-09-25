# Chapter 9.3 — HashMap and HashSet

> **Where this sits:** Part IX · Collections and Memory · chapter 3 of 5
> **Prerequisites:** Chapter 9.1 (growth and capacity), Chapter 4.2 (the entry API and problem case #3), Project L1
> (logstat's per-path counters).
> **After this chapter you can:** draw a SwissTable (control bytes, groups, h1/h2) and explain a lookup in SIMD terms;
> predict a map's capacity, allocations, and rehash cost; choose a hasher with HashDoS in mind; count the hashes and
> allocations of `get_mut` + `insert` versus `entry`; key maps by floats correctly; and estimate a large map's memory
> from its layout.

---

## Pass 1 · User level — *One allocation, open addressing, a keyed hash*

### 1. Problem

Project L1's `logstat` counted requests per path with `get_mut` followed by `insert` on a miss, and promised to explain
two things later: why that pattern instead of the entry API, and why its `HashMap` uses SipHash, "a DoS-resistant but
not the fastest hasher". Chapter 4.2 claimed the entry API does "one lookup instead of up to three". This chapter backs
up all three claims with measurements, and gives you the internals you need to reason about a map holding millions of
entries: its memory, its growth, its worst case, and its hasher.

### 2. Mental model

[LIB] Since Rust 1.36, `std::collections::HashMap` is the `hashbrown` crate, a Rust port of Google's **SwissTable**.
One allocation holds everything:

```text
                 one heap allocation
 ┌──────────────────────────────────────────────┬─────────────────────────────────────┐
 │ buckets: (K, V) slots, stored in reverse      │ control bytes: 1 per bucket         │
 │  ... │ (K,V)₃ │ (K,V)₂ │ (K,V)₁ │ (K,V)₀ │     │ c₀ c₁ c₂ c₃ ... c_{n-1} + 16 copies │
 └──────────────────────────────────────────────┴─────────────────────────────────────┘
                                          ▲ ctrl pointer points here; bucket i is at ctrl - (i+1)·size

 control byte:  0xFF = EMPTY    0x80 = DELETED (tombstone)    0x00..0x7F = FULL, holding h2 (7 bits of the hash)

 hash (64 bits) ──► h1 = low bits & bucket_mask   → which group to start probing at
                └─► h2 = top 7 bits              → stored in the control byte, used as a filter
```

A lookup:

1. Hash the key. `h1` picks a starting position.
2. Load **16 control bytes** at once (one SSE2 register on x86-64 [CPU]). Compare all 16 with `h2` in one instruction,
   giving a bitmask of candidates.
3. For each candidate (usually zero or one, since a random `h2` matches with probability 1/128), compare the actual
   key.
4. If the group contains an EMPTY byte, the key is absent: stop. Otherwise, move to the next group (triangular probing:
   +1, +2, +3 groups...).

The table never fills up: [LIB] the maximum load factor is 7/8 (for tables with at least 8 buckets), and the bucket
count is always a power of two. When it runs out of room, it allocates a table twice as large and **re-hashes every
key** into it, because the table stores no hash values.

### 3. Rust code

**Growth, measured** (listing `ch03-01-hashmap-growth.rs`, verified in debug and release):

```text
size_of HashMap<u64,u64> = 48, HashSet<u64> = 48, Vec<u64> = 24
1000 inserts: capacities [0, 3, 7, 14, 28, 56, 112, 224, 448, 896, 1792]
              10 allocations, 9 frees
with_capacity(1000): capacity 1792, 1 allocation
after clear(): len 0, capacity 114688
after shrink_to_fit(): capacity 0
set A iterates [2, 7, 0, 11, 8, 4, 10, 5, 9, 1, 3, 6]
set B iterates [2, 10, 0, 5, 6, 7, 9, 8, 11, 4, 1, 3]
same contents: true, same order: false
```

- **Capacities are `buckets × 7/8`:** 4 buckets hold 3, 8 hold 7, 16 hold 14, and so on. `with_capacity(1000)` needs
  ⌈1000 × 8/7⌉ = 1,143 buckets, rounded up to 2,048, so it reports capacity 1,792.
- **Ten allocations for a thousand inserts**, and each resize frees the old table. Presized, it's one.
- **The handle is 48 bytes:** 32 for the table (control pointer, bucket mask, item count, growth-left counter) and 16
  for the `RandomState` (two 64-bit SipHash keys). A `HashSet<K>` is literally a `HashMap<K, ()>`, and `()` takes no
  space in the buckets.
- **`clear()` keeps the table**, like `Vec::clear`. A map that once held 100,000 entries keeps room for 114,688.
- **Iteration order is unspecified, and differs between two maps with the same contents** in the same process.
  [LIB] Each `RandomState::new()` gets different keys (std seeds per thread from the OS and increments per instance),
  so tests that accidentally depend on order fail right away instead of after a JDK upgrade.

**Counting hashes: `get_mut` + `insert` vs `entry`** (listing `ch03-04-entry-hash-count.rs`, verified; a custom
`BuildHasher` counts every `finish()` call). The workload is 10,000 words with 100 distinct values, the shape of
logstat's counters:

```rust,ignore
// logstat's pattern (Project L1)
if let Some(c) = m.get_mut(*w) { *c += 1; } else { m.insert(w.to_string(), 1); }

// the entry API: needs an owned key up front
*m.entry(w.to_string()).or_insert(0) += 1;

// hashbrown's entry_ref: borrows the key, converts it to owned only on insert
*m.entry_ref(*w).or_insert(0) += 1;
```

```text
get_mut + insert:         10099 hashes,    100 allocations
entry(w.to_string()):     10000 hashes,  10000 allocations
hashbrown entry_ref(w):   10000 hashes,    100 allocations
1000 inserts from empty:   2788 hashes (1000 for the inserts, the rest are resize rehashes)
1000 inserts, presized:    1000 hashes
```

This is the whole trade-off in four numbers:

- **`entry` hashes once** per call: Chapter 4.2's claim, verified. But `std`'s `entry` takes the key **by value**, so
  with `String` keys you allocate a `String` on every call, including the 9,900 calls where the key was already
  present.
- **`get_mut` + `insert` hashes twice on a miss** (once to look, once to insert) and once on a hit, and allocates only
  on a miss. For logstat, where almost every line hits a known path, 99 extra hashes are far cheaper than 9,900 extra
  allocations. The L1 choice was right.
- **Why 10,099 and not 10,100?** The very first `get_mut` ran on an empty table, and [LIB] hashbrown returns `None`
  without hashing when there are no items. That's an implementation detail that the counter caught.
- **`entry_ref` gets both**: one hash, and an owned key built only when inserting. It's in the `hashbrown` crate, not
  in `std`'s API.
- **Growth costs hashes**: 1,788 rehashes for 1,000 inserts (3 + 7 + 14 + ... + 896, the contents at each resize).
  With an expensive hash (SipHash over long strings) presizing saves real CPU, not just allocations.

**Floats as keys** (listing `ch03-05-f64-key.rs`, verified error):

```rust,compile_fail
use std::collections::HashMap;

fn main() {
    let mut fx_rates: HashMap<f64, &str> = HashMap::new();
    fx_rates.insert(1.0842, "EURUSD");
    println!("{fx_rates:?}");
}
```

```text
error[E0599]: the method `insert` exists for struct `HashMap<f64, &str>`, but its trait bounds were not satisfied
 --> src/main.rs:6:14
  |
6 |     fx_rates.insert(1.0842, "EURUSD");
  |              ^^^^^^
  |
  = note: the following trait bounds were not satisfied:
          `f64: Eq`
          `f64: Hash`
```

Chapter 2.3 explained why `f64` isn't `Eq` (NaN ≠ NaN) or `Ord`. Notice where the error lands: [LANG] `HashMap::new`
has no bounds, so creating the map compiles, and the `K: Eq + Hash` bounds live on the `impl` block containing
`insert`. That's why you get E0599 ("method exists but its trait bounds were not satisfied") and not E0277. The fixes
(listing `ch03-06-f64-key-fixed.rs`, verified):

```text
lookup 1.0842 -> Some("EURUSD")
lookup NaN    -> Some("broken feed")
best level: Some((10842, 500))
sorted with total_cmp: [-1.0, -0.0, 0.0, 2.5, NaN]
```

- **`ordered_float::OrderedFloat<f64>`** ([LIB]) defines `Eq`, `Ord`, and `Hash` so that NaN equals NaN and sorts last,
  and `-0.0 == 0.0`. It's correct for keys, but ask first whether you want floats as keys at all.
- **Integer minor units** (price in 1/10,000ths, amounts in cents) are almost always the better model for money and
  prices (Chapter 2.3). Exact equality on a computed float is a bug waiting for a rounding difference.
- **`f64::total_cmp`** gives a total order for sorting without a wrapper.

---

## Pass 2 · Systems level — *Probing, hashing, and memory per entry*

### 4. Under the hood

**Why SwissTable is fast.** [LIB][CPU] Compared with a chained table (Java's `HashMap`: an array of pointers to linked
nodes), open addressing with control bytes has three advantages:

1. **A miss usually costs one cache line of control bytes.** Sixteen control bytes are checked with one `pcmpeqb` +
   `pmovmskb` pair, and an EMPTY among them ends the search. The buckets themselves are touched only for `h2` matches.
2. **No per-entry allocation.** Keys and values live *in* the table. There's no node object and no `next` pointer.
3. **`h2` filters out most key comparisons.** With 7 bits of hash in each control byte, a non-matching slot is rejected
   without touching the key. That matters when keys are strings: a failed comparison would otherwise be a pointer chase
   plus a `memcmp`.

On targets without SSE2, hashbrown uses a portable "generic" group of 8 control bytes processed with ordinary 64-bit
integer arithmetic (and NEON on AArch64, [LIB] depending on the version). The algorithm is the same.

**Deletion leaves tombstones.** Removing an entry sets its control byte to DELETED, unless its group has never been full
(then it can go straight back to EMPTY, because hashbrown can prove no probe sequence ever continued past it). Tombstones keep probe
sequences intact but consume capacity. A table with heavy insert/remove churn eventually rehashes *in place* to clear
them. It's rare, but it's an O(n) pause that shows up in latency tails.

**The entry API, mechanically.** [LIB] `entry(k)` hashes `k` once, probes, and returns either `Occupied` (holding the
bucket) or `Vacant` (holding the hash and, after reserving room for one more item, the key). `VacantEntry::insert` reuses
the stored hash to find a slot, so it never hashes again. That's why it's one hash, and why Chapter 4.2's "problem case
#3" fix is also the fast path.

**The `Borrow` trick behind `get(&str)`.** A `HashMap<String, V>` can be queried with a `&str` because `get` is generic:
`fn get<Q: ?Sized>(&self, k: &Q) where K: Borrow<Q>, Q: Hash + Eq`. [LANG] The contract of `Borrow` is that the
borrowed form hashes and compares identically to the owned form, which `String`/`str` honor. Break that contract in your
own types and lookups silently miss.

**The Hash/Eq contract.** If `a == b`, then `hash(a) == hash(b)`, the same rule as Java's `equals`/`hashCode`. [LANG]
Violating it, or mutating a key's hash-relevant state through interior mutability while it's in the map, is a **logic
error**: std documents that the map's behavior is then unspecified (wrong answers, panics, non-termination), but never
undefined behavior. The map's `unsafe` internals are written so that a lying `Hash` can't cause memory unsafety.

### 5. Memory

**Memory per entry**, from the layout ([LIB], arithmetic, not a measurement):

```text
bytes = buckets × (size_of::<(K, V)>() + 1) + 16          (+ allocator overhead, + the 48-byte handle)

HashMap<u64, u64>: 17 bytes per bucket
  right after a resize: load 7/16 ≈ 44%  → ~39 bytes per entry
  just before a resize: load 7/8  = 87.5% → ~19 bytes per entry
```

Compare Java's `HashMap<Long, Long>` with compressed oops: a `Node` object (header + `hash` + `key` + `value` + `next`,
about 32 bytes), two `Long` objects (24 bytes each, unless cached), and a 4-byte table slot at load factor 0.75. That's
roughly 80–90 bytes per entry, and every lookup follows at least two pointers. Treat these as order-of-magnitude
estimates. Measure your own with a heap histogram (`jcmd <pid> GC.class_histogram`) and the counting allocator.

**Large values change the calculus.** Buckets are sized for `(K, V)`, and up to 56% of them are empty right after a
resize. With a 16-byte key and a 200-byte value, an empty bucket wastes 216 bytes. Section 9 works through a real
example and the alternatives (boxing the value, or `IndexMap`).

**Ownership behavior.** The map owns its keys and values, **inline in the table**. Two consequences for Java
programmers:

- **Values move when the table resizes.** A `&V` from `get` borrows the whole map, and the borrow checker stops you from
  inserting while you hold it. In Java you'd hold a reference to a heap object that never moves; in Rust the equivalent
  is `HashMap<K, Box<V>>` or `HashMap<K, Arc<V>>`, at the cost of an allocation and a pointer chase.
- **`remove` returns the value** by move (`Option<V>`), so taking ownership out of a map is free.

### 6. CPU / OS

**Hashers, measured** (listing `ch03-02-hashers.rs`, release, one million keys, **one run on a shared machine: noisy**,
compare ratios only):

```text
1000000 random u64 keys
  std RandomState (SipHash-1-3)   build   115.71ms   lookup    97.90ms
  ahash::RandomState              build    50.59ms   lookup    25.88ms
  foldhash::fast::RandomState     build    47.97ms   lookup    23.17ms
  fxhash::FxBuildHasher           build    44.61ms   lookup    19.52ms
1000000 short String keys ("user:NNNNNNN")
  std RandomState (SipHash-1-3)   build   323.67ms   lookup   131.78ms
  ahash::RandomState              build   425.92ms   lookup    62.22ms
  foldhash::fast::RandomState     build   409.05ms   lookup    49.56ms
  fxhash::FxBuildHasher           build   401.69ms   lookup   126.69ms
```

What this run shows, and what it doesn't:

- **For integer keys, SipHash costs roughly 2–5× the fast hashers** on lookups. The hash dominates, because each
  lookup is otherwise one or two cache lines.
- **The String "build" column is dominated by cloning a million `String`s** (allocation), so it says little about
  hashers. The differences in it are noise and allocator state.
- **FxHash was no faster than SipHash on these strings.** A plausible explanation (a hypothesis, not measured here) is
  that the old `fxhash` crate's word-at-a-time multiply mixes similar strings poorly, producing more `h2` collisions
  and longer probe sequences, so its cheap hash is paid back in extra comparisons. The Systems exercise asks you to
  test that. The general lesson stands on the measurement alone: **the fastest hasher depends on the key type**.

**What SipHash buys you: HashDoS resistance.** In 2011, Klink and Wälde's 28C3 talk ("Efficient Denial of Service
Attacks on Web Application Platforms") showed that attackers who can predict a hash function can send keys that all
collide, turning every insert into a linear search. By their reported figures, a single request with crafted POST
parameters could keep a CPU core busy for minutes or more, depending on the platform. Languages responded in two ways:

- **Java (JEP 180, Java 8):** keep the fixed `hashCode`, but "treeify" a bin into a red-black tree once it holds 8
  entries (in tables of at least 64 bins), bounding the worst case to O(log n) for `Comparable` keys.
- **Rust:** make the hash *unpredictable*. [LIB] `RandomState` seeds SipHash-1-3 with 128 random bits, so an attacker
  can't compute colliding keys offline.

hashbrown has no treeification, so its only defense is the hasher. Here's what happens without one (listing
`ch03-03-hashdos.rs`, release; one noisy run, but the scaling is the point):

```text
n =  10000: FxHash benign   360.48µs | FxHash crafted     7.48ms | SipHash crafted   421.15µs
n =  20000: FxHash benign   604.21µs | FxHash crafted    30.05ms | SipHash crafted   928.58µs
n =  40000: FxHash benign     1.27ms | FxHash crafted   132.26ms | SipHash crafted     2.00ms
```

The crafted keys are `i << 32`. FxHash of a `u64` is essentially `k × constant` (mod 2⁶⁴), so if the low 32 bits of `k`
are zero, so are the low 32 bits of the hash. `h1` comes from the low bits, so **every key starts probing at group 0**,
and each insert walks past all previous keys. Doubling n quadruples the time (7.5 → 30 → 132 ms): O(n²). The same keys
under SipHash behave like any other keys. An attacker needs exactly this: a deterministic hasher, and control over keys.

**Choosing a hasher:**

| Hasher | Keyed? | Speed (this run, u64) | Use when |
|---|---|---|---|
| std `RandomState` (SipHash-1-3) | yes, random per map | baseline | keys come from outside your trust boundary: the default |
| `foldhash` / `ahash` | yes, random per process (per their docs) | ~4× faster lookups | internal maps on hot paths; per their docs, these resist *some* HashDoS, weaker guarantees than SipHash |
| `FxHash` (`fxhash`, `rustc-hash`) | no | fastest on ints | trusted keys only: compiler-internal tables (rustc uses it), keys you generate |
| an identity hash | no | free | pre-hashed keys (a random 128-bit ID's own bits); a `BuildHasherDefault` with a pass-through `Hasher` |

[LIB] The `hashbrown` crate's own default hasher is currently `foldhash`; `std` keeps SipHash. Changing the hasher is a
type change (`HashMap<K, V, S>`), so it's visible in every signature that mentions the map. That's a feature: a reviewer
can see where the decision was made.

**Concurrency.** `HashMap` has no internal synchronization, and std has no concurrent map. The standard options, all
covered with measurements in Part XI:

- `Mutex<HashMap<K, V>>` or `RwLock<...>`: simple, and contended under write load.
- **Sharding:** `Vec<Mutex<HashMap<K, V>>>`, indexed by a hash of the key; this is what `dashmap` packages.
- **Snapshot and swap** for read-mostly data: an `ArcSwap<HashMap<K, V>>` rebuilt on change (Chapter 3.1's route table).
- **One owner thread** with messages: no sharing at all.

Java's `ConcurrentHashMap` (CAS for empty bins, a lock per bin head, lock-free reads) has no direct std equivalent.

---

## Pass 3 · Architect level — *Maps at millions of entries*

### 7. Trade-offs

**`HashMap` vs `BTreeMap`** (SPEC comparison row; Chapter 9.4 measures it: 1M gets took 66 ms in a `HashMap`, 218 ms in
a `BTreeMap`, while a 1,000-key range query took 7 µs in a `BTreeMap` and 2 ms as a `HashMap` scan):

| Criterion | `HashMap` | `BTreeMap` |
|---|---|---|
| Memory | buckets × (entry + 1), 44–87% full | nodes of up to 11 entries, fuller on average |
| CPU | a hash + ~1 probe | ~log₆ n node visits, linear scan inside each |
| Latency | O(1) typical; resize and tombstone-rehash pauses | O(log n), no resize pauses |
| Throughput | best for point lookups | best for ordered scans and ranges |
| Contention | no built-in sync (same for both) | same |
| Cache | 1–2 lines per lookup | one line per level, top levels stay cached |
| Allocation | ~log₂ n tables, or 1 presized | one per node, incremental |
| Complexity | needs `Hash + Eq` and a hasher decision | needs `Ord` |
| Safety | both fully safe | same |
| Maintainability | iteration order random: forces order-independent code | deterministic order: output is stable |
| Failure modes | HashDoS with a weak hasher; logic errors from bad `Hash` | a bad `Ord` (non-total) gives wrong results |
| Operational | memory steps ×2 at resize | memory grows smoothly |

> **Why not always `BTreeMap` for deterministic output?** Because a point lookup costs 3× as much (in Chapter 9.4's
> measurement) and ordered output is often needed only at the end. Collect into a `Vec` and sort once, or use
> `IndexMap` ([LIB], the `indexmap` crate) for *insertion* order, which is what people usually mean by "deterministic".

### 8. Java comparison

| Java | Rust | Notes |
|---|---|---|
| `HashMap` (chained, nodes, treeified bins) | `HashMap` (open addressing, inline entries) | Different structure, same contract. |
| `hashCode()` cached in `String` and stored in each `Node` | no hash stored anywhere | Java resizes without re-hashing (it splits each bin by one bit); Rust re-hashes every key. Presize when hashing is expensive. |
| load factor 0.75, capacity a power of two | max load 7/8, buckets a power of two | |
| `LinkedHashMap` | `indexmap::IndexMap` ([LIB]) | Insertion order; `IndexMap` is a dense `Vec` of entries plus an index table. |
| `HashSet<E>` = `HashMap<E, Object>` | `HashSet<T>` = `HashMap<T, ()>` | Rust's unit value takes zero bytes. |
| `ConcurrentHashMap` | no std equivalent: sharded maps, `ArcSwap`, or a single owner | Part XI. |
| Java 7's infinite loop on concurrent resize | a compile error (Chapter 1.2) | Mutation needs `&mut`, which can't be shared across threads; shared `&HashMap` reads are fine. |
| `computeIfAbsent` | `entry(k).or_insert_with(f)` | One lookup in both. Java throws `ConcurrentModificationException` (best effort, Java 9+) if the function modifies the map; Rust rejects that at compile time, since the entry holds `&mut` to the map. |

> **Analogy limit.** "It's like Java's `HashMap`" is true for the API and false for the memory. A Java map of a million
> entries is a million-plus small objects the GC must trace; a Rust map is one allocation (for `Copy`-like keys and
> values) that the allocator sees as a single block. That's why a Rust service can hold a 10-million-entry map without a
> GC pause story, and also why resizing it means one 2× allocation plus a full re-hash: plan capacity for large maps.

### 9. Production scenario

**Sizing the session cache.** Meridian's session cache (Chapter 1.3's design exercise) holds 2 million sessions and
serves 100K lookups per second. The Rust port started as `HashMap<SessionId, Session>` with a 16-byte `SessionId` and a
200-byte `Session`. The memory, from the layout (arithmetic, [LIB]):

```text
with_capacity(2_000_000): buckets = next_pow2(2,000,000 × 8/7 = 2,285,715) = 4,194,304

HashMap<SessionId, Session>        4,194,304 × (216 + 1)          ≈ 910 MB
HashMap<SessionId, Box<Session>>   4,194,304 × (16 + 8 + 1)       ≈ 105 MB  + 2M × ~208 B ≈ 520 MB total
IndexMap<SessionId, Session>       entries Vec: 2M × ~224 B       ≈ 448 MB  (up to 2× while growing)
                                   + index table: 4,194,304 × ~9  ≈  38 MB  ≈ 490 MB total
```

The inline design wastes over 400 MB on empty buckets, because a table sized for 2 million entries has 4.2 million
buckets and each is sized for a whole `Session`. The team picked `IndexMap`: dense entries, and a small index table
that holds only positions. Two more decisions came out of the review:

- **The hasher stays keyed.** Session IDs are generated by Meridian but *sent by clients*, so an attacker controls the
  keys that are looked up and can try to plant crafted IDs. They chose `foldhash` with a random seed for its speed, and
  documented that, if the threat model tightens, the fallback is SipHash.
- **Presize and never shrink during the day.** The map is created with its peak capacity at startup, so it never
  resizes (and re-hashes 2 million keys) at 100K lookups per second.

### 10. Failure scenario

**The rate limiter that turned quadratic.** The gateway prototype keeps a per-client token bucket in a
`HashMap<u64, Bucket>`, keyed by a client ID taken from a request header. A profile showed SipHash at 4% of CPU, and an
engineer switched the map to `FxHashMap` in a PR titled "faster hashing". It passed review; the benchmark improved.

Three weeks later, a scraper started sending requests with client IDs it generated as `counter << 32`. The team later
concluded the scraper's authors weren't targeting Fx at all, and that the pattern was an accident of how they sharded
their ID space. Every new ID landed in the same probe sequence, so each insert scanned all previous IDs. At a few tens
of thousands of distinct IDs per pod, inserts were taking milliseconds, and gateway CPU climbed to 100% on the pods
behind that scraper's load balancer affinity.

- **Detection:** a CPU profile dominated by hashbrown's probe loop (`find_insert_slot`), with a hash function near zero.
  The give-away is time in probing, not in hashing.
- **Root cause:** an unkeyed hasher on attacker-influenced keys. The same failure mode as the 2011 HashDoS attacks,
  reintroduced for a 4% gain.
- **Fix:** revert to a keyed hasher (`foldhash` with a random seed was the compromise), and add a lint in CI: any
  `FxHashMap` or `BuildHasherDefault<FxHasher>` in a crate that parses network input needs a `// TRUSTED-KEYS:` comment
  naming why its keys are trusted.
- **Lesson:** the hasher is part of your security posture. "Faster hashing" PRs are threat-model changes.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part IX).*

1. Draw a SwissTable. What are `h1` and `h2`, what does a control byte hold, and how does one lookup use SIMD?
2. Why does a `HashMap<u64, u64>` created with `with_capacity(1000)` report capacity 1,792?
3. Why does growing a Rust `HashMap` re-hash every key while Java's doesn't? When does that matter?
4. Compare `get_mut` + `insert`, `entry`, and `entry_ref` by hashes and allocations for `String` keys. Which would you
   use for a counter where 99% of keys already exist?
5. Why does `HashMap<f64, V>::insert` fail with E0599 rather than E0277? Give two fixes and say when each is right.
6. What is HashDoS? Contrast Java's defense (treeification) with Rust's (keyed hashing). What happens to hashbrown under
   an unkeyed hasher and crafted keys?
7. When is `FxHash` appropriate, and when is it a vulnerability?
8. Estimate the memory of a `HashMap<[u8; 16], [u8; 200]>` with 2 million entries. What alternatives reduce it, and
   what do they cost?
9. Why is a lying `Hash` implementation a logic error and not undefined behavior?
10. std has no concurrent map. What are your options, and which would you choose for a read-mostly config table?

### 12. Exercises

- **Beginner.** Predict the capacity sequence of a `HashSet<u32>` over 100 inserts, and the number of allocations. Check
  by adapting `ch03-01-hashmap-growth.rs`.
- **Intermediate.** Implement a word counter three ways (`get_mut` + `insert`, `entry`, and `hashbrown`'s `entry_ref`)
  and measure time, not just counts, on 10 million words with 1% distinct.
- **Advanced.** Write a `Hasher` that returns the key's own bits for random 128-bit session IDs. Measure it against
  SipHash and `foldhash`, and explain precisely which property of the keys makes it safe (and what change to ID
  generation would make it unsafe).
- **Systems.** Test the chapter's hypothesis about `fxhash` and strings: wrap a hasher to record every hash, then compute
  the distribution of `h2` (top 7 bits) and of `h1 & mask` for the one million `"user:NNNNNNN"` keys. Compare with
  `foldhash`. Does the data support the hypothesis?
- **Architecture.** Find every `HashMap` in a service you know that is keyed by client input. For each, write down the
  hasher and whether an attacker could influence key choice.

### 13. Debugging exercise

```rust,ignore
#[derive(Hash, PartialEq, Eq)]
struct CacheKey {
    tenant: u32,
    path: String,
    request_id: u64, // added last sprint "for debugging"
}

fn lookup<'a>(cache: &'a HashMap<CacheKey, Response>, tenant: u32, path: &str, req: u64) -> Option<&'a Response> {
    cache.get(&CacheKey { tenant, path: path.to_string(), request_id: req })
}
```

1. After last sprint, the cache hit rate dropped from 94% to 0% and memory grew until OOM. Explain both.
2. The fix "remove `request_id` from the derive" isn't possible with `#[derive]`. Write the manual `Hash` and `PartialEq`
   that ignore it, and state the invariant they must keep.
3. The lookup allocates a `String` per call. Redesign the key so lookups don't allocate.

### 14. Design exercise

**Idempotency keys for the payments API.** Meridian's payments API must remember every idempotency key it has seen in
the last 24 hours (about 300 million keys per day at peak, 36-character UUID strings, client-supplied) and reject
duplicates with a cached response reference.

Design the in-memory index on each of 12 nodes (keys are routed by hash): the key representation (36-byte string,
16-byte binary UUID, or a 64-bit hash with collision handling?), the map type and hasher (the keys come from clients),
expiry of 24-hour-old keys without scanning the whole map, and the memory budget per node. Compare with Java's
`ConcurrentHashMap<String, Long>` for the same data, and state which number you'd measure first.
