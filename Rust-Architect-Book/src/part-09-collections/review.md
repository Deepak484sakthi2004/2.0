# Part IX Review — A Collections Design Review & Interview Mode

> Consolidate Part IX, then use it: review a Java-shaped port whose every collection choice is defensible in Java and
> costly in Rust, put a memory budget on it, and redesign it. Then answer senior-level questions without notes.
> Answers are in **Appendix A, Part IX**.

---

## Part IX on one page

```text
 Vec<T>        (cap, ptr, len) = 24 B [RUSTC order]; growth max(2·cap, cap+1), min 8/4/1 [LIB, verified]
               push fast path = compare + store + increment; grow_one is out of line
               clear/truncate KEEP capacity → every reused buffer needs a capacity policy
               retain O(n) vs remove-loop O(n²) (1,900× at n = 50K); exact-size collect = 1 alloc
        │
 String        Vec<u8> + UTF-8; format! = ≥1 alloc (heuristic sizing); write! into a buffer = 0
               s + &t reuses s (not Java's trap); format!("{s}..") IS quadratic (100×)
               edges: from_utf16 (lone surrogates fail), from_utf8_lossy (Cow), JNI Modified UTF-8 ≠ UTF-8
               case mapping allocates and can change length (ß→SS, İ→i̇); ASCII variants don't
        │
 HashMap       SwissTable [LIB]: one allocation, control bytes, h1 picks the group, h2 filters 16 slots per SIMD compare
               capacity = buckets·7/8; resize RE-HASHES everything (no stored hashes): 1,788 extra for 1,000 inserts
               entry = 1 hash but std wants an owned key; get_mut+insert = 2 hashes on a miss, allocs only on a miss
               SipHash = keyed, HashDoS-resistant; FxHash + crafted keys = O(n²) (measured ×4 per doubling)
        │
 Specialists   VecDeque = ring (as_slices may be two pieces); BTreeMap = B-tree, 11 keys/node, ordered + ranges
               BinaryHeap = MAX-heap in a Vec (Reverse for min); derived Ord = field order = policy
               LinkedList = n allocations, 4.7×–47× slower scans, cursors unstable on 1.98
        │
 Cache         bytes touched · dependent loads · predictability
               AoS→SoA 12× (one field); boxes 5× in order, 37× shuffled; dense index ≫ HashMap > BTreeMap ≈ bsearch at 1M
        │
 BFS vs DFS    frontier ORDER (answer), LOCATION (stack vs heap), BOUND (depth vs width)
               frame 224 B debug / 96 B release → ~9.4K / ~21.8K levels on 2 MiB; 110% = abort (verified)
               guard page + sigaltstack handler + stack probes = clean abort, never silent corruption
               explicit DFS: (node, edge) frames keep DFS semantics; mark-on-push does not
```

## Ten ideas to carry forward

1. **A collection is a memory layout with costs attached.** Ask where the bytes live before asking about Big-O.
2. **Growth is amortized, not free.** Presize when you know, and give every reused buffer a capacity policy.
3. **Measure allocations exactly and time approximately.** A counting allocator turns "probably allocates" into a
   number.
4. **Write into buffers.** `write!` into a reused `String` beats `format!` in a loop; `join` fixes gluing, not pieces.
5. **Every text boundary has a policy for invalid data.** `lossy` is a policy decision, not a fix.
6. **The hasher is part of the threat model.** Keyed by default; unkeyed only for trusted keys, with a comment saying
   why.
7. **Hashes and allocations trade against each other.** `entry` saves hashes, `get_mut` + `insert` saves allocations,
   and `entry_ref` saves both.
8. **Ordered structures pay on lookups and win on ranges.** `BTreeMap` for order and ranges, `HashMap` for points,
   dense indices for dense keys.
9. **Pointer chasing serializes latency.** Contiguity, SoA for scans, and batches of independent lookups are the
   remedies; `LinkedList` is almost never the answer.
10. **Recursion depth is a resource.** Measure the frame, know the stack, bound input-controlled depth, and move the
    frontier to the heap when depth is unbounded.

---

## Capstone: review the velocity store

Meridian's fraud team ported a "merchant velocity" component from Java: it records card transactions, keeps a one-hour
window per merchant, reports the busiest merchants, sums amounts, and finds merchants linked through shared cards. The
port (listing `review-velocity-store.rs`) compiles, passes its smoke test, and was about to ship.

