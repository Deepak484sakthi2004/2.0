# Chapter 2.7 — Modules, Visibility, and Crate Architecture

> **Where this sits:** Part II · Rust From First Principles · chapter 7 of 7
> **Prerequisites:** Chapters 2.1 and 2.6.
> **After this chapter you can:** lay out a crate's module tree; use Rust's visibility levels precisely; explain why
> privacy is the boundary that protects invariants (including memory-safety invariants); design a public API facade;
> and structure a Cargo workspace so that the crate graph enforces your architecture.

---

## Pass 1 · User level — *Who can see what*

### 1. Problem

Every invariant in a program ("balance never drops below the overdraft limit," "`len <= capacity`," "an order's items
are frozen once placed") is only as strong as the set of code that can break it. If any code anywhere can write the
field, the invariant is a hope. If only the type's own module can write it, the invariant is a property of a few hundred
lines you can review.

Java gives you `private`, package-private, `protected`, `public`, JPMS module `exports`, and, for layering rules
between packages, external tools like ArchUnit that run as tests. Rust gives you a **module tree with privacy by
default**, precise visibility modifiers, and a **crate graph that must be acyclic**. Together they can enforce
architecture *at compile time*.

### 2. Mental model

```text
 crate root (src/main.rs or src/lib.rs)
 ├── mod billing                       ← private module: visible to the crate root and its descendants
 │   ├── const MAX_DISCOUNT_BPS        ← private: visible in `billing` and everything under it
 │   ├── pub mod money
 │   │   └── pub struct Cents(i64)     ← public type, PRIVATE field (only `money` can touch .0)
 │   ├── pub struct Invoice { pub customer, total, discount_bps }
 │   └── pub(crate) fn audit_line      ← visible anywhere in this crate, never outside it
 └── pub use billing::Invoice          ← a re-export: the public name doesn't have to match the internal path
```

**The one rule:** an item is visible to code *inside* its **visibility scope**. With no modifier, that scope is **the
module where the item is declared, and all of that module's descendants**.

| Modifier | Visible to |
|---|---|
| *(none)* | The current module and its descendants |
| `pub(super)` | The parent module (and its descendants) |
| `pub(in crate::path)` | The named ancestor module (and its descendants) |
| `pub(crate)` | The whole crate, but not other crates |
| `pub` | Everyone who can reach the item through public paths |

Two consequences surprise Java engineers:

1. **Children see their ancestors' private items. Parents don't see their children's.** Privacy flows *downward* into
   nested modules, which encourages putting shared private helpers *higher* in the tree and implementation details
   *lower*.
2. **Field visibility is separate from type visibility.** `pub struct Cents(i64)` makes the type public and leaves the
   field private. Outside code can hold, pass, and compare a `Cents`, but it can neither read nor construct the inner
   `i64` directly.

**Files are storage, not modules.** A module exists because some parent declared it with `mod`. `mod billing;` in the
crate root tells the compiler to load `src/billing.rs` (or the older `src/billing/mod.rs`). A submodule `money` inside
it lives in `src/billing/money.rs`. A file nobody declares with `mod` is simply never compiled. That's a classic
first-week confusion.

### 3. Rust code

A billing module whose privacy structure protects its invariants (verified):

```rust
mod billing {
    // Private by default: visible inside `billing` and its descendants only.
    const MAX_DISCOUNT_BPS: u32 = 5_000;

    pub mod money {
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Cents(i64); // public type, PRIVATE field: the only way in is a constructor

        impl Cents {
            pub fn from_i64(v: i64) -> Option<Cents> {
                if v >= 0 { Some(Cents(v)) } else { None }
            }
            pub fn get(self) -> i64 {
                self.0
            }
            // Visible only inside `crate::billing`: trusted code may skip the check.
            pub(in crate::billing) fn raw(v: i64) -> Cents {
                Cents(v)
            }
        }
    }

    pub struct Invoice {
        pub customer: String, // public: plain data
        total: money::Cents,  // private: guarded by methods
        discount_bps: u32,
    }

    impl Invoice {
        pub fn new(customer: &str) -> Invoice {
            Invoice { customer: customer.to_string(), total: money::Cents::raw(0), discount_bps: 0 }
        }
        pub fn add_line(&mut self, amount: money::Cents) {
            self.total = money::Cents::raw(self.total.get() + amount.get());
        }
        pub fn apply_discount(&mut self, bps: u32) -> Result<(), String> {
            if bps > MAX_DISCOUNT_BPS {
                // a child module reading its parent's private item: allowed
                return Err(format!("discount {bps} bps exceeds cap"));
            }
            self.discount_bps = bps;
            Ok(())
        }
        pub fn due(&self) -> money::Cents {
            let t = self.total.get();
            money::Cents::raw(t - t * self.discount_bps as i64 / 10_000)
        }
    }

    // Visible anywhere in this crate, never outside it.
    pub(crate) fn audit_line(inv: &Invoice) -> String {
        format!("{}: due {} cents", inv.customer, inv.due().get())
    }
}

// A facade: re-export what callers should use; hide the internal module layout.
pub use billing::money::Cents;
pub use billing::Invoice;

fn main() {
    let mut inv = Invoice::new("acme");
    inv.add_line(Cents::from_i64(12_000).unwrap());
    inv.add_line(Cents::from_i64(3_000).unwrap());
    println!("{:?}", inv.apply_discount(9_000));
    inv.apply_discount(1_000).unwrap();
    println!("{}", billing::audit_line(&inv));
    println!("negative amount: {:?}", Cents::from_i64(-5));
}
```

```text
Err("discount 9000 bps exceeds cap")
acme: due 13500 cents
negative amount: None
```

The guarantees this structure buys, each enforced by the compiler:

- **No negative `Cents` from outside `billing`.** `from_i64` is the only public way to make one, and it checks.
  `raw` skips the check, but it's `pub(in crate::billing)`, trusted code only.
- **No `Invoice` with an arbitrary total.** `total` is private, so outside code can't read it, write it, or *construct
  an `Invoice` literal* with it.
- **The discount cap is enforced in one place**, `apply_discount`, because it's the only code that can write
  `discount_bps`.

What the compiler says when outside code tries (three separate verified listings):

```text
error[E0616]: field `total_cents` of struct `Invoice` is private
   |     println!("{} owes {}", inv.customer, inv.total_cents);
   |                                              ^^^^^^^^^^^ private field

error[E0451]: field `total_cents` of struct `Invoice` is private
   |     let forged = billing::Invoice { customer: "x".into(), total_cents: -1 };
   |                                                           ^^^^^^^^^^^ private field

error[E0603]: function `post_entry` is private
   |     println!("{}", ledger::post_entry(5));
   |                            ^^^^^^^^^^ private function
note: the function `post_entry` is defined here
```

E0451 is the important one. **Building a struct literal requires access to every field.** One private field is enough
to make `new` (or whatever associated functions you provide) the *only* door into the type.

---

## Pass 2 · Systems level — *Where privacy lives in the compiler*

### 4. Under the hood

**Resolution, then privacy.** [RUSTC] After macro expansion, **name resolution** builds the module tree and resolves
every path and `use` import. Privacy is then checked at several points. Field-access privacy is checked during type
checking (it needs to know the type of `inv` to find the field), and a separate **privacy pass** checks items and struct
literals.

> **What actually happens?** While verifying this chapter's listings, a first version put the E0616 mistake (reading a
> private field) and the E0451 mistake (a struct literal with a private field) in the *same* file. rustc reported
> **only E0616**. The type-checking error stopped compilation before the later privacy pass ran, so E0451 was never
> reported. The listings are now separate files. The lesson is general: **the set of errors you see depends on which
> compiler phase failed first**, so fixing one error can reveal "new" ones that were there all along.

