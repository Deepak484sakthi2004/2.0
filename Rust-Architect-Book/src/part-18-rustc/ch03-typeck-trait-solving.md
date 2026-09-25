# Chapter 18.3 — Type Checking and Trait Solving

> **Where this sits:** Part XVIII · How rustc Works · chapter 3 of 7
> **Prerequisites:** Chapter 18.1 (queries, per-body error gating), Chapter 18.2 (HIR), Chapter 17.5 (inference by
> unification), Chapters 6.1–6.3 (method resolution, blanket impls, coherence), Chapter 8.1 (`?`).
> **After this chapter you can:** say what the `typeck` query computes for a body and what it leaves to the borrow
> checker; read an E0277 as a failed proof and an E0275 as a proof that never ends; predict method resolution through
> autoderef, including how an unrelated `use` can change it; recognize inference fallback and why it changed in edition
> 2024; ask the compiler what it inferred; and keep trait-heavy designs inside a compile-time and error-quality budget.

---

## Pass 1 · User level — *Why the compiler says what it says*

### 1. Problem

Every Part so far has leaned on type checking without opening it. Chapter 6.1 gave you the method-resolution
algorithm. Chapter 8.1 showed that `?` calls `From::from` on the error. Chapter 10.1 showed closures capturing
individual fields. Chapter 7.3 showed a return-position `impl Trait` capturing lifetimes. You've read dozens of E0277s.

A few things that look arbitrary until you see the machinery:

- An E0277 often prints a *chain* of "required for … to implement …" notes, sometimes a dozen deep. Why a chain?
- An E0275 prints a type like `Vec<Box<Box<Box<Box<Box<Box<...>>>>>>>`, says "126 redundant requirements hidden", and
  writes the full type name to a file because it's too long for your terminal.
- Adding `use std::borrow::Borrow;` at the top of a module breaks a line that never mentions `Borrow`.
- The same four lines compile to different meanings in edition 2021 and edition 2024, and rustc 1.98.1 refuses both
  without an annotation.
- A crate built on a deeply layered middleware framework spends most of its build in the front end, before any code is
  generated.

All five come from the same two components: the **type checker**, which solves one inference problem per function body,
and the **trait solver**, which it calls to prove statements like `Vec<Cents>: Audit`.

### 2. Mental model

Type checking happens at two levels, and the split is a language rule, not an implementation detail:

1. **Items: declared, never inferred.** [LANG] Function signatures, struct fields, impl headers, where-clauses, and
   associated-type bounds are what you wrote. rustc *collects* them into queries (`type_of`, `fn_sig`,
   `predicates_of`, item bounds) and checks each item is well-formed, but it never infers a signature from a body.
2. **Bodies: an inference problem, one per body.** [RUSTC] The query `typeck(def_id)` walks a body's HIR (18.2),
   gives every expression a type, uses **inference variables** (`?T`, printed as `_`) where it doesn't know yet, and
   solves for them by **unification** (Chapter 17.5). Each requirement it meets, such as "this argument's type must
   implement `Clone`," becomes an **obligation** handed to the trait solver.

```text
 HIR body ──► typeck(body)                                        (one query per fn, const, static)
                │  give each expression a type or a variable ?T; unify when they must be equal
                │  coercion sites (let x: T = …, arguments, returns) ──► record ADJUSTMENTS (&mut→&, deref, unsize)
                │  method call? ──► PROBE: autoderef steps × {by value, &, &mut} × {inherent, then traits in scope}
                │  every bound ──► OBLIGATION ──► fulfillment loop: retry until no progress
                │                                        │
                │                                        └─► TRAIT SOLVER: "does Vec<Cents>: Audit hold?"
                │                                              candidates (impls, where-clauses, built-ins, auto traits)
                │                                              → pick one → NESTED obligations (Cents: Display) …
                │  leftovers: FALLBACK  {integer}→i32, {float}→f64, unconstrained diverging ?T → ! (2024) / () (≤2021)
                ▼
          TypeckResults ("writeback"): the type of every node, every adjustment, every method picked,
          every closure capture  ──► consumed by THIR/MIR building (18.4)
```

The trait solver answers each goal with one of three verdicts: **yes** (with nested obligations still to prove),
**no**, or **ambiguous** ("I can't tell until more types are known; ask me again"). Ambiguity is normal in the middle of
type checking. It's only an error if it's still there at the end.

