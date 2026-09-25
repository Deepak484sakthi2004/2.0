# Chapter 6.2 — Associated Types, Generic Traits, and Blanket Impls

> **Where this sits:** Part VI · Traits · chapter 2 of 5
> **Prerequisites:** Chapter 6.1.
> **After this chapter you can:** decide between an associated type and a trait type parameter, and explain the
> decision in terms of *who chooses the type*; read `Iterator::Item`, `From<T>`, and `Into<U>` as design decisions
> rather than trivia; write blanket impls and supertraits; explain why `From` gets implemented and `Into` comes for free;
> and predict when a blanket impl will collide with a specific one (E0119), including across crate versions.

---

## Pass 1 · User level — *Who chooses the type?*

### 1. Problem

Meridian's market-data service ingests several feeds. A legacy partner sends `SYMBOL,PRICE` text lines. The internal
sequencer sends 4-byte big-endian sequence numbers. Each feed needs a decoder, and the ingestion pipeline should be
written once, generically.

The decoders differ in *what they produce* and *how they fail*: a quote vs a `u32`, a CSV error vs "N bytes missing."
Somewhere the trait has to mention those types. There are two ways to put a type into a trait, and they mean different
things:

- As an **associated type**: `trait Decoder { type Output; … }`. Each implementor fixes its `Output`, once.
- As a **type parameter**: `trait Convert<T> { … }`. One type may implement `Convert<f64>` *and* `Convert<String>`.

Choosing wrong makes a trait either impossible to implement twice when it should be, or needlessly ambiguous at every
call site. Then there's the third tool from this chapter's title: a **blanket impl**, which implements a trait for
every type that meets a bound. That's how std gives every `Display` type a `to_string()`, and every `From` pair an
`Into`.

### 2. Mental model

**Associated types are outputs. Type parameters are inputs.** An impl is selected by `Self` plus the trait's type
parameters. Once it's selected, its associated types are *determined* by it:

```text
 trait Decoder { type Output; type Error; … }        trait Convert<T> { fn convert(&self) -> T; }

 impl Decoder for CsvQuoteDecoder                    impl Convert<f64>    for Cents
      Output = Quote, Error = CsvError               impl Convert<String> for Cents
         ▲                                                    ▲
  Self alone picks the impl;                       Self is NOT enough: (Self, T) picks the impl,
  the outputs follow from it                       so someone must say which T
```

The test: **can a type sensibly implement this trait more than once?** An iterator yields *one* kind of item, so
`Iterator::Item` is associated. A type can be built *from* many sources, so `From<T>` is generic. A decoder decodes
into *one* output, so `Decoder::Output` is associated.

**A blanket impl** is `impl<T: Bound> Trait for T`: "every type that satisfies `Bound` gets `Trait`." It's
implementation by rule instead of by enumeration.

**A supertrait** is `trait Auditable: Debug`: "to be `Auditable`, a type must also be `Debug`." Inside the trait,
`Debug` is then available on `self`.

### 3. Rust code

**Associated types.** Each decoder chooses its own output and error (verified):

