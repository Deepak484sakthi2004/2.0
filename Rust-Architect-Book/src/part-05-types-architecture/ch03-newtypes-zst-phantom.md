# Chapter 5.3 — Newtypes, Zero-Sized Types, PhantomData, and Markers

> **Where this sits:** Part V · Types as Architecture · chapter 3 of 4
> **Prerequisites:** Chapter 2.2 (the checkout discount incident), Chapter 4.4 (variance), Chapters 5.1–5.2.
> **After this chapter you can:** replace primitive types with domain types that cost nothing at run time (and prove it
> in assembly); build typed identifiers with `PhantomData` and choose the marker that gives the right variance, `Send`,
> and `Sync`; use zero-sized types as compile-time facts; encode policies as marker traits with custom error messages;
> and avoid the `derive` trap that makes a phantom-typed ID silently non-`Copy`.

---

## Pass 1 · User level — *Types that exist only for the compiler*

### 1. Problem

Three earlier chapters ended with the same unpaid debt:

- **Chapter 2.2**, the checkout discount incident: a value in **percent** was passed where **basis points** were
  expected. Both were plain integers, so the compiler saw nothing wrong.
- **The Part II review**: the quota service passed `u64` tenant IDs next to `u64` resource counts, and one call site had
  them in the wrong order.
- **The Part III review**: the order book's model answer used `OrderId(u64)`, `Price(i64)`, and `enum Side` without
  explaining why wrappers around integers were worth the typing.

The common problem is **primitive obsession**: using `i64`, `u64`, and `String` for things with very different rules.
Cents aren't basis points. A tenant ID isn't an order ID. A validated e-mail isn't any string. This chapter gives the
four tools that fix it, and shows that none of them costs a byte or an instruction at run time.

### 2. Mental model

Everything in this chapter exists at compile time and is **erased** before code generation:

| Tool | Bytes at run time | What the compiler learns | Example |
|---|---|---|---|
| **Newtype** `struct BasisPoints(u16)` | exactly the inner type's | "this is a different type with its own rules" | units, money, IDs, validated data |
| **Zero-sized type** (ZST) `struct Audited;` | 0 | "a value of this type exists" (a fact, a token) | unit structs, `()`, `PhantomData`, type-state markers |
| **`PhantomData<T>`** | 0 | "this type acts *as if* it holds a `T`": variance, auto traits, drop | typed IDs `Id<Order>`, handles, lifetimes on raw pointers |
| **Marker trait** `trait Idempotent {}` | 0 (no methods) | "this type has a property someone vouched for" | `Copy`, `Send`, `Sync`, `Eq`, policies |

A good one-line summary: **a newtype is a new name with a new set of rules and the same bytes.** The rules are *which
operations exist*: which functions accept it, which traits it implements, whether it can be constructed from a raw
value. You choose all of them, and you get nothing you didn't ask for. `BasisPoints` doesn't get `+`, `*`, or
`From<u16>` unless you write them.

### 3. Rust code

**The discount incident, fixed** (listing `ch03-01-percent-bps.rs`, verified):