**What `typeck` doesn't do: lifetimes.** [RUSTC] Type checking records region variables but doesn't solve them; its
results have regions erased. The borrow checker re-derives and solves all lifetime constraints on MIR (18.5). That's
why lifetime errors always arrive after type errors (18.1's table), and why a body with a type error is never borrow
checked.

### 3. Rust code

**Watching the solver answer "ambiguous."** A nightly-only testing attribute, `#[rustc_evaluate_where_clauses]`, makes
the compiler report how it evaluates a function's where-clauses at each call site (listing
`ch03-01-evaluate-where-clauses.rs`, nightly, verified):

```rust,ignore
#[rustc_evaluate_where_clauses]
fn needs_clone<T: Clone>(t: T) -> T {
    t.clone()
}

struct NotClone;

fn main() {
    needs_clone::<String>(String::new());
    needs_clone(NotClone);
}
```

```text
error: evaluate(Binder { value: TraitClause(<std::string::String as std::clone::Clone>, polarity:Positive), bound_vars: [] }) = Ok(EvaluatedToOk)
  --> src/main.rs:17:5
17 |     needs_clone::<String>(String::new());
error: evaluate(Binder { value: TraitClause(<_ as std::clone::Clone>, polarity:Positive), bound_vars: [] }) = Ok(EvaluatedToAmbig)
  --> src/main.rs:18:5
18 |     needs_clone(NotClone);
error[E0277]: the trait bound `NotClone: Clone` is not satisfied
  --> src/main.rs:18:17
18 |     needs_clone(NotClone);
   |     ----------- ^^^^^^^^ the trait `Clone` is not implemented for `NotClone`
```

(Trimmed; the dump also reports the implicit `T: Sized` bound for each call, with the same verdicts.) Two lessons in five
lines:

- With the turbofish, `T` is `String` when the callee's path is checked, so `String: Clone` is decided on the spot:
  `EvaluatedToOk`.
- Without it, `T` is still `_` at that moment. The argument hasn't been type-checked yet, so `_: Clone` is
  **ambiguous**, not false. The obligation is parked. Once the argument makes `T = NotClone`, the fulfillment loop
  retries it, the solver says no, and *that* is the real E0277, reported at the argument.

**An E0277 is a failed proof.** Listing `ch03-02-obligation-chain.rs` (verified):

```rust,compile_fail
use std::fmt::Display;

trait Audit {
    fn audit(&self) -> String;
}

impl<T: Display> Audit for Vec<T> {
    fn audit(&self) -> String {
        self.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")
    }
}

fn record<A: Audit>(a: &A) {
    println!("{}", a.audit());
}

struct Cents(i64); // no Display

fn main() {
    record(&vec![Cents(100), Cents(250)]);
}
```

```text
error[E0277]: `Cents` doesn't implement `std::fmt::Display`
  --> src/main.rs:23:12
   |
23 |     record(&vec![Cents(100), Cents(250)]);
   |     ------ ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ unsatisfied trait bound
   |     |
   |     required by a bound introduced by this call
   |
help: the trait `std::fmt::Display` is not implemented for `Cents`
help: the trait `Audit` is conditionally implemented for `Vec<T>`
  --> src/main.rs:10:1
   |
10 | impl<T: Display> Audit for Vec<T> {
   | ^^^^^^^^-------^^^^^^^^^^^^^^^^^^
   |         |
   |         unsatisfied requirement introduced here: `Cents: std::fmt::Display`
note: required for `Vec<Cents>` to implement `Audit`
note: required by a bound in `record`
```

(Trimmed.) Read it **bottom-up**, the way the solver built it:

```text
 goal      Vec<Cents>: Audit                     ← from `record`'s bound A: Audit, with A = Vec<Cents>
 candidate impl<T: Display> Audit for Vec<T>     ← the only impl whose header unifies (T = Cents)
 nested    Cents: Display                        ← the impl's where-clause, instantiated
 result    no candidate                          ← proof fails; the error names the LEAF, the notes give the path
```

That's the trait-solving version of Chapter 4.6's A–B–C method. The headline names the leaf goal that failed, and the
notes are the path from your code to it. When the chain is long, the useful line is usually the first `help:` pointing
at *your* impl or *your* type.

**A proof that never ends: E0275.** Listing `ch03-03-overflow.rs` (verified) has a blanket impl whose where-clause asks
about a *bigger* type than the one being proven:

```rust,compile_fail
trait Wire {
    fn size(&self) -> usize;
}

impl Wire for u64 {
    fn size(&self) -> usize {
        8
    }
}

impl<T> Wire for Vec<T>
where
    Vec<Box<T>>: Wire,
{
    fn size(&self) -> usize {
        self.len() * 8
    }
}

fn send<W: Wire>(w: &W) -> usize {
    w.size()
}

fn main() {
    println!("{}", send(&vec![1u64, 2, 3]));
}
```

