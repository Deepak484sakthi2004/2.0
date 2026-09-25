# Chapter 4.4 — Variance and Subtyping

> **Where this sits:** Part IV · The Borrow Checker · chapter 4 of 6
> **Prerequisites:** Chapter 4.3.
> **After this chapter you can:** explain Rust's only form of subtyping (between lifetimes); state the variance of
> references, `&mut`, containers, cells, and function pointers; prove with a verified example why `&mut T` must be
> invariant; and read error messages that mention "invariant" or "one type is more general than the other."

---

## Pass 1 · User level — *When can one type stand in for another?*

### 1. Problem

You've been relying on subtyping since Part II without noticing. A `&'static str` literal passes happily to a function
expecting `&'a str` for some short `'a`. But other substitutions that look just as harmless are rejected, and the error
messages use words like *invariant* and *more general*. Understanding why is the difference between memorizing
exceptions and seeing one rule.

The rule, stated for Java engineers: Rust has **no subtyping between struct types** (no inheritance). Its only subtyping
is **between lifetimes**, and **variance** says how that subtyping lifts through type constructors like `&T`,
`&mut T`, `Vec<T>`, `Cell<T>`, and `fn(T)`.

### 2. Mental model

**Longer lifetimes are subtypes of shorter ones.** If `'long: 'short` (`'long` covers at least everything `'short`
does), then a `&'long T` can be used wherever a `&'short T` is expected. Something valid for longer is certainly valid
for the shorter stretch. `'static` is the longest lifetime, so `&'static T` fits everywhere.

**Variance** answers: if `A` is a subtype of `B`, what's the relationship between `F<A>` and `F<B>`?

| Variance | Meaning | Example |
|---|---|---|
| **Covariant** | `F<A>` is a subtype of `F<B>` (same direction) | `&'a T` in `'a` and `T`; `Box<T>`; `Vec<T>` |
| **Contravariant** | `F<B>` is a subtype of `F<A>` (reversed) | `fn(T)` in its argument `T` |
| **Invariant** | No relationship: the types must match exactly | `&mut T` in `T`; `Cell<T>`; `UnsafeCell<T>` |

The full table [LANG] (from the Rust Reference):

| Type | Variance in `'a` | Variance in `T` |
|---|---|---|
| `&'a T` | covariant | covariant |
| `&'a mut T` | covariant | **invariant** |
| `*const T` | — | covariant |
| `*mut T` | — | invariant |
| `Box<T>`, `Vec<T>`, `Option<T>`, `Rc<T>`, `Arc<T>` | — | covariant |
| `Cell<T>`, `RefCell<T>`, `UnsafeCell<T>`, `Mutex<T>` | — | **invariant** |
| `fn(T) -> U` | — | **contravariant** in `T`, covariant in `U` |
| `PhantomData<T>` | — | covariant |

The intuition behind the pattern: **read-only positions can be covariant, write positions must not be.** If you can only
*read* a `T` out of something, a `T` that lives longer is harmless. If you can *write* a `T` into it, someone could write a
shorter-lived value into a slot the rest of the program believes holds a longer-lived one, and that's a dangling
reference waiting to happen. `&mut T`, `Cell<T>`, and `*mut T` all allow writes, so they're invariant.

### 3. Rust code

**Covariance at work** (verified):

```rust
fn pick<'a>(primary: &'a str, fallback: &'a str, use_primary: bool) -> &'a str {
    if use_primary { primary } else { fallback }
}

fn main() {
    let static_default: &'static str = "eu-west-1"; // lives for the whole program
    let from_request = String::from("ap-south-1"); // lives until the end of main
    // &'static str is a SUBTYPE of &'a str: it may be used wherever a shorter-lived one is expected.
    let region = pick(&from_request, static_default, false);
    println!("region = {region}");

    // Covariance lifts that through containers: Vec<&'static str> can become Vec<&'a str>.
    let defaults: Vec<&'static str> = vec!["us-east-1", "eu-west-1"];
    let mut candidates: Vec<&str> = defaults; // moved in, with its lifetime shortened
    candidates.push(&from_request);
    println!("candidates = {candidates:?}");
}
```

```text
region = eu-west-1
candidates = ["us-east-1", "eu-west-1", "ap-south-1"]
```

The `Vec` *moved*. The new owner has a shorter lifetime and may push shorter-lived strings, and nobody else holds the
old `Vec<&'static str>` to be surprised. That's why covariance is safe for owned containers.