```rust
mod pricing {
    /// A discount in whole percent, 0..=100. Private field: only `Percent::new` creates one.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Percent(u8);

    /// A discount in basis points (1/100 of a percent), 0..=10_000.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct BasisPoints(u16);

    /// Money in minor units (cents). Signed: refunds are negative.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Cents(pub i64);

    impl Percent {
        pub fn new(p: u8) -> Option<Percent> {
            (p <= 100).then_some(Percent(p))
        }
    }

    impl BasisPoints {
        pub fn new(bps: u16) -> Option<BasisPoints> {
            (bps <= 10_000).then_some(BasisPoints(bps))
        }
    }

    /// The one sanctioned conversion: lossless, so it is `From`, not a cast at the call site.
    impl From<Percent> for BasisPoints {
        fn from(p: Percent) -> BasisPoints {
            BasisPoints(p.0 as u16 * 100)
        }
    }

    /// The only discount function. It takes BasisPoints; a Percent must be converted explicitly.
    pub fn apply_discount(price: Cents, d: BasisPoints) -> Cents {
        // d <= 10_000 by construction, so the result is in 0..=price: no underflow possible.
        let off = price.0 as i128 * d.0 as i128 / 10_000;
        Cents(price.0 - off as i64)
    }
}

use pricing::{apply_discount, BasisPoints, Cents, Percent};

fn main() {
    let price = Cents(4_999);
    let promo = Percent::new(15).unwrap(); // marketing speaks percent
    let partner = BasisPoints::new(250).unwrap(); // the partner API speaks basis points

    println!("15%     -> {:?}", apply_discount(price, promo.into()));
    println!("250 bps -> {:?}", apply_discount(price, partner));
    println!("Percent::new(150) = {:?}", Percent::new(150));
    println!("BasisPoints::new(15_000) = {:?}", BasisPoints::new(15_000));
    println!("size_of::<BasisPoints>() = {}", std::mem::size_of::<BasisPoints>());
}
```

```text
15%     -> Cents(4250)
250 bps -> Cents(4875)
Percent::new(150) = None
BasisPoints::new(15_000) = None
size_of::<BasisPoints>() = 2
```

Three design decisions are hiding here. The **range** is part of the type: a `BasisPoints` above 10,000 can't exist, so
`apply_discount` can't underflow, which is the other half of the 2.2 incident. The **conversion** between units exists
exactly once, as `From`, because it's lossless. And **`Cents` has a public field**, deliberately: every `i64` is a
valid amount of cents, so there's no invariant to protect. The type exists only to keep cents from being confused with
other numbers. A newtype needs a private field only when it carries a proof.

The original bug, replayed (listing `ch03-02-percent-mismatch.rs`, verified):

```rust,ignore
    let promo = Percent(15);
    // The 2.2 incident, replayed: a percent passed where basis points are expected.
    println!("{:?}", apply_discount(Cents(4_999), promo));
```

```text
error[E0308]: mismatched types
16 |     println!("{:?}", apply_discount(Cents(4_999), promo));
   |                      --------------               ^^^^^ expected `BasisPoints`, found `Percent`
   |                      |
   |                      arguments to this function are incorrect
```

**Typed identifiers with `PhantomData`** (listing `ch03-03-typed-ids.rs`, verified). One generic `Id<T>` serves every
entity:

```rust,ignore
/// A typed identifier: the same u64 at run time, a different type per entity at compile time.
/// `PhantomData<fn() -> T>`: covariant in T, always Send + Sync, and does not claim to own a T.
pub struct Id<T> {
    raw: u64,
    _entity: PhantomData<fn() -> T>,
}

// Manual impls: `#[derive]` would add `T: Clone`, `T: PartialEq`, ... bounds (Chapter 5.3 §13).
impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Id<T> {}
impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
// ... Eq, Hash, Debug likewise

pub type TenantId = Id<Tenant>;
pub type OrderId = Id<Order>;
```

```text
Tenant#7 of acme -> Order#8: 4999 cents
Tenant#7 of acme -> Order#9: 150 cents
size_of: u64=8 Id<Order>=8 Option<Id<Order>>=16
```

In that run, order #7 exists too, but it belongs to tenant #9. With bare `u64`s, "7" is a valid tenant *and* a valid
order, and nothing stops you mixing them up. With typed IDs (listing `ch03-04-id-mixup.rs`, verified):

```rust,compile_fail
use std::marker::PhantomData;

#[derive(Debug)]
pub struct Id<T> {
    raw: u64,
    _entity: PhantomData<fn() -> T>,
}
impl<T> Id<T> {
    pub fn new(raw: u64) -> Self {
        Id { raw, _entity: PhantomData }
    }
}

pub struct Tenant;
pub struct Order;

fn cancel_order(id: Id<Order>) -> u64 {
    id.raw
}

