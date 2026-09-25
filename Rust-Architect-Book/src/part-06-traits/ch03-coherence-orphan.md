# Chapter 6.3 — Coherence and the Orphan Rule

> **Where this sits:** Part VI · Traits · chapter 3 of 5
> **Prerequisites:** Chapters 6.1–6.2. Chapter 2.1 and 2.7 mentioned the orphan rule. This chapter keeps that promise.
> **After this chapter you can:** state coherence and explain why Rust needs it; apply the orphan rule precisely,
> including fundamental types, trait objects, and local type parameters (verified); explain why an impl that overlaps
> nothing *today* is still rejected (E0119, "upstream crates may add a new impl"); choose between a newtype, an extension
> trait, and an upstream feature flag; and predict where impls must live in a multi-crate architecture.

---

## Pass 1 · User level — *One impl per trait and type, program-wide*

### 1. Problem

Meridian's frame-inspection tool prints binary frames as hex. The obvious code implements `Display` for `Vec<u8>`:

```rust,compile_fail
use std::fmt;

// Both the trait (Display) and the type (Vec<u8>) come from std: neither is local to this crate.
impl fmt::Display for Vec<u8> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

fn main() {
    println!("{}", vec![0xCA_u8, 0xFE]);
}
```

```text
error[E0117]: only traits defined in the current crate can be implemented for types defined outside of the crate
5 | impl fmt::Display for Vec<u8> {
  | ^^^^^^^^^^^^^^^^^^^^^^-------
  |                       |
  |                       `Vec` is not defined in the current crate
  = note: impl doesn't have any local type before any uncovered type parameters
  = note: define and implement a trait or new type instead
```

The rejection looks arbitrary until you imagine the alternative. If this compiled, so could the same impl in another
crate, printing *base64*. Link both into one program, and `println!("{}", frame)` has two meanings. Which one runs? It
could depend on link order, or it could differ between two modules of the same binary. `HashMap` has the same problem
with worse stakes. If two crates could each implement `Hash` for `uuid::Uuid`, a map built by one and queried by the
other would silently fail to find keys.

Rust's answer is **coherence**: [LANG] for any trait and type, there is **at most one** impl in the entire program.
The **orphan rule** is how the compiler guarantees coherence while checking one crate at a time. The **overlap check**
guarantees it within a crate.

### 2. Mental model

**The orphan rule, simplified: you may implement a trait for a type only if you own the trait or the type.**

```text
                         trait is LOCAL            trait is FOREIGN
                   ┌─────────────────────────┬──────────────────────────────┐
 type is LOCAL     │ impl Checksum for Frame │ impl Display for Frame       │  ✓ all allowed
                   ├─────────────────────────┼──────────────────────────────┤
 type is FOREIGN   │ impl Checksum for [u8]  │ impl Display for Vec<u8>     │  ✗ E0117: an "orphan":
                   │  ✓ (extension trait)    │                              │    it belongs to neither crate
                   └─────────────────────────┴──────────────────────────────┘
```

Why this guarantees global uniqueness: an impl can only live in the crate that defines the trait, or the one that
defines the type. There are at most two such crates, and one depends on the other (the impl mentions both), so the
compiler checks the later crate against the earlier one's impls.

**The overlap rule: no two impls of a trait may apply to the same type.** Within a crate, that's the E0119 from Chapter
6.2. It's also conservative about the *future*. An impl that relies on a foreign type *not* implementing a foreign
trait is rejected, because the foreign crate may add that impl in a minor release.

The two escape hatches follow from the grid:

- **Newtype**: wrap the foreign type in a local struct (`struct Hex(Vec<u8>)`), so the type becomes local.
- **Extension trait**: define a local trait (`trait Checksum`), so the trait becomes local.

### 3. Rust code

Both workarounds (verified):

