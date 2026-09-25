# Chapter 2.6 — Structs, Enums, and Methods

> **Where this sits:** Part II · Rust From First Principles · chapter 6 of 7
> **Prerequisites:** Chapters 2.3–2.5.
> **After this chapter you can:** model domain data with product types (structs) and sum types (enums with data); use
> `self`, `&self`, and `&mut self` receivers as ownership statements; explain method-call resolution; predict struct
> and enum layout (field reordering, niches, bloated variants); and say exactly what `#[derive]` generates.

---

## Pass 1 · User level — *Data and behavior, separated*

### 1. Problem

A Java class bundles five things: data, behavior, object identity, a place in an inheritance hierarchy, and (usually)
nullable references to other objects. Records (Java 16) and sealed interfaces (Java 17) help, but the default modeling
tool is still a class with fields that "should" be consistent.

Rust takes that bundle apart:

- **Data** is a `struct` (all of these fields) or an `enum` (exactly one of these variants, each with its own fields).
- **Behavior** lives in `impl` blocks, and in traits (Part VI).
- **There's no object identity.** Values are compared by value. Pointer identity has to be asked for explicitly.
- **There's no inheritance.** Reuse comes from composition and traits.
- **There are no null fields.** Absence is `Option<T>`, which is itself an enum.

The payoff is the modeling principle often summarized as **"make invalid states unrepresentable."** If a card payment
has a card number and a bank transfer has an IBAN, the type shouldn't *allow* a payment with both, or with neither.

### 2. Mental model

```text
 struct  = AND   Money { cents: i64, currency: Currency }        every value has ALL fields
 enum    = OR    PaymentMethod::Card { .. } | BankTransfer { .. }
                 | Wallet(..) | Cash                             every value is EXACTLY ONE variant,
                                                                 carrying only that variant's data
 impl    = functions namespaced under a type                     Money::new(...), m.is_negative()
```

**Method receivers are ownership statements.** They're the first place ownership shows up in everyday API design:

| Receiver | Meaning | The caller... | Example |
|---|---|---|---|
| *(none)* | Associated function; no instance | — | `Money::new(0, Eur)`, "constructors" |
| `&self` | Read-only access | keeps ownership; many readers at once | `fn is_negative(&self) -> bool` |
| `&mut self` | Exclusive, mutating access | keeps ownership; needs a `mut` binding | `fn deposit(&mut self, cents: i64)` |
| `self` | Takes ownership; consumes the value | loses the value (unless it's `Copy`) | `fn close(self) -> Money`, builders |

**There are no constructors.** A struct literal (`Money { cents, currency }`) is the *only* way to create a struct
value. `new` is just a conventional name for an associated function. Combined with private fields (Chapter 2.7), that
function becomes the *only door* into the type, which is where invariants get enforced.

### 3. Rust code

A small payments domain (verified):

```rust
#![allow(dead_code)] // some fields are only printed via derived Debug, which doesn't count as a "read"

#[derive(Debug, Clone, Copy, PartialEq)]
enum Currency {
    Eur,
    Usd,
    Inr,
}

#[derive(Debug, Clone, PartialEq)]
struct Money {
    cents: i64,
    currency: Currency,
}

#[derive(Debug)]
struct AccountId(u64); // a tuple struct: a named wrapper around one value

#[derive(Debug)]
struct Account {
    id: AccountId,
    balance: Money,
    frozen: bool,
}

#[derive(Debug, Clone, Copy)]
enum CardNetwork {
    Visa,
    Mastercard,
}

#[derive(Debug)]
enum PaymentMethod {
    Card { last4: [u8; 4], network: CardNetwork },
    BankTransfer { iban: String },
    Wallet(String),
    Cash,
}

impl Money {
    fn new(cents: i64, currency: Currency) -> Self {
        // an associated function: no `self`
        Money { cents, currency }
    }
    fn is_negative(&self) -> bool {
        // `&self`: reads
        self.cents < 0
    }
}

impl Account {
    fn open(id: u64, currency: Currency) -> Self {
        Account { id: AccountId(id), balance: Money::new(0, currency), frozen: false }
    }
    fn deposit(&mut self, cents: i64) {
        // `&mut self`: modifies in place
        self.balance.cents += cents;
    }
    fn close(self) -> Money {
        // `self`: consumes; the account no longer exists after this call
        self.balance
    }
}

impl PaymentMethod {
    fn fee_bps(&self) -> u32 {
        match self {
            PaymentMethod::Card { network: CardNetwork::Visa, .. } => 180,
            PaymentMethod::Card { network: CardNetwork::Mastercard, .. } => 200,
            PaymentMethod::BankTransfer { .. } => 20,
            PaymentMethod::Wallet(_) => 150,
            PaymentMethod::Cash => 0,
        }
    }
}

fn main() {
    let mut acct = Account::open(42, Currency::Eur);
    acct.deposit(12_500);
    Account::deposit(&mut acct, 500); // the same call, spelled out: a method is a function
    println!("{acct:?}");
    println!("negative? {}", acct.balance.is_negative());

    let methods = [
        PaymentMethod::Card { last4: *b"4242", network: CardNetwork::Visa },
        PaymentMethod::BankTransfer { iban: "DE89370400440532013000".to_string() },
        PaymentMethod::Wallet("paypal:ada".to_string()),
        PaymentMethod::Cash,
    ];
    for m in &methods {
        println!("{:>4} bps  {m:?}", m.fee_bps());
    }

    let final_balance = acct.close();
    println!("closed with {final_balance:?}");
}
```

```text
Account { id: AccountId(42), balance: Money { cents: 13000, currency: Eur }, frozen: false }
negative? false
 180 bps  Card { last4: [52, 50, 52, 50], network: Visa }
  20 bps  BankTransfer { iban: "DE89370400440532013000" }
 150 bps  Wallet("paypal:ada")
   0 bps  Cash
closed with Money { cents: 13000, currency: Eur }
```

Things to notice:

- `acct.deposit(12_500)` and `Account::deposit(&mut acct, 500)` are **the same call**. A method is a function whose
  first parameter is the receiver.
- `fee_bps` matches `self`, which is a `&PaymentMethod`, against non-reference patterns. Match ergonomics (Chapter 2.5)
  makes that work, and nested patterns (`Card { network: CardNetwork::Visa, .. }`) pick out exactly the case needed.
- After `acct.close()`, `acct` is gone: `close` took `self` by value. Adding `println!("{acct:?}")` afterwards would be
  E0382, *borrow of moved value*.
- The `#![allow(dead_code)]` exists because **derived `Debug` doesn't count as reading a field** for the dead-code lint.
  rustc says so explicitly in its warnings (`...has a derived impl for the trait Debug, but this is intentionally
  ignored during dead code analysis`).

A `&mut self` method needs a mutable binding:

```rust,compile_fail
struct Counter {
    hits: u64,
}

impl Counter {
    fn hit(&mut self) {
        self.hits += 1;
    }
}

fn main() {
    let c = Counter { hits: 0 };
    c.hit();
    println!("{}", c.hits);
}
```

```text
error[E0596]: cannot borrow `c` as mutable, as it is not declared as mutable
  --> src/main.rs:14:5
   |
14 |     c.hit();
   |     ^ cannot borrow as mutable
   |
help: consider changing this to be mutable
   |
13 |     let mut c = Counter { hits: 0 };
   |         +++
```

The error says *borrow*: calling a `&mut self` method **borrows the value mutably** for the duration of the call.
Methods are the most common place you'll create borrows without writing a single `&`.

---

## Pass 2 · Systems level — *Methods, derives, and layout*

### 4. Under the hood

**Method-call resolution.** [LANG] For `receiver.method(args)`, the compiler searches candidate receiver types in order:
the receiver's type `T`, then `&T`, then `&mut T`, then it dereferences (`*receiver`) and repeats. The first type with a
matching method wins, and the compiler **inserts the needed `&`, `&mut`, or `*` automatically**. That's how
`acct.deposit(5)` becomes `Account::deposit(&mut acct, 5)`, and later how a method on `String` can be called through a
`Box<String>` or an `Rc<String>` (auto-deref, Part VI). No dynamic dispatch is involved. [RUSTC] The call compiles to a
direct call to a function with a mangled name like `playground::Account::deposit`, and inlining usually makes even that
disappear.

**What `#[derive]` actually generates.** `derive` runs a procedural macro that writes ordinary `impl` blocks. Here's the
real expansion, from the nightly compiler's macro-expansion output, of:

```rust,ignore
#[derive(Debug, Clone, PartialEq)]
pub struct Money {
    cents: i64,
    currency: &'static str,
}
```

```rust,ignore
#[automatically_derived]
impl ::core::fmt::Debug for Money {
    #[inline]
    fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
        ::core::fmt::Formatter::debug_struct_field2_finish(f, "Money",
            "cents", &self.cents, "currency", &&self.currency)
    }
}
#[automatically_derived]
impl ::core::clone::Clone for Money {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            cents: ::core::clone::Clone::clone(&self.cents),
            currency: ::core::clone::Clone::clone(&self.currency),
        }
    }
}
#[automatically_derived]
impl ::core::marker::StructuralPartialEq for Money { }
#[automatically_derived]
impl ::core::cmp::PartialEq for Money {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.cents == other.cents && self.currency == other.currency
    }
}
```

[VERSION] That's the output of the nightly `-Zunpretty=expanded` tool, and the exact code is a std implementation detail
that changes between versions. What *is* stable: derives are **field-by-field**. `Clone` clones each field, `PartialEq`
compares each field in declaration order with `&&` short-circuiting, and `Debug` prints each field. The marker
`StructuralPartialEq` records that equality is structural, which is what allows constants of this type to be used as
`match` patterns. All paths are fully qualified (`::core::...`) so the generated code can't be broken by names in your
scope. And there's nothing magic about any of it: it's code you could have written by hand. That matters in §7, because
field-by-field equality is sometimes **wrong** for a type.

**Consuming methods and moves.** `close(self)` receives the `Account` by value, which is a move (Chapter 1.3). The
caller's binding is dead afterwards, and the compiler emits no destructor call for it. The `Money` inside is moved out
as the return value, and the rest of the `Account` is dropped when `close` returns. The builder pattern (§13) relies on
exactly this: each `self -> Self` method consumes the old builder and returns a new one.

### 5. Memory

**Struct layout.** [LANG] Only `#[repr(C)]` (C layout: declaration order, C padding rules), `#[repr(transparent)]`, and
primitive enum reprs have a *guaranteed* layout. The default representation promises nothing about field order, and
[RUSTC] rustc does reorder fields to reduce padding. Verified on rustc 1.98.1:

```text
Unordered {u8,u64,u8}:     size   16, align 8
repr(C)   {u8,u64,u8}:     size   24, align 8
Shape (largest payload 16): size   24
Option<Shape>:              size   24
bool / Option<bool>:        size    1 / 1
Message (Data is 4 KiB):    size 4097
MessageBoxed:               size    8
Option<MessageBoxed>:       size   16
```

```text
 default repr (rustc reordered):                repr(C) (declaration order):
 ┌──────────────────────┬───┬───┬──────────┐   ┌───┬───────────┬──────────────────────┬───┬───────────┐
 │ b: u64               │ a │ c │ pad (6)  │   │ a │  pad (7)  │ b: u64               │ c │  pad (7)  │
 └──────────────────────┴───┴───┴──────────┘   └───┴───────────┴──────────────────────┴───┴───────────┘
  16 bytes                                      24 bytes: 14 of them padding
```

Use `repr(C)` when the layout is an **interface**: FFI (Part XVI), memory-mapped file formats, and hardware registers.
Otherwise let the compiler choose.

**Enum layout: a tag plus a union of payloads.**

```text
 enum Shape { Circle { r: f64 }, Rect { w: f64, h: f64 }, Empty }      24 bytes
 ┌──────────────┬──────────────────────────────────────────┐
 │ tag (0/1/2)  │ payload: r  |  w, h  |  (nothing)         │   tag is padded to 8 because the payload
 │ 8 bytes      │ 16 bytes (the LARGEST variant)            │   contains f64s (alignment 8)
 └──────────────┴──────────────────────────────────────────┘

 Option<Shape>: 24 bytes, NOT 32. The tag uses 3 of its 256+ possible values; the spare values are a NICHE,
                so None is stored as tag = 3. No extra byte needed.

 enum MessageBoxed { Ping, Data(Box<[u8; 4096]>) }      8 bytes
 ┌──────────────────────────┐
 │ Box pointer, or NULL     │   A Box is never null, so null is a niche: Ping IS the null pointer.
 └──────────────────────────┘   No tag at all.

 Option<MessageBoxed>: 16 bytes. The only niche (null) is already taken by Ping, so Option needs a real tag.
                       [RUSTC] rustc doesn't use alignment bits as niches.
```

**The largest variant sets the size of every value.** `Message::Ping` carries no data, yet every `Message` is 4,097
bytes, because it must be able to hold a `Data`. Boxing the payload makes every `Message` 8 bytes, with one heap
allocation per `Data`. That's the trade-off in §10.

### 6. CPU / OS

- **Cache lines are 64 bytes** on common x86-64 and ARM server cores. Struct size and field order decide how many
  structs fit in a line, and whether the fields a hot loop reads share a line. Reordering to cut padding (which rustc
  does for you) and **hot/cold splitting** (moving rarely used fields behind a `Box` or into a side table) are the main
  levers. Part IX and Part XX measure them.
- **A `match` on an enum** loads the tag, a byte or a word, and dispatches (Chapter 2.5's jump table). With a niche, the
  "tag load" is a null check on a pointer.
- **Moves are `memcpy`s.** Moving a 4,097-byte `Message` into a `Vec`, through a channel, or out of a function copies
  4,097 bytes each time, unless the optimizer elides the copy. That's usually fine for small types and a real cost for
  large ones, and it's another reason to box large variants.
- Everything here also holds in `no_std` code: structs and enums need no runtime support at all.

---

## Pass 3 · Architect level — *Modeling decisions*

### 7. Trade-offs

**Enum vs struct-with-optional-fields.**

| | `enum PaymentMethod { Card {..}, BankTransfer {..}, .. }` | `struct Payment { kind: Kind, card: Option<..>, iban: Option<..> }` |
|---|---|---|
| Invalid combinations (card *and* IBAN, card kind with no card) | **Unrepresentable** | Representable; must be validated everywhere |
| Accessing data | `match`, which forces you to handle every variant | `.card.unwrap()`, which panics on bad data |
| Adding a variant | The compiler finds every `match` to update | Every consumer must remember to check the new field |
| Memory | Largest variant + tag (or a niche) | Sum of all fields |
| Evolving serialized data | Needs care (tagged representation, Part XXII) | Easy to add optional fields |

**Derive, or write it yourself?** Derived `PartialEq` is structural equality. That's wrong when fields are caches,
timestamps, or internal counters that shouldn't affect equality, or when two different representations mean the same
thing (a normalized vs a non-normalized email). Derived `Debug` prints every field, including secrets. A `Token`,
`Password`, or card-number type should implement `Debug` by hand and redact. Derived `Clone` on a type holding a large
`Vec` is a deep copy: correct, and possibly expensive in a hot path.

**Consuming builder vs `&mut` builder:**

| | `fn retries(self, n) -> Self` (consuming) | `fn retries(&mut self, n) -> &mut Self` |
|---|---|---|
| Chaining | `Builder::new().retries(3).build()` | `b.retries(3).timeout(5); b.build()` |
| Conditional configuration | Needs rebinding: `let b = if x { b.retries(3) } else { b };` | Natural: `if x { b.retries(3); }` |
| `build` | Can take `self`, moving the fields out with no clone | Takes `&self`, so it must clone the fields (or use `mem::take`) |
| Reuse the builder for several builds | No | Yes |

**Public fields vs private fields with methods.** Plain data with no invariant (a `Point`, a config struct) can have
public fields. Anything with an invariant ("balance never goes below the overdraft limit") must have private fields, so
the `impl` block is the only code that can break it. Chapter 2.7 shows what happens when a field is made public
"temporarily."

### 8. Java comparison

| Java | Rust | Note |
|---|---|---|
| `class` (data + behavior + identity + inheritance) | `struct` + `impl` (+ traits) | No inheritance: composition plus traits (Part VI) |
| Implicit `this` | Explicit `self`, `&self`, `&mut self` | The receiver states the ownership mode, and the compiler enforces it |
| Constructor | Struct literal + a conventional `new` | Private fields make `new` the only way in |
| `record Money(long cents, Currency c)` | `#[derive(Debug, Clone, PartialEq, Eq, Hash)] struct Money { .. }` | Records generate `equals`/`hashCode`/`toString`, and derives generate the equivalents |
| `enum Currency { EUR, USD }`: singleton **objects**, the same fields for every constant | `enum` with **per-variant data** | A Java enum constant can't carry per-instance data. A Rust variant carries its own. |
| `sealed interface Payment permits Card, Bank {}` + records | `enum PaymentMethod { Card {..}, BankTransfer {..} }` | The closest match in meaning; very different memory (heap objects vs an inline tagged union) |
| `a == b` compares references; `.equals` compares values | `a == b` calls `PartialEq` (values); `std::ptr::eq(a, b)` for identity | Rust values have no identity unless you ask for addresses |
| Nullable fields | `Option<T>`, with the niche making it free for references and boxes | Absence is part of the type |

> **Analogy limit.** "A Rust `enum` is a Java `enum`" is the most misleading analogy in the language. A Java enum is a
> fixed set of *objects* that share one class shape. A Rust enum is a *sum type*: each variant is a different shape of
> data, and a value is exactly one of them. The Java feature that corresponds to a Rust enum is a **sealed interface
> with record implementations**, and even that is heap objects plus type tests, not an inline tagged union.

### 9. Production scenario

**Meridian's payment method, before and after.** The legacy Java model looked like this:

```java
class Payment {
    String type;          // "CARD", "BANK", "WALLET", "CASH"
    String cardLast4;     // set when type == CARD... usually
    String cardNetwork;
    String iban;          // set when type == BANK
    String walletId;      // set when type == WALLET
}
```

A 2025 incident review found records in the database with `type = "CARD"` and a non-null `iban`, left behind by a
migration script. Fee calculation read `cardNetwork` (null), fell through to a default, and charged bank-transfer fees
on card payments. The Rust model is the `PaymentMethod` enum from §3. A `Card` *has* a `last4` and a `network` and has
no IBAN field to set wrongly. `fee_bps` is an exhaustive `match`, so a new method type such as `Crypto` can't be added
without deciding its fee. The data problem doesn't disappear. The database can still hold bad rows. But it moves to
**one place**, the conversion from database row to `PaymentMethod`, which either succeeds with a valid value or reports
an error. That boundary pattern, *parse, don't validate*, comes back in Parts V and VIII.

### 10. Failure scenario

**The heartbeat that ate 4 GB.** Meridian's market-data fan-out queues messages to slow consumers:

```rust,ignore
enum Message {
    Ping,
    Data([u8; 4096]),   // one market-data frame, inline
}
```

Most messages are `Ping` heartbeats. A slow consumer falls behind, and its queue grows to a million messages. Expected
memory is "mostly pings, so a few megabytes." Actual memory is **1,000,000 × 4,097 bytes ≈ 4.1 GB**, because every
`Ping` occupies the size of the largest variant (verified above: `Message` is 4,097 bytes). The pod is OOM-killed. And
before that, every enqueue and dequeue `memcpy`'d 4 KB per message, so CPU was already high.

The fix is `Data(Box<[u8; 4096]>)`, or better `Data(bytes::Bytes)`, a refcounted slice of a shared buffer. Every
`Message` is then 8–32 bytes, pings cost almost nothing, and data frames are one allocation (or a refcount bump). Clippy
flags this pattern: `clippy::large_enum_variant` warns when one variant is much larger than the others. The review rule
Meridian adopted: **for any enum stored in a collection or sent through a channel, check `size_of` in a unit test.**

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part II).*

1. What do `&self`, `&mut self`, and `self` each say about ownership? Give a real API example of each.
2. Why does Rust have no constructors? How do you guarantee an invariant holds from the moment a value is created?
3. What does `acct.deposit(5)` compile to? Explain auto-ref in method calls.
4. Why is `Unordered { u8, u64, u8 }` 16 bytes but the `#[repr(C)]` version 24? When must you use `repr(C)`?
5. Why is `Option<Shape>` the same size as `Shape`, while `Option<MessageBoxed>` is larger than `MessageBoxed`?
6. What does `#[derive(PartialEq)]` generate? Give two situations where a derived `PartialEq` or `Debug` is wrong for
   the type.
7. When would you model with an enum, when with a struct of `Option` fields, and when with trait objects?
8. Explain the difference between a Java enum and a Rust enum in capability and in memory representation.

### 12. Exercises

- **Beginner.** Model a shipment: `Created`, `PickedUp { courier, at }`, `InTransit { hub, eta }`,
  `Delivered { at, signed_by }`, `Lost { last_seen_hub }`. Write `fn is_final(&self) -> bool` and
  `fn describe(&self) -> String` with exhaustive matches.
- **Intermediate.** Implement `ClientBuilder` twice, once consuming (`self -> Self`) and once with `&mut self`. Write
  calling code that sets retries only when an environment variable is present, in both styles. Which reads better?
- **Advanced.** *Predict*, then verify with `size_of`: `Option<Option<bool>>`, `Result<u32, ()>`,
  `Option<(u8, bool)>`, `enum E { A(u32), B(u16), C }`, `Option<Box<str>>`. Explain every result in terms of tags and
  niches.
- **Systems.** Design a benchmark (don't trust your intuition) that measures pushing a million `Message` values into a
  `Vec` with the inline 4 KiB variant against the boxed variant, for 99% `Ping` and for 99% `Data`. Predict both results
  before running.
- **Architecture.** Take a Java class hierarchy you know (an abstract `Notification` with `Email`, `Sms`, and `Push`
  subclasses, say) and design the Rust equivalent. Say where you'd use an enum and where you'd use a trait, and why.

### 13. Debugging exercise

```rust,compile_fail
#[derive(Debug)]
struct ClientConfig {
    timeout_ms: u64,
    retries: u32,
}

struct ClientBuilder {
    timeout_ms: u64,
    retries: u32,
}

impl ClientBuilder {
    fn new() -> Self {
        ClientBuilder { timeout_ms: 1_000, retries: 0 }
    }
    fn timeout_ms(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }
    fn retries(mut self, n: u32) -> Self {
        self.retries = n;
        self
    }
    fn build(self) -> ClientConfig {
        ClientConfig { timeout_ms: self.timeout_ms, retries: self.retries }
    }
}

fn main() {
    let builder = ClientBuilder::new();
    builder.timeout_ms(250);
    builder.retries(3);
    let config = builder.build();
    println!("{config:?}");
}
```

rustc reports **two** E0382 errors. The first one:

```text
error[E0382]: use of moved value: `builder`
  --> src/main.rs:33:5
   |
31 |     let builder = ClientBuilder::new();
   |         ------- move occurs because `builder` has type `ClientBuilder`, which does not implement the `Copy` trait
32 |     builder.timeout_ms(250);
   |             --------------- `builder` moved due to this method call
33 |     builder.retries(3);
   |     ^^^^^^^ value used here after move
   |
note: `ClientBuilder::timeout_ms` takes ownership of the receiver `self`, which moves `builder`
  --> src/main.rs:17:23
   |
17 |     fn timeout_ms(mut self, ms: u64) -> Self {
   |                       ^^^^
```

1. Explain both errors in terms of the receiver type. Where did the configured builder returned by `timeout_ms(250)`
   go?
2. Fix `main` two ways: by chaining, and by rebinding (`let builder = builder.timeout_ms(250);`).
3. What change to the *API* would make the original `main` compile, and what would that cost `build()`?
4. Suppose you "fix" it by changing the setters to `fn timeout_ms(&mut self, ms: u64) -> &mut Self` and leave `main`
   unchanged, with `let builder` still immutable. Which error do you get now, and how does it relate to §3?

### 14. Design exercise

**Meridian's order model.** An order goes through `Draft` (items editable), `Placed` (items frozen, `placed_at`),
`Paid` (`payment_id`, `paid_at`), `Shipped` (one or more `tracking_numbers`), `Delivered`, and `Cancelled` (a `reason`,
and whether a refund was issued). Some data exists in every state (order ID, customer ID, currency). Some exists only in
some states.

Design it as (a) one `enum OrderState` with per-state data plus a wrapper `struct Order { id, customer, currency,
state }`, and (b) a single struct with a `status` field and `Option` fields. Compare them on invalid states, the ease of
writing each transition, database mapping, JSON API compatibility, and memory. Recommend one, and describe how you'd
migrate from the Java model, which is (b).
