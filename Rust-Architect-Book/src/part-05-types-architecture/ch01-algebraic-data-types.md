# Chapter 5.1 — Algebraic Data Types: Structs, Enums, Option, Result

> **Where this sits:** Part V · Types as Architecture · chapter 1 of 4
> **Prerequisites:** Chapter 2.6 (structs, enums, `Option`, `Result`, and their layout basics), Chapter 2.5 (`match`),
> Chapter 2.7 (privacy).
> **After this chapter you can:** count the states a type admits and compare that number with the states your domain
> actually has; make invalid states unrepresentable; apply *parse, don't validate* at a system boundary so validation
> happens once; use exhaustiveness checking as a change-management tool; and read how rustc lowers a `match` to MIR and
> machine code.

---

## Pass 1 · User level — *Types that say exactly what can happen*

### 1. Problem

Chapter 2.6 showed the syntax. You can declare a struct, an enum with data, an `Option`, and a `Result`, and you know
roughly how they're laid out. This chapter is about using them as a **design language**.

Every system has invariants: "an authenticated connection is also a connected one", "a rate limit of zero means
something specific", "every e-mail address we store has an `@`". There are three places an invariant can live:

1. **In people's heads and in comments.** It's free to write down and fails silently.
2. **In run-time checks.** Every function that might see a bad value has to check for it. Each check is a branch, a
   test case, and a way to forget.
3. **In the types.** A bad value can't be constructed, so no function ever has to check.

Java engineers mostly live in option 2, partly because Java made option 3 expensive for a long time. Every wrapper type
was a heap object and every reference could be null. Rust makes option 3 cheap: wrappers cost nothing at run time
(Chapter 5.3 proves it in assembly), and there's no null to defend against. This Part is about moving invariants from
option 2 to option 3. The first step is to count.

### 2. Mental model

**Types have sizes in a second sense: the number of values they admit.** Write `|T|` for the number of distinct values
of type `T` (its *cardinality*). Then [LANG]:

| Type | Values | Algebra |
|---|---|---|
| `!`, `std::convert::Infallible`, `enum Void {}` | none | 0 |
| `()`, a unit struct `struct Marker;` | exactly one | 1 |
| `bool` | `false`, `true` | 2 |
| `struct S { a: A, b: B }`, the tuple `(A, B)` | every combination | `|A| × |B|` (**product**) |
| `enum E { X(A), Y(B) }` | one of the alternatives | `|A| + |B|` (**sum**) |
| `Option<T>` | `None`, or `Some` of any `T` | `1 + |T|` |
| `Result<T, E>` | `Ok` of any `T`, or `Err` of any `E` | `|T| + |E|` |

That's why structs and enums are called **algebraic data types**: structs multiply, enums add. The algebra holds even
at the extremes. `Result<T, Infallible>` has `|T| + 0` values, so it's isomorphic to `T`. As §3 shows, it's also the
same size.

**The design rule this Part is built on:**

> **The number of values your type admits should equal the number of states your domain has.**
> Every extra value is an *invalid state*, and every function that receives the type must defend against it, or trust
> that nobody ever built it.

Two booleans describing a database connection admit 2 × 2 = 4 values. The domain has 3 states: disconnected,
connected, authenticated. The fourth value, "authenticated but not connected", is nonsense, but it's representable, so
someone will eventually construct it: through a bug, a partial update, or a deserialized row. An enum with three
variants admits exactly 3 values, so there's nothing to defend.

```text
 struct ConnFlags { connected: bool, authenticated: bool }      enum ConnState { Disconnected, Connected, Authenticated }

   connected →   false        true                               Disconnected ──► Connected ──► Authenticated
 authenticated
     false     Disconnected  Connected                           3 values, 3 states, 0 to defend
     true      ???  ◄── invalid, but representable
               4 values, 3 states, 1 to defend in every function
```

**Parse, don't validate** (the phrase is Alexis King's, from a 2019 essay of that name) is the same rule applied at
system boundaries. *Validation* checks a value and throws the knowledge away, returning `bool` or `()`, so the next
function has to trust or re-check. *Parsing* checks a value and **returns a more precise type** that carries the proof:
`fn parse(raw: &str) -> Result<Email, EmailError>`. Every function downstream takes `Email`, not `&str`, and can't be
called with an unchecked string.

