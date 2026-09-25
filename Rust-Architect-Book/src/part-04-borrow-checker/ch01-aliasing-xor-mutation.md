# Chapter 4.1 — Aliasing XOR Mutation

> **Where this sits:** Part IV · The Borrow Checker · chapter 1 of 6
> **Prerequisites:** Chapter 3.3 (the two borrowing rules).
> **After this chapter you can:** describe what the borrow checker tracks (places, loans, accesses) precisely enough
> to predict its verdicts; explain iterator invalidation at the machine level; use `split_at_mut` and
> `get_disjoint_mut` for provably disjoint mutation; and explain `UnsafeCell`, the one principled exception to the
> rule, with the LLVM IR that shows what it turns off.

---

## Pass 1 · User level — *The rule, made precise*

### 1. Problem

Chapter 3.3 stated the rule: many shared references **or** one mutable reference, never both. To predict verdicts
instead of reacting to them, you need to know three more things. What exactly does the checker consider "the same
value"? What counts as a conflicting access? And if mutation through a shared reference is forbidden, how do
`RefCell`, `Mutex`, and atomics work at all? This chapter answers all three, then shows the canonical bug the rule
exists to prevent: **iterator invalidation**.

### 2. Mental model

**The checker reasons about places.** A *place* is a path to a memory location:

```text
 x               a local
 x.limits        a field of x
 *r              what reference r points to
 (*r).accounts   a field of what r points to (written r.accounts)
 v[i]            for a slice: an element. For a Vec: really *Index::index(&v, i), a METHOD CALL on all of v
```

**A borrow creates a loan** on a place: `(place, kind, region)`, where the kind is shared or mutable, and the region is
the stretch of the program in which the loan may still be used (Chapter 4.2). While a loan is live, it restricts
accesses to **its place, any prefix of it (its parents), and any extension of it (its children)**:

| Live loan on place P | Read P (or parent/child) | Write P (or parent/child) | Move out of P | New `&` on P | New `&mut` on P |
|---|---|---|---|---|---|
| **Shared** (`&P`) | Allowed | Error (E0506) | Error (E0505) | Allowed | Error (E0502) |
| **Mutable** (`&mut P`) | Error (E0503/E0502) | Error (E0506) | Error (E0505) | Error (E0502) | Error (E0499) |

Two consequences you'll meet constantly:

- **Disjoint fields are independent.** A loan on `self.accounts` doesn't restrict `self.rate_bps`, since neither is a
  prefix of the other (Chapter 3.3's split borrow).
- **Indices are not.** The checker does no arithmetic. `balances[from]` and `balances[to]` are the same place,
  `balances[_]`, even when `from != to`.

**`&mut` means *unique*, and `&` means *shared*.** The names "mutable reference" and "immutable reference" are
approximations. The real guarantee of `&mut T` is exclusivity: no other live path to the value. The real guarantee of
`&T` is shared access, and shared access *can* permit mutation, but only through a type built on `UnsafeCell<T>`, which
takes on the job of keeping it safe some other way:

| Type | How it keeps "shared XOR mutable" safe | Checked | Threads |
|---|---|---|---|
| `Cell<T>` | Never hands out references to its interior: `get` copies out, `set` replaces | By construction | `!Sync`: one thread |
| `RefCell<T>` | Counts borrows at run time; conflicting borrows panic | At run time | `!Sync`: one thread |
| `OnceCell<T>` | Write once, then read-only | By construction | `!Sync` (`OnceLock` is `Sync`) |
| `Mutex<T>` / `RwLock<T>` | Blocks until access is exclusive (or shared, for readers) | At run time, by waiting | `Sync` |
| `AtomicU64` and friends | The hardware makes each access indivisible | By the CPU | `Sync` |

`UnsafeCell<T>` is the only legal way to mutate through `&` [LANG]. Every type in that table is a safe wrapper around
it, and each re-establishes the rule by other means: copying, counting, waiting, or atomicity.

### 3. Rust code

**Iterator invalidation, rejected** (verified):

```rust,compile_fail
fn main() {
    let mut orders = vec![1, 2, 3];
    for id in &orders {
        if *id == 2 {
            orders.push(4); // a follow-up order, added while iterating
        }
    }
    println!("{orders:?}");
}
```

```text
error[E0502]: cannot borrow `orders` as mutable because it is also borrowed as immutable
 --> src/main.rs:6:13
  |
4 |     for id in &orders {
  |               -------
  |               |
  |               immutable borrow occurs here
  |               immutable borrow later used here
5 |         if *id == 2 {
6 |             orders.push(4); // a follow-up order, added while iterating
  |             ^^^^^^^^^^^^^^ mutable borrow occurs here
```

"Later used here" points at the loop header. Every iteration calls `next()` on an iterator that holds a loan on
`orders`, so the loan is live for the whole loop body. Three correct designs (verified):

```rust
fn main() {
    // Fix 1: collect first (the loan on `orders` ends), then mutate.
    let mut orders = vec![1, 2, 3];
    let follow_ups: Vec<i32> = orders.iter().filter(|&&id| id == 2).map(|_| 4).collect();
    orders.extend(follow_ups);
    println!("collect-then-extend: {orders:?}");

    // Fix 2: a purpose-built method that owns the whole iteration: retain.
    let mut orders = vec![1, 2, 3, 4, 5, 6];
    orders.retain(|id| id % 2 == 0);
    println!("retain evens:        {orders:?}");

    // Fix 3: iterate by index over a length fixed up front, mutating through the Vec itself.
    let mut orders = vec![1, 2, 3];
    let n = orders.len();
    for i in 0..n {
        if orders[i] == 2 {
            orders.push(4);
        }
    }
    println!("index loop:          {orders:?}");
}
```

```text
collect-then-extend: [1, 2, 3, 4]
retain evens:        [2, 4, 6]
index loop:          [1, 2, 3, 4]
```

Fix 3 is legal because no loan outlives a single statement: `orders[i]` is a fresh, momentary borrow each time, and the
semantics (process only the original `n` elements) are explicit. It's also the fix that's easiest to get *wrong*, as §10
shows.

**Mutating through `&`, safely: `Cell`** (verified):

```rust
use std::cell::Cell;

/// Request-scoped statistics. Helpers only get `&RequestStats`, yet can still count:
/// Cell provides mutation through a shared reference (interior mutability).
#[derive(Default)]
struct RequestStats {
    cache_hits: Cell<u32>,
    cache_misses: Cell<u32>,
    db_queries: Cell<u32>,
}

fn lookup(key: &str, stats: &RequestStats) -> String {
    if key.starts_with("hot:") {
        stats.cache_hits.set(stats.cache_hits.get() + 1);
    } else {
        stats.cache_misses.set(stats.cache_misses.get() + 1);
        stats.db_queries.set(stats.db_queries.get() + 1);
    }
    format!("value-of-{key}")
}

fn main() {
    let stats = RequestStats::default();
    let a = &stats; // two shared references...
    let b = &stats; // ...both able to "mutate" the counters
    lookup("hot:user:1", a);
    lookup("cold:user:2", b);
    lookup("hot:user:3", a);
    println!(
        "hits={} misses={} db_queries={}",
        stats.cache_hits.get(),
        stats.cache_misses.get(),
        stats.db_queries.get()
    );
}
```

```text
hits=2 misses=1 db_queries=1
```

The rule isn't violated. It's enforced differently. `Cell` never gives anyone a reference *into* its value, so no
reference can observe a change underneath it. The cost is that `Cell` is single-threaded. Try to share it between two
threads and the compiler refuses:

```text
error[E0277]: `Cell<u32>` cannot be shared between threads safely
    = help: the trait `Sync` is not implemented for `Cell<u32>`
    = note: if you want to do aliasing and mutation between multiple threads, use `std::sync::RwLock` or `std::sync::atomic::AtomicU32` instead
    = note: required for `&Cell<u32>` to implement `Send`
```

The note chain is itself a lesson. Sharing `&Cell` across threads requires `&Cell<u32>: Send`, which requires
`Cell<u32>: Sync`, which `Cell` deliberately isn't. The compiler even suggests the thread-safe versions of the same
idea (Part XI).

**Indices aren't disjoint to the checker:**

```text
error[E0499]: cannot borrow `balances[_]` as mutable more than once at a time
 --> src/main.rs:4:13
  |
3 |     let a = &mut balances[from];
  |             ------------------- first mutable borrow occurs here
4 |     let b = &mut balances[to]; // even when from != to, the checker can't prove disjointness
  |             ^^^^^^^^^^^^^^^^^ second mutable borrow occurs here
5 |     *a -= amount;
  |     ------------ first borrow later used here
  |
  = help: use `.split_at_mut(position)` to obtain two mutable non-overlapping sub-slices
```

`balances[_]` is the place the checker sees. The two safe ways to prove disjointness (verified):

```rust
/// Disjointness proven by a SAFE API that checks at run time (and contains the unsafe inside std).
fn transfer(balances: &mut [i64], from: usize, to: usize, amount: i64) -> Result<(), String> {
    let [a, b] = balances
        .get_disjoint_mut([from, to])
        .map_err(|e| format!("bad accounts {from}->{to}: {e:?}"))?;
    *a -= amount;
    *b += amount;
    Ok(())
}

/// Disjointness proven by CONSTRUCTION: split the slice into two non-overlapping halves.
fn swap_halves(xs: &mut [u32]) {
    let mid = xs.len() / 2;
    let (left, right) = xs.split_at_mut(mid);
    for (l, r) in left.iter_mut().zip(right.iter_mut()) {
        std::mem::swap(l, r);
    }
}
```

```text
Ok(()) -> [70, 80, 10]
Err("bad accounts 2->2: OverlappingIndices")
Err("bad accounts 0->9: IndexOutOfBounds")
[3, 4, 1, 2]
```

`get_disjoint_mut` ([VERSION] stable since Rust 1.86) checks at run time that the indices are in bounds and distinct,
then hands out two `&mut`. A transfer from an account to itself becomes an explicit error instead of undefined
behavior. `split_at_mut` proves disjointness structurally: two halves can't overlap. Both are **safe APIs wrapping a
small `unsafe` core** in std, the pattern Part XV teaches you to write.

---

## Pass 2 · Systems level — *What the checker computes, and what `UnsafeCell` turns off*

### 4. Under the hood

**The checker's algorithm, in outline** [RUSTC]. On each function's MIR:

1. Every `&` and `&mut` expression creates a **loan** on a place.
2. Each loan gets a **region**: the set of program points where it may still be used (liveness, Chapter 4.2).
3. A dataflow pass computes which loans are **in scope** at each point.
4. Every statement's **accesses** (reads, writes, moves, new borrows) are checked against the loans in scope, using the
   table in §2. A conflict is an error, reported with the loan's creation point, the conflicting access, and the use
   that kept the loan alive.

It's intraprocedural, with other functions represented only by their signatures, and it treats `v[i]` on a `Vec` as a
call to `Index::index(&v, i)`, which borrows all of `v`. That's why the per-index precision you might hope for doesn't
exist.

**What `UnsafeCell` changes in the generated code.** Chapter 3.3 showed that `&T` and `&mut T` become `noalias` in
LLVM. `UnsafeCell` is precisely the thing that removes that promise. Two functions (listing `ch01-08-unsafecell-ir.rs`),
each reading a value, calling an opaque function, and reading again:

```rust,ignore
#[inline(never)]
pub fn read_twice(x: &i32) -> i32 {
    let a = *x;
    black_box(()); // an opaque call in between
    a + *x // `*x` can't have changed: &i32 is frozen
}

#[inline(never)]
pub fn read_twice_cell(x: &Cell<i32>) -> i32 {
    let a = x.get();
    black_box(()); // an opaque call in between
    a + x.get() // `x` MAY have changed: Cell allows mutation through a shared reference
}
```

LLVM IR (release, rustc 1.98.1):

```text
define ... @..read_twice(ptr noalias nofree noundef readonly align 4 captures(none) dereferenceable(4) %x)
define ... @..read_twice_cell(ptr noundef nonnull readonly align 4 captures(none) %x)
```

And the assembly:

```text
playground::read_twice:
	mov	eax, dword ptr [rdi]      ; load *x
	#APP                              ; black_box: an empty inline-asm barrier the optimizer can't see through
	#NO_APP
	add	eax, eax                  ; a + a: *x is known not to have changed
	ret

playground::read_twice_cell:
	mov	eax, dword ptr [rdi]      ; load x.get()
	#APP
	#NO_APP
	add	eax, dword ptr [rdi]      ; RELOAD: the opaque call may have changed the Cell
	ret
```

`&i32` gets `noalias`: while the reference is live, nothing else changes the value, not even an opaque call. So the
second read is folded into the first. `&Cell<i32>` doesn't: another `&Cell` to the same value may exist (that's the whole
point), and the opaque call might use it, so the compiler must reload. [LANG] Mutating memory reachable through a `&T`
where `T` contains no `UnsafeCell` is undefined behavior, precisely because the optimizer is allowed to assume it never
happens. `UnsafeCell` is how a type says "don't assume that here."