fn main() {
    let tenant = Id::<Tenant>::new(7);
    // Both are u64 underneath. With raw u64 parameters this call compiles and cancels order #7.
    println!("{}", cancel_order(tenant));
}
```

```text
error[E0308]: mismatched types
25 |     println!("{}", cancel_order(tenant));
   |                    ------------ ^^^^^^ expected `Id<Order>`, found `Id<Tenant>`
   = note: expected struct `Id<Order>`
              found struct `Id<Tenant>`
```

(`Option<Id<Order>>` is 16 bytes because `u64` has no niche. If IDs start at 1, make `raw` a `NonZeroU64` and it drops
to 8. That's what Chapter 5.2's order-book review did.)

**Marker traits as policy** (listings `ch03-10-marker-retry.rs` and `ch03-11-marker-retry-fail.rs`, verified). A trait
with no methods is a *claim*. Here the claim is "sending this twice has the same effect as sending it once," and the
retry helper accepts only types that make it:

```rust,ignore
/// Marker trait: "sending this request twice has the same effect as sending it once".
/// No methods. Implementing it is a claim that reviewers must check.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not marked Idempotent, so it must not be retried automatically",
    note = "retrying a non-idempotent request can double-apply it; add an idempotency key and implement `Idempotent`"
)]
pub trait Idempotent {}

/// Only idempotent requests may be retried: the bound is the policy.
pub fn with_retries<R: Request + Idempotent>(req: &R, max: u32) -> Result<String, String> { /* ... */ }
```

```text
Ok("balance of 7 = 1200")
Ok("charged 4999 once (key ord-42)") (applied 1 time)
```

And for a `Charge` without an idempotency key:

```text
error[E0277]: `Charge` is not marked Idempotent, so it must not be retried automatically
33 |     println!("{:?}", with_retries(&Charge { cents: 4_999 }, 5));
   |                      ------------ ^^^^^^^^^^^^^^^^^^^^^^^^ unsatisfied trait bound
   = note: retrying a non-idempotent request can double-apply it; add an idempotency key and implement `Idempotent`
note: required by a bound in `with_retries`
```

[VERSION] `#[diagnostic::on_unimplemented]` has been stable since Rust 1.78. It lets a library replace "the trait bound
is not satisfied" with the *reason* for the policy. The error message becomes documentation delivered at exactly the
moment someone needs it.

---

## Pass 2 · Systems level — *What the compiler tracks, and what it emits*

### 4. Under the hood

**Newtypes are nominal.** [LANG] Rust's structs are compared by *name*, not by shape. `struct Percent(u8)` and
`struct Level(u8)` have identical structure and are still unrelated types. Type checking (Chapter 17.5) never asks
"do these look alike?", only "are these the same definition?" That's the entire mechanism, and it's why a newtype is
free: the difference exists only in the type checker's tables.

**`PhantomData` tells the compiler three things a type parameter normally implies.** rustc rejects a struct with a
type parameter that no field uses (error E0392, "parameter `T` is never used"), because it couldn't answer three
questions about the struct:

1. **Variance** (Chapter 4.4): may `Id<&'long T>` be used where `Id<&'short T>` is expected?
2. **Auto traits**: is `Id<T>` `Send` and `Sync`?
3. **Drop check**: does dropping an `Id<T>` possibly drop a `T`?

A field of type `PhantomData<X>` answers all three "as if the struct held an `X`," and different `X`s give different
answers. This is the steering table Chapter 4.4 promised (from the Rustonomicon, [LANG]):