### 3. Rust code

**Counting states** (listing `ch01-01-cardinality.rs`, verified). The flags struct admits four values, and every
function has to call `flags_valid` to rule out the fourth. The enum has nothing to rule out:

```rust,ignore
#[derive(Debug, Clone, Copy)]
struct ConnFlags {
    connected: bool,
    authenticated: bool,
}

#[derive(Debug, Clone, Copy)]
enum ConnState {
    Disconnected,
    Connected,
    Authenticated,
}

fn flags_valid(f: ConnFlags) -> bool {
    // the invariant every function touching ConnFlags must remember
    !(f.authenticated && !f.connected)
}
```

```text
ConnFlags { connected: false, authenticated: false } -> valid: true
ConnFlags { connected: false, authenticated: true } -> valid: false
ConnFlags { connected: true, authenticated: false } -> valid: true
ConnFlags { connected: true, authenticated: true } -> valid: true
ConnFlags: 4 representable states; ConnState: 3
```

**Parse, don't validate** (listing `ch01-02-parse-dont-validate.rs`, verified). The field is private, so the only way to
obtain an `Email` is through `parse`:

```rust
mod domain {
    use std::fmt;

    /// An e-mail address that has been checked. The field is private: the only way to get
    /// an `Email` is `Email::parse`, so every `Email` in the program is valid.
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct Email(String);

    #[derive(Debug, PartialEq)]
    pub enum EmailError {
        Empty,
        MissingAt,
        BadDomain,
    }

    impl Email {
        pub fn parse(raw: &str) -> Result<Email, EmailError> {
            let raw = raw.trim();
            if raw.is_empty() {
                return Err(EmailError::Empty);
            }
            let (local, domain) = raw.split_once('@').ok_or(EmailError::MissingAt)?;
            if local.is_empty() || !domain.contains('.') || domain.starts_with('.') || domain.ends_with('.') {
                return Err(EmailError::BadDomain);
            }
            Ok(Email(raw.to_ascii_lowercase()))
        }

        pub fn as_str(&self) -> &str {
            &self.0
        }
    }

    impl fmt::Display for Email {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }
}

use domain::{Email, EmailError};

/// Takes an `Email`, not a `&str`: this function cannot be handed an unchecked string.
fn send_receipt(to: &Email, order_id: u64) -> String {
    format!("receipt for order {order_id} queued to {to}")
}

fn main() {
    for raw in ["  Ada@Example.COM ", "", "ada.example.com", "ada@localhost", "grace@navy.mil"] {
        match Email::parse(raw) {
            Ok(email) => println!("{raw:?} -> {}", send_receipt(&email, 42)),
            Err(e) => println!("{raw:?} -> rejected: {e:?}"),
        }
    }
    assert_eq!(Email::parse("x@y"), Err(EmailError::BadDomain));
    let e = Email::parse("Ops@Meridian.Example").unwrap();
    println!("normalized once, at the boundary: {}", e.as_str());
}
```

```text
"  Ada@Example.COM " -> receipt for order 42 queued to ada@example.com
"" -> rejected: Empty
"ada.example.com" -> rejected: MissingAt
"ada@localhost" -> rejected: BadDomain
"grace@navy.mil" -> receipt for order 42 queued to grace@navy.mil
normalized once, at the boundary: ops@meridian.example
```

Two things happen at the boundary: the input is **checked** and it's **normalized** (trimmed, lowercased). Both happen
once. `send_receipt` has no error path because it can't receive a bad address. (The e-mail grammar here is deliberately
simplified. Real address validation is a rabbit hole. The *architecture* is the point.)

**Privacy is what makes the proof unforgeable.** Constructing an `Email` directly from outside the module fails
(listing `ch01-03-email-private.rs`, verified):

```rust,compile_fail
mod domain {
    #[derive(Debug)]
    pub struct Email(String); // public type, private field

    impl Email {
        pub fn parse(raw: &str) -> Option<Email> {
            raw.contains('@').then(|| Email(raw.to_string()))
        }
    }
}

fn main() {
    let ok = domain::Email::parse("ada@example.com");
    let forged = domain::Email(String::from("not-an-email")); // bypass the parser?
    println!("{ok:?} {forged:?}");
}
```

