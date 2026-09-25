# Chapter 3.6 — Ownership for Graphs: Rc, Weak, Arenas, and Indices

> **Where this sits:** Part III · Ownership · chapter 6 of 6
> **Prerequisites:** Chapters 1.4 (the first look at graphs) and 3.1–3.5.
> **After this chapter you can:** model graph-shaped data in Rust with five strategies (shared ownership with `Rc`/`Weak`,
> interior mutability with `RefCell`, arenas with indices, generational indices, and index-linked structures);
> explain what each costs in memory, CPU, and safety guarantees; and choose among them for a real system.

---

## Pass 1 · User level — *When ownership isn't a tree*

### 1. Problem

Ownership forms a tree (Chapter 3.1). Many real domains don't:

- an org chart where each department knows its parent;
- a doubly linked list or an LRU cache (each node has a `prev` and a `next`);
- an AST where nodes point back to their scope, or a dependency graph with shared nodes;
- an observer or event bus where subscribers and publishers refer to each other;
- a session store that other structures reference by session.

In Java these are trivial: every reference is shared and the GC handles cycles. In Rust, `parent: &Node` inside a node
its parent owns and may mutate is aliasing plus a cycle, and the borrow checker can't prove it safe (Chapter 1.4 showed
the error). You aren't stuck. You choose a strategy, and each one is a *different answer to who owns what*.

### 2. Mental model

```text
                         Does the structure need DELETION of individual nodes?
                                  │
                ┌─────── no ──────┴────── yes ───────┐
                ▼                                     ▼
     Do nodes all live and die together?       Do outside holders need handles that
                │                              survive deletions safely?
       ┌── yes ─┴── no ──┐                        ┌── yes ──┴── no ──┐
       ▼                  ▼                        ▼                  ▼
   ARENA + INDICES    Rc/Arc + Weak           GENERATIONAL         ARENA + INDICES
   (Vec<Node>, usize) (+ RefCell/Mutex        INDICES              + free list
   bulk-freed          for mutation)          (slotmap-style)      (e.g. an index-
                                                                    linked LRU)

   Also possible, for foundational data structures only: raw pointers behind a safe API (std's LinkedList; Part XV).
```

The five strategies in one line each:

1. **`Rc<T>` / `Arc<T>`**: shared ownership with a reference count. The value dies when the last strong handle does.
2. **`Weak<T>`**: a non-owning handle for back-edges. It doesn't keep the value alive, so cycles don't leak.
3. **`RefCell<T>` / `Mutex<T>`**: interior mutability, moving the aliasing-XOR-mutation check from compile time to run
   time, so shared nodes can be modified.
4. **Arena + indices**: one collection owns every node, and edges are *positions*, not addresses.
5. **Generational indices**: arena indices plus a generation counter, so a handle to a deleted-and-reused slot is
   *detected* instead of silently pointing at the wrong node.

### 3. Rust code

**Strategy 1+2+3: an org chart with owning child edges and weak parent edges** (verified):

```rust
use std::cell::RefCell;
use std::rc::{Rc, Weak};

struct Dept {
    name: String,
    parent: RefCell<Weak<Dept>>,      // back-edge: does NOT keep the parent alive
    children: RefCell<Vec<Rc<Dept>>>, // owning edges
}

impl Drop for Dept {
    fn drop(&mut self) {
        println!("  drop {}", self.name);
    }
}

fn dept(name: &str) -> Rc<Dept> {
    Rc::new(Dept { name: name.into(), parent: RefCell::new(Weak::new()), children: RefCell::new(vec![]) })
}

fn adopt(parent: &Rc<Dept>, child: Rc<Dept>) {
    *child.parent.borrow_mut() = Rc::downgrade(parent);
    parent.children.borrow_mut().push(child);
}

fn path(d: &Rc<Dept>) -> String {
    let mut parts = vec![d.name.clone()];
    let mut current = d.parent.borrow().upgrade();
    while let Some(p) = current {
        parts.push(p.name.clone());
        current = p.parent.borrow().upgrade();
    }
    parts.reverse();
    parts.join(" / ")
}

fn main() {
    let payments;
    {
        let root = dept("Meridian");
        let eng = dept("Engineering");
        payments = dept("Payments");
        adopt(&eng, Rc::clone(&payments));
        adopt(&root, eng);
        println!("{}", path(&payments));
        println!("root:     strong={} weak={}", Rc::strong_count(&root), Rc::weak_count(&root));
        println!("payments: strong={} weak={}", Rc::strong_count(&payments), Rc::weak_count(&payments));
        println!("-- `root` goes out of scope --");
    }
    println!("parent still alive? {}", payments.parent.borrow().upgrade().is_some());
    println!("path now: {}", path(&payments));
    println!("-- end of main --");
}
```