| Marker field | Variance in `T` | `Send`/`Sync` | Drop check: "owns a `T`" | Typical use |
|---|---|---|---|---|
| `PhantomData<T>` | covariant | as `T` | yes | owning containers over raw pointers (`Vec<T>`'s internals) |
| `PhantomData<&'a T>` | covariant in `'a` and `T` | as `&T` | no | borrowed views over raw pointers (`slice::Iter<'a, T>`) |
| `PhantomData<*const T>` | covariant | **neither** | no | opting *out* of `Send`/`Sync` on stable |
| `PhantomData<fn() -> T>` | covariant | **always both** | no | **typed IDs and handles**: "about a `T`", not "holds a `T`" |
| `PhantomData<fn(T)>` | contravariant | always both | no | rare: consumers of `T` |
| `PhantomData<fn(T) -> T>` or `PhantomData<Cell<T>>` | **invariant** | both / `Send` only | no | brands, and lifetimes that must match exactly |

Two of these rows verified. **Invariance** (listing `ch03-08-variance.rs`): a covariant `Token<'a>` can be shortened, and
an invariant `Brand<'a>` can't:

```rust,compile_fail
use std::marker::PhantomData;

/// Covariant in 'a: a Token<'long> may be used where a Token<'short> is expected.
pub struct Token<'a>(PhantomData<&'a ()>);

/// Invariant in 'a: the lifetime is a *brand* that must match exactly.
pub struct Brand<'a>(PhantomData<fn(&'a ()) -> &'a ()>);

pub fn shorten_token<'short, 'long: 'short>(t: Token<'long>) -> Token<'short> {
    t // fine: covariance
}

pub fn shorten_brand<'short, 'long: 'short>(b: Brand<'long>) -> Brand<'short> {
    b // rejected: invariance
}

fn main() {}
```

```text
error: lifetime may not live long enough
15 |     b // rejected: invariance
   |     ^ function was supposed to return data with lifetime `'long` but it is returning data with lifetime `'short`
   = note: requirement occurs because of the type `Brand<'_>`, which makes the generic argument `'_` invariant
   = note: the struct `Brand<'a>` is invariant over the parameter `'a`
```

Invariant brands are how libraries tie an index to *one particular* container, so an index from arena A can't be used
on arena B even though both have the same type. Chapter 3.6's generational `Key` checks that at run time. A brand checks
it at compile time, at the cost of closure-scoped APIs.

**Auto traits** (listing `ch03-09-phantom-send.rs`): write the ID with `PhantomData<T>` instead, and an ID of a
single-threaded entity can't cross threads, even though all it holds is a `u64`:

```text
error[E0277]: `Rc<str>` cannot be sent between threads safely
22 |     let h = std::thread::spawn(move || audit(id));
   |                                ------- `Rc<str>` cannot be sent between threads safely
note: required because it appears within the type `Session`
note: required because it appears within the type `PhantomData<Session>`
note: required because it appears within the type `Id<Session>`
```

That's why typed IDs should use `PhantomData<fn() -> T>`. An ID *refers to* a `Session`, it doesn't *contain* one, and
its thread-safety shouldn't depend on the entity's. A subtlety found while writing this listing: an earlier version
used `move || id.raw`, and it **compiled** (listing `ch03-12-disjoint-capture.rs`, prints `42`). [VERSION] Since the
2021 edition, closures capture *disjoint fields*. That closure captured only `id.raw`, a `u64`, so the `!Send` ID never
crossed the thread boundary at all.

**Marker traits in std** are the same idea with compiler support:

- **`Send`, `Sync`, `Unpin`** are *auto traits*. The compiler implements them structurally: a type is `Send` if all its
  fields are. That's why `PhantomData` can steer them. Implementing them by hand is `unsafe` (Part XI).
- **`Copy`** has no methods and changes the *language*: values are copied instead of moved. It requires every field to
  be `Copy`, and it can't coexist with `Drop` (Chapter 3.2).
- **`Eq`** has no methods beyond `PartialEq`'s. It's a claim that equality is reflexive (so `f64` can't make it: NaN ≠
  NaN), and `HashMap` keys rely on it.
- **`Sized`** is an implicit bound on every type parameter; `?Sized` opts out (Part VI).

Your own marker traits work the same way, minus the compiler magic: a bound checks the claim, and code review checks the
`impl`.

### 5. Memory

**Zero-sized types are real types with no bytes** (listing `ch03-07-zst.rs`, verified, with the counting allocator from
Part III):

```rust,ignore
struct Audited; // a unit struct: one value, zero bytes

    let (v, n) = allocs_during(|| {
        let mut v: Vec<Audited> = Vec::new();
        for _ in 0..1_000_000 {
            v.push(Audited);
        }
        v
    });
    println!("Vec<Audited>: len={} capacity={} allocations={}", v.len(), v.capacity(), n);
```

```text
size_of: ()=0 Audited=0 PhantomData<String>=0 [Audited; 1000]=0
Vec<Audited>: len=1000000 capacity=18446744073709551615 allocations=0
Vec<u8>:      len=1000000 capacity=1048576 allocations=18
size_of::<(u64, ())>()=8  HashSet<u64>=48 HashMap<u64,()>=48
```

A million pushes of a ZST never call the allocator. [LIB] `Vec` reports a capacity of `usize::MAX` for zero-sized
elements and counts pushes in `len`. The `Vec<u8>` comparison shows the normal growth pattern: 18 calls (one allocation
at 8 bytes, then 17 doublings to 1,048,576). A pointer to a ZST is non-null and aligned but points at nothing, and
reading zero bytes through it is always fine.

The most useful ZST in std is the one you use without noticing: [LIB] `HashSet<K>` is a wrapper around
`HashMap<K, ()>` (via hashbrown). Every "value" slot is zero bytes, so the entry is just the key (`(u64, ())` is 8
bytes). A set *is* a map with a trivial value, and the layout makes that free.

**The same trick in your own types.** Where Java uses a `Map<K, Boolean>` or `Map<K, Object>` placeholder, Rust uses
`()`. And a generic container parameterized by a ZST policy (`Cache<K, V, NoEviction>`) carries the policy at zero
cost. Part VII builds on that.

### 6. CPU / OS

**The assembly proof.** Newtypes and phantom types disappear completely (listing `ch03-06-zero-cost-asm.rs`,
`emit.ps1 -Target asm -Mode release`, rustc 1.98.1, trimmed):

```rust,ignore
#[inline(never)]
pub fn discount_raw(price: i64, bps: u16) -> i64 {
    price - price * bps as i64 / 10_000
}

#[inline(never)]
pub fn discount_typed(price: Cents, bps: BasisPoints) -> Cents {
    Cents(price.0 - price.0 * bps.0 as i64 / 10_000)
}

#[inline(never)]
pub fn shard_raw(id: u64, shards: u64) -> u64 { id % shards }

#[inline(never)]
pub fn shard_typed(id: Id<Order>, shards: u64) -> u64 { id.raw % shards }
```

```text
playground::discount_raw:
	movzx	eax, si
	imul	rax, rdi
	movabs	rcx, -3777893186295716171     ; division by 10_000 as a multiply by a magic constant
	imul	rcx
	mov	rax, rdx
	shr	rax, 63
	sar	rdx, 11
	add	rax, rdx
	add	rax, rdi
	ret

playground::discount_typed = playground::discount_raw
```

That last line isn't an abbreviation. It's what the compiler emitted. The typed function's machine code was
*identical* to the raw one, so LLVM merged them and made `discount_typed` an alias of the same address. `shard_typed` and
`shard_raw` compile to the same instructions too (`test rsi, rsi` for the divide-by-zero check, then `div`). They
weren't merged only because their panic messages point at different source lines.

**ABI and `repr(transparent)`.** [RUSTC] In practice, a single-field struct like `Cents(i64)` is passed in a register,
exactly like an `i64`, as the assembly shows. [LANG] Only `#[repr(transparent)]` *guarantees* it. That matters for
`extern "C"` signatures and for `unsafe` casts between `&[u16]` and `&[BasisPoints]` (Chapter 16.1). The rule: add
`repr(transparent)` whenever code outside the type's module might depend on the newtype being "just" the inner type.
There's no cost to adding it.

---

## Pass 3 · Architect level — *How far to take domain types*

### 7. Trade-offs

**Newtypes have friction**, and it's the price of the rules you get:

| Friction | Resolution |
|---|---|
| No arithmetic, `Display`, or serde by default | Implement exactly the operations the domain allows (`Cents + Cents`, never `Cents * Cents`); derive the rest; `#[serde(transparent)]` for the wire |
| Conversions at every boundary | `From` only for lossless conversions, `TryFrom` for fallible ones; one module owns them |
| Tempting to add `Deref<Target = u64>` | Don't. `Deref` is for smart pointers. It makes `id.pow(2)` and `id + 1` compile again, which undoes the newtype |
| A public `new(raw: u64)` lets anyone forge an ID | Keep raw constructors `pub(crate)` or on the data-access layer; IDs should come from the database or a parser |
| Generic `Id<T>` vs one struct per entity | Generic: one impl, uniform. Per entity: each can differ (UUID vs `u64`, custom `Display`) |

**Where newtypes pay for themselves**, in rough order of value: money and units (the costliest bugs), identifiers
(cross-entity mix-ups, cross-tenant leaks), validated strings (e-mail, IBAN, currency code), and anything parsed at a
trust boundary. **Where they don't:** local intermediate values inside one function, and the internals of a numeric
algorithm, where the friction exceeds the risk.

**Choosing a `PhantomData` marker**: for typed IDs and handles, use `PhantomData<fn() -> T>`. Use `PhantomData<T>` only
when the type really owns `T`s through a raw pointer. Use an invariant marker only when you mean "exactly this one."
Getting it wrong has consequences users feel: either spurious `Send` errors (`PhantomData<T>`) or lifetime errors that
look like the borrow checker is broken (unneeded invariance).

### 8. Java comparison

| Rust | Java today | Java with Valhalla |
|---|---|---|
| `struct Cents(i64)` — 8 bytes, in registers | `record Cents(long value)`: an object with a header; often heap-allocated unless escape analysis removes it | value class: no identity, and the JVM *may* flatten it |
| `Id<Order>` with `PhantomData` | `final class Id<T> { final long raw; }`: phantom generics work in Java too, erased at run time | same, flattenable |
| `Vec<Cents>` = contiguous `i64`s | `List<Cents>` = pointers to objects | flat arrays of value objects (the goal) |
| marker trait + bound (`R: Idempotent`) | marker interface (`Serializable`, `RandomAccess`), checked with `instanceof` at run time, or by bounds on generics | — |
| `Send`/`Sync` computed from fields | no equivalent; thread-safety is documentation | — |

**Project Valhalla, hedged.** Value classes (JEP 401, "Value Classes and Objects") would give Java identity-free
objects the JVM can flatten into fields and arrays. As of this writing (2026), they have been available in preview or
early-access builds, and aren't a standard feature of a released LTS JDK. Check the JEP's current status before relying
on it. Even when they ship, flattening is the JVM's *optimization*, and nullability of value types is handled
separately. Rust's `Cents(i64)` is *guaranteed* to be an `i64` at run time (with `repr(transparent)`), and can't be null.

Phantom types in Java deserve credit: `Id<Order>` vs `Id<Tenant>` is caught by javac, just as in Rust.

> **Analogy limit.** Java erases generics, so at run time `Id<Order>` and `Id<Tenant>` are the same class. An
> `equals` between them returns `true` for equal raw values, a raw type or an unchecked cast defeats the check, and
> reflection-based frameworks see only `Id`. Rust's phantom types are erased too, but only *after* type checking, and
> there's no raw-type escape hatch. `Id<Order> == Id<Tenant>` doesn't compile, because `PartialEq` is implemented only
> between identical types.

### 9. Production scenario

**Meridian's domain-types rollout.** After the Part II and Part III reviews, the order and pricing services adopted a
small shared crate, `meridian-types`:

- **Money and units**: `Cents(i64)`, `Percent`, and `BasisPoints` with checked constructors, and the discount function
  from listing `ch03-01-percent-bps.rs` as the *only* discount code path. The checkout incident class from Chapter 2.2 is
  now a compile error (listing `ch03-02-percent-mismatch.rs`).
- **Identifiers**: `Id<T>` with `PhantomData<fn() -> T>` and manual trait impls, and aliases `TenantId`, `OrderId`,
  `AccountId`. The raw constructor is `pub(crate)` to the persistence layer, so IDs come from rows or parsers, not from
  arithmetic.
- **At the JSON boundary**, serde does the parsing (listing `ch03-13-serde-boundary.rs`, verified):

```rust,ignore
/// On the wire, just a number. In the program, not interchangeable with any other number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrderId(u64);

/// Deserialization goes THROUGH the parser: an out-of-range value never becomes a BasisPoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct BasisPoints(u16);
```

```text
parsed: DiscountRequest { order: OrderId(42), discount: BasisPoints(250) }
serialized: {"order":42,"discount":250}
rejected: 15000 bps is more than 100% at line 1 column 32
```

The wire format didn't change, which mattered for the Java clients. The 15,000-basis-point request is rejected *during
deserialization*, with a position in the input, before any handler code runs. That's "parse, don't validate" delegated
to the framework (Chapter 22.2 covers serde in depth).

- **Policies as markers**: the gateway's retry middleware takes `R: Idempotent` (listing `ch03-10-marker-retry.rs`).
  `GET`-style requests implement it, and charges implement it only in the variant that carries an idempotency key. The
  pull request that adds an `impl Idempotent` is the one that gets the payments team's review, which is where the
  policy decision belongs.

### 10. Failure scenario

**The swapped IDs.** A Java batch job at Meridian cancelled stale orders with
`orderService.cancel(long tenantId, long orderId)`. A refactor reordered the parameters to `(orderId, tenantId)` to
match a new internal convention and updated 11 of 12 call sites. The twelfth, in a rarely run cleanup job, passed
`(tenantId, orderId)`. Both were `long`, so it compiled, and the tests used fixtures where tenant and order IDs never
collided. In production, tenant 7's cleanup cancelled order #7, which belonged to tenant 9. The multi-tenant isolation
guarantee failed because of a parameter order.

Typed IDs turn this into listing `ch03-04-id-mixup.rs`: `expected Id<Order>, found Id<Tenant>`. Three follow-ups made
the fix durable:

1. **No `u64` in service signatures.** A lint-like review rule: public functions in the order service take `OrderId`,
   `TenantId`, and so on. A raw `u64` parameter needs a comment explaining why.
2. **No forging.** `Id::new` is `pub(crate)` in the persistence layer. Batch jobs receive IDs from queries, so they
   can't construct `OrderId(7)` from a tenant's number.
3. **Tests with colliding IDs.** Fixtures now deliberately give tenants and orders overlapping numeric IDs, which
   would have caught the Java bug too. Types prevent the bug; colliding fixtures detect the class of bug in code that
   still uses raw numbers.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part V).*