```text
error[E0603]: tuple struct constructor `Email` is private
 4 |     pub struct Email(String); // public type, private field
   |                      ------ a constructor is private if any of the fields is private
15 |     let forged = domain::Email(String::from("not-an-email")); // bypass the parser?
   |                          ^^^^^ private tuple struct constructor
```

A `pub` field would turn the type back into a label: anyone could write `Email(anything)`. Chapter 2.7's `pub`-field
incident at Meridian was this failure with money in it.

**Sentinels are invalid states in disguise** (listing `ch01-05-sentinel-bug.rs`, verified). "`0` means unlimited" packs
two meanings into one integer. An enum separates them, and `NonZeroU32` removes the ambiguous value altogether:

```rust
use std::num::NonZeroU32;

// BEFORE: a sentinel. `0` means "unlimited" -- by convention, in a comment, somewhere.
fn allowed_sentinel(requests_this_second: u32, limit_per_second: u32) -> bool {
    limit_per_second == 0 || requests_this_second < limit_per_second
}

// AFTER: the two meanings are two variants; a zero limit is not representable.
#[derive(Debug, Clone, Copy)]
enum Limit {
    Unlimited,
    PerSecond(NonZeroU32),
}

fn allowed(requests_this_second: u32, limit: Limit) -> bool {
    match limit {
        Limit::Unlimited => true,
        Limit::PerSecond(n) => requests_this_second < n.get(),
    }
}

fn main() {
    // An operator sets a tenant's limit to 0 intending "block this tenant entirely"...
    let blocked_tenant_limit = 0;
    println!("sentinel: request #1000 allowed? {}", allowed_sentinel(1000, blocked_tenant_limit));

    // With the enum, "0 per second" must be written down as a decision:
    println!("enum: 0 as a limit is {:?}", NonZeroU32::new(0));
    println!("enum: unlimited allows #1000? {}", allowed(1000, Limit::Unlimited));
    let ten = Limit::PerSecond(NonZeroU32::new(10).unwrap());
    println!("enum: 10/s allows #9? {}  #10? {}", allowed(9, ten), allowed(10, ten));
    println!("size_of::<Limit>() = {}", std::mem::size_of::<Limit>());
}
```

```text
sentinel: request #1000 allowed? true
enum: 0 as a limit is None
enum: unlimited allows #1000? true
enum: 10/s allows #9? true  #10? false
size_of::<Limit>() = 4
```

Note the last line. The enum is **4 bytes**, the same as the `u32` sentinel. `NonZeroU32` leaves the bit pattern 0
unused, and rustc stores `Unlimited` in it. The safer model costs nothing in memory. (Chapter 5.2 is about exactly this
trick.) If "blocked" is a real state, it becomes a third variant, `Blocked`. The point is that someone has to *decide*
that, where the sentinel let the decision happen by accident.

**Option and Result as control flow** (listing `ch01-04-combinators.rs`, verified). A config loader where one key is
required and another is optional but must be well-formed if present:

```rust,ignore
fn parse_pool(raw: &HashMap<&str, &str>) -> Result<PoolConfig, ConfigError> {
    let max_conns = raw
        .get("max_conns")
        .ok_or(ConfigError::Missing("max_conns"))? // Option -> Result, then `?`
        .parse::<u32>()
        .map_err(|e| ConfigError::NotANumber("max_conns", e))?;

    let idle_timeout_s = raw
        .get("idle_timeout_s")
        .map(|v| v.parse::<u32>()) // Option<Result<u32, _>>
        .transpose() // Result<Option<u32>, _>
        .map_err(|e| ConfigError::NotANumber("idle_timeout_s", e))?;

    Ok(PoolConfig { max_conns, idle_timeout_s })
}
```

```text
Ok(PoolConfig { max_conns: 64, idle_timeout_s: Some(30) })
Ok(PoolConfig { max_conns: 64, idle_timeout_s: None })
Err(Missing("max_conns"))
Err(NotANumber("idle_timeout_s", ParseIntError { kind: InvalidDigit }))
```