```text
Meridian / Engineering / Payments
root:     strong=1 weak=1
payments: strong=2 weak=0
-- `root` goes out of scope --
  drop Meridian
  drop Engineering
parent still alive? false
path now: Payments
-- end of main --
  drop Payments
```

Follow the counts. `root` has one strong owner (the local) and one weak reference (Engineering's parent edge).
`payments` has two strong owners: the outer local and Engineering's `children` vector. When `root` goes out of scope, its
strong count reaches zero. "Meridian" drops, which drops its `children`, which drops the only strong handle to
Engineering, which drops Engineering, which releases *one* of Payments' two strong handles. Payments survives because the
outer local still owns it, and its `Weak` parent edge now fails to `upgrade()`. **No leak, no dangling pointer**, and the
lifetimes of individual nodes are independent.

**Strategy 4+5: a generational arena** (a minimal version of the `slotmap` crate's idea, verified):

```rust,ignore
/// A handle into an Arena: an index plus the generation it was issued for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    index: u32,
    generation: u32,
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// Owns every value; hands out Copy keys instead of references.
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Arena<T> {
    pub fn get(&self, key: Key) -> Option<&T> {
        let slot = self.slots.get(key.index as usize)?;
        if slot.generation == key.generation { slot.value.as_ref() } else { None }
    }

    pub fn remove(&mut self, key: Key) -> Option<T> {
        let slot = self.slots.get_mut(key.index as usize)?;
        if slot.generation != key.generation {
            return None;
        }
        let value = slot.value.take()?;
        slot.generation = slot.generation.wrapping_add(1); // every outstanding key to this slot goes stale
        self.free.push(key.index);
        self.len -= 1;
        Some(value)
    }
    // insert(), get_mut(), len(): see listing ch06-02-generational-arena.rs
}
```

```text
alice = Key { index: 0, generation: 0 }
bob   = Key { index: 1, generation: 0 }
remove(alice) = Some("alice")
carol = Key { index: 0, generation: 1 }
get(alice) = None   <- stale key detected, not carol's data
get(carol) = Some("carol")
get(bob)   = Some("bob (vip)")
len = 2
```

Carol reused Alice's slot (index 0), but with generation 1. The old `alice` key, still held somewhere, now returns
`None` instead of Carol's data. That's the **ABA problem** of index-based designs, solved with 4 bytes per handle. Note
what the arena *doesn't* give you: a `Key` doesn't keep anything alive. Validity is checked **at run time, on every
access**, and "the value is gone" is an ordinary `None` that you handle.

---

## Pass 2 · Systems level — *What each strategy costs*

### 4. Under the hood

**`Rc<T>`** is a pointer to a heap block, [LIB] conventionally called an `RcBox`:

```text
 Rc<Dept> (8 bytes) ──►  ┌──────────────────────┐
 Rc<Dept> (clone)   ──►  │ strong: Cell<usize>  │  clone → strong += 1       drop → strong -= 1
 Weak<Dept>         ──►  │ weak:   Cell<usize>  │  downgrade → weak += 1     when strong hits 0: DROP the value
                         │ value:  Dept         │                            when weak also hits 0: FREE the block
                         └──────────────────────┘
```

- **`clone` is a count increment, not a copy.** That's why `Rc` is the cheap way to "copy" an expensive value
  (Chapter 3.2's table).
- **`Weak::upgrade`** checks `strong > 0` and, if so, increments it and returns `Some(Rc)`. A `Weak` keeps the
  *allocation* alive (so the counts can be read) but not the *value*.
- [LANG] **`Rc` is `!Send` and `!Sync`**: its counts are plain `Cell<usize>` updates, so two threads could lose
  updates (Chapter 1.3's E0277). **`Arc`** uses atomic read-modify-writes for exactly that reason, and costs more per
  clone and drop.
- `size_of::<Rc<T>>()` is 8, and `Option<Rc<T>>` is also 8 (a non-null niche, Chapter 2.6).

**`RefCell<T>` moves the borrow check to run time.** It stores a borrow counter next to the value: a positive count for
active shared borrows, a special marker for one active mutable borrow. `borrow()` and `borrow_mut()` check and update it,
and returned guards restore it on drop. Violate aliasing XOR mutation and you get a **panic**, not a compile error:
`RefCell already borrowed` (verified in §10). That's the price of shared mutability without threads. The rules are the
same as the compile-time ones, but they're enforced later, and a violation is a crash instead of a build failure.
`try_borrow_mut()` returns a `Result` if you'd rather handle the conflict.

**Arenas are plain `Vec`s.** Indices are `usize` or `u32` values. The "graph" is just data, so the borrow checker sees
one owner (the arena) and ordinary `&`/`&mut` borrows of it. That's why an index-linked structure like the LRU in §9
needs **no `unsafe`, no `Rc`, and no `RefCell`**: mutation goes through `&mut self` on the whole arena. The cost moves
to *logic*: an index can be stale, out of range, or pointing at the wrong node. Bounds checks catch out-of-range
indices (as a panic), and generations catch stale ones (as `None`).

### 5. Memory

```text
 Rc graph: every node is its own heap allocation, anywhere in memory
 ┌────────────┐        ┌────────────┐        ┌────────────┐
 │ counts     │        │ counts     │        │ counts     │
 │ Meridian   │ ─────► │ Engineering│ ─────► │ Payments   │     links are 8-byte pointers;
 │ children ──┼─┘      │ parent(W)──┼──┐     │ parent(W)  │     each node is a separate cache miss
 └────────────┘        └────────────┘  │     └────────────┘
       ▲───────────────────────────────┘

 Arena: every node in ONE contiguous buffer
 slots: ┌───────────────┬───────────────┬───────────────┬───────────────┐
        │ gen 1 "carol" │ gen 0 "bob"   │ gen 3  (free) │ gen 0 "dave"  │    links are 4-byte indices;
        └───────────────┴───────────────┴───────────────┴───────────────┘    neighbours share cache lines
 free:  [2]                                                                   one allocation for the lot
```

Concrete consequences: an `Rc` node carries 16 bytes of counts plus its own allocation (allocator metadata and
alignment padding). An arena node carries a 4-byte generation (or nothing, for a plain index arena), and the whole arena
is one or a few allocations. Dropping the graph is one free per node for `Rc`, and one free per arena buffer for the
arena (Chapter 3.1's frees measurement, applied to graphs).

### 6. CPU / OS

- **Pointer chasing vs scanning.** Walking an `Rc` graph means one dependent load per edge, each potentially a cache miss
  (~100 ns to DRAM, Chapter 1.4's table). Walking an arena often means sequential or nearby accesses that the hardware
  prefetcher handles well. For traversal-heavy workloads the difference can be large. Measure yours (Part XX).
- **"Reads cause writes."** Merely cloning and dropping an `Rc` to *look* at a node writes to its count, which dirties
  the cache line. With `Arc` across threads, that write is an atomic RMW on a line other cores are also reading, the
  contention pattern from Chapter 1.3. Read-mostly structures shared across threads usually do better handing out `&T`
  borrows from a structure held in a single `Arc`, than an `Arc` per node.
- **Index width.** A `u32` index is half the size of a pointer. In a structure with millions of links, that's a real
  memory and cache saving, and it caps the arena at about 4 billion slots, which is usually fine.

---

## Pass 3 · Architect level — *Choosing a graph strategy*

### 7. Trade-offs

| | `Rc`/`Arc` + `Weak` | + `RefCell`/`Mutex` | Arena + indices | Generational indices | Raw pointers (Part XV) |
|---|---|---|---|---|---|
| **Who owns nodes** | Shared (counts) | Shared (counts) | The arena | The arena | You, by hand |
| **Node lifetimes** | Independent | Independent | Together (or free list) | Independent, via a free list | Anything |
| **Mutation** | No (immutable) | Yes, checked at run time | Via `&mut arena` | Via `&mut arena` | Anything |
| **Invalid access becomes** | Impossible (strong) / `None` (weak) | A **panic** on a borrow conflict | A panic (OOB) or **the wrong node** (stale index) | `None` (stale key detected) | **UB** |
| **Cycles** | Leak unless back-edges are `Weak` | Same | No problem | No problem | No problem |
| **Memory per node** | 16 B of counts + an allocation | + a borrow flag | 0 overhead | 4 B generation | 0 |
| **Cache behavior** | Scattered | Scattered | Contiguous | Contiguous | Depends |
| **Thread-safe variant** | `Arc` (atomic counts) | `Arc<Mutex<_>>` (contention) | Shard, or lock the arena | Same | Your problem |
| **Best for** | Shared immutable data, trees with parent links, small dynamic graphs | Rarely the first choice | Parse trees, graphs with a shared lifetime, compilers | Entity systems, caches, handles held by outside code | Foundational std-level data structures |

A rule of thumb Meridian adopted: **if you find yourself writing `Rc<RefCell<...>>` more than once in a module, stop and
consider an arena.** `Rc<RefCell<T>>` is Java's object model re-created at run time. It works, and it gives up most of
what the compiler could have checked.

### 8. Java comparison

| Java | Rust |
|---|---|
| Any object can reference any other; the GC collects cycles | Ownership is a tree; cycles need `Weak`, or an arena |
| `LinkedHashMap(capacity, 0.75f, true)` + `removeEldestEntry`: an LRU cache in about 5 lines | An index-linked list plus a `HashMap` (§9), or a crate such as `lru` |
| `WeakReference<T>`: cleared by the GC *at some point* after the referent becomes unreachable | `Weak<T>`: fails to upgrade *exactly* when the last strong handle drops, deterministically |
| `ConcurrentModificationException`: detects some aliasing-while-mutating, at run time, best-effort | `RefCell`: detects all of it, at run time, always (a panic) |

> **Analogy limit.** "`Rc<RefCell<T>>` is a Java object reference" is the most dangerous analogy in this Part. It's
> *true enough* to compile Java-shaped designs, and that's the problem. You get a GC-free object graph with run-time
> borrow panics, leaks from cycles, and none of the GC's cycle collection. Use it deliberately, not as a translation
> layer.

### 9. Production scenario

**Meridian's session LRU, index-linked.** The gateway caches recently used sessions with LRU eviction. The Java version
was a `LinkedHashMap` in access order. The Rust version threads a doubly linked list through a `Vec` (the arena), with
links as indices and a `HashMap` from key to slot. It uses no `unsafe`, no `Rc`, and no `RefCell`, and makes no
allocation per eviction (listing `ch06-03-lru.rs`, verified):

```rust,ignore
const NIL: usize = usize::MAX;

struct Entry<V> {
    key: u64,
    value: V,
    prev: usize, // links are INDICES into `entries`, not pointers
    next: usize,
}

pub struct Lru<V> {
    map: HashMap<u64, usize>,
    entries: Vec<Entry<V>>,
    head: usize, // most recently used
    tail: usize, // least recently used
    capacity: usize,
}

impl<V> Lru<V> {
    fn unlink(&mut self, i: usize) {
        let (prev, next) = (self.entries[i].prev, self.entries[i].next);
        if prev != NIL { self.entries[prev].next = next } else { self.head = next }
        if next != NIL { self.entries[next].prev = prev } else { self.tail = prev }
    }

    pub fn get(&mut self, key: u64) -> Option<&V> {
        let i = *self.map.get(&key)?;
        self.unlink(i);
        self.push_front(i);
        Some(&self.entries[i].value)
    }

    /// Inserts or updates; returns the evicted entry, if any.
    pub fn put(&mut self, key: u64, value: V) -> Option<(u64, V)> {
        // ... update in place, or append while below capacity, or:
        // Full: reuse the least-recently-used slot in place (no allocation per eviction).
        let i = self.tail;
        self.unlink(i);
        let old = std::mem::replace(&mut self.entries[i], Entry { key, value, prev: NIL, next: NIL });
        self.map.remove(&old.key);
        self.map.insert(key, i);
        self.push_front(i);
        Some((old.key, old.value))
    }
}
```

```text
order (MRU..LRU): [3, 2, 1]
get(1) = Some("ada")
order (MRU..LRU): [1, 3, 2]
put(4) evicted Some((2, "grace"))
order (MRU..LRU): [4, 1, 3]
get(2) = None
```

The eviction path uses Chapter 3.2's `mem::replace` to *move* the old entry out of its slot while putting the new one
in, so the evicted value is returned to the caller (to close a connection, say), not silently dropped. Everything is
checked by ordinary borrow rules, because every mutation goes through `&mut self` on the one structure that owns
everything. (Thread safety is a separate question: one `Mutex<Lru<_>>`, or sharding, is Part XI.)

### 10. Failure scenario

**The event bus that panicked on its busiest event.** A Meridian service built an in-process event bus with shared
ownership and interior mutability (verified):

```rust
use std::cell::RefCell;
use std::rc::Rc;

type Handler = Box<dyn Fn(&str)>;

struct EventBus {
    handlers: RefCell<Vec<Handler>>,
}

impl EventBus {
    fn subscribe(&self, handler: Handler) {
        self.handlers.borrow_mut().push(handler);
    }

    fn publish(&self, event: &str) {
        for handler in self.handlers.borrow().iter() {
            // a shared borrow held for the whole loop
            handler(event);
        }
    }
}

fn main() {
    let bus = Rc::new(EventBus { handlers: RefCell::new(Vec::new()) });
    bus.subscribe(Box::new(|e| println!("audit: {e}")));

    let bus_for_handler = Rc::clone(&bus);
    bus.subscribe(Box::new(move |e| {
        if e == "user.created" {
            // A handler that registers a follow-up handler while it is being notified.
            bus_for_handler.subscribe(Box::new(|e| println!("welcome-email: {e}")));
        }
    }));

    bus.publish("user.created");
}
```

```text
audit: user.created
thread 'main' (14) panicked at src/main.rs:13:23:
RefCell already borrowed
```

`publish` holds a shared borrow of `handlers` for the whole loop. A handler, called *inside* that loop, calls
`subscribe`, which asks for a mutable borrow of the same `RefCell`. Aliasing XOR mutation is violated, and because the
check moved to run time, the result is a panic on the first `user.created` event, the busiest event in the system. In
Java the equivalent code throws `ConcurrentModificationException`, or, with a copy-on-write list, quietly works.

Two lessons, one of which is hiding:

1. **Reentrancy is where `RefCell` panics live.** Callbacks that call back into the structure that invoked them are the
   classic trigger. Fixes: iterate over a snapshot (`let hs = self.handlers.borrow().clone()`, which needs `Rc`-shared
   handlers), queue subscriptions made during `publish` and apply them afterwards, or use `try_borrow_mut` and fail
   gracefully.
2. **There's also a leak.** The bus owns the handler closure, and the closure owns `bus_for_handler`, an `Rc` to the
   bus. That's a strong cycle, so this bus can never be freed (Chapter 1.3's `Rc` cycle, disguised as a closure). The
   closure should capture `Rc::downgrade(&bus)` and `upgrade()` when it runs.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part III).*

1. Why can't a child node simply hold `parent: &Node`? Explain in terms of ownership and aliasing.
2. What exactly does `Rc::clone` do? What does `Weak` keep alive, and what doesn't it?
3. Why is `Rc` not thread-safe, and what does `Arc` change? What does each clone cost?
4. What does `RefCell` check, when, and what happens on a violation? Why is that strictly worse than a compile error,
   and when is it worth it?
5. Explain the ABA problem for index-based handles and how generations solve it. What does a generation check cost?
6. Why can an index-linked LRU be written without `unsafe`, `Rc`, or `RefCell`, while a pointer-linked one can't (in
   safe Rust)?
7. Compare pointer chasing in an `Rc` graph with traversal of an arena, in cache terms.
8. "`Rc<RefCell<T>>` is Java's object model re-created at run time." Explain, and say when it's nonetheless the right
   choice.

### 12. Exercises

- **Beginner.** Add a `fn depth(&self) -> usize` to the `Dept` example by walking parent `Weak` links. What happens if a
  parent was dropped mid-walk?
- **Intermediate.** Complete the generational arena with `iter()` (yielding `(Key, &T)`) and `retain(|key, value| ...)`.
  Make sure `retain` bumps generations for removed slots.
- **Advanced.** Build a directed graph (`nodes: Arena<Node>`, edges as `Vec<Key>` per node) and implement cycle
  detection with DFS using an **explicit stack**, not recursion. Why does recursion risk a stack overflow for large
  graphs (Chapter 2.4)?
- **Systems.** Benchmark a traversal over 1M nodes in (a) an `Rc`-linked list and (b) an index-linked list in a `Vec`.
  Predict the ratio first, then measure it with `perf stat` (cache misses) on Linux.
- **Architecture.** Take a Java domain model with bidirectional associations (Customer ↔ Orders ↔ LineItems ↔ Products).
  Design its Rust representation, deciding for each association whether it's owned, borrowed, shared, `Weak`, or an ID,
  and justify each.

### 13. Debugging exercise

For the event bus in §10:

1. Explain the panic as an aliasing-XOR-mutation violation. Which borrow is shared, which is mutable, and why does the
   check happen at run time?
2. Explain the leak: draw the ownership cycle. Why doesn't `Rc` break it?
3. Fix both problems. Queue subscriptions made during `publish` and apply them after the loop, and make the handler
   capture a `Weak<EventBus>`. Verify with a `Drop` impl on `EventBus` that it's now freed.

### 14. Design exercise

**Meridian's routing graph.** The gateway's routing configuration is a graph: route groups contain routes, routes
reference shared upstream pools, pools reference health-check policies, and some policies are shared by many pools. The
configuration is rebuilt every few minutes (Chapter 3.1's refresh scenario) and read by thousands of requests per
second across 16 threads. Individual nodes are never mutated after construction.

Choose a representation: `Arc` per node, one `Arc` around an arena with index links, or something else. Analyze
construction cost, per-request read cost (including refcount traffic), cache behavior, the cost of dropping the old graph
on refresh, and how a request refers to "its" upstream pool for the duration of the request. Justify your choice and
state what would change if nodes *were* mutated in place at run time.