```rust
use std::fmt::Debug;

/// A wire-format decoder. Each decoder decides its own output and error types: ASSOCIATED TYPES.
trait Decoder {
    type Output: Debug;
    type Error: Debug;
    fn decode(&self, input: &[u8]) -> Result<Self::Output, Self::Error>;
}

#[derive(Debug)]
struct Quote {
    symbol: String,
    price_cents: i64,
}

/// "SYMBOL,PRICE" text lines.
struct CsvQuoteDecoder;

#[derive(Debug)]
enum CsvError {
    NotUtf8,
    MissingField,
    BadPrice(String),
}

impl Decoder for CsvQuoteDecoder {
    type Output = Quote;
    type Error = CsvError;
    fn decode(&self, input: &[u8]) -> Result<Quote, CsvError> {
        let text = std::str::from_utf8(input).map_err(|_| CsvError::NotUtf8)?;
        let (symbol, price) = text.split_once(',').ok_or(CsvError::MissingField)?;
        let price_cents = price.trim().parse().map_err(|_| CsvError::BadPrice(price.to_string()))?;
        Ok(Quote { symbol: symbol.to_string(), price_cents })
    }
}

/// A 4-byte big-endian sequence number.
struct SeqDecoder;

impl Decoder for SeqDecoder {
    type Output = u32;
    type Error = usize; // "how many bytes were missing"
    fn decode(&self, input: &[u8]) -> Result<u32, usize> {
        let bytes: [u8; 4] = input.get(..4).ok_or(4 - input.len().min(4))?.try_into().unwrap();
        Ok(u32::from_be_bytes(bytes))
    }
}

/// Generic over ANY decoder; the output type is determined by the decoder, not chosen by the caller.
fn decode_all<D: Decoder>(decoder: &D, frames: &[&[u8]]) -> Vec<Result<D::Output, D::Error>> {
    frames.iter().map(|f| decoder.decode(f)).collect()
}

fn main() {
    let csv: [&[u8]; 3] = [b"MRDN,12550", b"ACME", b"GLBX,abc"];
    for r in decode_all(&CsvQuoteDecoder, &csv) {
        println!("{r:?}");
    }
    let seq: [&[u8]; 2] = [&[0, 0, 1, 2], &[7]];
    for r in decode_all(&SeqDecoder, &seq) {
        println!("{r:?}");
    }
}
```

```text
Ok(Quote { symbol: "MRDN", price_cents: 12550 })
Err(MissingField)
Err(BadPrice("abc"))
Ok(258)
Err(3)
```

Notice what `decode_all`'s callers never write: the output type. Passing `&CsvQuoteDecoder` fixes `D`, and `D` fixes
`D::Output = Quote`. The `: Debug` bounds on the associated types are part of the contract: every implementor must
choose `Debug` types, and in return, generic code may print them.

**A generic trait**, implemented twice for one type, plus the std pattern that makes `Into` free (verified):

```rust
/// A GENERIC trait: one type can implement it many times, once per `T`.
trait Convert<T> {
    fn convert(&self) -> T;
}

struct Cents(i64);

impl Convert<f64> for Cents {
    fn convert(&self) -> f64 {
        self.0 as f64 / 100.0
    }
}

impl Convert<String> for Cents {
    fn convert(&self) -> String {
        format!("{}.{:02}", self.0 / 100, self.0 % 100)
    }
}

// The std pattern: From is generic (many sources), Into comes from a BLANKET impl over From.
struct Basis(u32);

impl From<u32> for Basis {
    fn from(bps: u32) -> Self {
        Basis(bps)
    }
}

impl From<f64> for Basis {
    fn from(percent: f64) -> Self {
        Basis((percent * 100.0).round() as u32)
    }
}

fn main() {
    let price = Cents(12_550);
    let as_float: f64 = price.convert(); // the annotation selects the impl
    let as_text = <Cents as Convert<String>>::convert(&price); // or fully qualified syntax
    println!("{as_float} / {as_text}");

    let a: Basis = 250u32.into(); // Into<Basis> for u32 exists because From<u32> for Basis does
    let b = Basis::from(1.75);
    println!("{} bps, {} bps", a.0, b.0);
}
```

```text
125.5 / 125.50
250 bps, 175 bps
```

With a generic trait, `Self` isn't enough to pick the impl. Something else must fix `T`: an annotation
(`let as_float: f64`), a fully qualified path, or the context the value flows into. Drop all of them and the compiler
asks you. That's this chapter's debugging exercise. (`Basis` is a nod to Chapter 2.2's percent-vs-basis-points
incident: `From<f64>` here reads *percent*, which is exactly the kind of implicit unit conversion Part V argues should
be a named constructor instead. Keep that in mind for the exercises.)

**Blanket impls and supertraits** (verified):