```rust
use std::fmt;
use std::ops::Deref;

// Workaround 1: a NEWTYPE makes the type local. Local type + foreign trait: allowed.
struct Hex(Vec<u8>);

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl Deref for Hex {
    type Target = [u8]; // keep slice methods available on the wrapper
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

// Workaround 2: an EXTENSION TRAIT makes the trait local. Local trait + foreign type: allowed.
trait Checksum {
    fn checksum(&self) -> u32;
}

impl Checksum for [u8] {
    fn checksum(&self) -> u32 {
        self.iter().fold(0u32, |acc, &b| acc.rotate_left(5) ^ u32::from(b))
    }
}

fn main() {
    let frame = Hex(vec![0xCA, 0xFE, 0x01]);
    println!("frame = {frame}, len = {}", frame.len()); // len() via Deref to [u8]
    println!("checksum = {:#010x}", frame.checksum()); // extension trait on [u8], reached through Deref
    println!("checksum of literal = {:#010x}", b"MRDN".checksum());
}
```

```text
frame = cafe01, len = 3
checksum = 0x000337c1
checksum of literal = 0x0027c0ce
```

`frame.checksum()` shows the two workarounds composing. Method resolution (Chapter 6.1) derefs `Hex` to `[u8]` and
finds the extension trait's method there. `b"MRDN".checksum()` works on a `&[u8; 4]`, thanks to resolution's final
unsizing step (array to slice). The `Deref` impl is a deliberate choice for a *read-only view* type like `Hex`. §7
discusses when it's a mistake.

"Local" is more subtle than "defined in this crate." Three impls that look like orphans but aren't (verified):

```rust
use std::fmt;

trait Rule {
    fn id(&self) -> &str;
}

struct LargeAmount;

impl Rule for LargeAmount {
    fn id(&self) -> &str {
        "large-amount"
    }
}

// 1. `dyn LocalTrait` is a local type, so a foreign trait may be implemented for it.
impl fmt::Debug for dyn Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Rule({})", self.id())
    }
}

// 2. Box is #[fundamental]: Box<LocalType> counts as local too.
impl fmt::Display for Box<dyn Rule> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rule {}", self.id())
    }
}

// 3. A local type as a TRAIT PARAMETER makes `impl ForeignTrait<Local> for ForeignType` legal.
struct Cents(i64);

impl From<Cents> for f64 {
    fn from(c: Cents) -> f64 {
        c.0 as f64 / 100.0
    }
}

fn main() {
    let rules: Vec<Box<dyn Rule>> = vec![Box::new(LargeAmount)];
    println!("{rules:?}"); // Vec's Debug -> Box's Debug -> our `impl Debug for dyn Rule`
    println!("{}", rules[0]);
    let euros: f64 = Cents(12_550).into();
    println!("{euros}");
}
```

```text
[Rule(large-amount)]
rule large-amount
125.5
```

The third one matters most in practice: `impl From<MyType> for String` is how your types convert into std types.
[VERSION] Rust 1.41 (RFC 2451) relaxed the rule further, so a type parameter may precede the local type if a foreign
type *covers* it. `impl<T: From<i64>> From<Ledger> for Vec<T>` is legal, verified in listing `ch03-07-local-first.rs`.
The second shows that
`Box<dyn LocalTrait>` gets local treatment. `Vec<Box<dyn LocalTrait>>` does **not**, because `Vec` isn't fundamental.
The Part VI review's capstone trips on exactly that.

---

## Pass 2 · Systems level — *How rustc checks a global property one crate at a time*

### 4. Under the hood

**The precise orphan rule** [LANG] (the Reference, "Orphan rules"). Given `impl<P1..Pn> Trait<T1..Tn> for T0`, the impl
is allowed if `Trait` is local, **or** if both hold:

1. At least one of `T0..Tn` is a **local type**. Call the first one `Ti`.
2. No **uncovered** type parameter `P` appears in `T0..Ti`, excluding `Ti` itself. (Uncovered means not nested inside
   some other, non-fundamental type: in `Vec<P>`, `P` is covered, and in `P` or `Box<P>` it isn't.)

**Local types** are structs, enums, and unions defined in this crate, whatever their type arguments are
(`LocalType<ForeignType>` is local), and `dyn LocalTrait`. **Fundamental** type constructors, `&T`, `&mut T`, `Box<T>`,
and `Pin<P>`, are transparent to the rule: `Box<Local>` counts as local. That's why `Vec<Box<dyn Rule>>` fails (the
first type is `Vec<…>`, foreign and not fundamental), while `Box<dyn Rule>` passes. The note in the E0117 message,
"impl doesn't have any local type before any uncovered type parameters," is rule 2 speaking.

