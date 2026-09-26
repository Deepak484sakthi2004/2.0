# Chapter 17.5 — Type Checking and Type Inference

> **Where this sits:** Part XVII · Compilers · chapter 5 of 8
> **Prerequisites:** Chapter 17.4 (resolution and side tables); Chapter 2.3 (Rust's local type inference, E0282,
> `{integer}` and `{float}`); Part VII (generics and monomorphization).
> **After this chapter you can:** write a bidirectional type checker that reports each mistake once, at the innermost
> wrong expression; implement Hindley–Milner inference with unification over a union-find; explain let-polymorphism
> and the occurs check; and say exactly where Rust, Java, and ML draw their inference boundaries, and why.

---

## Pass 1 · User level — *Proving that operations make sense*

### 1. Problem

Chapter 17.1's pipeline rejected `x + true` with "Add needs Int operands, found Int and Bool". That one line hides the
three hard problems of type checking:

- **Where to report.** In `let total: int = if flag { 10 } else { false };` the mistake is the `false`. A checker that
  computes the `if`'s type first and then complains about the whole `let` points at the wrong place.
- **How often to report.** After `let z = flag + 1;` fails, `z` has no sensible type. If every later use of `z`
  produces another error, the user gets ten messages for one mistake.
- **How much to infer.** Writing `let n: int = add(2, 3) * 4;` is tedious; the compiler can work out `int`. Can it
  also work out a function's parameter types from its body? Rust says no, ML says yes, Java says "only for locals with
  an initializer". Each answer is a design decision with consequences for error messages, compile time, and API
  stability.

This chapter builds two checkers for Ore. The first is **bidirectional**: it answers the first two problems with a
checking mode and an error type. The second is **Hindley–Milner inference**: it answers the third problem the way ML
does, by solving equations between type variables. Then it maps both onto rustc.

### 2. Mental model

**Bidirectional checking** runs in two modes:

```text
infer(e)            -> type          synthesis: "what type does e have?"      bottom-up
check(e, expected)  -> ok | error    checking:  "does e have this type?"      pushes the expectation DOWN

check(if c { 10 } else { false }, int):
    check(c, bool)       ok
    check(10, int)       ok
    check(false, int)    ERROR at `false`: expected int, found bool     <- the innermost wrong expression
```

Literals and variables are *inferred*. `if`, blocks, and function arguments are *checked* when an expected type is
known, so the expectation flows into their branches and tails, and a mismatch is reported where it actually is. Two
special types make the checker robust:

- **`{error}`**: the type of anything that already produced an error. It's compatible with everything, so it
  never produces a second error.
- **`!` (never)**: the type of `return`. It coerces to every type, so `if flag { n } else { return 0 }` has type
  `int`.

**Inference** treats unknown types as variables and solves equations:

```text
fn pick(c, a, b) { if c { a } else { b } }

  assign variables:  c: t0   a: t1   b: t2   result: t3
  constraints:       t0 = bool          (an `if` condition)
                     t1 = t2            (both branches have the same type)
                     t3 = t1            (the result is the branches' type)
  solution:          fn(bool, t1, t1) -> t1
  generalize:        forall a. fn(bool, a, a) -> a      (t1 is unconstrained: pick works for any a)
```

Solving is **unification**: to make two types equal, bind a type variable to the other type, or, if both are
constructed types like `fn(..) -> ..`, unify their parts. The variables and their bindings live in a **union-find**
structure. One check prevents nonsense: the **occurs check** refuses to bind `t` to a type that contains `t` itself,
since `t = fn(t) -> u` has no finite solution. Finally, **generalization** turns leftover variables of a top-level
function into `forall` parameters (*let-polymorphism*), so one definition can be used at several types.

### 3. Rust code

**A bidirectional checker.** Listing `ch05-01-type-checker.rs` type-checks Ore (resolution and checking are fused here,
as small compilers often do). The two modes are two methods. `check` handles the forms that can push an expectation
down, and falls back to `infer` plus a comparison:

```rust,ignore
    fn check(&mut self, e: &Expr, expected: &Ty) {
        match &e.kind {
            ExprKind::If(c, then, els) => {
                self.check(c, &Ty::Bool);
                self.check_block(then, expected);
                match els {
                    Some(els) => self.check(els, expected),
                    None if !compatible(&Ty::Unit, expected) => {
                        self.error(e.span, format!("`if` without `else` has type (), expected {expected}"));
                    }
                    None => {}
                }
            }
            ExprKind::Block(b) => self.check_block(b, expected),
            _ => {
                let found = self.infer(e);
                if !compatible(&found, expected) {
                    self.error(e.span, format!("mismatched types: expected {expected}, found {found}"));
                }
            }
        }
    }
```

The error type is one line in `compatible` and one early return in `infer`:

```rust,ignore
fn compatible(found: &Ty, expected: &Ty) -> bool {
    found == expected || matches!(found, Ty::Never | Ty::Error) || *expected == Ty::Error
}
```

```rust,ignore
            ExprKind::Binary(op, a, b) => {
                let (ta, tb) = (self.infer(a), self.infer(b));
                if ta == Ty::Error || tb == Ty::Error {
                    return Ty::Error; // already reported: stay quiet
                }
```

(Excerpts of listing 17.5-1.) The test program has one correct function and one with five mistakes:

```text
fn main() -> int {
    let flag = 3 > 2;
    let total: int = if flag { 10 } else { false };
    let y = add(1);
    let z = flag + 1;
    let w = z * 2;
    while total { }
    if flag { return true; }
    total
}
```

Real output:

```text
fn add   lets []  0 errors
fn good  lets [n: int, big: bool, v: int]  0 errors
fn main  lets [flag: bool, total: int, y: {error}, z: {error}, w: {error}]  5 errors

12:44: error: mismatched types: expected int, found bool
      at `false`
13:13: error: this function takes 2 arguments but 1 was supplied
      at `add(1)`
14:13: error: no implementation for `bool + int`
      at `flag + 1`
16:11: error: mismatched types: expected bool, found int
      at `total`
17:22: error: mismatched types: expected int, found bool
      at `true`
```