```rust
use std::fmt::{Debug, Display};

/// A BLANKET impl: every type that is Display gets `log_line` for free.
trait LogLine {
    fn log_line(&self, level: &str) -> String;
}

impl<T: Display + ?Sized> LogLine for T {
    fn log_line(&self, level: &str) -> String {
        format!("[{level}] {self}")
    }
}

/// A SUPERTRAIT: anything Auditable must also be Debug, so default methods can use {:?}.
trait Auditable: Debug {
    fn actor(&self) -> &str;
    fn audit(&self) -> String {
        format!("{} did {:?}", self.actor(), self)
    }
}

#[derive(Debug)]
struct Refund {
    order: u64,
    cents: i64,
    by: String,
}

impl Auditable for Refund {
    fn actor(&self) -> &str {
        &self.by
    }
}

fn main() {
    println!("{}", 42.log_line("INFO"));
    println!("{}", "gateway started".log_line("INFO"));
    println!("{}", 3.5f64.log_line("WARN"));

    let r = Refund { order: 9001, cents: 2_500, by: "ada".into() };
    println!("{}", r.audit());
    println!("order {} for {} cents", r.order, r.cents);
}
```

```text
[INFO] 42
[INFO] gateway started
[WARN] 3.5
ada did Refund { order: 9001, cents: 2500, by: "ada" }
order 9001 for 2500 cents
```

We never wrote `impl LogLine for i32`, `for str`, or `for f64`. The blanket impl covers them, and every future
`Display` type too. `?Sized` widens the blanket to unsized types such as `str` and `dyn Display`, since a type
parameter is implicitly `Sized` otherwise [LANG].

---

## Pass 2 · Systems level — *Projection, selection, and the overlap check*

### 4. Under the hood

**Projection and normalization.** [RUSTC] `D::Output` is shorthand for the *projection* `<D as Decoder>::Output`.
Inside `decode_all`, it's an opaque type the body can use only through its bounds (`Debug`). At each call site, once
`D = CsvQuoteDecoder` is known, the trait solver *normalizes* the projection by finding the impl and reading its
`type Output = Quote`. That's why type information flows **outward** from `Self` through an associated type with no
annotation. With `Convert<T>`, nothing flows: knowing `Self = Cents` leaves two candidate impls, and inference stops.

**Constraining associated types at use sites** [LANG]:

```text
 fn ingest<D: Decoder<Output = Quote>>(d: &D)        equality: only decoders that produce Quotes
 fn log_all<D: Decoder<Error: Display>>(d: &D)       associated type bound [VERSION] stable since Rust 1.79
 fn ticks() -> impl Iterator<Item = u32>             the same syntax on impl Trait
```

Associated types can themselves be generic: `type Item<'a> where Self: 'a;`. These *generic associated types* (GATs,
stable since Rust 1.65) are what a "lending iterator" needs, one that yields references into its own buffer
[VERSION]. Part X returns to them.

**std's two most important blanket impls** [LIB]:

```rust,ignore
// alloc::string (abridged)
impl<T: fmt::Display + ?Sized> ToString for T { fn to_string(&self) -> String { /* format via Display */ } }

// core::convert (abridged)
impl<T, U> Into<U> for T where U: From<T> { fn into(self) -> U { U::from(self) } }
impl<T> From<T> for T { fn from(t: T) -> T { t } }   // reflexive: every type converts into itself
```

These explain two pieces of Rust folklore. **"Implement `Display`, never `ToString`"**: the blanket already provides
`ToString`, and a manual impl would conflict with it. **"Implement `From`, never `Into`"**: the blanket turns every
`From<A> for B` into `Into<B> for A`, which is why `250u32.into()` worked above. std's `to_string` also uses an unstable
internal mechanism (*specialization*) so that `str::to_string` skips the formatting machinery. That's a privilege of
the standard library: on stable Rust, your blanket impl can't be overridden for special cases [LIB] [VERSION].

**Supertraits are where-clauses on `Self`** [LANG]. `trait Auditable: Debug` means `trait Auditable where Self: Debug`.
Every impl of `Auditable` must be for a `Debug` type, and every `A: Auditable` bound *implies* `A: Debug`. That's the
one kind of bound Rust does imply (Chapter 6.1 §7). With `dyn Auditable`, the supertrait's methods travel in the same
vtable (Chapter 6.4).

**The overlap check.** [RUSTC] A blanket impl claims every type that satisfies its bound, so the compiler must prove no
other impl of the same trait claims any of those types. `impl<T: Display> LogLine for T` plus `impl LogLine for Money`,
where `Money: Display`, is two impls for one `(trait, type)` pair. That's rejected (E0119, §10). Chapter 6.3 shows the
check is stricter than "overlaps *today*": it also rejects impls that *could* overlap after a dependency adds an impl.