### 5. Memory

Why iterator invalidation is a memory-safety bug, not just a logic bug:

```text
 a slice iterator is two raw pointers into the Vec's buffer
 ┌────────────────┐           heap buffer (cap 3)
 │ iter.ptr ──────┼─────────► ┌────┬────┬────┐
 │ iter.end ──────┼──────────────────────────► (one past the end)
 └────────────────┘           │ 1  │ 2  │ 3  │
                              └────┴────┴────┘
 orders.push(4) with len == cap:
   allocate a new buffer (cap 6), copy 1,2,3, write 4, FREE the old buffer
 iter.ptr / iter.end now point into freed memory: the next next() reads garbage   (C++'s behavior)
```

In Rust the program never gets that far: the loan held by the iterator makes `push` a compile error. In C++ it's
undefined behavior. In Java, `ArrayList`'s iterator detects the structural change with a modification counter and
throws `ConcurrentModificationException`, on a best-effort basis (Chapter 1.1 showed the silent case).

### 6. CPU / OS

- **Interior mutability without threads is cheap.** `Cell::get` and `Cell::set` compile to plain loads and stores. The
  cost is only the lost optimizations (the reload above), and it applies only to values inside the `Cell`.
- **`RefCell` adds a counter update per borrow** and a branch that's almost never taken (the panic path).
- **Across threads, plain loads and stores aren't enough.** Two cores writing the same memory need atomic instructions
  or locks, and memory ordering rules (Part XIV), which is why `Cell` and `RefCell` are `!Sync` and their thread-safe
  counterparts (`AtomicU32`, `Mutex`, `RwLock`) exist. The compiler's note in §3 connects the two worlds for you.