**Modules have no runtime existence.** [RUSTC] A module shapes two things in the output: **symbol names** (mangling
encodes the path, so `audit_line` becomes a symbol containing `playground`, `billing`, and `audit_line`, as seen in the
assembly listings throughout this Part), and **linkage**. Items that can't be reached from outside the crate can be
given internal linkage, which lets LLVM inline them freely, specialize them, or delete them. `pub` items of a library
are recorded in its metadata so other crates can use them. In a `cdylib` (a C-compatible shared library, Part XVI), only
`#[no_mangle] extern "C"` functions are exported symbols.

**The dead-code lint follows reachability.** An item unreachable from the crate's *public API* and unused inside it
gets a `dead_code` warning. In a library, `pub` items reachable from the root are never flagged, because another crate
might use them. In a binary, everything is internal, so unused `pub` items are flagged too.

**Privacy is compile-time only, and there's no way around it.** [LANG] Rust has no reflection. No run-time mechanism
reads a private field by name. Java's `setAccessible(true)` has no counterpart. (Since JDK 16/17, strong encapsulation
blocks it too for packages a module doesn't open, but inside the classpath world it's routine.) The only way to reach a
private field from outside is `unsafe` code that reinterprets memory, and code that does that to break a library's
invariant is **unsound** by definition.