### 5. Memory

Associated types and trait parameters are compile-time information. After monomorphization,
`Vec<Result<D::Output, D::Error>>` for the CSV decoder *is* a `Vec<Result<Quote, CsvError>>`: each element laid out
inline, with the `Quote` and `CsvError` stored inside the `Result`, not behind pointers. Compare Java: a
`List<Result<Quote, CsvError>>` holds references to heap objects, and a `Decoder<Integer, …>` for the sequence decoder
must box every `u32` into an `Integer`. Rust's `SeqDecoder` returns a plain `Result<u32, usize>`, in registers.

A blanket impl adds nothing to any type's layout. `42.log_line(..)` works on a bare 4-byte `i32`.

### 6. CPU / OS

Everything in this chapter resolves at compile time. `price.convert()` with `f64` expected is a direct call to
`<Cents as Convert<f64>>::convert`. The `String` version is a *different function*, selected before code generation.
Blanket impls generate code **per type actually used**: `42.log_line`, `"…".log_line`, and `3.5.log_line` are three
instances of the same source. That's monomorphization's code-size bill, and Part VII, Chapter 7.3 shows how to measure
and control it. The OS sees none of this.

---

## Pass 3 · Architect level — *Designing trait signatures that age well*

### 7. Trade-offs

**Associated type or type parameter?**

| Question | Associated type (`type Output`) | Type parameter (`Trait<T>`) |
|---|---|---|
| Impls per `Self` | Exactly one | Many (one per `T`) |
| Who chooses the type | The implementor | The user, per use |
| Call-site annotations | None: inference flows from `Self` | Often needed (E0282 when missing) |
| Bounds in generic code | `D: Decoder` (plus `Decoder<Output = X>` when needed) | `C: Convert<f64>`: must name `T` every time |
| Std examples | `Iterator::Item`, `Deref::Target`, `Add::Output`, `IntoIterator::IntoIter` | `From<T>`, `AsRef<T>`, `PartialEq<Rhs>`, `Add<Rhs>` |

`Add` shows that both can coexist in one trait: `trait Add<Rhs = Self> { type Output; … }`. The *right-hand side* is an
input (you can add `Money + Money` and `Money + i64`), and the *result* is an output determined by the pair. The
default `Rhs = Self` keeps the common case short.

**Blanket impls are powerful and exclusive.** A blanket impl in a library gives every qualifying type the behavior for
free, and it **removes the ability to write a specific impl** for any type it covers, in that crate or downstream. Once
published, adding one is a breaking change for any downstream crate that already implemented the trait for a covered
type (§10), so treat it as a major version bump. Alternatives that keep flexibility:

- **Opt-in marker**: `impl<T: Display + LogViaDisplay> LogLine for T`, where `LogViaDisplay` is an empty trait that types
  implement to join. Everyone else writes their own impl.
- **A free function or extension method** (`fn log_line<T: Display>(t: &T)`) instead of a trait impl: no coherence
  footprint at all.
- **Keep the blanket internal** (a private trait) until the design settles.

**Supertrait or method-level bound?** `trait Auditable: Debug` forces `Debug` on every implementor forever.
`fn audit(&self) -> String where Self: Debug` requires it only where the method is used. Prefer supertraits for true
"is-a" requirements, like `Error: Debug + Display`, and method bounds for conveniences.

### 8. Java comparison

**Implementing a generic interface twice is impossible in Java.** `class Cents implements Convert<Double>,
Convert<String>` fails with "cannot be inherited with different arguments." After erasure, both would be the same
interface `Convert`, with one `convert()` method returning `Object`. Rust's `Convert<f64>` and `Convert<String>` are
different traits to the compiler, with different impls and different machine code.

**Java has no associated types.** The idiom is to add type parameters: `interface Decoder<O, E extends Exception>`. Every
signature that touches a decoder must then carry them (`<O, E extends Exception> List<O> decodeAll(Decoder<O, E> d)`),
or fall back to wildcards (`Decoder<?, ?>`) and lose the types. An associated type is a *type member*. Scala has them;
Java doesn't. It lets `decode_all<D: Decoder>` stay one parameter wide however many types the contract carries.