**Invariance, and the bug it prevents** (verified):

```rust,compile_fail
/// Overwrites the reference stored in `slot`.
fn overwrite<'a>(slot: &mut &'a str, value: &'a str) {
    *slot = value;
}

fn main() {
    let mut label: &'static str = "default";
    {
        let local = String::from("short-lived");
        overwrite(&mut label, &local); // &mut T is INVARIANT in T: 'a must equal 'static
    }
    println!("{label}"); // if this were allowed, `label` would point into freed memory
}
```

```text
error[E0597]: `local` does not live long enough
  --> src/main.rs:11:31
   |
 8 |     let mut label: &'static str = "default";
   |                    ------------ type annotation requires that `local` is borrowed for `'static`
 9 |     {
10 |         let local = String::from("short-lived");
   |             ----- binding `local` declared here
11 |         overwrite(&mut label, &local); // &mut T is INVARIANT in T: 'a must equal 'static
   |                               ^^^^^^ borrowed value does not live long enough
12 |     }
   |     - `local` dropped here while still borrowed
```

Follow the checker's reasoning. `&mut label` has type `&mut &'static str`. Because `&mut T` is invariant in `T`, it
can't be treated as `&mut &'a str` for a shorter `'a`, so `'a` must be exactly `'static`. Then `value: &'a str` must
be `&'static str`, and `&local` isn't. Hence *"type annotation requires that `local` is borrowed for `'static`"*. If
`&mut` were covariant, `overwrite` would store a pointer to `local` in `label`, `local` would be freed at the end of the
block, and the `println!` would read freed memory. **Invariance of `&mut T` is a memory-safety rule, not a pedantic
one.**

**Function pointers are contravariant in their arguments** (verified both ways):

```rust
fn print_any(s: &str) {
    // works for ANY lifetime: its type is for<'a> fn(&'a str)
    println!("any: {s}");
}

fn print_static(s: &'static str) {
    // demands a 'static argument
    println!("static: {s}");
}

fn main() {
    // A function that accepts MORE (any lifetime) can stand in for one that accepts LESS ('static only):
    // function parameters are CONTRAVARIANT.
    let f: fn(&'static str) = print_any;
    f("literal");

    let g: fn(&'static str) = print_static;
    g("literal");

    let owned = String::from("from a request");
    let h: fn(&str) = print_any;
    h(&owned);
}
```

```text
any: literal
static: literal
any: from a request
```

The reverse, using a `'static`-only function where any lifetime must be accepted, is rejected:

```text
error[E0308]: mismatched types
 --> src/main.rs:9:23
  |
9 |     let h: fn(&str) = print_static;
  |            --------   ^^^^^^^^^^^^ one type is more general than the other
  |            |
  |            expected due to this
  |
  = note: expected fn pointer `for<'a> fn(&'a _)`
               found fn item `fn(&'static _) {print_static}`
```

The note shows the desugaring Chapter 4.5 builds on. `fn(&str)` is really `for<'a> fn(&'a _)`: "a function that works
for *every* lifetime." `print_static` works for only one, so it's *less general*, and it can't stand in.

---

## Pass 2 · Systems level — *Where variance comes from*

### 4. Under the hood

**Variance is inferred from fields.** [RUSTC] For your own structs, rustc computes each parameter's variance from how it's
used in the fields. A struct holding `&'a T` is covariant in `'a` and `T`. One holding `Cell<&'a T>` is invariant in `'a`.
Combine a covariant use and an invariant use of the same parameter and the result is invariant. (Uses that are
covariant and contravariant at once also make it invariant.) You never write variance down. It's a consequence of your
field types, which is why adding a `Cell` field can suddenly break distant code that relied on shortening a lifetime.

**You can steer it with `PhantomData`.** A type that owns no `T` but should behave as if it did (a typed ID, a handle,
a raw-pointer wrapper) uses `PhantomData<T>` (covariant), `PhantomData<fn(T)>` (contravariant), or
`PhantomData<fn(T) -> T>` or `PhantomData<Cell<T>>` (invariant). Part V uses this for type-state and branded handles, and
Part XV for `unsafe` pointer wrappers, where getting variance wrong is a soundness bug.

**In the borrow checker, subtyping is just constraints.** When a `&'long T` is used where `&'short T` is expected, the
solver adds `'long: 'short` (Chapter 4.2's outlives constraints). Invariance adds constraints in *both* directions,
which forces equality. That's why the error above talks about `'static` even though you never wrote it on `overwrite`:
equality propagated it.