**That's why privacy is part of memory safety.** Chapter 1.3 said Vec's soundness rests on invariants like `len <= cap`
held in private fields. If those fields were public, *safe* code could set `len` to a billion and the next `v[i]` would
read out of bounds, with no `unsafe` keyword anywhere in the caller. **Privacy is what lets a module's `unsafe` code
trust its own fields.** It's the boundary Part XV calls the *safety boundary*.

**Crate boundaries and inlining.** [RUSTC] A **generic** function from another crate is instantiated in *your* crate
(Chapter 2.1), so it can always be inlined. A **non-generic** function from another crate is normally just a call to
the other crate's compiled code. It can't be inlined unless it's marked `#[inline]` (which ships its MIR in the
metadata), it's small enough for rustc's automatic cross-crate inlining (since 1.75; Chapter 1.3), or you enable LTO
(Chapter 2.2). For a library's hot-path API, like a small accessor called in a tight loop, `#[inline]` can matter.
Measure before you sprinkle it.

### 5. Memory

Modules, visibility, and re-exports cost **nothing at run time**: no tables, no metadata in the process, no
indirection. `pub use` re-exports are compile-time aliases. The architectural decisions in this chapter are free in
bytes and CPU. They cost only in compile structure and API surface.

### 6. CPU / OS

- **Symbol visibility** matters for shared libraries. Exported symbols add to dynamic-linking work at load time and
  are part of your binary interface. A `cdylib` built from Rust exports only what you mark, and the Rust ABI is never
  exported in a stable form (Part XVI).
- **Codegen-unit partitioning** [RUSTC] tends to group items by module, so how you split a crate into modules can
  (slightly) affect which functions end up in the same unit and get inlined without LTO. It's rarely worth designing
  around. Settle it with `codegen-units = 1` in release (Chapter 2.2) instead.
- **Stripping**: private functions that were all inlined leave no symbol at all, so a stripped release binary exposes
  almost nothing about your module structure. That's convenient for size and occasionally inconvenient for
  profiling. Keep line tables (Chapter 2.2).

---

## Pass 3 · Architect level — *Boundaries that the compiler enforces*

### 7. Trade-offs

**Organizing modules: by layer or by feature?**

| | By layer (`handlers/`, `services/`, `repositories/`) | By feature (`billing/`, `accounts/`, `orders/`) |
|---|---|---|
| Privacy | Everything that cooperates across layers must be `pub(crate)` | A feature's internals stay private to its module |
| Change locality | A feature change touches every layer directory | A feature change stays in one directory |
| Invariant protection | Weak: the fields must be visible to "the service layer" | Strong: the type and the code that maintains it live together |
| Rust recommendation | Rarely a good fit | **Preferred**, with a thin facade at the crate root |

**Crate boundaries as architecture enforcement.** Cargo requires the crate dependency graph to be **acyclic**, so a
workspace can express *allowed dependency directions*, and the compiler rejects anything else:

```text
             ┌──────────────────────┐
             │ gateway (binary)     │   wires everything: config, runtime, main
             └──┬───────────────┬───┘
                │               │
     ┌──────────▼───┐     ┌─────▼────────────┐
     │ gateway-io   │     │ gateway-proto    │   adapters: Tokio/Hyper, parsers
     └──────┬───────┘     └─────┬────────────┘
            │                   │
            └─────────┬─────────┘
               ┌──────▼──────┐
               │ gateway-core│   domain: routing rules, rate-limit policy.
               └─────────────┘   NO dependency on tokio, hyper, or any I/O crate.
```

