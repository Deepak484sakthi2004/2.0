# Chapter 18.5 — Borrow Checking on MIR

> **Where this sits:** Part XVIII · How rustc Works · chapter 5 of 7
> **Prerequisites:** Part IV (places, loans, NLL, variance, error reading), Chapter 18.4 (MIR), Chapter 17.6 (dataflow
> on CFGs).
> **After this chapter you can:** describe the borrow checker's pipeline (renumbering, MIR type check, liveness, region
> inference, dataflow); explain why unreachable code is still checked, why a `Drop` impl extends a borrow, why method
> syntax borrows differently from an explicit `&mut` argument, and how closures hand requirements to their creators;
> read a nightly region dump; state what Polonius changes and where it stands; and treat "adding `Drop`" and similar
> changes as the API breaks they are.

---

## Pass 1 · User level — *The checker you know, seen as an algorithm*

### 1. Problem

Part IV taught *what* the borrow checker proves: every access to a place is compatible with the loans live at that
point (Chapter 4.1), and a loan lives as long as something that holds it is still going to be used (Chapter 4.2). You
learned to read its errors as proofs (Chapter 4.6).

This chapter is about *how* rustc computes that, because the implementation explains behaviors the rules alone make
look arbitrary:

- Code inside `if false { … }` is still rejected.
- Adding an empty `impl Drop` to a type breaks code that never calls anything on it.
- `v.push(v.len())` compiles, but `Vec::push(&mut v, v.len())` doesn't.
- A closure's borrow errors can mention lifetimes you never wrote, like `'1` and `'2`.
- The same function, problem case #3 from Chapter 4.2, is rejected by stable 1.98.1 and accepted by the current
  nightly.

### 2. Mental model

Borrow checking is the query `mir_borrowck(def_id)`, run once per body on **promoted MIR** (18.4): after MIR building,
**before** any optimization. [RUSTC] (rustc-dev-guide, *MIR borrow check*.) It has five steps:

```text
 promoted MIR for one body
   │ 1. RENUMBER     replace every lifetime in the body with a fresh region variable  '?0, '?1, …
   │                 (the signature's lifetimes stay "universal": the caller chooses them, Chapter 4.3)
   │ 2. MIR TYPE CHECK  re-type-check every statement; each subtyping step (variance, Chapter 4.4) and
   │                 each call's where-clauses produce OUTLIVES constraints:  '?3: '?1  "at point P"
   │ 3. LIVENESS     a region must contain every point where a variable whose type mentions it is live
   │                 ("live" = may be used later, including by its destructor)
   │ 4. REGION INFERENCE  solve: each region = the smallest SET OF POINTS satisfying all constraints;
   │                 check the universal regions against the signature ("lifetime may not live long enough")
   │ 5. DATAFLOW     (a) which loans are in scope at each point → conflicting access? E0499/E0502/E0506/E0505
   │                 (b) which places are initialized/moved → use of moved or uninitialized value? E0382/E0381
   ▼
 errors, or success (and the regions are then ERASED: nothing downstream ever sees a lifetime)
```

The key idea, from Chapter 4.2, stated in this chapter's terms: a **region is a set of points in the MIR graph**
(locations: a basic block and a statement index), not a lexical scope. A loan created at point P with region R is
**in scope** at the points of R reachable from P without passing a point that kills it. An access at a point where a
conflicting loan is in scope is an error.

### 3. Rust code

**Unreachable code is still checked** (listing `ch05-01-dead-code-borrowck.rs`, verified):

```rust,compile_fail
fn main() {
    let mut balance = 100;
    if false {
        let a = &mut balance;
        let b = &mut balance; // never executes, still rejected
        *a += 1;
        *b += 1;
    }
    println!("{balance}");
}
```

```text
error[E0499]: cannot borrow `balance` as mutable more than once at a time
 --> src/main.rs:7:17
  |
6 |         let a = &mut balance;
  |                 ------------ first mutable borrow occurs here
7 |         let b = &mut balance; // never executes, still rejected
  |                 ^^^^^^^^^^^^ second mutable borrow occurs here
8 |         *a += 1;
  |         ------- first borrow later used here
```