`transpose` is the combinator that keeps "absent" and "present but malformed" apart. `Option<Result<T, E>>` becomes
`Result<Option<T>, E>`, so `?` can propagate the malformed case. That distinction is §13's bug. Part VIII covers `?`
and error-type design properly.

**Patterns are a query language over these shapes** (listing `ch01-07-patterns.rs`, verified):

```rust,ignore
fn describe(f: &Frame) -> String {
    match f {
        Frame::Ping => "ping".to_string(),
        // binding + guard + @-binding on a range
        Frame::Data { stream: s @ 1..=15, payload } if payload.len() <= 4 => format!("small data on control stream {s}"),
        Frame::Data { stream, payload: [first, .., last] } => format!("data on {stream}: {first:#04x}..{last:#04x}"),
        Frame::Data { stream, payload: [] | [_] } => format!("tiny data on {stream}"),
        Frame::Close { code: 1000 } => "normal close".to_string(),
        Frame::Close { code } => format!("close with {code}"),
    }
}

fn port_of(addr: &str) -> Option<u16> {
    // let-else: the happy path stays unindented
    let Some((_, port)) = addr.rsplit_once(':') else {
        return None;
    };
    port.parse().ok()
}

// in main:
    // Since 1.82 an Err(Infallible) arm may be omitted: the pattern is irrefutable.
    let Ok(mode) = parse_mode("STRICT");
    // let chains (edition 2024, stable since 1.88)
    if let Some(p) = port_of(addr) && p >= 1024 { /* ... */ }
```

```text
ping
small data on control stream 3
data on 40: 0x68..0x6f
tiny data on 40
normal close
close with 4001
mode = strict
10.0.0.7:8443: unprivileged port 8443
size_of: Result<u64, Infallible> = 8, Option<u64> = 16, Frame = 24
```

Three things to notice:

- **Slice patterns** (`[first, .., last]`, `[] | [_]`) together cover every length, so the `match` is exhaustive without
  a wildcard. The guarded arm doesn't count toward exhaustiveness [LANG]: the compiler can't know when a guard is true.
- **`let Ok(mode) = parse_mode(..)` is irrefutable** because `Result<String, Infallible>` has no `Err` values.
  [VERSION] Before Rust 1.82 you had to write `let Ok(mode) = .. else { unreachable!() }`. Since 1.82, patterns on
  visibly empty types may be omitted.
- **`Result<u64, Infallible>` is 8 bytes, the same as `u64`.** The algebra (`|T| + 0 = |T|`) is also the layout. Compare
  `Option<u64>` at 16 bytes: `1 + |u64|` values don't fit in 64 bits.

---

## Pass 2 · Systems level — *How rustc represents, checks, and compiles ADTs*

### 4. Under the hood