**Java has no blanket impls.** A default method applies only to classes that *declare* `implements`. There's no way to
say "every `Comparable` is also `Rankable`" after the fact, so you write a static utility (`Rankings.rank(c)`) or an
adapter class. Java's `Object.toString()` makes every object printable by *inheritance*. Rust's `ToString` does it by
*rule*: exactly the types that are `Display`, and nothing else.

> **Analogy limit.** Supertraits look like interface inheritance (`interface Auditable extends Debug`), and for
> "requires" they are. But there's no subtype relationship between a type and its traits: a `Refund` is not "an
> `Auditable`" the way a Java object is an instance of its interfaces. Traits constrain types, and values don't carry
> them, unless you ask for `dyn` (Chapter 6.4). Only then is there an actual `&dyn Auditable` value, and converting it to
> `&dyn Debug` is an explicit *upcast*, stable since Rust 1.86 [VERSION].

### 9. Production scenario

**Meridian's feed decoders.** The market-data team built the ingestion pipeline on the `Decoder` trait from §3, and
the associated types did three jobs:

1. **Precise errors per feed.** `CsvError::BadPrice("abc")` carries the offending text, and the sequencer's error is a
   byte count. A shared `DecodeError` enum would have forced every feed into the lowest common denominator. When the
   pipeline needs uniform handling, it adds one bound, `D::Error: std::error::Error`, and Part VIII turns these into
   proper error types.
2. **No annotations in pipeline code.** Stages are written as `fn stage<D: Decoder>(…) -> D::Output`, and adding a feed
   never touches them.
3. **Constrained composition.** The quote book accepts only `D: Decoder<Output = Quote>`, so wiring the sequence decoder
   into it is a compile error at the wiring site, not a runtime `ClassCastException`.

The one generic trait in the design is a conversion: `From<RawQuote> for Quote` and `From<VendorQuote> for Quote`, two
sources into one type. That's the textbook case for a type parameter.

### 10. Failure scenario

**The blanket impl that broke payments.** Meridian's platform team maintains `meridian-log`. Version 1.4 added a
convenience: every `Display` type is loggable, via a blanket impl. The payments team, on 1.3, had a hand-written impl
for `Money` that redacts amounts from logs, a compliance requirement. The upgrade broke their build. Here's the same
conflict in one crate (listing `ch02-05-blanket-overlap.rs`):

```rust,compile_fail
use std::fmt;

// --- the logging library (v1.4 added the blanket impl for convenience) ---
trait LogLine {
    fn log_line(&self) -> String;
}

impl<T: fmt::Display> LogLine for T {
    fn log_line(&self) -> String {
        format!("[INFO] {self}")
    }
}

// --- the payments code (written against v1.3) ---
struct Money {
    cents: i64,
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:02} EUR", self.cents / 100, self.cents % 100)
    }
}

// A hand-written impl that redacts amounts in logs. Money is Display, so the blanket impl covers it too.
impl LogLine for Money {
    fn log_line(&self) -> String {
        "[INFO] <amount redacted>".to_string()
    }
}

fn main() {
    println!("{}", Money { cents: 12_550 }.log_line());
}
```

```text
error[E0119]: conflicting implementations of trait `LogLine` for type `Money`
 9 | impl<T: fmt::Display> LogLine for T {
   | ----------------------------------- first implementation here
...
27 | impl LogLine for Money {
   | ^^^^^^^^^^^^^^^^^^^^^^ conflicting implementation for `Money`
```

Across two crates, the message is the same, reported in the payments crate. That makes the breakage visible, which is
the *good* outcome. The team's first "fix" was worse: delete the redacting impl so the build passes. The blanket impl
then logged `125.50 EUR` in clear text, and nothing failed until an audit found amounts in the logs.

What the teams agreed afterwards:

- **Treat a new blanket impl as a breaking change**, and version it as a major release. It was published as a minor.
- **Use an opt-in marker** (`impl<T: Display + LogViaDisplay> LogLine for T`), so `Money` keeps its own impl and other
  types join the blanket explicitly.
