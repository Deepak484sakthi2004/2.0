# Chapter 4.2 — Non-Lexical Lifetimes, Liveness, and Reborrowing

> **Where this sits:** Part IV · The Borrow Checker · chapter 2 of 6
> **Prerequisites:** Chapter 4.1.
> **After this chapter you can:** say exactly when a loan ends (at its last use, per control-flow path); explain how
> the checker computes that; use implicit and explicit reborrows, and avoid the generic-parameter trap; recognize the
> sound code the current checker still rejects; and use the entry API and similar idioms that sidestep it.

---

## Pass 1 · User level — *When does a loan end?*

### 1. Problem

A loan restricts the owner for as long as it's live. So "how long is it live?" decides whether your code compiles.
Before the 2018 edition, the answer was **lexical**: a borrow lasted until the end of the block containing the
reference, even if the reference was never used again. That rejected large amounts of obviously correct code and taught
a generation of Rust programmers to add artificial `{ }` blocks. [VERSION] **Non-lexical lifetimes (NLL)** replaced it:
a loan lasts until its **last use**, computed per control-flow path. (NLL shipped with the 2018 edition in Rust 1.31,
was extended to all editions in 1.36, and the old checker was removed entirely in 1.63.)

Two more questions follow. What happens when you pass a `&mut` along, and why do you sometimes keep it and sometimes
lose it? And where does NLL *still* reject correct code?

### 2. Mental model

**A loan is live at a point if the reference, or anything derived from it, may still be used on some path from that
point.** That's liveness, the same analysis a compiler uses to decide which variables need registers.

```text
 let r = &v;            ── loan L created
 if verbose {
     use(r);            ── L live on this path, up to here
 }
 v.push(x);             ── on EVERY path, r has no later use → L is dead here → push allowed
```

**Reborrowing is nesting.** `&mut *r` creates a new mutable loan *derived from* `r`. While the child is live, the parent
is suspended: you can't use `r`. When the child dies, `r` becomes usable again. Loans form a stack (strictly, a tree):

```text
 let r = &mut hits;          r        ─────────────────────────────────────────────►
 bump(&mut *r);              child 1      ├──┤                         (r suspended during the call)
 let s = &mut *r;            child 2            ├───────────┤          (r suspended while s is live)
 *s += 1;
 *r += 1;                    r usable again after s's last use
```

**Implicit reborrowing** happens when the compiler knows the expected type is a reference: passing `r` to a parameter
of type `&mut T`, or calling a method with a `&mut self` receiver. Then `bump(r)` means `bump(&mut *r)`, and `r` survives.
**It doesn't happen for a generic parameter `T`.** There the compiler sees "a value of type `&mut u32`," which isn't
`Copy`, so it's *moved*. §3 and §10 show the consequences.

### 3. Rust code

**Liveness is per path** (verified):

```rust
use std::collections::HashMap;

fn main() {
    let mut limits: HashMap<&str, u32> = HashMap::from([("gold", 1000), ("free", 10)]);
    let verbose = std::env::args().count() > 5; // false on the Playground

    let gold = &limits["gold"]; // a shared loan on `limits`...
    if verbose {
        println!("gold limit: {gold}"); // ...used on ONE path only
    }
    // On the path where `verbose` is false, `gold` is dead here, so the loan is not live:
    limits.insert("trial", 50); // allowed. On the verbose path the loan also ended at its last use.

    let mut total = 0;
    for (tier, limit) in &limits {
        total += limit;
        if *tier == "free" {
            // mutation here would conflict with the iteration loan
        }
    }
    limits.insert("enterprise", 10_000); // the iteration loan ended with the loop
    println!("tiers={} total before enterprise={total}", limits.len());
}
```

```text
tiers=4 total before enterprise=1060
```

Under lexical lifetimes, `gold`'s loan would have lasted to the end of `main` and the first `insert` would have been
rejected. Under NLL, it's fine.

**The generic-parameter trap** (verified):

```rust,compile_fail
fn bump(counter: &mut u32) {
    *counter += 1;
}

fn record<T: std::fmt::Debug>(value: T) {
    // a generic "metrics" wrapper added during a refactor
    println!("recorded {value:?}");
}

fn main() {
    let mut hits = 0u32;
    let r = &mut hits;
    bump(r); // implicit reborrow: &mut *r
    bump(r); // r is still usable
    record(r); // generic parameter T = &mut u32: NO implicit reborrow, `r` is MOVED
    *r += 1;
    println!("{hits}");
}
```