If someone imports `tokio` into `gateway-core`, they have to edit its `Cargo.toml` to add the dependency, a change that
shows up plainly in review. And `gateway-core` can never import `gateway-io` at all, because that would create a cycle.
Java teams get the same rule with ArchUnit tests or JPMS modules. Rust gets it from the build graph for free. The
costs: more manifests, the orphan rule (Part VI) limiting which traits you can implement for which types across crates,
and every cross-crate item needing `pub`, which widens the surface you have to keep stable.

**Every `pub` is a promise.** In a library, anything reachable publicly is part of your **semver contract**. Removing
it, renaming it, or changing its signature is a breaking change. That makes `pub(crate)` the right default for anything
internal, and a facade of `pub use` re-exports lets you reorganize modules without breaking callers. Tools like
`cargo-semver-checks` compare two versions of a crate's public API and flag breaking changes before release.

**Public dependencies.** If your public API mentions another crate's type (`pub fn client() -> reqwest::Client`), that
crate's **major version is now part of your API**. Upgrading `reqwest` to a new major version becomes a breaking change
for your users even though your own code didn't change. Wrap foreign types behind your own types at API boundaries
unless exposing them is the whole point of the library.

### 8. Java comparison

| Java | Rust | Note |
|---|---|---|
| Package (a flat namespace) | Module (a node in a tree) | Java's `com.acme.billing.money` has *no* special access to `com.acme.billing`. Rust's `billing::money` can see `billing`'s private items. |
| Default (package-private) access | Default (module-private) access | Similar in spirit. Rust's includes descendants. |
| `private` (the class only) | No direct equivalent; privacy is per module, not per type | Two types in one module see each other's private fields. |
| `protected` | None | There's no inheritance to protect for. |
| JPMS `exports com.acme.billing` | `pub` items reachable from the crate root | The crate root is the module descriptor. |
| JPMS `requires transitive` | A public dependency / `pub use` of another crate | Both mean the dependency is part of your API. |
| Reflection, `setAccessible` | None | Privacy can't be bypassed at run time. |
| ArchUnit layering tests | The crate graph (acyclic, compile-time) | Enforced by the build, not by a test you might skip. |
| One public top-level class per file | Any number of items per file; files map to modules | — |

> **Analogy limit.** "A Rust module is a Java package" holds for namespacing. It fails for access control in two
> places. Java's `private` is per *class*, while Rust's privacy is per *module*, so two structs in one module can see
> each other's private fields. And Java packages don't nest for access, while Rust modules do: children see their
> parents' private items.

### 9. Production scenario

**Meridian's gateway workspace**, shown in §7, in practice:

- **`gateway-core`** holds routing rules, rate-limit policies, and the tenant model: pure logic with no I/O and no
  async. It's tested with ordinary unit tests in milliseconds, and it compiles without Tokio, which also keeps it quick
  to build.
- **`gateway-proto`** holds zero-copy parsers for the header formats (Chapters 2.4–2.5). It depends on `gateway-core`
  for the types parsed values turn into. It's fuzzed separately (Part XXII).
- **`gateway-io`** holds the Tokio and Hyper adapters. It's the only crate with network dependencies.
- **`gateway`** is the binary: configuration, startup, wiring, signal handling.

The effect: when the rate-limit policy changes, only `gateway-core` and its dependents rebuild. Security review of the
parsers is scoped to one crate. And the question "can domain logic block on I/O?" has a structural answer, because
`gateway-core` has no I/O dependencies to block on.

### 10. Failure scenario

**The field made public "temporarily."** A data-migration script needed to fix balances, so someone changed
`balance_cents` to `pub` "just for the migration":

```rust
mod accounts {
    #[derive(Debug)]
    pub struct Account {
        pub balance_cents: i64, // made `pub` "temporarily" for a data-migration script
        overdraft_limit: i64,
    }

    impl Account {
        pub fn new(overdraft_limit: i64) -> Account {
            Account { balance_cents: 0, overdraft_limit }
        }

        /// Invariant: balance_cents >= -overdraft_limit
        pub fn withdraw(&mut self, cents: i64) -> Result<(), String> {
            if self.balance_cents - cents < -self.overdraft_limit {
                return Err("insufficient funds".to_string());
            }
            self.balance_cents -= cents;
            Ok(())
        }

        pub fn invariant_holds(&self) -> bool {
            self.balance_cents >= -self.overdraft_limit
        }
    }
}

fn main() {
    let mut acct = accounts::Account::new(5_000);
    println!("{:?}", acct.withdraw(10_000)); // rejected by the guarded path...
    acct.balance_cents -= 10_000; // ...and bypassed through the public field
    println!("{acct:?}, invariant holds: {}", acct.invariant_holds());
}
```