Why the "uncovered parameter" clause? Compare `impl<T: Into<i64>> Add<T> for Cents`, which is fine because `Cents` is
local and comes first (listing `ch03-07-local-first.rs` adds an `i32` and a `u8` to it, printing `13005`), with `impl<T> From<Cents> for
T`, which is rejected (listing `ch03-05-uncovered-param.rs`):

```text
error[E0210]: type parameter `T` must be covered by another type when it appears before the first local type (`Cents`)
3 | impl<T> From<Cents> for T {
  |      ^ uncovered type parameter
```

The rejected impl would implement `From<Cents>` for *every* type, including types in crates downstream of you, and those
crates' own impls would then collide with yours. (One trap nearby: `impl<T> From<T> for Cents` passes the orphan check
but collides with core's reflexive `impl<T> From<T> for T`, which already covers `From<Cents> for Cents`. That's E0119,
verified in `ch03-06-reflexive-from.rs`.)

**The overlap check runs in "intercrate mode."** [RUSTC] To decide whether two impls of the same trait can apply to one
type, the trait solver unifies their headers and checks the where-clauses. For types and traits outside the current
crate, it doesn't take the *current* absence of an impl as proof that none exists. It treats "a foreign crate might add
this impl in a future minor version" as possible. **Negative reasoning** ("`X` does not implement `Y`") is allowed only
where this crate controls the answer:

- `impl<T: Display> Render for T` plus `impl Render for Vec<u8>` is **rejected** (the debugging exercise). `Vec<u8>` isn't
  `Display` today, but std could add that impl, and then the two would overlap. rustc says exactly this: "upstream
  crates may add a new impl of trait `std::fmt::Display` for type `std::vec::Vec<u8>`."
- `impl Display for Box<dyn Rule>` above is **accepted**, even though std has
  `impl<T: Display + ?Sized> Display for Box<T>`. Those two overlap only if `dyn Rule: Display`. `dyn Rule` is local, so
  only this crate could add that impl, and it hasn't.

`#[fundamental]` is a *promise by the upstream crate*: adding a blanket impl for `Box<T>` would be a breaking change, so
downstream crates may reason as if `Box<Local>` were local.

**No specialization on stable** [VERSION]. Overlapping impls where one is "more specific" (`impl<T> Trait for T` plus
`impl Trait for u8`) are what *specialization* would allow. It has been unstable for years, blocked by soundness
problems with lifetimes, and std uses a restricted internal form (`min_specialization`). As of Rust 1.98, stable code
gets one impl per type, no overrides.

### 5. Memory

The workarounds are free at run time. A newtype around one field has the field's layout in practice [RUSTC]. With
`#[repr(transparent)]`, same layout *and* same calling convention are guaranteed [LANG], which matters when the value
crosses an FFI boundary (Part XVI). `Hex` is a 24-byte `Vec` header, and `Hex(vec)` compiles to no instructions beyond
the move. An extension trait adds nothing to any type. Coherence itself is compile-time only. There's no run-time
registry of impls to consult or to get wrong.

### 6. CPU / OS

Because each (trait, type) pair has one impl, every static trait call resolves to one address at compile time, and
every vtable for `dyn Trait` (Chapter 6.4) has one well-defined content. There's no lookup table at startup and no
ordering dependency. Compare two systems without coherence:

- **C++ templates and the One Definition Rule.** Two translation units may each define `std::hash<Uuid>` differently.
  That's ill-formed, *no diagnostic required*: the linker silently keeps one of the definitions, and the program's
  behavior is undefined. Rust's coherence is the ODR made checkable. The compiler refuses to let the second
  definition exist.
- **Runtime registries** (Java serializer modules, `ServiceLoader`, DI containers). Resolution happens at startup or
  first use, and conflicts are settled by registration or classpath order. The failure is a production behavior
  change, not a build error.

---