```rust,ignore
use std::collections::{HashMap, LinkedList};

#[derive(Clone, Debug)]
pub struct Event {
    pub merchant: String,
    pub card_hash: String, // 64 hex chars
    pub amount: f64,
    pub ts_ms: u64,
}

#[derive(Default)]
pub struct VelocityStore {
    events: HashMap<String, LinkedList<Event>>,
    amounts_by_merchant: HashMap<String, Vec<Box<f64>>>,
    card_graph: HashMap<String, Vec<String>>, // card -> merchants it was used at
    hot_merchants: Vec<(String, u64)>,
}

impl VelocityStore {
    pub fn record(&mut self, e: Event) {
        let key = format!("{}", e.merchant);
        let list = self.events.entry(key.clone()).or_insert(LinkedList::new());
        list.push_back(e.clone());
        self.amounts_by_merchant.entry(key).or_default().push(Box::new(e.amount));
        self.card_graph.entry(e.card_hash.clone()).or_default().push(e.merchant.clone());

        // keep a one-hour window of events per merchant
        let cutoff = e.ts_ms.saturating_sub(3_600_000);
        let list = self.events.get_mut(&e.merchant).unwrap();
        while list.front().map_or(false, |x| x.ts_ms < cutoff) {
            list.pop_front();
        }

        // top 10 merchants by events in the window, refreshed on every event
        self.hot_merchants = self.events.iter().map(|(m, l)| (m.clone(), l.len() as u64)).collect();
        self.hot_merchants.sort_by(|a, b| b.1.cmp(&a.1));
        self.hot_merchants.truncate(10);
    }

    pub fn total(&self, merchant: &str) -> f64 {
        self.amounts_by_merchant.get(merchant).map_or(0.0, |v| v.iter().map(|b| **b).sum())
    }

    /// Every merchant linked to `card` through chains of shared cards.
    pub fn linked_merchants(&self, card: &str, seen: &mut Vec<String>) {
        if let Some(ms) = self.card_graph.get(card) {
            for m in ms {
                if !seen.contains(m) {
                    seen.push(m.clone());
                    for (c, ms2) in &self.card_graph {
                        if ms2.contains(m) {
                            self.linked_merchants(c, seen);
                        }
                    }
                }
            }
        }
    }
}
```

The smoke test's output (verified). The last event, for `books-7`, arrives an hour after the others:

```text
total coffee-42 = 7.60
total books-7   = 20.25
linked to card-a: ["books-7", "coffee-42", "games-9"]
hot merchants: 3 entries, top count 2
```

**The production profile:** one pod handles ~2,000 events per second for ~150,000 active merchants and ~5 million
distinct cards per day.

### Task A — Find the issues

Find at least twelve problems. For each, name the resource it costs (memory, CPU, allocations, latency, correctness,
safety, operations), the chapter whose tool finds or fixes it, and whether it gets worse with time, with load, or with
adversarial input. Two of them are visible in the smoke test's output.

### Task B — Put a memory budget on it

Estimate the memory after one hour and after one day at the production profile, as the code stands. State your
per-entry assumptions (Chapters 9.1, 9.3, and 9.4 give you the layouts), and identify which structures grow without
bound.

### Task C — Redesign it

Write the redesigned `VelocityStore` (types and signatures, plus the body of `record`), with:

- merchants and cards as compact IDs,
- a window structure that expires *every* merchant's old events, not just the current one's,
- running totals in integer minor units, kept consistent with the window,
- a top-10 that doesn't sort 150,000 entries per event,
- a linked-merchant search with a hop limit and a frontier cap, on the heap.

Estimate the new memory budget and the allocations per `record` in steady state.

### Task D — One decision matrix

For the window structure, fill in the twelve-criterion matrix (the Interlude's format) comparing the original
`LinkedList<Event>` per merchant, a `VecDeque<(u32 ts, i64 amount)>` per merchant, and one global
`VecDeque<(ts, merchant_id, amount)>` with per-merchant running counters. Choose one.

---

## Interview mode

Answer aloud, in two minutes each, without notes. The answer key has model answers.

1. Walk through `Vec::push` from the assembly up: fast path, slow path, the growth rule, and what's guaranteed.
2. A service's RSS never returns to baseline after a traffic spike, but a heap profile shows live data has. What's
   your first hypothesis in a Rust service, and how do you confirm it?
3. Why is `format!` in a loop an allocation per iteration even when you push the result into one `String`? What do you
   write instead?
4. A partner sends text through JNI. What can go wrong between Java's `String` and Rust's `str`, and what's the
   correct transfer?
5. Explain a SwissTable lookup, including what the SIMD instruction compares.
6. When would you replace SipHash, with what, and what review question must the PR answer?
7. `entry` or `get_mut` + `insert`? Answer with hash counts and allocation counts for `String` keys.
8. Why is std's ordered map a B-tree, and what does that mean for comparison-heavy keys?
9. Give the measured case against `LinkedList`, and the one use case that remains.
10. A colleague says binary search on a sorted `Vec` always beats a `HashMap` "because of cache locality". Respond with
    mechanism and numbers.
11. What does "7 ns per lookup" from a benchmark loop tell you about a single lookup's latency?
12. Your recursive tree walk works in tests and crashes in production with "has overflowed its stack". List every
    factor that could make the depth limit differ between the two.
13. How does Rust turn a stack overflow into a clean abort instead of memory corruption?
14. "Why BFS instead of DFS?" Give the architect's answer.
15. Pick any two structures from this Part and present the twelve-criterion decision matrix for them from memory.

---

## Where this goes next

Part X takes the loops you've been writing over these collections and asks what they compile to: closures as
structs, iterators as state machines, and the zero-cost claim tested in assembly, including the in-place `collect`
specialization that Chapter 9.1 measured at zero allocations.