```text
error[E0275]: overflow evaluating the requirement `Vec<Box<Box<Box<Box<Box<Box<...>>>>>>>: Wire`
  --> src/main.rs:17:18
   |
17 |     Vec<Box<T>>: Wire,
   |                  ^^^^
   |
   = help: consider increasing the recursion limit by adding a `#![recursion_limit = "256"]` attribute to your crate (`playground`)
   = note: 126 redundant requirements hidden
   = note: required for `Vec<Box<T>>` to implement `Wire`
   = note: the full name for the type has been written to '/playground/target/debug/deps/playground-c8502a156dae5745.long-type-12816773857436999290.txt'
```

(Trimmed.) Proving `Vec<u64>: Wire` needs `Vec<Box<u64>>: Wire`, which needs `Vec<Box<Box<u64>>>: Wire`, and so on. The
solver has no way to know this never terminates, so it stops at the recursion limit ([LANG] default 128, the same
`#![recursion_limit]` that bounds macro expansion in 18.2; 126 hidden plus the ones shown). The help text suggests
raising the limit. For this program, and for most real ones, that only makes the failure slower. The type is also why
rustc writes its name to a file: an interned type can grow faster than any terminal can print it.

**Inference fallback, and an edition that changed it.** Listing `ch03-04-never-fallback.rs` (verified under both
editions):

```rust,compile_fail
fn load<T: Default>() -> Result<T, String> {
    Ok(T::default())
}

fn run() -> Result<(), String> {
    load()?; // T is never constrained: only the fallback decides it
    Ok(())
}
```

Edition 2024:

```text
error[E0277]: the trait bound `!: Default` is not satisfied
  --> src/main.rs:10:5
   |
10 |     load()?; // T is never constrained: only the fallback decides it
   |     ^^^^^^ the trait `Default` is not implemented for `!`
   |
   = note: this error might have been caused by changes to Rust's type-inference algorithm (see issue #148922 <https://github.com/rust-lang/rust/issues/148922> for more information)
   = help: you might have intended to use the type `()` here instead
```

Edition 2021, same compiler:

```text
error: this function depends on never type fallback being `()`
  --> src/main.rs:9:1
   |
 9 | fn run() -> Result<(), String> {
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: specify the types explicitly
note: in edition 2024, the requirement `!: Default` will fail
   = warning: this was previously accepted by the compiler but is being phased out; it will become a hard error in Rust 2024 and in a future release in all editions!
   = note: `#[deny(dependency_on_unit_never_type_fallback)]` (part of `#[deny(rust_2024_compatibility)]`) on by default
help: use `()` annotations to avoid fallback changes
   |
10 |     load::<()>()?; // T is never constrained: only the fallback decides it
   |         ++++++
```

(Trimmed.) Nothing in `load()?;` says what `T` is. The statement discards the value, and the only other thing that
touches it is the `?` desugaring from 18.2, whose `Break` arm `return`s: an expression of type `!` (never). An
inference variable that only ever meets a diverging expression is a **diverging type variable**, and at the end of
type checking it gets a **fallback** type. [VERSION] Before edition 2024 the fallback was `()`; in edition 2024 it's
`!` (the edition guide's "Never type fallback change"). So in 2021 this program silently meant `load::<()>()`, and in
2024 it means `load::<!>()`, which fails because `!` isn't `Default`. On 1.98.1, *relying* on the old fallback is a
deny-by-default lint even in 2021. Listing `ch03-05-never-fallback-fixed.rs` writes `load::<()>()?;` and prints
`Ok(())` under both editions.

The integer version of the same idea is older and stable: [LANG] an integer literal whose type nothing constrains is an
`{integer}` variable that falls back to `i32` (and `{float}` to `f64`). That's why `let x = 10;` in Chapter 2.3 was an
`i32`.

**Method lookup, and a `use` that changes it.** Listing `ch03-06-borrow-import.rs` (verified):

```rust,compile_fail
use std::borrow::Borrow; // added by an IDE auto-import for an unrelated helper
use std::cell::RefCell;
use std::rc::Rc;

fn main() {
    let routes: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(vec!["/pay".to_string()]));
    let r = routes.borrow(); // meant: RefCell::borrow
    println!("{} route(s)", r.len());
}
```

```text
error[E0282]: type annotations needed for `&_`
  --> src/main.rs:11:9
   |