**One representation for both.** [RUSTC] Inside rustc, structs and enums are both an `AdtDef` ("algebraic data type
definition"). A struct is an ADT with exactly one variant, and an enum has zero or more. Unions are the third kind. That
uniformity is why patterns, privacy, and layout work the same way for both, and why `Option` and `Result` need no
special compiler support. They're ordinary library enums. (A few lang items let `?` and `for` find them.)

**Exhaustiveness is an algorithm, not a lint.** When you write a `match`, rustc builds a matrix of your patterns and asks,
for each arm, "is this arm *useful*: is there a value it matches that no earlier arm matches?" and, at the end, "is a
wildcard still useful?" If one is, some value escapes every arm, and that's error E0004. [RUSTC] The implementation
lives in the `rustc_pattern_analysis` crate and runs on THIR, the typed tree just before MIR (Chapter 18.4). It's based
on Luc Maranget's usefulness algorithm ("Warnings for pattern matching", 2007). The same analysis reports unreachable
arms: an arm that isn't useful is dead code.

The verified consequence (listing `ch01-06-exhaustive.rs`): add a variant, and every `match` that doesn't cover it stops
compiling.

```rust,compile_fail
// The ledger's event enum gained a variant. Every `match` without a wildcard now fails to compile.
#[derive(Debug)]
enum LedgerEvent {
    Charge { cents: i64 },
    Refund { cents: i64 },
    Chargeback { cents: i64, reason_code: u16 }, // NEW
}

fn balance_delta(e: &LedgerEvent) -> i64 {
    match e {
        LedgerEvent::Charge { cents } => *cents,
        LedgerEvent::Refund { cents } => -*cents,
    }
}

fn main() {
    let e = LedgerEvent::Chargeback { cents: 4_999, reason_code: 4837 };
    println!("{e:?} -> {}", balance_delta(&e));
}
```

```text
error[E0004]: non-exhaustive patterns: `&LedgerEvent::Chargeback { .. }` not covered
11 |     match e {
   |           ^ pattern `&LedgerEvent::Chargeback { .. }` not covered
 7 |     Chargeback { cents: i64, reason_code: u16 }, // NEW
   |     ---------- not covered
```

This is Chapter 2.5's chargeback incident seen from the other side. There, a wildcard `_ => 0` absorbed the new
variant, and the only signal was a dead-code warning. **A wildcard converts "the compiler finds every place this change
matters" into "you find them".** For enums you own, prefer listing variants explicitly in business logic. Save `_` for
cases where "everything else" really is a single decision.

**How a `match` compiles.** The MIR for the three-variant version (listing `ch01-09-match-mir.rs`, `emit.ps1 -Target
mir`, rustc 1.98.1, trimmed):

```text
fn balance_delta(_1: &LedgerEvent) -> i64 {
    bb0: {
        _2 = discriminant((*_1));
        switchInt(move _2) -> [0: bb4, 1: bb3, 2: bb2, otherwise: bb1];
    }
    bb1: {
        unreachable;
    }
    bb4: {
        _3 = &(((*_1) as Charge).0: i64);
        _0 = copy (*_3);
        goto -> bb7;
    }
    ...
```

Read the discriminant, then `switchInt` on it. The `otherwise` edge leads to `unreachable`, a promise to LLVM that no
other discriminant exists. That promise rests on the validity invariant from Chapters 1.3 and 2.3: an enum with any
other tag is undefined behavior, so the compiler doesn't have to handle one. `(*_1) as Charge` is a **downcast**, a
place projection that reinterprets the enum's payload as one variant's fields, and it's only legal on the edge where the
discriminant said so.

In release mode, LLVM notices that two arms compute the same thing (listing `ch01-09-match-mir.rs`, `-Target asm -Mode
release`):

```text
playground::balance_delta:
	movzx	ecx, word ptr [rdi]        ; load the tag
	mov	rax, qword ptr [rdi + 8]   ; load cents (same offset in every variant)
	test	ecx, ecx                   ; Charge?
	je	.LBB0_2
	cmp	ecx, 1                     ; (dead: Refund and Chargeback both negate)
	neg	rax
.LBB0_2:
	ret
```

rustc placed `cents` at the same offset in all three variants, so after the match the code is a single test. Neither the
source nor the MIR asked for this: the layout and the optimizer found it together. The leftover `cmp` is harmless:
`neg` overwrites the flags, and the result is never read.

### 5. Memory

The algebra carries over into layout, with one important correction:

- **Products add sizes** (plus alignment padding, Chapter 5.2).
- **Sums take the largest variant plus a tag**, *unless* rustc can hide the tag in values the payload can never hold: a
  **niche** (Chapters 1.2 and 2.6, and all of Chapter 5.2).

The verified numbers from this chapter:

| Type | Values | Size | Why |
|---|---|---|---|
| `Limit { Unlimited, PerSecond(NonZeroU32) }` | 1 + (2³² − 1) = 2³² | 4 | exactly fits 32 bits: `Unlimited` is the bit pattern 0 |
| `Result<u64, Infallible>` | 2⁶⁴ + 0 | 8 | the empty variant needs no encoding |
| `Option<u64>` | 1 + 2⁶⁴ | 16 | one more value than 64 bits can hold → a separate tag, padded to alignment 8 |

Counting values tells you whether a niche is *possible*. If `1 + |T|` exceeds `2^(8 × size_of::<T>())`, the tag needs
space of its own. `Limit` is the example a model designer should remember: **replacing a sentinel with a precise type
cost zero bytes.**

A parsed newtype like `Email(String)` is exactly a `String`, 24 bytes: pointer, capacity, length. The proof it carries
("this was checked") exists only at compile time.

### 6. CPU / OS

Two effects matter at the machine level, and both are about branches.

**A `match` is a branch or a jump table.** Dense discriminants give a jump table or a short compare chain. Chapter 2.5
showed the table without a bounds check, because the validity invariant makes an out-of-range tag impossible. Branch
predictors handle stable patterns well. A `match` over events that are 99% `Charge` costs close to nothing per event.
A random mix costs mispredictions, about 15–20 cycles each on current x86 cores (order of magnitude). Chapter 20.5
measures it.

**Parsing once removes checks everywhere else.** Code that validates at every layer pays for it in every layer: a
branch, a call, and the validation code's instruction-cache footprint. With parsed types, the checks run once at the
boundary, and inner functions have *no code* for invalid cases. The effect on throughput is predicted from mechanism,
not measured here. It's usually small next to I/O. The real payoff is correctness: there's no path on which the check
was forgotten.

There's nothing OS-specific about ADTs. Their one OS-adjacent consequence is at the boundaries where bytes come in from
sockets and files, which is exactly where parsing belongs (Chapter 5.2 §10 and Part XXIII).

---

## Pass 3 · Architect level — *Choosing how much to encode*

### 7. Trade-offs

| Technique | Invalid states | Cost | Use when |
|---|---|---|---|
| Primitives + checks (`u32`, `String`, `bool` flags) | Representable; every function defends | Checks at every layer; bugs where one is missing | Truly unconstrained data; throwaway code |
| Newtype with private field + `parse` (this chapter, 5.3) | Unrepresentable after the boundary | One type per concept; conversion at the edges | Identifiers, money, units, validated strings |
| Enum of states with per-state data | Unrepresentable combinations | Exhaustive `match` at use sites; migrations when variants change | Lifecycles, protocol messages, "one of" data |
| Type-state (Chapter 5.4) | Invalid *transitions* rejected at compile time | Generic signatures; hard to store and deserialize | Short-lived, in-process protocols with a fixed order |

**Where to stop.** Encoding has diminishing returns:

- **Combinatorial explosion.** Five independent optional features don't need 32 enum variants. Independent facts are
  a *product* of small types (a struct of enums), not one huge sum.
- **Boundaries still speak strings.** JSON, SQL rows, and protobufs come in as loose data. Every precise type needs a
  parser at each boundary. Budget for it: it's code, tests, and error messages.
- **Evolution.** Exhaustiveness is a gift inside one codebase and a burden across crate boundaries. Adding a variant to
  a *public* enum breaks every downstream exhaustive `match`, so it's a semver-major change. `#[non_exhaustive]` [LANG]
  forces downstream crates to include a wildcard, buying you freedom to add variants and costing them exhaustiveness.
  (Within the defining crate the attribute has no effect, so the Playground's single-crate setup can't demonstrate it:
  check it in a two-crate workspace with `cargo build`.) Use it on public error enums and protocol enums likely to grow.
  Don't use it on your own domain enums inside an application.

### 8. Java comparison

Modern Java has most of the vocabulary. The gaps are null, cost, and defaults.

| Concept | Java (21+) | Rust |
|---|---|---|
| Product type | `record Point(double x, double y)` (JDK 16) | `struct Point { x: f64, y: f64 }` |
| Sum type | `sealed interface Shape permits Circle, Rect` (JDK 17) + records | `enum Shape { Circle { r: f64 }, Rect { .. } }` |
| Exhaustive match | `switch` with patterns over a sealed hierarchy (JDK 21) | `match`, E0004 |
| Optional value | `Optional<T>` (not for fields; can itself be `null`) | `Option<T>`, no null anywhere |
| Recoverable error | checked exceptions, or a hand-rolled result type | `Result<T, E>` + `?` |
| Parse, don't validate | record + compact constructor that validates | newtype + private field + `parse` |
| Cost of a wrapper | an object: header + pointer, often a heap allocation | nothing (Chapter 5.3, verified) |

Java *can* express "parse, don't validate". A record's compact constructor can reject bad input, and the record is then
valid by construction. What's different in practice:

1. **Every Java reference can be null**, so an `Email` parameter can still be `null`. Rust has no null. An absent value
   is `Option<Email>`, and the type says so.
2. **Wrappers cost allocations** until Project Valhalla's value classes ship (Chapter 5.3), so Java codebases tend to
   pass `String` and `long` around and validate repeatedly. Rust's wrappers are free, so the incentive runs the other
   way.
3. **Enums vs sealed hierarchies.** A Java `enum` is a closed set of *singletons* with no per-value data. Per-variant data
   needs a sealed interface and records. That's more ceremony, but it's the same algebra.

> **Analogy limit.** A Java `switch` over a sealed interface is checked for exhaustiveness when *it* is compiled. But
> Java links classes at run time. If a new permitted subtype shows up in a separately compiled class file, the `switch`
> meets a case it never saw, and the compiler-inserted default throws `MatchException` (JDK 21 behavior for pattern
> switches). Rust links statically. Nothing can add a variant to your enum after compilation, so exhaustiveness is a
> property of the binary, not only of the source.

### 9. Production scenario

**Meridian's merchant onboarding: parse at the edge.** The onboarding service (Java) accepts merchant applications
through a REST API, a partner CSV bulk import, and an internal admin UI. E-mail addresses, IBANs, and country codes are
validated by a `Validators` utility class that each entry point is supposed to call. A 2025 audit found 14 call sites,
and one missing: the CSV importer, added later by a different team, skipped `Validators.email()`. The system had stored
about 3,000 malformed contact addresses before bounced payout notifications revealed it.

The Rust rewrite of the onboarding core applies this chapter's rule:

- **Boundary types**: `Email`, `Iban`, `CountryCode`, each a newtype with a private field and a `parse` returning
  `Result<_, ParseError>`. Every entry point (HTTP handler, CSV row reader, admin command) converts raw strings into
  these types first.
- **Core types take only parsed values**: `fn register(app: MerchantApplication)`, where
  `MerchantApplication { contact: Email, payout: Iban, country: CountryCode, .. }`. There's no `Validators` class to
  forget, because the core can't be *called* with a raw string.
- **Errors are data**: the CSV importer now collects `Vec<(row_number, ParseError)>` and reports all bad rows at once.
  That was impossible when validation was scattered and threw on the first failure.

The effect on the codebase is structural. A new entry point *cannot* skip validation, because it has to produce the
types the core requires. The 14 call sites became three parse functions and one rule. Code review now looks for
`String` fields in core types: each one is either legitimately free-form or a type that hasn't been written yet.

### 10. Failure scenario

**The rate-limit sentinel.** Meridian's API gateway enforces per-key rate limits (Part I). The Java config model had
`int requestsPerSecond`, and a comment in the loader said `0 = unlimited (internal keys)`. During an abuse incident, an
on-call engineer set an abusive partner's limit to 0 to block it. The gateway read 0 as "unlimited". For 40 minutes,
the partner's traffic was the only traffic *exempt* from rate limiting, until someone read the loader's source.

Listing `ch01-05-sentinel-bug.rs` reproduces both sides. The sentinel version answers "request #1000 allowed? true" for
a limit of 0. The enum version has no way to say "0 per second": `NonZeroU32::new(0)` is `None`. Unlimited is spelled
`Unlimited`, and "blocked", if it's a real operator action, becomes a variant with its own code path and audit log. The
fix cost zero bytes: `Limit` is 4 bytes, like the `int`.

The lesson generalizes: **any value that means "not a value" (0, -1, empty string, `Long.MIN_VALUE`, epoch 0) is an
enum variant that hasn't been written.** Find them in config models and wire formats first. Humans type those values
into systems during incidents, which is exactly when conventions are forgotten.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part V).*

