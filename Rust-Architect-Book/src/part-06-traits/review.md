# Part VI Review — Trait-Design Review & Interview Mode

> Consolidate Part VI, then use it: review a plugin SDK whose trait design fails in three different ways (two compile
> errors and one silent bug), redesign it, and answer senior-level questions without notes. Answers are in
> **Appendix A, Part VI**.

---

## Part VI on one page

```text
 CONTRACT        trait = methods (+ assoc types/consts); impl = evidence; bound = requirement on a type parameter
                 generic bodies checked against bounds at the DEFINITION (E0369), unlike C++ templates
                 defaults are copied per impl; a default must be right for implementors who never heard of it
                 method resolution: R, *R, **R… × (U, &U, &mut U); inherent before trait; traits must be in scope
        │
 TYPE MEMBERS    associated type = OUTPUT (one impl per Self: Iterator::Item)
                 type parameter  = INPUT  (many impls per Self: From<T>)       missing input → E0282
                 blanket impl: impl<T: Bound> Trait for T  (ToString over Display, Into over From)
        │
 COHERENCE       ≤ 1 impl per (trait, type) in the whole program
                 orphan rule: own the trait or the type (fundamental: &, &mut, Box, Pin; dyn LocalTrait is local;
                 local type may appear as a trait parameter if no uncovered T precedes it: E0117 / E0210)
                 overlap check assumes upstream may add impls (E0119 "upstream crates may add a new impl")
        │
 TRAIT OBJECTS   dyn Trait: unsized; &dyn / Box<dyn> / Arc<dyn> = 16 bytes (data, vtable)
                 vtable: [drop | size | align | methods…]  (rustc: null drop entry for no drop glue: implementation detail)
                 call: mov rax,[fat+8]; call [rax + 24]   → no inlining, spills, predictor-dependent
                 dyn compatible ⇔ every method fits a vtable slot: no generics, no Self by value, no Sized supertrait
                 escape: where Self: Sized, extension trait over ?Sized, clone_box; guard with fn(&dyn Trait)
                 + Send + Sync are part of the type; generic args invariant, lifetime bound covariant
        │
 DECISION        generics: 0.8 ns · enum: 1.0 ns · dyn grouped: 2.0 ns · dyn mixed: 3.3 ns   (measured, noisy)
                 1 / 3 vs 1,000,001 allocations; ratio test: dispatch cost ÷ work per call → "where is the loop?"
                 dyn is not a plugin ABI: C ABI (hand-built vtable) / WASM / out of process
```

## Ten ideas to carry forward

1. **A trait is a contract that lives apart from the type.** Implementations are retroactive, within coherence.
2. **Generic code may use only what its bounds promise**, which makes signatures honest and errors local.
3. **Defaults are API decisions.** A wrong default fails silently, and a missing required method fails loudly.
4. **Associated types are outputs, parameters are inputs.** Ask "can a type implement this twice?"
5. **Blanket impls buy reach and cost exclusivity.** Adding one to a published trait is a breaking change.
6. **Coherence makes every trait call unambiguous**, and it's why impls must live with the trait or the type.
7. **A trait object is a fat pointer to a vtable.** Objects carry nothing, and the reference carries the dispatch table.
8. **Dyn compatibility is part of a trait's public API.** Design for it, and guard it with a compile-time assertion.
9. **Static vs dynamic is a per-call-site decision.** Dispatch *inside* hot loops is what costs, not dispatch per
   request.
10. **`dyn Trait` is an in-process mechanism, not an ABI.** Plugin boundaries need `extern "C"`, WebAssembly, or a
    process boundary.

---

## Capstone: review the fraud-rules SDK

Meridian's fraud team drafted **v0.1 of a rules SDK**: other teams implement `Rule`, and the engine loads the configured
rules into a `Vec<Box<dyn Rule>>`. The draft (listing `review-rules-sdk.rs`) doesn't compile, and one of its problems
would survive a fix to both compile errors:

```rust,ignore
use std::fmt;

pub struct Txn {
    pub amount_cents: i64,
    pub country: &'static str,
}

/// Meridian fraud SDK v0.1: teams implement Rule; the engine loads rules chosen by config.
pub trait Rule: Clone {
    fn id(&self) -> &str;
    fn score(&self, txn: &Txn) -> u32;
    fn explain<W: fmt::Write>(&self, txn: &Txn, out: &mut W) -> fmt::Result;

    /// Scores at or above this block the payment.
    fn block_at(&self) -> u32 {
        0
    }
}

#[derive(Clone)]
pub struct LargeAmount {
    pub over_cents: i64,
}

impl Rule for LargeAmount {
    fn id(&self) -> &str {
        "large-amount"
    }
    fn score(&self, txn: &Txn) -> u32 {
        if txn.amount_cents > self.over_cents { 60 } else { 0 }
    }
    fn explain<W: fmt::Write>(&self, txn: &Txn, out: &mut W) -> fmt::Result {
        write!(out, "{} > {} in {}", txn.amount_cents, self.over_cents, txn.country)
    }
}

impl fmt::Display for Vec<Box<dyn Rule>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in self {
            write!(f, "{} ", r.id())?;
        }
        Ok(())
    }
}

pub struct Engine {
    rules: Vec<Box<dyn Rule>>,
}

impl Engine {
    pub fn blocked(&self, txn: &Txn) -> bool {
        self.rules.iter().any(|r| r.score(txn) >= r.block_at())
    }
}

fn main() {
    let engine = Engine { rules: vec![Box::new(LargeAmount { over_cents: 500_000 })] };
    let small = Txn { amount_cents: 1_200, country: "IE" };
    println!("rules: {}", engine.rules);
    println!("blocked a 12.00 payment? {}", engine.blocked(&small));
}
```