---

## Pass 3 · Architect level — *Choosing where mutation is checked*

### 7. Trade-offs

| Mechanism | When it's checked | Cost | Failure mode | Threads | Use for |
|---|---|---|---|---|---|
| `&mut T` | Compile time | None | Compile error | Per the owner's thread (movable with `Send`) | The default |
| `Cell<T>` | By construction | Copy in and out; lost optimizations | None (it can't fail) | One | Small `Copy` counters and flags in shared structures |
| `RefCell<T>` | Run time | A counter per borrow | **Panic** | One | Genuinely dynamic sharing (graphs, callbacks) with short borrows |
| `Mutex<T>` / `RwLock<T>` | Run time, by waiting | Atomics; contention; possible syscalls | Deadlock, poisoning | Many | Shared mutable state across threads |
| Atomics | The CPU | One atomic instruction | None (but ordering bugs) | Many | Counters, flags, lock-free structures |

The architect's rule: **push checking as early as the design allows.** Compile time beats construction, construction
beats run time, and run time checks that panic are for cases where the sharing is genuinely dynamic. A `RefCell` whose
borrow pattern you could have expressed statically is a panic you chose to keep.

### 8. Java comparison

In Java, **every field of every object is interior-mutable through every reference**, and there's no `&mut` at all.
Consequences a Java engineer knows well:

| Java | Rust |
|---|---|
| Any holder of a reference may mutate (unless the class is immutable by design) | Mutation requires `&mut` (exclusive) or an explicit `UnsafeCell`-based type |
| `ConcurrentModificationException`: a best-effort run-time check | E0502: always, at compile time |
| `volatile`, `AtomicInteger`, `synchronized` for cross-thread mutation | Atomics and `Mutex`, plus `Cell`/`RefCell` for single-threaded interior mutation |
| Immutability by convention (`final` fields, unmodifiable wrappers, records) | Immutability by default; mutation is visible in the types |

> **Analogy limit.** "`RefCell` is like a Java object: shared and mutable" holds mechanically. It breaks on *intent*.
> In Java, shared mutability is the default and immutability is the discipline. In Rust, exclusivity is the default and
> each `RefCell` is a documented exception. A Rust codebase where `RefCell` is everywhere has recreated Java's model
> without Java's cycle-collecting GC (Chapter 3.6).

### 9. Production scenario

**Meridian's request statistics.** Request handling in the gateway passes a context object down through a dozen
helper functions (authentication, cache lookup, rate limiting, upstream selection). Each wants to record a few counters
for the request's access-log line. Threading `&mut RequestStats` through every helper would force every helper's
signature to take `&mut`, and forbid holding any other borrow of the context at the same time. Instead, the stats use
`Cell<u32>` counters (the listing in §3). Helpers take `&RequestContext`, any number of them can hold it at once, and
counting is plain loads and stores.

The design stays correct because the context is **per request, on one thread** (in async code, per task, Part XIII).
When the team later wanted *global* counters aggregated across all threads, the compiler refused to let the same `Cell`
type be shared (E0277), and pointed at `AtomicU32`. That became the design for the global metrics: per-thread `Cell`
counters for request-local data, atomics for process-wide data.

### 10. Failure scenario

**The index loop that satisfied the checker and broke the logic.** A developer "fixed" an E0502 in a session-expiry
loop by switching from an iterator to indices (verified):

```rust
fn main() {
    let mut sessions = vec!["ok", "expired", "expired", "ok"];
    // The borrow checker is satisfied (no loan outlives a mutation), and the LOGIC is wrong:
    // indices shift after remove(), and `0..len` was fixed before the loop started.
    let n = sessions.len();
    for i in 0..n {
        if sessions[i] == "expired" {
            sessions.remove(i);
        }
    }
    println!("{sessions:?}");
}
```

```text
thread 'main' (15) panicked at src/main.rs:8:20:
index out of bounds: the len is 3 but the index is 3
```

Two bugs in six lines. After `remove(1)`, the second `"expired"` shifts into index 1, which the loop has already
passed, so it's **skipped**. And the loop still runs to the original length, so it **indexes past the end** and panics.
The panic is memory-*safe* (a bounds check caught it), but it's still an outage: the expiry job crashes on every run
where two adjacent sessions expire.

The lesson is about what the checker guarantees. It rejected the iterator version because that version was
*unsound*: the iterator's pointers could dangle. It accepted the index version because indices can't dangle. But indices
can be *wrong*. Replacing a borrow with an index moves the burden from the compiler to you. The right fix was the
purpose-built operation: `sessions.retain(|s| *s != "expired")`, which is correct, O(n), and has no index bookkeeping.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part IV).*