1. Why are structs called *product* types and enums *sum* types? Compute the number of values of
   `(bool, Option<bool>)` and of `Result<bool, ()>`.
2. State the "values = states" design rule. Give an example of a type with more values than its domain has states, and
   the bug it invites.
3. What's the difference between *validating* and *parsing* an input? Why does the difference matter more in a large
   codebase than in a small one?
4. What stops code outside a module from constructing an `Email` without calling `parse`? What would weaken that
   guarantee?
5. How does rustc decide that a `match` is non-exhaustive? Why don't guarded arms count?
6. What does `switchInt ... otherwise: unreachable` in MIR promise, and what language rule makes the promise true?
7. Why is `Result<u64, Infallible>` 8 bytes but `Option<u64>` 16? What does counting values tell you about when a niche
   is possible?
8. When should a public enum be `#[non_exhaustive]`, and what does that cost its users?

### 12. Exercises

- **Beginner.** Replace `struct Shipment { shipped: bool, delivered: bool, tracking: Option<String> }` with an enum that
  makes "delivered but not shipped" and "shipped without tracking" unrepresentable. Count the values before and after
  (treat `String` as one abstract value).
- **Intermediate.** Write `Percent::parse(&str) -> Result<Percent, PercentError>` accepting `"15"`, `"15%"`, and
  `" 15 % "`, and rejecting `"150"`, `"-1"`, and `"abc"` with distinct errors. Then write the one function in your
  program that should still accept raw strings, and justify it.