**Where soundness got hard.** Chapter 1.3 mentioned rust-lang/rust#25860, a soundness hole open since 2015. It lives
exactly here: the interaction between **implied bounds** (the compiler assumes `&'a &'b T` implies `'b: 'a`, Chapter 4.3)
and **variance of function types** lets a carefully constructed program convert a short lifetime into `'static`. It needs
deliberately contrived code, and public proofs of concept exist. Fixing it properly has required deep changes to the trait
system. The existence of that bug is itself a lesson: variance and lifetime subtyping are subtle enough that the Rust
team is still closing gaps a decade later. In your code, you only meet them as errors that protect you.

### 5. Memory

What invariance prevents, drawn out:

```text
 before the call                 if &mut were covariant             after the block ends
 ┌────────────────────┐          ┌────────────────────┐            ┌────────────────────┐
 │ label: &'static str│─► "default" (static data)                  │ label ──────────────┼──► ??? (freed)
 └────────────────────┘          │ label ─────────────┼──► "short-lived"  (local's heap buffer)
                                 └────────────────────┘                 local dropped: buffer FREED
                                                                        println!("{label}")  → use-after-free
```

That's exactly Chapter 1.1's temporal bug, arising from a single `*slot = value;`, which is why the type system has to
rule it out.

### 6. CPU / OS

None: variance is a compile-time property with no representation at run time. Its practical effect is on *API
flexibility*. A covariant type lets callers pass longer-lived data freely. An invariant one forces exact matches, and that
shows up as borrow errors in code that looks innocent.

---

## Pass 3 · Architect level — *Designing types with the right variance*

### 7. Trade-offs

| Field choice | Variance in `'a` | Consequence for users |
|---|---|---|
| `data: &'a T` | covariant | Users can mix longer-lived data freely; the most flexible |
| `cache: Cell<Option<&'a T>>` | **invariant** | `'a` must match exactly everywhere; more "does not live long enough" errors downstream |
| `items: &'a mut Vec<&'a str>` | **invariant** (and ties two lifetimes together) | A classic self-inflicted trap: the same `'a` forced on the container and its contents |
| Owned data (`String`, indices) | no lifetime at all | No variance questions |

Guidelines:

- **Prefer covariant designs.** Keep interior mutability (`Cell`, `RefCell`, `Mutex`) away from fields that hold
  references with lifetimes. Store owned values or indices instead.
- **Don't reuse one lifetime name for unrelated things.** `&'a mut Vec<&'a str>` says "the vector lives exactly as long
  as the strings it holds," which is almost never what you meant. Use `&'v mut Vec<&'s str>`.
- **Choose `PhantomData`'s variance deliberately** for handle and ID types (Part V).

### 8. Java comparison

Java has the same problem Rust solves with invariance, and it chose differently in two places:

**Arrays are covariant in Java**, which is unsound, so Java checks every array store **at run time**:

```java
Object[] objects = new String[1];     // allowed: String[] is a subtype of Object[] (covariant arrays)
objects[0] = Integer.valueOf(42);     // compiles... and throws ArrayStoreException at run time
```

That's precisely the `overwrite` bug at the type level: writing a "narrower" thing into a slot typed more widely. Java
pays with a type check on every reference-array store, plus a run-time exception. Rust makes the mutable case invariant
and rejects it at compile time.