```text
Err("insufficient funds")
Account { balance_cents: -10000, overdraft_limit: 5000 }, invariant holds: false
```

The migration shipped, the `pub` stayed, and eight months later a refund feature wrote `acct.balance_cents -= amount`
directly because it compiled and was simpler than calling `withdraw`. Accounts went past their overdraft limits, and
reconciliation found it a month after that. The compiler had been enforcing the invariant, and one keyword switched that
off for the whole codebase.

The fixes: keep the field private and give the migration a dedicated, clearly named, narrowly visible function
(`pub(crate) fn migrate_set_balance(...)`, or a separate migration binary that goes through `withdraw`/`deposit`). And
treat **any widening of visibility on a type with invariants as a design-review item**, in the same category as adding
`unsafe`.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part II).*

1. What exactly does "private" mean in Rust? How does it differ from Java's `private` and package-private?
2. Why can a child module read its parent's private items, but not the other way round? What code organization does
   that encourage?
3. Why does building a struct literal require every field to be visible? How does that make `new` the only door?
4. How is privacy part of *memory safety* and not just encapsulation? Use `Vec` as the example.
5. What is a facade (`pub use`), and why decouple a crate's public API from its module layout?
6. How can a Cargo workspace enforce a hexagonal architecture? What does it cost?
7. What is a "public dependency," and why does exposing `reqwest::Client` in your API matter for semver?
8. When might a hot function in a library crate need `#[inline]`, and when is it unnecessary?

### 12. Exercises

- **Beginner.** Split the billing listing into files (`src/main.rs`, `src/billing.rs`, `src/billing/money.rs`) and make
  it compile. Then add a file `src/unused.rs` without a `mod` declaration and confirm it's never compiled. (Put a
  deliberate syntax error in it.)
- **Intermediate.** Add a `pub(super)` function to `money` that only `billing` can call, and show a call from `main`
  failing with the right error code.
- **Advanced.** Design a library crate with a `prelude` module (`pub use` of the ten most common items). Discuss the
  risks of glob imports (`use mylib::prelude::*`) when two preludes export the same name, and how Rust resolves such
  conflicts.
- **Systems.** Build a release binary and list its symbols (`nm -C` or `objdump -t`, on Linux). Which of your functions
  survive as symbols? Where do the module paths appear? Then build a `cdylib` with one `#[no_mangle] pub extern "C"`
  function and compare its exported symbols (`nm -D`).
- **Architecture.** Draw the crate graph for a service you know, arranged hexagonally. For each edge, write the one-line
  rule it enforces. Then list what the Rust version *can't* enforce with crates alone (for example, "no blocking calls
  in async code").

### 13. Debugging exercise

Three listings, three errors: E0616, E0451, and E0603 (§3).

1. For each, state which rule was broken, in terms of the "visibility scope" rule from §2.
2. The first draft of these listings combined the E0616 and E0451 mistakes in one file, and rustc reported only E0616.
   Explain why, in terms of compiler phases. What does that imply when you "fix the only error" in a large change?
3. For E0451: the author's intent was a test fixture that builds an `Invoice` with a specific total. Give two
   legitimate ways to support that without making `total_cents` public (hint: `#[cfg(test)]`, and where test modules
   live in the module tree).

### 14. Design exercise

**Module and crate design for `logstat`** (Project Level 1, next). The tool has argument parsing, line parsing, a
statistics engine, and report formatting. Later you'll want to (a) reuse the parser and statistics engine from a Tokio
service that tails logs over the network, and (b) add a JSON output format.

Propose: the module tree for a single-crate version, what's `pub` vs `pub(crate)` vs private, and at what point (and
along which lines) you'd split it into a workspace of crates. For each boundary, say which invariant or dependency rule
it protects. Then compare your design with the one in the project chapter.