11 |     let r = routes.borrow(); // meant: RefCell::borrow
   |         ^
12 |     println!("{} route(s)", r.len());
   |                             - type must be known at this point
```

The probe (Chapter 6.1's algorithm, now with its consequence) walks the receiver's **autoderef steps**
(`Rc<RefCell<Vec<String>>>`, then `RefCell<Vec<String>>`, then `Vec<String>`, then `[String]`), and at each step tries
by value, then `&`, then `&mut`, looking at inherent methods first and then methods of **traits in scope**. Without the
import, step 0 finds nothing named `borrow` (`Rc` has no inherent `borrow`), and step 1 finds the inherent
`RefCell::borrow`. With the import, step 0 already has a candidate, `Borrow::borrow` via `&Rc<…>`, because `Rc<T>`
implements both `Borrow<Rc<T>>` (every type borrows as itself) and `Borrow<T>`. The probe stops at the first step
with a match, so it never reaches `RefCell`. And the result type `&Borrowed` is ambiguous between the two impls, hence
E0282. The fix (listing `ch03-07-borrow-import-fixed.rs`) names the method: `RefCell::borrow(&routes)`.

**Asking the compiler what it decided.** Type checking's results are mostly invisible, but nightly testing attributes
can print them (listing `ch03-08-typeck-dumps.rs`, nightly, verified):

```rust,ignore
#![feature(rustc_attrs, stmt_expr_attributes)]
#![allow(internal_features, dead_code)]
#![rustc_dump_hidden_type_of_opaques]

fn evens() -> impl Iterator<Item = u32> {
    (0..10).filter(|x| x % 2 == 0)
}

trait Store {
    #[rustc_dump_item_bounds]
    type Key: Clone + Send;
}

fn main() {
    let route = String::from("/pay");
    let log = #[rustc_capture_analysis]
    || println!("{}", route.len());
    log();
    let _ = evens();
}
```

```text
error: First Pass analysis includes:
note: Capturing route[] -> Immutable
error: Min Capture analysis includes:
note: Min Capture route[] -> Immutable
error: rustc_dump_item_bounds
14 |     type Key: Clone + Send;
   = note: Binder { value: TraitClause(<<Self as Store>::Key as std::marker::Send>, polarity:Positive), bound_vars: [] }
   = note: Binder { value: TraitClause(<<Self as Store>::Key as std::clone::Clone>, polarity:Positive), bound_vars: [] }
   = note: Binder { value: TraitClause(<<Self as Store>::Key as std::marker::Sized>, polarity:Positive), bound_vars: [] }
error: Filter<std::ops::Range<u32>, {closure@src/main.rs:9:20: 9:23}>
 --> src/main.rs:8:15