The borrow checker sees MIR *before* optimization. `if false` is still a `switchInt` on a constant with two
successors; nothing has folded it away. That's deliberate (a design principle of the language, rather than a sentence
in the Reference): whether a program is accepted must not depend on the optimizer. A later compiler that got smarter at
constant folding must not start accepting (or rejecting) programs.

**A `Drop` impl is a use at the end of the scope.** Listing `ch05-04-no-drop-ok.rs` (verified) compiles and prints:

```rust
struct Span<'a> {
    name: &'a str,
}

fn main() {
    let mut name = String::from("checkout");
    let span = Span { name: &name };
    println!("span: {}", span.name);
    name.push_str("-v2"); // fine: span is dead here
    println!("{name}");
}
```

```text
span: checkout
checkout-v2
```

Add an empty `impl Drop for Span<'_> { fn drop(&mut self) {} }` (listing `ch05-05-drop-is-a-use.rs`):

```text
error[E0502]: cannot borrow `name` as mutable because it is also borrowed as immutable
  --> src/main.rs:16:5
   |
14 |     let span = Span { name: &name };
   |                             ----- immutable borrow occurs here
15 |     println!("span: {}", span.name);
16 |     name.push_str("-v2"); // span's last *visible* use was above...
   |     ^^^^^^^^^^^^^^^^^^^^ mutable borrow occurs here
17 |     println!("{name}");
18 | }
   | - immutable borrow might be used here, when `span` is dropped and runs the `Drop` code for type `Span`
```

Step 3 in action. Without `Drop`, `span` is dead after the `println!`, so the region of the loan on `name` ends there.
With `Drop`, MIR has a `drop(span)` terminator at the end of `main` (18.4), and `Drop::drop` receives `&mut Span`, which
*could* read `self.name`. So `span` is **drop-live** until that point, and the loan must survive until then. The note
on line 18 is the compiler telling you exactly that. The body of `drop` doesn't matter; its existence does. (The escape
hatch for collections, `#[may_dangle]`, is Chapter 15.3's topic.)

**Method-call syntax gets a two-phase borrow.** Listing `ch05-03-two-phase-explicit.rs` (verified):

```rust,compile_fail
fn main() {
    let mut v: Vec<usize> = vec![10, 20];
    v.push(v.len()); // method-call syntax: accepted (two-phase borrow)
    Vec::push(&mut v, v.len()); // explicit `&mut v` argument: no two-phase borrow
    println!("{v:?}");
}
```

```text
error[E0502]: cannot borrow `v` as immutable because it is also borrowed as mutable
 --> src/main.rs:7:23
  |
7 |     Vec::push(&mut v, v.len()); // explicit `&mut v` argument: no two-phase borrow
  |     --------- ------  ^ immutable borrow occurs here
  |     |         |
  |     |         mutable borrow occurs here
  |     mutable borrow later used by call
```

The error is on line 7, not line 6. [RUSTC] (rustc-dev-guide, *Two-phase borrows*.) MIR building creates a
**two-phase borrow** in three places: the autoref of a method receiver (`v.push(…)` borrows `&mut v` for you), the
implicit reborrow of a `&mut` value passed as an argument, and the implicit `&mut` of an overloaded compound assignment
(`a += b` on a non-primitive type). A two-phase borrow is *reserved* when created, behaves like a shared borrow until
it's *activated* at the call, and only then becomes exclusive. Arguments evaluated in between (`v.len()`) may read `v`.
When you write `&mut v` yourself, it's an ordinary mutable borrow from the moment it's created, so `v.len()` conflicts.
(Chapter 3.3 introduced two-phase borrows; this is where the rule lives.)

The Playground's MIR for `push_len` in listing `ch05-02-two-phase.rs` is worth a look, because it shows the limit of
post-borrowck MIR:

```text
    bb0: {
        _4 = &(*_1);
        _3 = Vec::<usize>::len(move _4) -> [return: bb1, unwind continue];
    }

    bb1: {
        _2 = Vec::<usize>::push(copy _1, move _3) -> [return: bb2, unwind continue];
    }
```