## Pass 3 · Architect level — *Where impls live decides your crate graph*

### 7. Trade-offs

**Workarounds compared:**

| Option | What it takes | Costs | Choose when |
|---|---|---|---|
| **Newtype** `struct Hex(Vec<u8>)` | Wrap at boundaries, unwrap to use | Friction at every boundary. The wrapper hides the inner API unless you re-expose it (forwarding methods, `Deref`, `AsRef`) | You need a *foreign trait* (`Display`, `Serialize`, `Hash`) on a foreign type, or you want to add invariants anyway (Part V) |
| **Extension trait** `trait Checksum for [u8]` | `use` the trait where you call it | Only works for traits *you* define, so no help with `Display`/`Serialize` | You want new *methods* on foreign types |
| **Upstream impl behind a Cargo feature** | A PR to the type's or trait's crate | Their maintenance burden, their release schedule | The impl is broadly useful (the ecosystem's `serde` feature pattern) |
| **Pass behavior as a value** | `sort_by(cmp)`, `HashMap::with_hasher(h)`, a formatter struct | More parameters | You need *several* behaviors for one type (two orderings, two formats) |

**`Deref` on a newtype: a view or a leak?** `Hex: Deref<Target = [u8]>` is fine: `Hex` is a presentation wrapper with
no invariants, and read-only access to its bytes can't break anything. `struct TenantId(String)` with
`Deref<Target = String>` is a leak. Every `String` method is now a `TenantId` method, the type stops being a boundary,
and adding `DerefMut` would let callers mutate a validated identifier into an invalid one. Newtypes that carry
invariants (Part V) should expose explicit methods: `as_str()`, not `Deref`.

**Sealed traits: closing the set of impls on purpose.** Chapter 5.4 closed its set of payment states with a sealed trait
(listing `listings/part-05/ch04-10-sealed.rs`, verified there): `pub trait State: sealed::Sealed`, where `Sealed` is a
public trait inside a *private* module. Why it works: an impl of `State` for any type creates the obligation
`Type: Sealed` (a supertrait is a where-clause on `Self`, Chapter 6.2 §4). Code outside the module can't *name* `Sealed`
to write that impl, so the obligation can never be met, and rustc's E0277 even calls the idiom a "sealed trait" by name.
The seal is enforced by **privacy**, not by the orphan rule. Coherence is what makes it pay off: because the complete
set of impls is known to the defining crate, that crate can add methods, even required ones, without a breaking change,
and can reason about every implementor when it changes the trait. Seal traits that are *interfaces to your crate*
(inputs you consume) and leave open the ones that are *extension points* (inputs others provide).

**The architectural consequence: impls live with the trait or the type.** No third crate can connect a trait from crate
A to a type from crate B. So:

- Crates that define widely used **types** grow optional features for widely used **traits** (`uuid` with
  `features = ["serde"]`, `chrono` with `serde`). The ecosystem's feature flags are coherence's shadow.
- In your own workspace, put shared **traits** in a low-level crate everyone depends on. Put impls either next to the
  types or next to the traits, never in a "glue" crate, which can't hold them.
- Cargo's **feature unification** means an impl behind a feature can appear because *some other* dependency enabled
  that feature, and disappear when that dependency is removed. If you rely on an impl, enable its feature yourself.

### 8. Java comparison

Java never has an orphan problem, because Java never lets you add an interface to an existing class. `java.util.UUID`
implements exactly what its authors declared. Everything else is an **adapter**: Rust's newtype, written as a wrapper
class, with the same boundary friction and allocation on top.

What Java has instead are **runtime** coherence problems:

| Java mechanism | Where uniqueness is decided | Failure mode |
|---|---|---|
| `equals`/`hashCode` | At the class declaration (one per class, by construction) | A subclass that breaks the contract. `HashMap` misbehaves at run time |
| Jackson serializer modules | Registration order at `ObjectMapper` setup | Two modules customize `UUID` differently. Which serializer wins depends on registration order |
| `ServiceLoader` / DI containers | Classpath and scan order at startup | Two implementations of an SPI on the classpath. Which one is used can change with a dependency bump |
| Guava `Equivalence`, `Comparator` | Passed as values | None: this is the "behavior as a value" row of §7's table |

Rust's `Hash`/`Eq` coherence gives `HashMap` the same guarantee Java's `equals`/`hashCode` gives, one definition per
type, but it's enforced at compile time across crates, including for types you don't own.

> **Analogy limit.** It's tempting to see an extension trait as a Kotlin or C# *extension method*. The call syntax is
> similar. The semantics aren't. An extension trait is a real trait: generic code can require it
> (`fn f<T: Checksum + ?Sized>`), it can be used as `dyn Checksum`, and its methods participate in coherence. A C#
> extension method is a static method with call-site sugar, invisible to generics and interfaces. Haskell is the closer
> relative. It calls these impls *orphan instances* and only *warns*. Rust makes them errors.

### 9. Production scenario

**Exporting connection-pool metrics.** Meridian's services use a third-party Postgres pool crate (call it `pgpool`,
with a `PoolStatus { size, idle, waiting }` type) and a third-party metrics crate whose `Collector` trait makes
something scrapeable. The obvious impl, `impl Collector for pgpool::PoolStatus`, is an orphan: both sides are foreign.
The platform team weighed the options from §7:

1. **Newtype `PoolCollector(Arc<Pool>)`** implementing `Collector`: reads `pool.status()` at scrape time and emits three
   gauges. It works today, it's owned by Meridian, and it's 40 lines.
2. **Extension trait `PoolMetricsExt`** with `fn register_metrics(&self, registry: &Registry)`: a nicer call site
   (`pool.register_metrics(&registry)`), but it still needs a local type to implement `Collector`, so it wraps option 1.
3. **Upstream:** open a PR adding `features = ["metrics"]` to `pgpool`, with the impl there. That's the right long-term
   home, but the metrics crate is a heavy dependency for the pool's authors to take on.

They shipped options 1 and 2, with the extension trait as the public face and the newtype as its implementation, and
opened the upstream issue. The review comment that settled it: *"We can't write this impl in a shared glue crate
either. Coherence leaves exactly two homes for it, and we own neither, so the wrapper is the honest design."*

### 10. Failure scenario

**Two `Uuid`s that aren't the same type.** Meridian's audit library implements its `AuditKey` trait for `uuid::Uuid`,
from `uuid` 0.8, the version it was written against. A service on `uuid` 1.x passes its `Uuid` to the audit API and
gets:

```text
error[E0277]: the trait bound `Uuid: AuditKey` is not satisfied
```

The type *looks* identical, but `uuid 0.8` and `uuid 1.x` are **different crates** to rustc. Semver-incompatible
versions can coexist in one build, and their types are distinct, so impls written for one don't apply to the other.
Coherence isn't at fault. It's working on precise information: the impl exists, for a different type. Recent rustc
versions add a note pointing out that multiple versions of the crate are in the dependency graph.

(Not verified here: this needs a Cargo workspace with two versions of one crate, which the Playground can't provide.
To reproduce locally, depend on `uuid = "0.8"` in a library crate that implements a trait for `uuid::Uuid`, and on
`uuid = "1"` in the binary that calls it. `cargo tree -d` lists the duplicated crates.)

The fixes are organizational, and they're why workspaces pin shared dependencies:

- Align versions with `[workspace.dependencies]`, and check for duplicates in CI with `cargo tree -d` or `cargo deny`
  (Part XXII).
- A library whose *public API* exposes a third-party type (`Uuid` in an `AuditKey` impl) has made that crate's major
  version part of its own API. Bumping it is a breaking change for the library too.
- Where possible, expose your own newtype (`AuditId`) at API boundaries, with conversions from the external types, so
  the dependency's version becomes an internal detail.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part VI).*

1. What is coherence, and what would break without it? Give a `HashMap` example.
2. State the orphan rule precisely. Why does it guarantee coherence while checking only one crate at a time?
3. Why is `impl Display for Box<dyn Rule>` allowed while `impl Display for Vec<Box<dyn Rule>>` is not?
4. Why is `impl From<Cents> for f64` legal? What did RFC 2451 (Rust 1.41) add, with `impl<T> From<Ledger> for Vec<T>`,
   and why is a *covered* type parameter safe where an uncovered one isn't?