Each error points at the innermost wrong expression: `false` inside the `else` (not the whole `let`), `total` as a
`while` condition, `true` inside `return` (checked against the function's return type). And `let w = z * 2;` produced
**no** error. `z` already has type `{error}`, so `z * 2` quietly has type `{error}` too, and `w` inherits it. Five
mistakes, five messages. In `good`, `let v: int = if flag { n } else { return 0 };` checks without complaint because
`return 0` has type `!`.

**Hindley–Milner inference.** Listing `ch05-02-inference.rs` infers Ore functions that have *no annotations at all*.
The core is `find` (follow bindings, compressing paths) and `unify`:

```rust,ignore
    /// unify(): make two types equal by binding variables, or fail.
    fn unify(&mut self, a: &Ty, b: &Ty) -> Result<(), String> {
        let (a, b) = (self.find(a), self.find(b));
        if let Some(log) = &mut self.log {
            log.push(format!("unify {} ~ {}", show(&a, &[]), show(&b, &[])));
        }
        match (&a, &b) {
            (Ty::Var(x), Ty::Var(y)) if x == y => Ok(()),
            (Ty::Var(v), t) | (t, Ty::Var(v)) => {
                if self.occurs(*v, t) {
                    let t = self.zonk(t);
                    return Err(format!("infinite type: t{v} = {}", show(&t, &[])));
                }
                self.parent[*v as usize] = Some(t.clone()); // union
                Ok(())
            }
            (Ty::Int, Ty::Int) | (Ty::Bool, Ty::Bool) | (Ty::Unit, Ty::Unit) => Ok(()),
            (Ty::Fn(p1, r1), Ty::Fn(p2, r2)) if p1.len() == p2.len() => {
                for (x, y) in p1.iter().zip(p2) {
                    self.unify(x, y)?;
                }
                self.unify(r1, r2)
            }
            _ => {
                let (a, b) = (self.zonk(&a), self.zonk(&b));
                Err(format!("mismatched types: {} vs {}", show(&a, &[]), show(&b, &[])))
            }
        }
    }
```

(Excerpt of listing 17.5-2.) The input:

```text
fn add(a, b) { a + b }
fn pick(c, a, b) { if c { a } else { b } }
fn id(x) { x }
fn twice(f, x) { f(f(x)) }
fn fact(n) { if n < 2 { 1 } else { n * fact(n - 1) } }
fn use_them() {
    let w = if pick(false, true, false) { 1 } else { 0 };
    pick(true, 1, 2) + add(3, 4) + twice(id, 5) + w
}
fn self_apply(x) { x(x) }
fn branches(x) { if x { 1 } else { false } }
fn plus_bool(n) { n + true }
```

Real output, with the unification log printed for `twice`:

```text
add        : fn(int, int) -> int
pick       : forall a. fn(bool, a, a) -> a
id         : forall a. fn(a) -> a
twice      : forall a. fn(fn(a) -> a, a) -> a
             unify t9 ~ fn(t10) -> t12
             unify fn(t10) -> t12 ~ fn(t12) -> t13
             unify t10 ~ t12
             unify t12 ~ t13
             unify t13 ~ t11
fact       : fn(int) -> int
use_them   : fn() -> int
self_apply : error: infinite type: t26 = fn(t26) -> t28
branches   : error: mismatched types: int vs bool
plus_bool  : error: mismatched types: bool vs int
```

Every result is worth reading:

- `add` is monomorphic: `+` forced both parameters to `int`.
- `pick`, `id`, and `twice` are **polymorphic**. Nothing in their bodies fixed the type variables, so generalization
  quantified them. `use_them` then calls `pick` at `bool` *and* at `int`, and `twice(id, 5)` instantiates both
  schemes with fresh variables. That's let-polymorphism.
- The `twice` log shows the algorithm at work. The inner call `f(x)` constrains `f` (t9) to `fn(t10) -> t12`. The outer
  call `f(f(x))` then requires `f` to *also* be `fn(t12) -> t13`, so the argument and result types must be equal:
  `t10 = t12 = t13`, and finally the body's type is the return type `t11`. One variable survives: `forall a.
  fn(fn(a) -> a, a) -> a`.
- `fact` recurses. Inside its own body a function is monomorphic (the listing binds it without generalizing), and
  `n < 2` plus `n * ...` pin everything to `int`.
- `self_apply` needs `x` to be a function that accepts itself: `t26 = fn(t26) -> t28`. The occurs check rejects it.
- `branches` and `plus_bool` are ordinary type errors, found by unification failing.

Note the error messages: "mismatched types: int vs bool" with no location. HM inference is famous for this. The
equation that fails is often far from the mistake, because an earlier, *wrong* use of a variable already fixed its
type. Bidirectional checking with annotations at boundaries (listing 17.5-1) produces better messages, which is a
large part of why modern languages mix the two.

---

## Pass 2 · Systems level — *Where Rust draws the lines*

### 4. Under the hood

**Rust's inference is HM-style unification inside a function body** [LANG] [RUSTC]. The type checker (`rustc_hir_typeck`)
creates inference variables for every unknown type in a body, and unifies them as it walks the body in order,
together with *trait obligations* (`T: Add<U>`, method calls) that the trait solver discharges (Chapter 18.3). It's
also bidirectional: an *expectation* is passed down into expressions, which is how `let x: Vec<u8> = Vec::new();`
and closure argument types work. Listing `ch05-04-rust-inference-flow.rs` shows information flowing both forward and
backward within a body (real output):

```text
a: i32
b: f64
c: u64 (d = 7)
v: alloc::vec::Vec<u8>
parsed: alloc::vec::Vec<u16> = [1, 2, 3]
```

`a` and `b` had no constraint beyond being literals, so they fell back to `i32` and `f64` (the `{integer}` and
`{float}` inference variables of Chapter 2.3). `c` was fixed by a *later* line, `let d: u64 = c;`. `v`'s element type
came from a `push(1u8)` two lines after `Vec::new()`. And `parse()` in the middle of an iterator chain got its target
type `u16` from the annotation on the *collection*, through `collect`'s `FromIterator` bound.

**But the boundaries are firm.** Two deliberate restrictions separate Rust from ML:

*Signatures are not inferred.* Listing `ch05-05-rust-e0121.rs`:

```rust,compile_fail
fn answer() -> _ {
    42
}
```

```text
error[E0121]: the placeholder `_` is not allowed within types on item signatures for return types
 --> src/main.rs:4:16
  |
4 | fn answer() -> _ {
  |                ^ not allowed in type signatures
  |
help: replace with the correct return type
  |
4 - fn answer() -> _ {
4 + fn answer() -> i32 {
  |
```

rustc *knows* the answer (it suggests `i32`) and refuses anyway. A signature is a contract, checked on its own, so each
function body can be type-checked independently of every other body. That's what makes type checking parallelizable
and incremental in rustc's query system (Chapter 18.1), and what makes a function's API independent of its
implementation (a body change can't silently change callers' types, a semver property; Part XXII).

*Closures are not generalized.* Listing `ch05-03-rust-closure-monomorphic.rs`:

```rust,compile_fail
fn main() {
    let id = |x| x;
    let a = id(1);
    let b = id(true);
    println!("{a} {b}");
}
```

```text
error[E0308]: mismatched types
 --> src/main.rs:8:16
  |
8 |     let b = id(true);
  |             -- ^^^^ expected integer, found `bool`
  |             |
  |             arguments to this function are incorrect
  |
note: expected because the closure was earlier called with an argument of type `{integer}`
 --> src/main.rs:7:16
  |
7 |     let a = id(1);
  |             -- ^ expected because this argument is of type `{integer}`
  |             |
  |             in this closure call
note: closure parameter defined here
 --> src/main.rs:6:15
  |
6 |     let id = |x| x;
  |               ^
```

Listing 17.5-2's `use_them` called `pick` at two types; Rust's closure `id` gets exactly one parameter type, fixed by
its first use. HM would accept this program. Rust asks you to write a generic function (`fn id<T>(x: T) -> T`), which
is explicitly polymorphic *and* monomorphized (Part VII): each instance is compiled separately, so polymorphism never
costs a boxed, uniform representation at run time. Notice the diagnostic's second note. rustc remembers *why* the
closure's parameter got its type and reports the earlier call, which is exactly the "the equation that fails is far
from the mistake" problem, solved by recording provenance.

**Error types in rustc** [RUSTC]. rustc has an error type (`ty::Error`, which can only be created together with an
`ErrorGuaranteed` token proving that an error was already emitted) that plays the role of listing 17.5-1's `{error}`:
it unifies with anything and suppresses follow-up errors. The token is a nice piece of API design: the type system
of the compiler itself makes it impossible to create the silent error type without having reported a real error.

### 5. Memory

Type checkers allocate types constantly, and compare them even more often. Three representations matter:

- **Interned types.** Listing 17.5-2 clones `Ty` trees freely, which is fine for a teaching checker. rustc *interns*
  every type: `Ty<'tcx>` is a pointer to a unique, arena-allocated `TyKind`, so type equality is pointer equality and
  a type used a million times exists once [RUSTC]. That's Chapter 17.4's interning applied to structured data
  (hash-consing).
- **Union-find.** The listing's `parent: Vec<Option<Ty>>` stores one entry per type variable. `find` follows bindings
  and compresses paths as it goes, so repeated lookups get shorter. With path compression plus union by rank, a
  sequence of operations is nearly linear (the inverse Ackermann bound); the listing uses path compression alone, which
  is enough for bodies of realistic size. rustc's inference tables use a union-find (the `ena` crate) with *snapshots*,
  so a speculative unification (trying a method candidate) can be rolled back [LIB].
- **Side tables for results.** The type of every expression is recorded against its node id (rustc's `TypeckResults`
  per body), not stored in the tree (Chapter 17.4's design).

*Zonking* (listing 17.5-2's `zonk`) is the step that replaces every variable by what it's bound to, producing a type
with no variables left. rustc does the same at the end of a body ("writeback"), and an inference variable that is
still unbound at that point is an error: E0282, "type annotations needed" (Chapter 2.3).

### 6. CPU / OS

**Asymptotics.** Checking and HM inference are nearly linear in the size of the program in practice. In theory, HM
with let-polymorphism is exponential in the worst case: each nested `let` can double the size of a type, and typability
for ML is DEXPTIME-complete (Mairson, 1990). Real programs never hit that case, but it's the reason a type checker's
running time can be surprising on generated code.

**Where Rust's type checking time goes.** Rust's inference is *local*: one body at a time, so its cost is bounded by
the size of the largest function. The expensive parts are elsewhere (predicted from mechanism, not measured here):
trait solving, where each obligation may search impls (Chapter 18.3), and very large types. Chapter 7.3 showed
`type_name` growth for nested generic stacks; an iterator chain or a Tower service stack produces types whose printed
names are thousands of characters long, and every unification over them walks the structure (interning makes
*equality* cheap, not construction). Chapter 18.1 shows how to measure it locally with `-Z self-profile`.

**Parallelism.** Because signatures are boundaries, bodies can be checked in any order and in parallel. rustc's
parallel front end (nightly `-Z threads`, Chapter 18.1) relies on that property; a language that infers signatures
from bodies has to check in dependency order.

---

## Pass 3 · Architect level — *Choosing an inference boundary*

### 7. Trade-offs

| Language style | What is inferred | What must be written | Error messages | Compile-time model |
|---|---|---|---|---|
| Full HM (ML, Haskell, OCaml) | everything, including signatures | nothing (by convention: top-level signatures) | can be far from the mistake | whole module, in dependency order |
| Local inference (Rust, Kotlin, Swift, C#'s `var`) | types inside bodies | every signature | good: each body checked against fixed contracts | per body, parallel, incremental |
| Bidirectional with target typing (Java lambdas, TypeScript contextual typing) | expressions from their context | enough context | good at the use site | per expression |
| None (C, Java before 10 for locals) | nothing | every declaration | simple | trivial |

The rule of thumb Rust embodies: **infer inside, annotate at the boundaries**. Boundaries (function signatures, struct
fields, public APIs) are where humans read types, where other code depends on them, and where a change must be
deliberate. Inside a body, the types are an implementation detail and inference removes noise.

> **Why not infer signatures?** Three reasons, each sufficient on its own. A function's type would depend on its body,
> so an innocent change inside a library function could change what callers compile against, with no signature change
> to review. Type checking would become whole-program and ordered, so the query system couldn't check bodies
> independently (Chapter 18.1). And errors would drift: a mistake in one function would surface as a type error in
> another. E0121's help text shows rustc *could* infer `-> i32`. Declining is the feature.

Two design choices recur in every checker:

- **Error recovery by an error type** versus stopping at the first error. The error type costs one variant and a few
  checks, and it's what makes a checker usable.
- **Implicit conversions.** Every implicit conversion (int to float, bool to int, string to number) adds a way for a
  typed program to mean something unintended. Rust has almost none (deref coercion and unsizing are the main ones,
  Chapter 6.1). Chapter 17.1's JSON rule failure was an implicit string comparison; §10 is another unit mismatch.

### 8. Java comparison

Java's type checking happens in javac's *Attr* phase, fused with resolution (Chapter 17.4) [LIB]. Its inference is
narrower than Rust's in some places and similar in others:

- **`var` (JDK 10) is local and initializer-only.** `var list = new ArrayList<String>();` works; `var x;` doesn't, and
  unlike Rust's `let mut v = Vec::new(); v.push(1u8);`, a later use never fixes an earlier `var`'s type.
- **Lambdas are checked, never inferred.** A Java lambda is a *poly expression*: it has no type of its own and is
  checked against its *target type*, a functional interface. That is exactly bidirectional checking mode. It's why
  `var f = x -> x;` is rejected (the lambda needs an explicit target type), while Rust's `let f = |x| x;` is accepted
  and gets its parameter type from the first call.
- **Generic method inference** (JLS chapter 18) collects constraints from arguments *and* the target type, and solves
  them; the diamond `new HashMap<>()` is the same machinery. It's constraint solving, but for one call at a time.

| | Java | Rust |
|---|---|---|
| Locals | `var` from the initializer only | from any later use in the body |
| Lambdas / closures | target-typed (checked) | inferred from uses; not generalized |
| Method signatures | always written | always written (E0121) |
| Generic calls | inferred per call (JLS 18) | inferred per body, with trait obligations |
| Error type | javac marks erroneous types and suppresses cascades | `ty::Error` + `ErrorGuaranteed` |

> **Analogy limit.** "Both languages infer generic arguments per call" hides a difference in consequence. Java's
> generics are erased (Chapter 7.2), so whether javac inferred `List<Object>` or `List<String>` changes which casts are
> inserted, not which code runs. In Rust, the inferred type selects a monomorphized instance and a trait impl: inferring
> `u16` instead of `u8` for `parse()` changes what code executes and which inputs fail. Inference results are part of a
> Rust program's behavior, which is one more reason Rust keeps them local and visible at the boundaries.

### 9. Production scenario

**Sieve's type system.** Sieve's types are few and blunt: `Int`, `Bool`, `Str`, `Money`, `Duration`, `Percent`, and
lists of those. The checker is bidirectional, like listing 17.5-1, with these rules:

- **No implicit conversions, ever.** Not string to number (the Chapter 17.1 incident), not bool to int, not percent to
  number. `amount > "1000"` is a type error with a span.
- **Literals are checked against their context.** In `refund_rate > 2.5%`, the literal is checked against `Percent`
  because the left operand is a `Percent` feature. A bare `2.5` against a `Percent` is an error with a suggestion
  (`2.5%`).
- **Money literals carry a currency.** `amount > 1000` is an error ("`amount` is Money; write a currency, for example
  `EUR 1000.00`"). §10 is why.
- **An error type in the editor.** Each mistake is reported once, so the rule editor's squiggles map one-to-one to
  mistakes.
- **Local inference only.** Rule-local `let` bindings are inferred from their initializers; there are no user-defined
  functions with inferred signatures.

### 10. Failure scenario

**The threshold that meant ¥100,000.** Before Sieve, a global review rule in the old engine read
`amount_minor > 100000`: "review payments over €1,000.00", with the amount in minor units (cents). In May 2026 Meridian
launched its marketplace in Japan, and the rule started evaluating JPY payments too. The yen has no minor unit in
practice (zero decimal places), so for a JPY payment `amount_minor` is the amount in yen, and the threshold meant
¥100,000, a few hundred euros.

The rule began sending about one JPY payment in nine to manual review, instead of the intended one in two hundred. The
new market's operations team noticed within three days, because Japanese merchants complained about payout delays.
Nothing had failed: every value was an integer, every comparison was well-typed *as integers*. The type system simply
had no word for "cents of which currency".

In Sieve the rule can't be written that way. `amount` has type `Money`, a literal must name its currency, and comparing
a JPY amount with a EUR threshold is a run-time decision that the rule must state explicitly: either
`amount.in(EUR) > EUR 1000.00` (using the FX feature, whose rate source and staleness policy are visible), or a
per-currency threshold table. The type checker doesn't know which currency a payment has; it knows that the question
exists, and refuses to let a rule skip it. That's the general lesson of units in types: **static types can't always
supply the answer, but they can make it impossible to forget the question.** (Part V made the same argument for
`Percent` versus `BasisPoints` with Rust newtypes.)

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVII).*

1. What are synthesis and checking modes in a bidirectional type checker? Why does checking mode produce better error
   locations?
2. What is an error type, and how does it prevent cascading errors? What does rustc's `ErrorGuaranteed` add?
3. Walk through HM inference for `fn pick(c, a, b) { if c { a } else { b } }`: variables, constraints, solution,
   generalization.
4. What is the occurs check? Give a program that needs it and say what goes wrong without it.
5. What is let-polymorphism, and why isn't a Rust closure generalized? What do you write instead, and what does it cost
   at run time?
6. Why does Rust refuse to infer function signatures (E0121) even when it could? Give three reasons.
7. In `let c = 7; let d: u64 = c;` what is `c`'s type, and how did the compiler find out? What if `d`'s line is removed?
8. Java lambdas are "poly expressions". What does that mean in this chapter's vocabulary, and why is `var f = x -> x;`
   rejected?
9. Why are HM error messages often far from the mistake? What do rustc's "expected because..." notes do about it?
10. Your rule language needs money amounts in several currencies. What can a static type system guarantee, what
    can't it, and how does the language make the rest explicit?

### 12. Exercises

- **Beginner.** Add a `let` annotation check to listing 17.5-1: `let x: bool = 5;` should report at `5`. It already
  does; find the line of code responsible and explain the flow.
- **Intermediate.** Remove the `{error}` short-circuit from `infer`'s `Binary` case in listing 17.5-1 (return a
  concrete type instead). Count the errors for `main` now, and identify each new one as a cascade or a real mistake.
- **Advanced.** Add *local* let-polymorphism to listing 17.5-2 (generalize `let`-bound lambdas over variables not free
  in the environment). You'll need to compute the environment's free variables. Test with `let id = ...` used at two
  types; Ore has no lambdas, so add a minimal `|x| e` form or generalize `let`-bound *functions* only.
- **Systems.** Instrument listing 17.5-2's `find` to count path steps, with and without path compression, over a
  generated function with 10,000 chained `let`s (`let a1 = a0; let a2 = a1; ...`). Explain the difference.
- **Architecture.** Sieve wants user-defined helper functions (`fn is_new(m) { m.age_days < 30 }`). Decide what must
  be annotated, what is inferred, and how a change to a helper's body is prevented from silently changing the rules
  that call it.

### 13. Debugging exercise

A teammate "optimizes" listing 17.5-2 by deleting the occurs check from `unify` ("it walks the whole type on every
bind"). The program still compiles, and `add`, `pick`, `id`, `twice`, `fact`, and `use_them` still infer the same types.

1. What happens on `fn self_apply(x) { x(x) }` now? Trace `unify` and then `zonk` on the resulting binding.
2. Which of the listing's functions, `find` or `zonk`, fails first, and how does the process die (Chapter 17.3 §6)?
3. Is there a cheaper way to keep the check? (Hint: when is it actually needed?)

### 14. Design exercise

**Design the type system for Sieve v1.** Features include amounts in 14 currencies, durations, counts, percentages,
country codes, merchant category codes, and sets of those. Rules are written by analysts, not programmers. Decide:

- The types, their literals, and which operations exist between them (can you add a `Duration` to a timestamp? divide
  two `Money`s? multiply `Money` by a `Percent`?).
- Where types are checked (upload time) and where only run time can decide (currency conversion), and how the rule
  states the run-time policy.
- What is inferred and what is annotated (rule-local lets, helper functions, snippets).
- The five error messages analysts will see most often, word for word, including the suggestion each one makes.