There's no `&mut` reborrow in sight: by the time MIR reaches this output, the receiver's reborrow (`&mut (*_1)` in the
MIR the borrow checker sees) has been simplified to a plain copy of the pointer (observed on 1.98.1). The two-phase
reservation existed only at the borrowck stage. To see it, dump
MIR at the borrowck stage on a local nightly (`-Z dump-mir=push_len`, files in `mir_dump/`; not verified here).

**Moves are dataflow over the graph** (listing `ch05-08-move-dataflow.rs`, verified):

```rust,compile_fail
fn publish(batch: Vec<u64>) -> usize {
    batch.len()
}

fn main() {
    let batch = vec![1u64, 2, 3];
    for attempt in 0..3 {
        let sent = publish(batch); // moved in the first iteration...
        println!("attempt {attempt}: sent {sent}");
    }
}
```

```text
error[E0382]: use of moved value: `batch`
  --> src/main.rs:11:28
   |
 9 |     let batch = vec![1u64, 2, 3];
   |         ----- move occurs because `batch` has type `Vec<u64>`, which does not implement the `Copy` trait
10 |     for attempt in 0..3 {
   |     ------------------- inside of this loop
11 |         let sent = publish(batch); // moved in the first iteration...
   |                            ^^^^^ value moved here, in previous iteration of loop
```

(Trimmed.) Step 5(b): at the call, is `batch` *maybe moved* on some path to this point? Yes: along the loop's back edge
from the previous iteration. The same dataflow ("maybe uninitialized") drives drop elaboration's drop flags (18.4).
It's also the analysis Java uses for definite assignment (§8).

**Closures are checked first and hand their requirements to the creator.** A closure is a separate MIR body, borrow
checked on its own, but its signature mentions regions that belong to the enclosing function. When the closure needs a
relationship between those regions it can't prove itself, it **propagates** the requirement outward. A nightly-only
dump (listing `ch05-06-closure-regions.rs`, `#[rustc_regions]`, verified):

```rust,ignore
#[rustc_regions]
fn collect<'a>(src: &'a [String], out: &mut Vec<&'a str>) {
    src.iter().for_each(|s| out.push(s.as_str()));
}
```

```text
note: external requirements
 --> src/main.rs:9:25
  |
9 |     src.iter().for_each(|s| out.push(s.as_str()));
  |                         ^^^
  |
  = note: defining type: collect::{closure#0} with closure args [
              i16,
              extern "rust-call" fn((&std::string::String,)),
              (&mut std::vec::Vec<&str>,),
          ]
  = note: late-bound region is '?4
  = note: late-bound region is '?5
  = note: number of external vids: 6
  = note: where '?1: '?3

note: no external requirements
 --> src/main.rs:8:1
  |
8 | fn collect<'a>(src: &'a [String], out: &mut Vec<&'a str>) {
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = note: defining type: collect
```

The closure pushes a `&str` borrowed from `*s` into a `Vec<&'a str>`, so it needs "the region of the strings outlives
the region of the vector's elements": that's `where '?1: '?3`, expressed in the enclosing function's region variables.
`collect` then proves it from its signature (`src: &'a [String]` and `Vec<&'a str>` share `'a`), and reports "no
external requirements" of its own. The closure-args list is also informative: the closure's signature
(`fn((&String,))`) and its captured upvars (`(&mut Vec<&str>,)`, one mutable borrow of `out`, Chapter 10.1). This is
why closure errors mention numbered lifetimes like `'1`: they're region variables the closure received from outside.

**Problem case #3: the version question.** Listing `ch05-07-problem-case-3.rs` carries two headers, and both pass:

```rust,compile_fail
use std::collections::HashMap;

fn get_or_default(cache: &mut HashMap<u32, String>, key: u32) -> &String {
    if let Some(v) = cache.get(&key) {
        return v;
    }
    cache.insert(key, String::from("default"));
    cache.get(&key).unwrap()
}
```