5. The compiler rejects `impl<T: Display> Render for T` plus `impl Render for Vec<u8>` even though `Vec<u8>` isn't
   `Display`. Explain the reasoning, and why it's the right call.
6. Newtype, extension trait, or upstream feature: give a scenario where each is the best choice.
7. Why do so many crates have a `serde` feature? What does that say about coherence's effect on the ecosystem?
8. Why is a newtype with `Deref` to its inner type sometimes fine and sometimes a design bug?

### 12. Exercises

- **Beginner.** Make `impl Display for Vec<u8>` compile two ways: a newtype and an extension trait with a `to_hex()`
  method. Which one lets you use `{}` in `format!`?
- **Intermediate.** Without looking back at §4, predict the verdict on `impl<T> From<Cents> for T` and on
  `impl<T: Into<i64>> Add<T> for Cents`, using the "uncovered type parameter" clause. Then check both on the
  Playground.
- **Advanced.** Write `trait Redact { fn redacted(&self) -> String; }` with a blanket
  `impl<T: Debug> Redact for T`, then try a specific impl for a local type that isn't `Debug`. Does it overlap? Now
  derive `Debug` on it. What changed, and why is that the right behavior?
- **Systems.** Add `#[repr(transparent)]` to `Hex`. Using `size_of` and `align_of`, confirm the layout matches
  `Vec<u8>`. What *additional* guarantee does `repr(transparent)` give that the default repr doesn't, and when does it
  matter?
- **Architecture.** Sketch Meridian's crate graph for types (`meridian-types`), traits (`meridian-audit`), storage
  (`meridian-db`), and three services. For each impl you need (`AuditKey for OrderId`, `Serialize for OrderId`,
  `ToSql for OrderId`), name the only crates that could contain it.

### 13. Debugging exercise

A rendering helper with a blanket impl and one special case (listing `ch03-03-future-compat.rs`):

```rust,compile_fail
use std::fmt::Display;

trait Render {
    fn render(&self) -> String;
}

// Blanket impl for every Display type...
impl<T: Display> Render for T {
    fn render(&self) -> String {
        self.to_string()
    }
}

// ...plus a specific impl for Vec<u8>. Vec<u8> is NOT Display today. Is this allowed?
impl Render for Vec<u8> {
    fn render(&self) -> String {
        format!("{} bytes", self.len())
    }
}

fn main() {
    println!("{}", vec![1u8, 2, 3].render());
}
```

```text
error[E0119]: conflicting implementations of trait `Render` for type `Vec<u8>`
 9 | impl<T: Display> Render for T {
   | ----------------------------- first implementation here
...
16 | impl Render for Vec<u8> {
   | ^^^^^^^^^^^^^^^^^^^^^^^ conflicting implementation for `Vec<u8>`
   = note: upstream crates may add a new impl of trait `std::fmt::Display` for type `std::vec::Vec<u8>` in future versions
```

1. The two impls don't overlap today. Explain, in terms of *negative reasoning*, why the compiler still rejects them.
2. What would happen to this program, and to every program with this pattern, if the compiler accepted it and a later
   std added `impl Display for Vec<u8>`?
3. Give two fixes: one that keeps the blanket impl and one that doesn't. Then say why `impl Render for Hex` (the newtype
   from §3) is accepted alongside the blanket, even if `Hex` isn't `Display`.

### 14. Design exercise

**Where do the codecs live?** Return to Chapter 6.2's design exercise: 40 message types, two wire formats, a third
format next year. Using the orphan rule, decide which crate owns each trait and each impl, in these layouts:

- (a) Message types in `meridian-messages`, codec traits in `meridian-codec`, formats in `meridian-json` and
  `meridian-binary`.
- (b) The same, but `meridian-codec` is a third-party crate you don't control.

In each layout, which impls are impossible, and what restructuring (newtypes, extension traits, a feature flag,
moving a trait) fixes it? Which layout lets the third format ship without touching `meridian-messages`?