8 | fn evens() -> impl Iterator<Item = u32> {
```

(Spans trimmed.) Three decisions made visible:

- **Closure captures are computed by type checking.** The closure uses `route.len()`, which needs only `&route`, so it
  captures `route` by immutable reference (`[]` means the whole variable, no field projection). This is the analysis
  behind Chapter 10.1's capture rules and edition 2021's disjoint field capture.
- **Associated types carry an implicit `Sized` bound** you never wrote, listed alongside `Clone` and `Send`.
- **An `impl Trait` return has a hidden type** that the body defines: here `Filter<Range<u32>, {closure@…}>`, a type you
  couldn't even write (closure types are anonymous). Callers see only `impl Iterator<Item = u32>`.

On stable, the oldest trick still works: bind a value to a pattern of the wrong type and read the error (18.7 uses it).
Your editor's inlay hints ask rust-analyzer the same question.

---

## Pass 2 · Systems level — *Inside `typeck` and the solver*

### 4. Under the hood

**Collecting items.** [RUSTC] Before any body is checked, `rustc_hir_analysis` turns item signatures into queries:
`type_of` (the type of an item), `fn_sig`, `predicates_of` (every where-clause and bound, including the implicit
`Sized` ones the dump showed), and the headers of impls. It then **checks well-formedness** of each item: every type
mentioned must satisfy its own bounds (`struct Index<K: Hash>(HashMap<K, u32>)` requires `K: Hash` wherever
`Index<K>` appears). Coherence (Chapter 6.3) runs here too, over all impls of each trait.

**One inference context per body.** [RUSTC] `typeck(def_id)` creates a function context around an **inference
context**: a union-find table of type variables, plus separate kinds for integer (`{integer}`) and float (`{float}`)
literals. Closures are checked *together with* their enclosing body (a closure's `typeck` returns its parent's
results), which is how a closure's parameter types can be inferred from how the closure is used later. Speculative work,
such as trying a method candidate, runs inside a **snapshot** of the inference table that can be rolled back.

**Expectations and coercions.** [LANG] At a **coercion site** (a `let` with a type, a function argument, a return, a
struct field), the checker may insert a coercion instead of requiring equal types: `&mut T` to `&T`, deref coercion
(`&String` to `&str`), unsizing (`&[u8; 4]` to `&[u8]`, `Box<Circle>` to `Box<dyn Shape>`), reborrows, and function
items to function pointers (Chapter 2.4). [RUSTC] Each coercion, and each autoderef and autoref from method calls, is
recorded as an **adjustment** on the expression. THIR (18.4) turns adjustments into explicit `Deref` and `Borrow`
nodes, which is why the MIR you've been reading has `&(*_1)` reborrows you never wrote.

**The method probe.** [RUSTC] The listing above showed the order. Two details matter in practice:

- The probe collects the autoderef steps by following `Deref::Target` repeatedly (a walk over types; no code runs),
  stopping at a type with no `Deref` impl or at the recursion limit. `Box`, `Rc`, `Arc`, `String`, `Vec`, and your own smart pointers
  (Chapter 6.1's `Tracked<T>`) all add steps.
- Inherent methods beat trait methods **only at the same step**. A trait method found at an earlier step wins over an
  inherent method at a later one. That's the whole `Borrow` story, and it's why `use` statements that bring traits into
  scope are not free.

**Obligations and fulfillment.** [RUSTC] Every bound the checker meets (a callee's where-clause, a `?` needing
`From`, a `for` loop needing `IntoIterator`) is registered as an **obligation** with a **cause** chain recording *why*
it's required. Those causes become the "required for … / required by a bound in …" notes. The fulfillment context
processes pending obligations whenever the checker needs progress: each is handed to the solver; "yes" may add nested
obligations, "no" is an error, "ambiguous" stays pending. At the end of the body, fallback runs, fulfillment runs once
more, and anything still ambiguous becomes an E0282/E0283 ("type annotations needed").

**The solver.** [RUSTC] For a goal like `Vec<Cents>: Audit` it:

1. **Assembles candidates:** impls whose header could unify with the goal (including blanket impls), where-clauses from
   the current function's environment (inside `fn record<A: Audit>`, the fact `A: Audit` is simply assumed), built-in
   rules (`Sized`, `Copy` for primitives, `Fn*` for closures), auto-trait rules (`Send`/`Sync` are proven structurally
   from fields, and cycles are allowed: auto traits are *coinductive*), and object candidates for `dyn Trait`.
2. **Winnows** them to one, preferring where-clauses over impls. If more than one survives, the answer is ambiguous.
3. **Confirms** the winner: unifies the impl header with the goal (`T = Cents`) and returns the impl's where-clauses as
   nested obligations (`Cents: Display`).

Results are cached. To make a cache hit possible across different bodies, goals are **canonicalized**: inference
variables are renamed to placeholders, so `?7: Clone` in one function and `?3: Clone` in another are the same query.
Recursion depth is tracked, and exceeding the limit produces E0275.

**Two solvers.** [VERSION] rustc has been migrating to a new implementation, the "next-generation trait solver"
(crate `rustc_next_trait_solver`), designed with better caching, lazy normalization of associated types, and a cleaner
basis for closing soundness holes in the #25860 family (Chapter 4.4). Per the Rust 1.84 release announcement (January
2025), stable rustc uses it for **coherence checking**; the rest of type checking still used the old solver at that
point, and on nightly `-Znext-solver` enables it everywhere (not verified here: the Playground doesn't take `-Z`
flags). Check the types team's current status before relying on either's quirks. For you, the practical difference is
error wording and the occasional program that one accepts and the other rejects, which is exactly what the migration
works to eliminate.

**Opaque types.** [LANG] A return-position `impl Trait` is an opaque type whose hidden type is inferred from the body
(the dump's `Filter<…>`) and checked against the declared bounds. Callers can't see it, with one deliberate leak: auto
traits. Whether `evens()` is `Send` is decided by the hidden type, which is how an `async fn`'s future can be `Send` or
not depending on what it holds across an `.await` (Chapter 12.3).

**Writeback.** [RUSTC] At the end, everything is resolved into `TypeckResults`: the type of every HIR node, every
adjustment, which method each call resolved to, each closure's captures, and each pattern's binding mode. THIR and MIR
building (18.4) read these results. They never re-run inference.

> **What actually happens?** Why doesn't the compiler just try `RefCell::borrow` when `Borrow::borrow` turns out to
> be ambiguous? Because the probe picks a *method* before it knows the full types, and backtracking across method
> choices would make type checking exponential and its results hard to predict. rustc commits to the first step with
> a match. The price is the occasional surprise you saw above; the benefit is that method resolution is a fast,
> deterministic, local decision you can reason about.

### 5. Memory

- **Types are interned, and nested generics create many of them.** [RUSTC] Every distinct type (`Vec<u64>`,
  `Vec<Box<u64>>`, `Filter<Range<u32>, {closure}>`) is allocated once in the compiler's arena. Designs that nest
  generics deeply (layered middleware, iterator and future combinators) create many large, distinct types, each of
  which is interned, hashed, and compared.
- **Type names can outgrow a terminal.** The E0275 above wrote its type to a `.long-type-….txt` file. When you see
  that note on a real project, the type in the file is usually the best explanation of why the build is slow.
- **Obligation trees and caches live for the whole session.** A trait-heavy crate holds a large evaluation cache in
  memory, one more reason (after 18.1's) why one huge crate costs more RAM to compile than several smaller ones.

### 6. CPU / OS

Type checking is part of the front end, which runs mostly on one thread per crate (18.1, §4). Trait solving is
therefore serial time on your build's critical path. To see it, profile one crate on a local nightly toolchain (not
verified here):

```text
cargo +nightly rustc -p gateway -- -Z self-profile
summarize summary gateway-<pid>.mm_profdata | head -30     # from the measureme tools
```

In trait-heavy crates, `typeck` and the trait-evaluation queries rise toward the top of that table. The fix is almost
never a faster machine. It's usually a type that's too deep, a blanket impl that's too broad, or a combinator stack that
should be boxed at a boundary (§7 and §10).

---

## Pass 3 · Architect level — *Designing for the solver*

### 7. Trade-offs

| Design choice | Helps | Costs | Guidance |
|---|---|---|---|
| Blanket impls (`impl<T: Display> Audit for T`) | Everything that qualifies works automatically | Coherence and semver (a new blanket impl is breaking, 6.2); more candidates for every goal; confusing errors | Prefer opt-in (a marker trait or explicit impls) for public traits |
| Associated types vs generic trait parameters (6.2) | An associated type is determined by the implementing type, so `<T as Iterator>::Item` needs no search | Less flexible: one `Item` per type | Use associated types for outputs; parameters only when one type really has many impls |
| Deep static nesting (combinators, layers) | Zero-cost dispatch (6.5), full inlining | Front-end time, huge types, unreadable errors, E0275 risk | Box or erase at layer-group boundaries; keep hot inner layers static |
| Inference-heavy code (`collect()`, `parse()`, `into()`, `Default::default()`) | Concise | Meaning depends on distant context; editions can change fallback; an added impl can create ambiguity | Annotate at API boundaries and wherever a type is a *decision*, not an accident |
| `impl Trait` returns | Hide long or unnameable types; errors mention the short name | Opaque to callers except auto traits; can't name it in a struct field | Default for iterator/future-returning APIs; name a concrete type when callers must store it |

> **Why not whole-program type inference, like ML or Haskell, where signatures are optional?** Because Rust keeps
> inference **local to one body** [LANG]. Signatures are the contract between bodies, which is what makes each body
> checkable on its own. That locality gives you readable errors (the problem is in this function), incremental
> compilation's early cutoff (18.1: a body edit that keeps the signature doesn't re-check callers), and semver (a
> function's type can't change because its body did). Return-position `impl Trait` is the one deliberate hole:
> auto traits leak from the body. That's why changing what an `async fn` holds across an `.await` can be a breaking
> change.

### 8. Java comparison

javac's attribute phase is Java's type checker, and it does real inference too. Since Java 8, generic method
inference is specified as constraint solving over **inference variables and bound sets** (JLS chapter 18). That's why
Java errors say things like "inference variable T has incompatible bounds." Overload resolution picks among methods in
three phases (JLS §15.12.2: strict, then loose with boxing, then varargs). Lambdas are typed from their target
("poly expressions").

What Java doesn't have is a *search* for implementations. Whether `Invoice implements Comparable<Invoice>` is a fact
written on the class. The compiler looks it up. Rust's `impl` blocks are separate, possibly conditional, possibly
blanket facts, closer to logic-programming clauses:

```text
 impl<T: Display> Audit for Vec<T>      ≈   Audit(Vec<T>)  :-  Display(T).
 goal: Audit(Vec<Cents>)?               →   need Display(Cents)?  →  no clause  →  E0277
```

(The chalk project, which fed ideas into the new solver, framed Rust trait solving explicitly in these terms.)

| | javac | rustc |
|---|---|---|
| Inference scope | An expression / call, with target typing | One whole body (all statements) |
| Signatures inferred? | No (`var` infers locals only, JEP 286, Java 10) | No (`let` infers locals; items never) |
| "Does type X implement Y?" | Lookup in declared supertypes | Proof search over impls, where-clauses, built-ins, auto traits |
| Conditional implementation | Not expressible | `impl<T: Display> Audit for Vec<T>` |
| Failure | "incompatible types", "cannot find symbol" | A failed proof with its derivation (E0277), or an endless one (E0275) |
| Fallback | None by divergence | `{integer}`→`i32`, diverging `?T` → `!` (2024) |

> **Analogy limit.** "A Rust trait is a Java interface" holds for dispatch and fails for *implementation*. In Java,
> implementing an interface is part of a class's identity and can't depend on type arguments (`List<String>` and
> `List<Integer>` implement the same interfaces). In Rust, `Vec<Cents>` and `Vec<u64>` can implement different traits,
> and whether they do is computed. That's what makes Rust's errors longer and its abstractions more precise.

### 9. Production scenario

**payments-core's edition-2024 migration.** After the session manager's hand-flipped migration (Chapter 7.3's
incident), Meridian's runbook requires `cargo fix --edition` and a review of every change it makes. When payments-core
(Chapter 8.4) moved from edition 2021 to 2024, `cargo fix` applied the compiler's `::<()>` suggestion at eleven call
sites flagged by `dependency_on_unit_never_type_fallback`, the lint from §3.

The review of those eleven sites was the valuable part. Nine were harmless: calls like `load()?;` whose only purpose
was the side effect, where `()` was genuinely meant. Two were not. They were calls of the form `settings::load()?;`
placed at startup as **validation**: "fail fast if the environment is misconfigured." The loader was generic,
`fn load<T: FromEnv>() -> Result<T, ConfigError>`, and `FromEnv` had an impl for `()` (for components with no
settings), which reads nothing and always succeeds. With `T` unconstrained, fallback had chosen `()` since the lines
were written. The startup validation had been validating nothing for about a year.

The fix was to name the type (`settings::load::<PaymentsConfig>()?;`), and the team added two rules:

1. A generic function called only for its effect must have its type argument written at the call site. CI runs with
   the lint at deny (as 1.98.1 does by default), so fallback-dependent calls can't come back.
2. Startup validation loads the real configuration type and *passes the value on* to the components that need it. A
   generic loader called only for its side effect is a review finding, because nothing in the code says what it
   checked.

### 10. Failure scenario

**The middleware stack that stopped compiling.** A partner-API team at Meridian built its edge service with a fully
static middleware stack, modeled on Chapter 6.5's `Stack<A, B>` but with a recursive "wrap every layer in a metrics
layer" combinator. Its core impl had the shape of listing `ch03-03-overflow.rs`: an impl for `Stack<L>` whose
where-clause required `Stack<Metered<L>>: Layer`, a *bigger* type.

It worked with four layers. The fifth produced an E0275 with a type name written to a file. The developer followed the
compiler's help text and set `#![recursion_limit = "512"]`. The crate compiled again, but its `cargo check` time went
from under a minute to several minutes, and error messages anywhere in the crate became pages of nested types. Two
weeks later, a sixth layer hit the new limit and the CI job that ran `cargo check` timed out.

What went wrong, in this chapter's terms:

- The where-clause asked about a larger type than the impl's self type, so each proof step created a new goal instead of
  reducing to a known one. Raising the recursion limit let the solver go deeper; it didn't make the proof terminate
  sooner.
- Every intermediate type was interned and hashed, and every error printed them.

The fix had two parts. The recursive where-clause was replaced by applying the metrics wrapper once, at construction
(the combinator became a function, not a trait bound). The layer stack was split into two groups with a boxed service
between them (Tower's `BoxService`, Chapter 22.3), which caps type depth at the price of one indirect call per request,
negligible next to the network I/O. The team's review rules now say: never raise `recursion_limit` without a design
review, and a where-clause that mentions a type bigger than the impl's self type is a red flag.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVIII).*

1. What does the `typeck` query compute for a body, and what does it deliberately not compute? Where do those results
   go next?
2. Explain "ambiguous" as a trait-solver verdict. Why is it normal in the middle of type checking, and when does it
   become an error?
3. Read an E0277 with three "required for" notes. In what order did the solver build them, and which line usually
   points at the fix?
4. Why does E0275 happen, and why is raising `recursion_limit` usually the wrong fix?
5. Walk through method lookup for `x.borrow()` where `x: Rc<RefCell<T>>`, with and without `use std::borrow::Borrow`.
6. What is never-type fallback, what changed in edition 2024, and why does rustc 1.98.1 reject code that relies on the
   old fallback even in edition 2021?
7. Why are Rust item signatures never inferred? Name three properties of the compiler or ecosystem that depend on it.
8. Compare "does `Invoice` implement `Comparable`?" in javac with "does `Vec<Cents>` implement `Audit`?" in rustc.

### 12. Exercises

- **Beginner.** Remove the turbofish from `needs_clone::<String>(String::new())` in listing
  `ch03-01-evaluate-where-clauses.rs`. Predict the dump's verdict for that call, then check it on nightly.
- **Intermediate.** Write three E0277s whose note chains are one, two, and four "required for" notes deep (hint: nest
  `Vec`, `Option`, and a blanket impl). For each, identify the leaf goal and the note you'd act on.
- **Advanced.** Make the E0275 in listing `ch03-03-overflow.rs` go away *without* raising the recursion limit and
  without changing `send`'s signature, while keeping `Vec<T>: Wire` for every `T: Wire`. Explain why your where-clause
  terminates.
- **Systems.** On a local nightly toolchain, run `-Z self-profile` on a crate that uses a layered middleware or parser
  combinator library, and report the share of time in `typeck` and trait evaluation. Then box one layer boundary and
  measure again. (Not verifiable on the Playground.)
- **Architecture.** Your platform team wants a blanket `impl<T: Serialize> Auditable for T` in `meridian-types`.
  Analyze it for coherence (Chapter 6.3), semver (6.2), inference ambiguity, and compile time, and propose an
  alternative that keeps most of the convenience.

### 13. Debugging exercise

A market-data snapshot function doesn't compile (listing `ch03-09-clone-ref.rs`, verified):

```rust,compile_fail
#[derive(Debug)]
struct Quote {
    symbol: String,
    px: u64,
}

fn snapshot(book: &[&Quote]) -> Vec<Quote> {
    book.iter().map(|q| q.clone()).collect()
}
```

```text
error[E0277]: a value of type `Vec<Quote>` cannot be built from an iterator over elements of type `&Quote`
    --> src/main.rs:11:36
     |
  11 |     book.iter().map(|q| q.clone()).collect()
     |                                    ^^^^^^^ value of type `Vec<Quote>` cannot be built from `std::iter::Iterator<Item=&Quote>`
     |
note: the method call chain might not have had the expected associated types
    --> src/main.rs:11:17
     |
  11 |     book.iter().map(|q| q.clone()).collect()
     |     ---- ------ ^^^^^^^^^^^^^^^^^^ `Iterator::Item` changed to `&Quote` here
     |     |    |
     |     |    `Iterator::Item` is `&&Quote` here
     |     this expression has type `&[&Quote]`
```

(Trimmed.)

1. What type is `q`, and which `clone` did method lookup pick? Walk the autoderef steps.
2. Why does the error appear at `collect`, not at `clone`?
3. Give two fixes, one that changes `Quote` and one that doesn't, and say which you'd choose for a hot path.
4. If someone changes the return type to `Vec<&Quote>` so it compiles, which lint flags the `clone` then, and why wasn't
   it reported in the failing version? (Hint: Chapter 18.1's table.)

### 14. Design exercise

**A `Money` API that keeps inference predictable.** `meridian-types` (Chapter 5.3) is adding arithmetic to `Money`.
One proposal: `impl<T: Into<i64>> Add<T> for Money`, so `price + 5`, `price + qty_u32`, and `price + cents_i64` all
work. Another: only `impl Add<Money> for Money` plus explicit constructors (`Money::cents(5)`).

Evaluate both against this chapter: how each affects inference for integer literals (what is `5` when several
`Into<i64>` impls apply?), the error messages callers will see, the risk of E0282/E0283 when a new impl is added
later, coherence with a future `impl Add<Percent> for Money`, and front-end cost in the thirty services that depend on
the crate. Decide, and write the two rules you'd put in the crate's contribution guide.