1. What is a newtype, and why is it free at run time? What evidence from this chapter's assembly proves it?
2. When should a newtype's field be private, and when is a public field fine? Use `Cents` and `BasisPoints` as
   examples.
3. What three things does `PhantomData<X>` tell the compiler? Why does rustc reject unused type parameters?
4. Why is `PhantomData<fn() -> T>` the right marker for a typed ID? What goes wrong with `PhantomData<T>` and with
   `PhantomData<*const T>`?
5. What is a zero-sized type? What does `Vec<()>` do on `push`, and why is `HashSet<K>` "free" on top of
   `HashMap<K, ()>`?
6. What's the difference between a marker trait like `Idempotent` and an auto trait like `Send`? Who checks each claim?
7. When is `repr(transparent)` required rather than merely harmless?
8. Why is implementing `Deref<Target = u64>` for `OrderId` an anti-pattern?

### 12. Exercises

- **Beginner.** Write `Meters(f64)` and `Feet(f64)` with a `From<Feet> for Meters`, and a function that only accepts
  `Meters`. Show the compile error when you pass `Feet`.
- **Intermediate.** Implement `Add` and `Sub` for `Cents`, but not `Mul<Cents>`. Add `Mul<BasisPoints>` returning
  `Cents` (rounding explicitly). Write the test that proves `Cents * Cents` doesn't compile (a `compile_fail` doctest).