```text
error[E0382]: use of moved value: `r`
  --> src/main.rs:17:5
   |
13 |     let r = &mut hits;
   |         - move occurs because `r` has type `&mut u32`, which does not implement the `Copy` trait
...
16 |     record(r); // generic parameter T = &mut u32: NO implicit reborrow, `r` is MOVED
   |            - value moved here
17 |     *r += 1;
   |     ^^^^^^^ value used here after move
   |
help: consider creating a fresh reborrow of `r` here
   |
16 |     record(&mut *r); // generic parameter T = &mut u32: NO implicit reborrow, `r` is MOVED
   |            ++++++
```

The compiler's suggestion is exactly the fix (verified: prints `recorded 2`, `recorded 3`, `3`): write the reborrow
yourself, `record(&mut *r)`, or `record(&*r)` if the callee only reads.

**Sound code NLL still rejects: "problem case #3."** A get-or-insert on a map, written the obvious way (verified
rejected on rustc 1.98.1):

```rust,compile_fail
use std::collections::HashMap;

/// Returns the cached value, inserting a default first if the key is missing.
/// This is SOUND (the early return only happens when no insert follows), but the current
/// borrow checker (NLL) rejects it; the next-generation checker (Polonius) accepts it.
fn get_or_default(cache: &mut HashMap<u32, String>, key: u32) -> &String {
    if let Some(v) = cache.get(&key) {
        return v;
    }
    cache.insert(key, String::from("default"));
    cache.get(&key).unwrap()
}
```

```text
error[E0502]: cannot borrow `*cache` as mutable because it is also borrowed as immutable
  --> src/main.rs:11:5
   |
 7 | fn get_or_default(cache: &mut HashMap<u32, String>, key: u32) -> &String {
   |                          - let's call the lifetime of this reference `'1`
 8 |     if let Some(v) = cache.get(&key) {
   |                      ----- immutable borrow occurs here
 9 |         return v;
   |                - returning this value requires that `*cache` is borrowed for `'1`
10 |     }
11 |     cache.insert(key, String::from("default"));
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ mutable borrow occurs here
```

On the path that reaches `insert`, the loan from `get` is *not* returned: `v` was `None`. So there's no actual
conflict. NLL can't see that (§4 explains why), and it rejects the code. The idiomatic fix is also the better code
(verified):

```rust
use std::collections::HashMap;

/// The idiomatic shape: ONE lookup that returns a handle to the slot, occupied or vacant.
fn get_or_default(cache: &mut HashMap<u32, String>, key: u32) -> &String {
    cache.entry(key).or_insert_with(|| String::from("default"))
}

/// When the key must be computed or the value is expensive: or_insert_with only runs on a miss.
fn get_or_load<'c>(cache: &'c mut HashMap<u32, String>, key: u32, loads: &mut u32) -> &'c String {
    cache.entry(key).or_insert_with(|| {
        *loads += 1;
        format!("loaded-{key}")
    })
}

fn main() {
    let mut cache = HashMap::new();
    println!("{}", get_or_default(&mut cache, 7));
    let mut loads = 0;
    for key in [1, 2, 1, 1, 3] {
        get_or_load(&mut cache, key, &mut loads);
    }
    println!("entries={} loads={loads}", cache.len());
}
```

```text
default
entries=4 loads=3
```

The entry API does **one** hash lookup instead of up to three, and it expresses "occupied or vacant" as a value. It
avoids the checker's limitation by being better code, which happens more often than you'd expect.

---

## Pass 2 · Systems level — *How liveness is computed*

### 4. Under the hood

**NLL in five steps** [RUSTC], on each function's MIR:

1. **Region variables.** Every reference type in the MIR gets a region variable: a yet-unknown set of program points.
2. **Liveness constraints.** If a variable holding a reference is live at point P (it may be used later on some path),
   its region must contain P.
3. **Outlives constraints.** Assignments, calls, and returns relate regions. `let a: &'x T = b` (where `b: &'y T`)
   requires `'y: 'x` ("`'y` outlives `'x`", or as sets of points, `'y` ⊇ `'x`).
4. **Solve** for the smallest regions satisfying all constraints.
5. **Check** each access against the loans whose regions contain that point (Chapter 4.1's table).

**Why problem case #3 fails.** The returned reference must have region `'1`, the caller's lifetime, which covers the
*rest of the function body* from the caller's perspective, **on every path**. The loan from `cache.get` flows into the
return value on one path, so its region must include `'1`, and NLL's regions are plain sets of points. They can't say
"this loan is only in `'1` on the path where we returned it." So the loan is considered live at `insert` too. NLL is
*location-sensitive* about liveness but not about *which loans* flow into a region along which path.