1. What is a *place*, and what is a *loan*? Which accesses does a live shared loan forbid, and which does a live mutable
   loan forbid?
2. Why are `self.a` and `self.b` independent to the checker, while `v[i]` and `v[j]` aren't?
3. Why is "mutable reference" an approximation? What do `&` and `&mut` actually guarantee?
4. What is `UnsafeCell`, and why is it the only legal way to mutate through `&`? What does it change in the LLVM IR, and
   why?
5. How does each of `Cell`, `RefCell`, `Mutex`, and atomics re-establish aliasing XOR mutation?
6. Explain iterator invalidation at the machine level. What do C++, Java, and Rust each do about it?
7. How do `split_at_mut` and `get_disjoint_mut` prove disjointness? What does each cost?
8. When is replacing a borrow with an index a good idea, and what does it cost you?

### 12. Exercises

- **Beginner.** For each pair, predict whether it compiles with both borrows alive: `&mut s.a` + `&s.b`; `&mut s.a` +
  `&s`; `&mut v[0]` + `&v[1]`; `&mut *r` + `&r.x` (where `r: &mut S`). Check each on the Playground.
- **Intermediate.** Rewrite the session-expiry loop correctly three ways: `retain`, a reverse index loop, and a
  `drain`/`filter` rebuild. Which is O(n²) in the worst case, and why?
- **Advanced.** Implement `fn pairwise_swap(xs: &mut [u32])` that swaps elements 0↔1, 2↔3, and so on, using
  `chunks_exact_mut(2)` and no indexing. Why doesn't this need `split_at_mut`?
- **Systems.** Extend the `read_twice` experiment. Add `read_twice_mut(x: &mut i32)` and `read_twice_raw(x: *const
  i32)`. Predict each one's assembly before looking. Which reload, and why?
- **Architecture.** For each piece of shared mutable state in a service you know, classify it by where mutation is
  checked today, and where it *could* be checked in a Rust design (compile time, construction, run time, CPU).

### 13. Debugging exercise

```rust,ignore
fn prune(scores: &mut Vec<u32>, threshold: u32) {
    for (i, s) in scores.iter().enumerate() {
        if *s < threshold {
            scores.remove(i);
        }
    }
}
```

1. Predict the error code and the three program points the error will cite.
2. Explain why accepting this would be unsound, in terms of the iterator's two pointers.
3. Fix it with `retain`. Then explain why the fix is also *faster* in the worst case than a correct remove-based loop.

### 14. Design exercise

**Per-connection state in Meridian's gateway.** Each client connection has a small state record (bytes in and out,
request count, last activity time, a "draining" flag). It's read and updated by the connection's own handler, by a
periodic idle-timeout sweeper, and by an admin endpoint that lists connections.

Choose where mutation is checked: `&mut` owned by the connection task with messages to the others; `Cell` fields (and
what that implies about threads); `Mutex<ConnState>` per connection; atomics per field; or a sharded table owned by one
thread. Analyze the cost on the hot path (every request), the consistency the admin endpoint sees (can it read a torn
combination of fields?), and failure modes. Pick one and name the measurement that would change your mind.