Stable 1.98.1 (and beta 1.99.0-beta.7, checked separately):

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

Nightly 1.100.0 (2026-09-24), no flags: compiles and prints `default`. §4 explains the difference and what it means for
you. [VERSION]

---

## Pass 2 · Systems level — *Regions as sets of points*

### 4. Under the hood

**Universal and existential regions.** [RUSTC] After renumbering, a body has two kinds of region variables.
**Universal** regions come from the signature (`'a`, the anonymous lifetimes of reference parameters, `'static`): the
body must work for *every* choice the caller makes, so the body can't make them bigger or smaller. The error's "let's
call the lifetime of this reference `'1`" names one. **Existential** regions are everything inside the body (the loan on
`name`, the type of `span`), and the solver picks them. Universal regions are represented as containing every point in
the body plus an abstract "end of `'a`" element.

**Constraint generation.** [RUSTC] The **MIR type check** walks every statement and terminator and relates the types
involved:

- `_2 = &'?5 _1` creates a **loan** (place `_1`, kind shared, region `'?5`) and constrains the reference type.
- Assigning a `&'?5 T` into a place of type `&'?3 T` requires `&'?5 T <: &'?3 T`, which by variance (Chapter 4.4)
  means `'?5: '?3`, an **outlives constraint**.
- Calling a function instantiates its where-clauses (`T: 'a`, implied bounds) as more constraints.
- Returning a value relates its region to the universal region in the return type. That's where problem case #3's
  requirement "`*cache` is borrowed for `'1`" comes from.

**Liveness constraints.** For every point P where a local is live, every region in the local's type must contain P. A
local of a type with drop glue is **drop-live** at its `drop` terminator. That single rule produces the `Drop`-is-a-use
behavior from §3.

**Region inference.** [RUSTC] The solver builds a graph with an edge for each `'a: 'b`, collapses its strongly connected
components (regions that must be equal), and propagates point sets along the edges until nothing grows: the smallest
solution. Then it checks the universal regions: if an existential region that must outlive `'1` ended up needing to
contain points *after* the function returns in a way the signature doesn't allow, or if one universal region must
outlive another without a declared bound, that's an error like "lifetime may not live long enough". When several
constraints are to blame, the diagnostic code searches the constraint graph for the most explainable path ("best
blame", Chapter 4.6).

**Borrows in scope.** A forward dataflow computes, for each point, which loans are in scope. A loan is **generated**
where it's created and **killed** where it leaves its region, or where a reference it was taken *through* is overwritten
(after `let r = &mut *p; p = &mut other;`, loans of `*p` can no longer be reached through `p`). At each access, the
checker compares the access with every loan in scope using Chapter 4.1's conflict table (prefixes, disjoint fields,
deref through `&mut`).

**Move and initialization dataflow.** Separate "maybe initialized / maybe uninitialized" analyses give E0382 (use of a
moved value), E0381 (use of an uninitialized one), and the answers drop elaboration needs (18.4).

**Why problem case #3 fails under NLL, precisely.** The loan created by `cache.get(&key)` flows into `v`, and on the
`return v` path it must outlive `'1`, which is universal: it contains every point in the body. NLL's outlives
constraints are **location-insensitive**. "The loan's region must include `'1`" holds everywhere, including on the
path that skips the `return` and reaches `cache.insert`. So the loan is in scope at the insert, which is a conflict. The
requirement really only applies on the path that returns.

**Polonius.** The next formulation (Chapter 4.2) flips the representation. Instead of regions as sets of points, it
tracks **origins as sets of loans**, and asks per point: "which loans might flow into a live origin *from here*?" On the
fall-through path, the loan from `get` doesn't flow into anything live (`v` is dead, the return didn't happen), so it's
not in scope at the insert, and the program is accepted. [VERSION] The Rust project has worked on this for years: first
a Datalog prototype (the `polonius` crate), then a location-sensitive analysis built into rustc (a 2025 Rust project
goal aimed at "stabilizable Polonius support on nightly"). On the Playground, the nightly of 2026-09-24 accepts problem
case #3 without any flag, while stable 1.98.1 and beta 1.99.0-beta.7 reject it. That's consistent with that work being
enabled by default on nightly, but the Playground can't show *which* analysis ran, so read the current release notes
before relying on it. The stated intent is that the new analysis accepts **more** programs, never fewer: every
NLL-accepted program should stay accepted.