- **Advanced.** Write a `match` over `(Option<u8>, Option<u8>)` that is exhaustive without a wildcard, using at most four
  arms. Then add a guard to one arm and explain the compiler's response.
- **Systems.** Emit the MIR (`-Target mir`) for a `match` on `Option<&u64>`. Where is the discriminant read, and what does
  `discriminant(..)` of a niche-encoded `Option` turn into in the release assembly?
- **Architecture.** Audit a config model you know (Java, YAML, anything) for sentinel values. For each one, write the
  enum that replaces it and the migration: how do old config files map onto the new type?

### 13. Debugging exercise

This loader compiles, runs, and passed code review (listing `ch01-08-ok-swallow.rs`, verified):

```rust
use std::collections::HashMap;

#[derive(Debug)]
struct PoolConfig {
    max_conns: u32,
    idle_timeout_s: Option<u32>, // None means "never time out"
}

fn parse_pool(raw: &HashMap<&str, &str>) -> Option<PoolConfig> {
    let max_conns = raw.get("max_conns")?.parse().ok()?;
    let idle_timeout_s = raw.get("idle_timeout_s").and_then(|v| v.parse().ok());
    Some(PoolConfig { max_conns, idle_timeout_s })
}

fn main() {
    let typo = HashMap::from([("max_conns", "64"), ("idle_timeout_s", "30s")]);
    let cfg = parse_pool(&typo).expect("config loads");
    println!("{cfg:?}");
    match cfg.idle_timeout_s {
        Some(s) => println!("idle connections close after {s}s (max {})", cfg.max_conns),
        None => println!("idle connections are never closed (max {})", cfg.max_conns),
    }
}
```