**Generics are invariant in Java**, with **use-site** variance via wildcards: `List<? extends Number>` for reading
(covariant), `List<? super Integer>` for writing (contravariant), remembered as PECS ("producer extends, consumer
super"). Rust uses **declaration-site** variance, inferred from the type's fields, and only for lifetimes. There's no
wildcard syntax, because there's no subtyping between struct types to be variant over.

> **Analogy limit.** "Rust variance is Java generics' PECS" holds for the principle: read positions can be covariant,
> write positions contravariant or invariant. It fails on *what* varies. In Java it's class types in an inheritance
> hierarchy. In Rust it's **lifetimes only**. A `Vec<Dog>` is never a `Vec<Animal>` in Rust, because there's no such
> subtyping at all (Part VI uses traits and generics for that).

### 9. Production scenario

**Meridian's request-scoped string table.** An engineer built a per-request table to deduplicate strings borrowed from
the request buffer, `struct Interner<'a> { seen: RefCell<Vec<&'a str>> }`, shared by helper functions through `&`. It
worked until a helper tried to intern a `&'static str` constant alongside request strings, and a separate helper passed
a slightly shorter-lived slice. The `RefCell` made `Interner` **invariant** in `'a`, so every string had to have
*exactly* the request's lifetime. Neither the longer-lived constant nor the shorter-lived slice could be shortened or
lengthened to match, and the team spent a day on "does not live long enough" errors pointing at code that looked
correct.

The redesign stored **indices into the request buffer** (`(start, len)` pairs) instead of `&'a str`, keeping the table
lifetime-free and trivially `Copy`, with a method to turn an index back into `&str` given the buffer. The variance
problem disappeared because the lifetime disappeared: Part III's "store positions, not addresses" once more.

### 10. Failure scenario

**`ArrayStoreException` in a Java pipeline.** A Java event pipeline stored handlers in an array typed through a
supertype (`Handler[] handlers = new AuditHandler[8];`) and later wrote a `MetricsHandler` into it. It compiled, and it
failed at run time on the first metrics event of the day with `ArrayStoreException`, in a code path the tests didn't
cover. The design problem, a narrower-typed container written through a wider-typed view, is the same one §3's
`overwrite` shows. Rust's answer is to refuse the write-through-a-wider-view at compile time (invariance), which costs
some flexibility and removes a class of run-time failure. Java's answer is to allow it and check every store. Both are
coherent. Only one finds the bug before deployment.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part IV).*

1. What is the only form of subtyping in Rust? Why doesn't `Vec<Dog>` convert to `Vec<Animal>`?
2. Define covariance, contravariance, and invariance with a Rust example of each.
3. Why must `&mut T` be invariant in `T`? Walk through the `overwrite` example and what would go wrong.
4. Why is `Cell<T>` invariant? Why is `Vec<T>` covariant, even though you can push to a `Vec`?
5. Why are function pointers contravariant in their argument types? Explain "one type is more general than the other."
6. How does rustc determine the variance of your own struct? How can you change it?
7. Compare Java's covariant arrays with Rust's invariant `&mut T`. What does each language pay, and when?
8. What is rust-lang/rust#25860 about, in one sentence, and what does its existence teach an architect?

### 12. Exercises

- **Beginner.** For each type, state its variance in `'a`: `&'a u8`, `&'a mut &'a u8`, `Option<&'a str>`,
  `Cell<&'a str>`, `fn(&'a str)`, `Box<dyn Fn(&'a str)>`.
- **Intermediate.** Write `struct Holder<'a> { value: &'a str }` and show that a `Holder<'static>` can be passed to
  `fn use_holder<'a>(h: Holder<'a>)`. Then add a `Cell<&'a str>` field and show what breaks.
- **Advanced.** Write `fn push_label<'v, 's>(labels: &'v mut Vec<&'s str>, label: &'s str)` and contrast it with a
  version using a single lifetime `'a` for both. Construct a caller that compiles with the two-lifetime version and
  fails with the single-lifetime one.
- **Systems.** Explain why variance has *no* effect on generated code, and confirm it by comparing the release assembly
  of a function taking `&'static str` with one taking `&'a str`.
- **Architecture.** Find a type in a codebase you know that holds references inside interior mutability. What variance
  does it have, and has it caused lifetime errors elsewhere? Redesign it with owned data or indices.

### 13. Debugging exercise

The `overwrite` listing fails with E0597, and the message says *"type annotation requires that `local` is borrowed for
`'static`"* on a line that doesn't mention `local` at all.

1. Trace the constraint chain from `let mut label: &'static str` to the requirement on `local`. Which step is the
   invariance?
2. If `overwrite` took `slot: &mut &'b str, value: &'a str` with `'a: 'b`, would that fix it? Why, or why not?
3. Rewrite `overwrite`'s *caller* so it's correct: a `label` that's `String` (owned), or an `Option<&str>` local to the
   block. What does each change in the design?

### 14. Design exercise

**A request context with borrowed and static data.** Meridian's request context holds borrowed slices of the request
(method, path, headers), static configuration (`&'static Config`), and a scratch area that helpers append to (formatted
log fragments). Design `RequestContext<'r>`: which fields borrow from `'r`, which are `'static`, which are owned. Where
does the scratch area go, so that the context stays covariant in `'r`? Then explain what would happen to downstream
helper signatures if the scratch area were `RefCell<Vec<&'r str>>`, and why owned fragments (`String`) or indices are
better.