**Erasure.** Once `mir_borrowck` succeeds, regions have served their purpose. [RUSTC] Every later stage works on types
with regions erased: the optimized MIR you've read prints `&std::string::String`, never `&'?3 String`, and the LLVM IR
in Chapter 18.6 has no lifetimes at all. That's the mechanical reason for Chapter 4.3's statement that lifetimes aren't
monomorphized and cost nothing at run time.

> **What actually happens?** Why doesn't the borrow checker run on optimized MIR, where more code is visibly dead and
> more borrows are gone? Because then what compiles would depend on the optimizer, which changes every release and
> differs between debug and release builds. A program must be accepted or rejected by rules you can reason about from
> the source (Part IV's rules), so the checker runs on MIR that faithfully mirrors the source.

### 5. Memory

- **Regions don't exist at run time.** Not as data, not as checks, not as metadata. Erasure (§4) is total. Every cost
  of borrow checking is paid at compile time.
- **Compile-time cost scales with loans × points.** [RUSTC] The dataflow analyses keep bitsets per basic block (loans in
  scope, initialized places), and region values are sets of points. For ordinary functions this is small. For huge
  generated bodies (a derive on a 200-field struct, a giant `match` from a state-machine macro, a big `async fn`) it can
  make `mir_borrowck` visible in a `-Z self-profile` report (order of magnitude, labeled; not verified here).
- **Closure bodies are separate.** Each closure (and each `async` block) is its own MIR body with its own borrow check,
  plus the propagated requirements the creator must prove. Combinator-heavy code has many small bodies instead of one
  large one.

### 6. CPU / OS

There's nothing to measure at run time: borrow checking leaves no trace in the binary. At compile time, it's part of
the per-crate front end (18.1), so its cost lands on your build's critical path, body by body. The practical levers are
the ones in §5: split giant generated functions, and keep macro output proportional to the input.

The important *OS-level* fact is indirect. Because the checker proves exclusive access at compile time, the generated
code needs no locks, reference counts, or barriers for ordinary `&mut` access, and LLVM gets `noalias` (Chapter 18.6
traces the attribute from the ABI computation to the IR).

---

## Pass 3 · Architect level — *API design against the checker*

### 7. Trade-offs

**Changes to a public type that change what borrow-checks downstream:**

| Change | Effect on callers' borrow checking | Semver |
|---|---|---|
| Add `impl Drop` to a type that holds a borrow (has a lifetime parameter) | Loans it holds now live until the value is dropped, not until last use (§3) | **Breaking** for callers that relied on the earlier end |
| Add a lifetime parameter to a type | Every user must now name or elide it; new constraints | Breaking |
| Change `&self` to `&mut self` on a method | Calls now need exclusive access; existing shared borrows conflict | Breaking |
| Return `impl Trait` that captures more lifetimes (edition 2024 RPIT, Chapter 7.3) | The returned value keeps more borrows alive | Breaking |
| Return owned data or an index instead of a reference | Callers' loans end immediately | Usually compatible to *add*; changes performance |
| Make a method take a closure instead of returning a reference | The borrow is confined to the call (Chapter 4.5's HRTB callbacks) | A redesign; strong isolation |

**Working with NLL today versus waiting for Polonius:** code written for NLL (the entry API, a second lookup, an
index) stays correct forever and often stays the better design. Rewriting to depend on Polonius-only acceptance before
it's stable ties your code to nightly. The only right time to simplify a workaround is when the stable compiler you pin
(Chapter 2.1) accepts the simpler form.

> **Why not make the checker path-sensitive in general, like a model checker?** Cost and predictability. Full path
> sensitivity is exponential in the number of branches, and its verdicts are hard to explain. NLL chose sets of points
> (polynomial, explainable) and gave up some precision. Polonius adds precision where it matters (a loan flowing
> through a conditional return) without exploring paths one by one.

### 8. Java comparison

Java has exactly one compile-time analysis in this family: **definite assignment** (JLS chapter 16). javac proves that
every local variable is assigned before it's read, and that a `final` variable is assigned at most once, by a dataflow
analysis over the method's control flow. That's Rust's step 5(b), the initialization dataflow behind E0381 and
E0382. Java has no counterpart to steps 1–4 and 5(a): there are no loans, so there's nothing to prove about aliasing.

The JVM does reason about object lifetimes, but at run time and for a different purpose. HotSpot C2's **escape
analysis** decides whether an allocation can escape its method or thread, and if not, it may replace the object with
scalars (scalar replacement) or remove locking on it (lock elision).

> **Analogy limit.** "Escape analysis is the JVM's borrow checker" fails in three ways. Escape analysis **optimizes and
> never rejects**: a program where objects escape is still valid Java. It's **speculative**: a JIT decision can be
> undone by deoptimization. And it's **invisible**: nothing in the source says what escaped. The borrow checker is a
> *language rule* that rejects programs, is decided once at compile time, and is written into your signatures.

### 9. Production scenario

**Meridian's policy for compiler-version differences.** Meridian pins stable 1.98.1 (Chapter 2.1). Its platform team
also runs a weekly **nightly canary** job: the full workspace built and tested on the latest nightly, allowed to fail,
reported to a channel. The job exists to see what's coming (new lints, new errors from soundness fixes, performance
changes) weeks before a stable release.

When the canary started compiling a deliberately rejected test case (problem case #3 kept as a "known NLL limitation"
test in the JWT claims cache crate, Chapter 4.2's entry-API rewrite), the team didn't change any production code. They
recorded three rules:

1. **Borrow-checker workarounds get a greppable marker**, `// NLL-LIMITATION: problem case #3`, next to the entry API
   call or double lookup. When a stable release accepts the simpler form, the marker finds every candidate.