- **Advanced.** Make `Id<T>` use `NonZeroU64` so `Option<Id<T>>` is 8 bytes. Verify with `size_of`. Then write
  `Id::<T>::parse(&str)`, and decide what it returns for `"0"`.
- **Systems.** Emit the assembly for a function that sums a `&[Cents]` and one that sums a `&[i64]`. Are they merged
  like `discount_typed` was? Explain what you see.
- **Architecture.** List every `u64` and `String` parameter in one service's public API you know. Classify each: an ID
  (which entity?), a quantity (which unit?), validated text, or truly free-form. Estimate how many newtypes you'd need
  and where the parsers would live.

### 13. Debugging exercise

The first version of Meridian's `Id<T>` derived everything (listing `ch03-05-derive-bound.rs`, verified):

```rust,compile_fail
use std::marker::PhantomData;

// The first version of Id<T>: derive everything, as you would for a plain struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id<T> {
    raw: u64,
    _entity: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    pub fn new(raw: u64) -> Self {
        Id { raw, _entity: PhantomData }
    }
}

pub struct Order {
    pub cents: i64, // not Copy: owns heap data in the real system
    pub lines: Vec<String>,
}

fn audit(id: Id<Order>) {
    println!("audit {}", id.raw);
}

fn main() {
    let id = Id::<Order>::new(42);
    audit(id);
    audit(id); // Id<Order> is "just a u64"... isn't it Copy?
}
```