**Polonius**, the next-generation formulation, models regions as *sets of loans* ("origins") and tracks, per program
point, which loans could actually reach each origin. On the `insert` path, the loan from `get` can't reach the returned
value, so there's no conflict. [VERSION] Polonius has been under development for years. As verified above, rustc 1.98.1
stable still uses NLL and still rejects problem case #3. Check the current status before relying on Polonius.

**Two-phase borrows** [RUSTC]: for autoref method calls like `v.push(v.len())`, the `&mut v` is created in a *reserved*
state (behaving like a shared borrow) while the arguments are evaluated, then *activated*. That special case exists
because the pattern is so common. It doesn't generalize to arbitrary code.

**Reborrows in MIR** appear as `&mut (*_r)`: a new loan whose place is *through* `_r`. The checker treats the new loan's
place `*r` as an extension of `r`, so while it's live, using `r` directly conflicts with it (Chapter 4.1's
prefix/extension rule). That's the "suspension" in §2. Miri's aliasing models for `unsafe` code (Stacked Borrows and
Tree Borrows, Part XV) are run-time versions of the same stack-of-loans idea.

### 5. Memory

Nothing in this chapter exists at run time. Loans, regions, and reborrows are erased before code generation, and every
reference is a plain pointer. The *memory* consequence is indirect but real: NLL is what lets you write zero-copy code
naturally. With lexical lifetimes, you'd reach for `.clone()` to end a borrow early far more often, and pay for it in
allocations.

### 6. CPU / OS

Borrow checking costs compile time, not run time. On typical code it's a modest share of the front end (type checking
and trait solving usually dominate). `cargo check` runs it without generating code, which is why it's the right command
for iterating on borrow errors. You'll see a borrow error in seconds without waiting for LLVM.

---

## Pass 3 · Architect level — *Working with the checker's limits*

### 7. Trade-offs

When the checker rejects sound code (problem case #3 and its relatives), you have options, each with a cost:

| Workaround | Example | Run-time cost | Code cost |
|---|---|---|---|
| **A better API** (entry, `get_or_insert_with`, `retain`, `split_at_mut`) | `cache.entry(k).or_insert_with(..)` | Often *less*: one lookup | None; usually clearer |
| **Two lookups** | `if !cache.contains_key(&k) { insert } cache.get(&k).unwrap()` | An extra hash lookup | Slightly redundant |
| **Restructure control flow** | Compute the decision first, borrow after | None | Can be awkward |
| **Return an owned value or an index** | Return `String` (clone) or a key | Allocation, or an extra lookup later | Changes the API |
| **`unsafe` to "extend" the borrow** | Raw-pointer round trip | None | **You now carry the proof**, and a mistake is UB (Chapter 4.6) |

The last row is almost never justified for this class of problem. The checker is being conservative about sound code,
and a safe restructuring exists. Save `unsafe` for places where no safe formulation exists (Part XV).

### 8. Java comparison

Java has no loans, but its compiler runs the **same family of analysis**. The Java Language Specification's
*definite assignment* rules (JLS chapter 16) are a flow analysis over the same control-flow structure: a local must be
definitely assigned on *every* path before it's read. NLL's liveness is the mirror image: a reference is live if it may
be used on *some* path afterwards. If you've ever seen `variable might not have been initialized`, you've seen a
path-sensitive dataflow verdict. NLL applies that machinery to loans.

> **Analogy limit.** Java's definite-assignment analysis protects one thing (reading an uninitialized local) and is
> never an obstacle for correct code. NLL protects aliasing and lifetimes, and it's *conservative*: it rejects some
> correct programs. That's the price of checking a much stronger property without whole-program analysis.

### 9. Production scenario

**Meridian's token cache.** The gateway caches decoded JWT claims per token for 60 seconds. The first implementation
was problem case #3 almost exactly: look up, return on a hit, insert on a miss, look up again. After the E0502, the team
wrote it with the entry API: `claims.entry(token_id).or_insert_with(|| decode(token))`. Code review then noticed the
bonus: the old logic (had it compiled) would have hashed the token ID up to three times per request. The entry version
hashes once. At 400K requests per second with ~95% hits, that's a measurable amount of CPU for free (Part XX would
measure it). The borrow checker's limitation led to the better design.