2. **"Accepted on nightly" is not a reason to change code.** The pinned stable compiler is the contract.
3. **Compile-fail tests of borrow-checker behavior track the compiler, not the product.** They moved out of the
   product test suite into the canary job's expectations, so an improvement to the checker never breaks a release
   build.

### 10. Failure scenario

**The telemetry `Drop` that broke nine services.** `meridian-telemetry` (Chapter 7.3's shared crate, used by thirty
services) has a `Span<'a>` type that borrows a request's route name and records timings. Services call
`span.finish()` explicitly at the end of a handler.

In version 2.4.0, a minor release, the team added `impl Drop for Span<'_>` so that a span nobody finished would still be
recorded ("defensive telemetry"). The crate's own tests passed. Within a day, nine services failed to build after a
routine `cargo update`, with errors like listing `ch05-05-drop-is-a-use.rs`'s: E0502, "immutable borrow might be used
here, when `span` is dropped and runs the `Drop` code for type `Span`". Each failure had the same shape: a handler
created a span borrowing a field of the request, and later in the same scope mutated the request (normalizing a header,
setting a response code). Before 2.4.0 the span was dead after its last use. After it, the span was drop-live until the
end of the scope, and so was the loan.

The fix and the rules that followed:

- 2.4.0 was yanked, and 3.0.0 shipped with `Span` storing its route name as an `Arc<str>` (no lifetime parameter, so
  no loans to extend), with the `Drop` backstop kept. The design exercise below asks you to evaluate the alternatives.
- The crate's review checklist now includes this chapter's §7 table: adding `Drop` to a type with a lifetime parameter
  is a major-version change.
- Shared crates build a sample of downstream services in CI before release (a "reverse dependency" check), because a
  crate's own tests can't see how callers borrow.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVIII).*

1. List the borrow checker's steps on MIR and say which step produces each of: E0502, E0382, "lifetime may not live long
   enough".