rustc 1.98.1 reports ten errors. They're three kinds (trimmed; line numbers refer to the listing file):

```text
error[E0117]: only traits defined in the current crate can be implemented for types defined outside of the crate
39 | impl fmt::Display for Vec<Box<dyn Rule>> {
   | ^^^^^^^^^^^^^^^^^^^^^^------------------
   |                       |
   |                       `Vec` is not defined in the current crate

error[E0038]: the trait `Rule` is not dyn compatible
49 |     rules: Vec<Box<dyn Rule>>,
   |                    ^^^^^^^^ `Rule` is not dyn compatible
11 | pub trait Rule: Clone {
   |           ----  ^^^^^ ...because it requires `Self: Sized`
   |           |
   |           this trait is not dyn compatible...

   (the same E0038 six more times, at every use of `dyn Rule`)

error[E0308]: mismatched types
59 |     let engine = Engine { rules: vec![Box::new(LargeAmount { over_cents: 500_000 })] };
   |                                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected `Vec<Box<dyn Rule>>`, found `Vec<Box<LargeAmount>>`
```

And the compiler shows **one dyn-compatibility reason at a time**. With `: Clone` removed (listing
`review-rules-sdk-step2.rs`), the next one surfaces:

```text
error[E0038]: the trait `Rule` is not dyn compatible
14 |     fn explain<W: fmt::Write>(&self, txn: &Txn, out: &mut W) -> fmt::Result;
   |        ^^^^^^^ ...because method `explain` has generic type parameters
   = help: consider moving `explain` to another trait
```

**Your task:**

1. For **each** error kind, name the Part VI rule behind it (chapter and section) and explain it in one or two sentences
   in terms of *mechanism*: what the orphan check protects, what a vtable can't hold, and why the E0308 is a consequence
   rather than a separate bug.
2. Find the **silent bug**: assume both compile errors are fixed without touching `block_at`. What does
   `engine.blocked(&small)` return for a 12.00 payment, and why? Which Chapter 6.1 rule does the design break?
3. The engine will be shared by the fraud library's worker threads. What's missing from the trait, and what error would
   you get when you put the engine in an `Arc` and spawn workers?
4. **Redesign v0.2.** Keep: pluggable rules chosen by config, explanations for blocked payments, a printable rule list,
   and thread-safe sharing. Decide where the blocking threshold belongs. Decide whether `explain` stays generic (and
   how), and what replaces `Clone`.

Then answer three design questions:

- Should the *core* rules owned by the fraud team go through `dyn Rule` at all? Apply Chapter 6.5's matrix and ratio
  test.
- The analytics team asks to ship rules as separately compiled `.so` files implementing `Rule`. What do you tell them,
  and what do you offer instead?
- Which of your v0.2 decisions are breaking changes if made *after* v1.0 is published? List them.

A redesigned version (`review-rules-sdk-fixed.rs`, verified) prints:

```text
engine: [large-amount, risky-country] block_at=80
   1200 IE: score   0 -> allow
 750000 IE: score  60 -> allow
 750000 XX: score  90 -> large-amount: 750000 > 500000; risky-country: country XX;
```

Write your redesign first, then compare it with that listing and with the model answer.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language

1. What is the difference between `fn f<T: Trait>(x: T)`, `fn f(x: impl Trait)`, and `fn f(x: &dyn Trait)`? What does
   each compile to?
2. When do you use an associated type, and when a generic parameter? Use `Iterator`, `From`, and `Add` as evidence.
3. State coherence and the orphan rule. Why can't a third crate connect crate A's trait to crate B's type?
4. What makes a trait dyn compatible? Derive the rules rather than listing them.

### Compiler

5. How does the compiler resolve `x.method()` when `x: Rc<Box<String>>`? Where do `Deref` impls come in?
6. How does the overlap check treat impls from upstream crates? Why is the conservative choice the right one?
7. What does a vtable contain in current rustc, and which parts of that are guarantees?
8. Why can't a generic method live in a vtable, when Java interfaces can have generic methods?

### Performance

9. What does a dynamic call cost beyond the call instruction? Use this Part's assembly (the spill, the unrolled loop).
10. In the benchmark, grouped `dyn` beat mixed `dyn` with identical code. Explain the difference and how to prove it.
11. When does monomorphization hurt, and what do you do about it?

### Architecture

12. Walk through the decision between generics, an enum, and `dyn` for a middleware pipeline, a rule engine, and a
    decoder's inner loop.
13. How do you evolve a widely used trait without breaking downstream crates? Cover required vs default methods, blanket
    impls, sealing, and dyn compatibility.
14. Design a plugin system for code written by another team, including the ABI decision.
15. Compare Rust's dispatch model with the JVM's (inline caches, deoptimization, erasure) for a latency-sensitive
    service. Where does each win?

---

## Looking ahead: Part VII

Part VI kept saying "monomorphization" and moved on. Part VII opens it up. Chapter 7.1, *Generics from Call Site to
Binary*, follows a generic function all the way into machine code. Chapter 7.2, *Monomorphization vs Java Type
Erasure*, puts the two models side by side. It's the deeper version of this Part's recurring observation that Java
interfaces can hold generic methods while Rust vtables can't, and Rust can store `i64`s unboxed while Java can't.
Chapter 7.3, *Code Bloat, Compile Time, and How to Control Them*, presents the bill that this Part's static-dispatch
wins run up, and how to keep it in check.