### 10. Failure scenario

**The metrics refactor that broke forty call sites.** A team added a generic helper to record values for debugging,
`fn record<T: Debug>(value: T)`, and wired it into forty functions that held `&mut` references to connection state.
Every call site that used its `&mut` afterwards failed with E0382 (`use of moved value`), exactly the §3 listing. The
refactor stalled while people argued about whether Rust "can't do generics with references."

It can. The rules are consistent, and the fix is a one-word decision. At the call site, reborrow explicitly
(`record(&mut *conn)`, or `record(&*conn)` for reading). At the definition, a better API is `fn record(value: &impl
Debug)`: a helper that only *reads* should take `&`, and then passing `&*conn` or `conn` works everywhere. The lesson for
API designers: **a generic `T` parameter consumes whatever it's given, including references. Take `&T` when you only
need to look.**

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part IV).*

1. What changed with non-lexical lifetimes? Give an example that NLL accepts and lexical lifetimes rejected.
2. Define "a loan is live at point P." Why must the definition be per path?
3. What is a reborrow? Why does passing `r: &mut T` to `fn f(x: &mut T)` keep `r` usable, while passing it to
   `fn g<U>(x: U)` doesn't?
4. Walk through the five NLL steps for a simple function with one borrow and one conditional use.
5. Why does NLL reject problem case #3, even though it's sound? What does Polonius track that NLL doesn't?
6. What is a two-phase borrow, and why is it a special case rather than a general rule?
7. Relate NLL liveness to Java's definite-assignment analysis. What does each protect?
8. Your team hits problem case #3 in a hot path. Rank the workarounds, and justify the ranking with costs.

### 12. Exercises

- **Beginner.** Write three snippets: one rejected because a shared loan is live at a mutation, one accepted because the
  loan died on every path, and one accepted because the loan is live only on a path that doesn't mutate.
- **Intermediate.** Write `fn longest_word<'a>(words: &'a mut Vec<String>) -> &'a str` that first sorts the vector,
  then returns the longest word. Why does the mutable borrow for sorting not conflict with the returned shared borrow?
- **Advanced.** Rewrite problem case #3 without the entry API, using `contains_key` and two lookups. Then write a version
  that returns an owned `String`. Compare the three versions' hash-lookup counts and allocations.
- **Systems.** Time `cargo check` against `cargo build` on a medium crate (any real project). What fraction of build
  time does the front end take? (Use `cargo build --timings`.)
- **Architecture.** Find a function in a Rust codebase you know that uses `.clone()` only to end a borrow early. Can NLL,
  reordering, or an API like `entry` remove the clone?

### 13. Debugging exercise

The E0382 in §3 (`record(r)` moving `r`).

1. Explain why `bump(r)` doesn't move `r` but `record(r)` does, in terms of what the compiler knows about the parameter
   type at each call.
2. Fix it at the call site, then fix it at the definition. Which fix is better for a function that only prints its
   argument?
3. Would `record(r.clone())` compile? What would it mean? (Hint: what is `Clone` for `&mut u32`, if anything?)

### 14. Design exercise

**A get-or-compute cache API for Meridian's services.** Many teams write "look up; if missing, compute and insert;
return a reference." Design the shared library function:

- The signature. Does it return `&V`, `V: Clone`, `Arc<V>`, or a guard?
- How it handles the compute function failing (`Result`), and whether a failed computation is cached.
- What happens when two callers request the same missing key concurrently (single-flight). Sketch it even though Part XI
  covers threads.

Compare your API with Java's `Map.computeIfAbsent`, including its well-known restriction that the mapping function
mustn't modify the map. What's the Rust equivalent of that restriction, and is it enforced at compile time?