```text
PoolConfig { max_conns: 64, idle_timeout_s: None }
idle connections are never closed (max 64)
```

1. The operator wrote `30s`. What did the service do, and why is this worse than refusing to start?
2. Which single call destroys the distinction between "absent" and "malformed"? Count the values of the types before and
   after that call.
3. Fix it so a malformed value is an error, without making the key required. Compare with listing
   `ch01-04-combinators.rs`.
4. Write a code-review rule of thumb for `.ok()` on a `Result` in config and input parsing.

### 14. Design exercise

**Merchant verification at Meridian.** A merchant goes through KYC: *unverified* → *documents submitted* (with a list
of document IDs) → *verified* (by a named reviewer, at a time) or *rejected* (with a reason code). A verified merchant can
be *suspended* (with a reason and an optional end date) and reinstated. Only verified, unsuspended merchants may receive
payouts.

1. Write the Java-style model first: one class, nullable fields, a `String status`. Count the representable
   combinations, roughly.
2. Design the Rust model: an enum with per-state data. Where do the reviewer, timestamps, and reasons live? How many
   invalid combinations remain?
3. Write the signature of `fn payout(...)` so it's impossible to call for an unverified or suspended merchant. What
   does the caller have to do first? (Chapter 5.4 will offer a second answer.)
4. The data lives in PostgreSQL. Sketch the table and the parse function from a row into your enum. What happens to a
   row in a state your enum doesn't know?