- **Put compliance behavior in the type, not the log call.** `Money`'s `Display` itself should decide whether it's safe
  to print, or `Money` shouldn't be `Display` at all, just a `Debug` that redacts. A newtype `Redacted<T>` can make the
  choice explicit at the call site.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part VI).*

1. State the rule for choosing between an associated type and a trait type parameter. Apply it to `Iterator`, `From`,
   and `Add`.
2. Why does `decode_all(&CsvQuoteDecoder, …)` need no annotation, while `let x = price.convert();` does?
3. Why do you implement `From` and never `Into`? Why `Display` and never `ToString`?
4. What does `?Sized` do in `impl<T: Display + ?Sized> LogLine for T`?
5. What's the difference between `trait Auditable: Debug` and adding `where Self: Debug` to one method?
6. Why is adding a blanket impl to a published trait a breaking change? Give the concrete downstream failure.
7. Why can't a Java class implement `Comparable<A>` and `Comparable<B>`, and why can a Rust type implement
   `PartialEq<A>` and `PartialEq<B>`?
8. What does `D: Decoder<Output = Quote>` express that Java would express with a type parameter on every method?

### 12. Exercises

- **Beginner.** Add a `JsonQuoteDecoder` (use `serde_json`, available on the Playground) with `Output = Quote` and
  `Error = serde_json::Error`. Does `decode_all` change?
- **Intermediate.** Replace `From<f64> for Basis` with a named constructor `Basis::from_percent(f64)`. Argue why
  unit-changing conversions shouldn't be `From` impls. (Hint: what does `let b: Basis = 1.75.into();` communicate to a
  reader?)
- **Advanced.** Implement `Add<Money> for Money` and `Add<i64> for Money` (cents), with `type Output = Money`. Then try
  `impl Add<Money> for i64`. Which coherence rule from Chapter 6.3 allows it?
- **Systems.** Write a blanket `impl<T: Display> LogLine for T` and call `log_line` on five types. Emit the release
  assembly with `#[inline(never)]` on `log_line`. How many copies appear? What would erasure (Java) produce instead?
- **Architecture.** Your team maintains a trait used by 30 services. You want to add a blanket impl. Write the rollout
  plan: marker trait or not, semver bump, how you find downstream conflicts before release, and what the release notes
  must say.

### 13. Debugging exercise

The conversion from §3, with the annotation removed (listing `ch02-03-ambiguous-generic.rs`):

```rust,compile_fail
trait Convert<T> {
    fn convert(&self) -> T;
}

struct Cents(i64);

impl Convert<f64> for Cents {
    fn convert(&self) -> f64 {
        self.0 as f64 / 100.0
    }
}

impl Convert<String> for Cents {
    fn convert(&self) -> String {
        format!("{}.{:02}", self.0 / 100, self.0 % 100)
    }
}

fn main() {
    let price = Cents(12_550);
    let shown = price.convert(); // which Convert<T>?
    println!("{}", shown.len());
}
```

```text
error[E0282]: type annotations needed
22 |     let shown = price.convert(); // which Convert<T>?
   |         ^^^^^
23 |     println!("{}", shown.len());
   |                    ----- type must be known at this point
help: consider giving `shown` an explicit type
22 |     let shown: /* Type */ = price.convert(); // which Convert<T>?
```

1. Only `String` has a `len()` method here, so why can't the compiler work backwards from `.len()` to pick
   `Convert<String>`? (Hint: method calls need a known receiver type.)
2. Fix it three ways: a `let` annotation, fully qualified syntax, and a helper `fn show(c: &impl Convert<String>)`.
3. Would this error exist if `Convert` had an associated type instead? What would you lose?

### 14. Design exercise

**Meridian's message-bus codecs.** Services exchange about 40 message types over a bus, in two wire formats (JSON for
debugging, a compact binary format in production). Design the codec traits:

- One trait `Codec<M>` implemented by `Json` and `Binary` for every message type `M`? Or a trait `Message` with
  associated `type Wire`? Or both?
- Where do blanket impls help (for example, every `serde::Serialize` message gets JSON for free), and what do they rule
  out?
- A new wire format arrives next year. Count how many impls each design needs, and which crate must contain them under
  the orphan rule (Chapter 6.3 will check your answer).