```text
error[E0382]: use of moved value: `id`
27 |     let id = Id::<Order>::new(42);
   |         -- move occurs because `id` has type `Id<Order>`, which does not implement the `Copy` trait
28 |     audit(id);
   |           -- value moved here
29 |     audit(id); // Id<Order> is "just a u64"... isn't it Copy?
   |           ^^ value used here after move
note: if all bounds were met, you could clone the value
 5 | #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
   |                 ----- derived `Clone` adds implicit bounds on type parameters
 6 | pub struct Id<T> {
   |               - introduces an implicit `T: Clone` bound
   = help: consider manually implementing `Clone` to avoid undesired bounds
```

1. `Id<T>` derives `Copy`. Why is `Id<Order>` not `Copy`? Write out the `impl` header that `#[derive(Copy)]`
   generates.
2. Which other derived traits have the same problem? Which operations on `Id<Order>` would fail next (think `HashMap`
   keys and `==`)?
3. Fix it the way listing `ch03-03-typed-ids.rs` does. Why can't the derive macro "just know" that `T` is phantom?
4. The same trap appears in Chapter 5.4's type-state `Payment<S>` with `#[derive(Debug)]`. Predict the error before
   reading that chapter's listing notes.

### 14. Design exercise

**Multi-currency money in the fraud feature library.** The Rust fraud library called from Java (Chapter 1.4) computes
features like "sum of a customer's transactions in the last hour." Transactions arrive in 30 currencies, and a 2024 bug
summed EUR and JPY amounts directly.

Compare two designs:

- **A. Currency as a phantom type**: `Money<C: Currency>` with `enum Eur {}` and `enum Jpy {}` markers. Adding a
  `Money<Eur>` to a `Money<Jpy>` doesn't compile.
- **B. Currency as data**: `Money { minor: i64, currency: CurrencyCode }`, with `checked_add` returning
  `Result<Money, CurrencyMismatch>`.

1. Transactions arrive as JSON with a currency string. Which design handles "currency known only at run time"
   naturally? What does design A need at the boundary (hint: an enum over `Money<C>` for every `C`, like Chapter 5.4's
   `AnyPayment`)?
2. Minor-unit exponents differ (JPY has 0 decimals, EUR 2, some currencies 3). Where does that knowledge live in each
   design?
3. Choose one for the feature library and justify it. Is there a hybrid, with phantom types inside a computation and
   data at the edges?
4. What would the Java side of the FFM boundary see in each design (Chapter 16.3)?