2. Why does the borrow checker run on MIR before optimization? Give the consequence you can observe with `if false`.
3. What is a two-phase borrow, where is it created, and why does `Vec::push(&mut v, v.len())` fail when `v.push(v.len())`
   compiles?
4. Explain "drop-live". Why does an empty `impl Drop` change which programs compile?
5. What's the difference between universal and existential regions? Which one is `'1` in an error message?
6. How are closures borrow-checked, and what are "external requirements"?
7. Explain precisely why NLL rejects problem case #3 and what Polonius tracks differently. What does "accepted on
   nightly" mean for production code?
8. Compare borrow checking with Java's definite assignment and with HotSpot's escape analysis.

### 12. Exercises

- **Beginner.** For listing `ch05-04-no-drop-ok.rs`, mark the points of the loan's region by hand (statement by
  statement). Then do the same for `ch05-05-drop-is-a-use.rs` and circle the point that makes the difference.
- **Intermediate.** Write three programs that differ only in how they call a `&mut self` method with an argument that
  reads the receiver: method syntax, `Type::method(&mut x, …)`, and `(&mut x).method(…)`. Predict which compile, then
  check. Explain the third.
- **Advanced.** Rewrite problem case #3 so it compiles on stable 1.98.1, twice: once with `contains_key` and once with
  the entry API. Count the hash lookups each version does on the hit path and on the miss path, and explain, in terms of
  loans and regions, why each version is accepted when the original isn't.
- **Systems.** On a local nightly, use `-Z dump-mir=get_or_default -Z dump-mir-dir=mir_dump` on problem case #3 and
  find the borrowck-stage MIR and the region annotations. Identify the loan created by `get` and the constraint that
  forces it to contain the `insert` (not verifiable on the Playground).
- **Architecture.** You maintain a shared crate. Write the checklist a reviewer applies to any change of a public type
  that holds a borrow, covering `Drop`, lifetime parameters, receivers, and returned `impl Trait`. For each item,
  write the downstream code that would break.

### 13. Debugging exercise

A log-parsing helper collects request methods (listing `ch05-09-loop-backedge.rs`, verified):

```rust,compile_fail
fn main() {
    let lines = ["GET /pay", "GET /refund", "POST /pay"];
    let mut buf = String::new();
    let mut methods: Vec<&str> = Vec::new();
    for line in lines {
        buf.clear();
        buf.push_str(line);
        let method = buf.split(' ').next().unwrap();
        methods.push(method);
    }
    println!("{methods:?}");
}
```

```text
error[E0502]: cannot borrow `buf` as mutable because it is also borrowed as immutable
  --> src/main.rs:9:9
   |
 9 |         buf.clear();
   |         ^^^^^^^^^^^ mutable borrow occurs here
10 |         buf.push_str(line);
11 |         let method = buf.split(' ').next().unwrap();
   |                      --- immutable borrow occurs here
12 |         methods.push(method);
   |         ------- immutable borrow later used here
```

(A second, identical E0502 is reported at `buf.push_str`.)

1. Name the loan, its region, and the path by which the region reaches `buf.clear()`. Which step of §2 puts that point
   in the region?
2. Why is the error at `buf.clear()` in the *next* iteration rather than at `methods.push`?
3. Give three fixes with their costs: one that allocates, one that doesn't allocate per line, and one that changes
   what `methods` stores. Which would you pick for a log file with ten million lines?
4. Would Polonius accept this program? Justify with §4.

### 14. Design exercise

**A span type that can't break its callers.** Design `meridian-telemetry` 3.x's span API so that forgotten spans are
still recorded (the `Drop` backstop), without extending any caller's borrows. Evaluate at least these options:
`Arc<str>` names (the incident's fix), `&'static str` names only, an interned `RouteId(u32)`, a closure-scoped API
(`telemetry.span("route", |span| { … })`), and a guard that borrows nothing but the telemetry registry. For each, give
the allocation and hashing cost per request at the gateway's 400K requests per second, the ergonomics at the call site,
what the borrow checker sees (loans, drop-liveness), and the semver impact of migrating from 2.x. Choose one, and write
the one-paragraph rationale for the crate's changelog.
