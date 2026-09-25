# Chapter 6.1 — Traits, Bounds, and Default Methods

> **Where this sits:** Part VI · Traits · chapter 1 of 5
> **Prerequisites:** Parts II–IV. Chapter 2.6 (method-resolution preview), Chapters 3.3–3.4 (parameter types, deref
> coercion).
> **After this chapter you can:** define traits with required and default methods; write bounds three ways and choose
> between them; explain why Rust checks generic code where it's *defined*, not where it's *used* (E0369); trace method
> resolution through the auto-deref chain, including your own `Deref` types; use `AsRef` and `Into` for flexible
> parameters and know what each costs (measured); disambiguate with fully qualified syntax; and spot a default method
> that's a trap.

---

## Pass 1 · User level — *A contract, separate from the type*

### 1. Problem

Meridian's API gateway enforces per-key rate limits. Two algorithms are in production: a **token bucket** for API keys
that may burst, and a **fixed window** for partner quotas. More are coming (sliding log, a global concurrency cap). The
code that *uses* a limiter (the admission path, the load simulator, the logging) shouldn't care which algorithm it has.

A Java engineer reaches for an interface, and the Rust answer is a **trait**. The resemblance is real, and so are three
differences that change how you design:

1. **Implementation is separate from the type.** An `impl Trait for Type` block can live apart from the type's
   definition. You can implement your trait for types you didn't write, including `u32` and `Vec<T>`, within limits
   (the orphan rule, Chapter 6.3).
2. **One trait, two dispatch models.** The same trait supports compile-time (static) polymorphism through generics and
   run-time (dynamic) polymorphism through `dyn Trait`. The *caller* chooses, per use (Chapters 6.4–6.5).
3. **Generic code is checked against its bounds, once.** A generic function may use only what its bounds promise. That
   makes errors appear at the definition, not deep inside some caller's instantiation.

### 2. Mental model

A **trait** is a named contract: methods (required or defaulted), plus associated types and constants (Chapter 6.2).
An **impl** is *evidence* that a type satisfies the contract. A **bound** (`L: RateLimiter`) is a requirement on a type
parameter: "whatever `L` turns out to be, there must be evidence."

```text
 trait RateLimiter                  the contract: try_acquire (required); name, admit_batch (defaults)
        ▲                 ▲
 impl … for TokenBucket   impl … for FixedWindow        evidence, written separately from the types
        ▲                 ▲
 fn simulate<L: RateLimiter>(limiter: &mut L, …)        a bound: any L with evidence;
                                                         the body may use ONLY what the contract says
```

The compiler conceptually keeps a table of *(trait, type) → impl*. Every trait-method call on a generic `L` is a lookup
in that table, done at compile time. [LANG] Rust guarantees the table has at most one entry per pair, across every
crate in the program. That property, **coherence**, is Chapter 6.3.

There are three spellings of a bound, all static dispatch [LANG]:

| Spelling | Example | Notes |
|---|---|---|
| Inline | `fn simulate<L: RateLimiter>(l: &mut L)` | A named parameter. Callers may write `simulate::<TokenBucket>(…)` |
| `where` clause | `fn f<L>(l: &mut L) where L: RateLimiter + Send` | Same meaning. Use it for long bounds, bounds on other types (`Vec<T>: Debug`), or bounds on associated types |
| `impl Trait` in argument position | `fn describe(l: &impl RateLimiter)` | An *anonymous* type parameter: it can't be named or turbofished |

A **default method** is a method with a body in the trait. It may call the required methods, which makes it the
template-method pattern without inheritance. An implementor may override it.

### 3. Rust code

The limiter contract, two algorithms, and two generic consumers (verified):

```rust
/// A rate-limiting algorithm. Implementors provide `try_acquire`; everything else has a default.
trait RateLimiter {
    /// Try to admit one request at time `now_ms`. Returns true if admitted.
    fn try_acquire(&mut self, now_ms: u64) -> bool;

    /// A human-readable name for logs. Default: a generic label.
    fn name(&self) -> &str {
        "limiter"
    }

    /// Admit up to `n` requests; a DEFAULT METHOD built on the required one (template method).
    fn admit_batch(&mut self, n: u32, now_ms: u64) -> u32 {
        (0..n).filter(|_| self.try_acquire(now_ms)).count() as u32
    }
}

/// Token bucket: `capacity` tokens, refilled at `per_sec` tokens per second.
struct TokenBucket {
    capacity: f64,
    tokens: f64,
    per_sec: f64,
    last_ms: u64,
}

impl RateLimiter for TokenBucket {
    fn try_acquire(&mut self, now_ms: u64) -> bool {
        let elapsed = (now_ms - self.last_ms) as f64 / 1000.0;
        self.tokens = (self.tokens + elapsed * self.per_sec).min(self.capacity);
        self.last_ms = now_ms;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn name(&self) -> &str {
        "token-bucket" // overrides the default
    }
}

/// Fixed window: at most `limit` requests per `window_ms`.
struct FixedWindow {
    limit: u32,
    window_ms: u64,
    window_start: u64,
    used: u32,
}

impl RateLimiter for FixedWindow {
    fn try_acquire(&mut self, now_ms: u64) -> bool {
        if now_ms - self.window_start >= self.window_ms {
            self.window_start = now_ms;
            self.used = 0;
        }
        if self.used < self.limit {
            self.used += 1;
            true
        } else {
            false
        }
    }
    // `name` and `admit_batch` use the defaults
}

/// A trait bound: works for ANY RateLimiter, resolved at compile time.
fn simulate<L: RateLimiter>(limiter: &mut L, requests_per_tick: u32) -> Vec<u32> {
    (0..5).map(|tick| limiter.admit_batch(requests_per_tick, tick * 250)).collect()
}

/// The same bound written with `impl Trait` in argument position.
fn describe(limiter: &impl RateLimiter) -> String {
    format!("[{}]", limiter.name())
}

fn main() {
    let mut bucket = TokenBucket { capacity: 4.0, tokens: 4.0, per_sec: 4.0, last_ms: 0 };
    let mut window = FixedWindow { limit: 4, window_ms: 1000, window_start: 0, used: 0 };
    println!("{} admitted per 250 ms tick: {:?}", describe(&bucket), simulate(&mut bucket, 3));
    println!("{} admitted per 250 ms tick: {:?}", describe(&window), simulate(&mut window, 3));
}
```

```text
[token-bucket] admitted per 250 ms tick: [3, 2, 1, 1, 1]
[limiter] admitted per 250 ms tick: [3, 1, 0, 0, 3]
```

Same offered load (3 requests every 250 ms), two behaviors. The bucket starts full, then settles at its refill rate of
one token per tick. The window spends its quota of 4 in the first two ticks, rejects everything until the window rolls
over at 1,000 ms, then admits a burst again. That's the known weakness of fixed windows: up to 2× the limit around a
boundary. `FixedWindow` never wrote `name` or `admit_batch`, so it got the defaults, including the generic label
`[limiter]`.

**Generic code is checked where it's defined.** Here's a function that forgets to say what it needs from `T`
(listing `ch01-02-missing-bound.rs`):

```rust,compile_fail
fn largest<T>(items: &[T]) -> Option<&T> {
    let mut best = items.first()?;
    for item in items {
        if item > best {
            best = item;
        }
    }
    Some(best)
}

fn main() {
    println!("{:?}", largest(&[3, 9, 4]));
}
```

```text
error[E0369]: binary operation `>` cannot be applied to type `&T`
5 |         if item > best {
  |            ---- ^ ---- &T
  |            |
  |            &T
help: consider restricting type parameter `T` with trait `PartialOrd`
2 | fn largest<T: std::cmp::PartialOrd>(items: &[T]) -> Option<&T> {
```

The error is reported **at the definition**, even though the only caller uses `i32`, which *is* comparable. [LANG] Inside
`largest`, `T` has exactly the capabilities its bounds list, and here that's none. C++ templates work the other way:
the body is checked per instantiation, so the same mistake surfaces inside whichever caller first uses a type without
`>`, often pages deep. (C++20 concepts move C++ partway toward Rust's model.) Checking at the definition costs you some
typing, since every capability must be named. It buys you honest signatures: a generic function can't depend on
anything its signature doesn't promise, so callers and semver tools can trust the signature.

**Method calls go through the auto-deref chain.** Chapter 2.6 previewed this, and Chapter 3.4 used deref coercion for
`&String → &str`. Here is the whole mechanism, including a smart pointer of our own (verified):

```rust
use std::cell::Cell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

/// A smart pointer that counts how often its contents are accessed.
struct Tracked<T> {
    value: T,
    reads: Cell<u32>,
}

impl<T> Tracked<T> {
    fn new(value: T) -> Self {
        Tracked { value, reads: Cell::new(0) }
    }
}

impl<T> Deref for Tracked<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.reads.set(self.reads.get() + 1);
        &self.value
    }
}

impl<T> DerefMut for Tracked<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

fn shout(text: &str) -> String {
    text.to_uppercase()
}

fn main() {
    // Auto-deref in method calls: `len` is a method of str, found through Box -> String -> str.
    let boxed: Box<String> = Box::new(String::from("gateway"));
    let shared: Rc<String> = Rc::new(String::from("ledger"));
    println!("boxed.len() = {}, shared.len() = {}", boxed.len(), shared.len());

    // Deref coercion at a call site: &Box<String> -> &String -> &str.
    println!("{}", shout(&boxed));

    // Our own smart pointer participates in the same machinery.
    let mut name = Tracked::new(String::from("meridian"));
    name.push_str("-eu"); // DerefMut: &mut Tracked<String> -> &mut String
    let n = name.len(); // Deref (counted)
    let upper = shout(&name); // deref coercion &Tracked<String> -> &String -> &str (counted)
    println!("{upper} ({n} bytes), reads through Deref = {}", name.reads.get());
}
```

```text
boxed.len() = 7, shared.len() = 6
GATEWAY
MERIDIAN-EU (11 bytes), reads through Deref = 2
```

The counter proves the compiler really calls our `deref`: once for `name.len()`, once for the coercion in
`shout(&name)`. `push_str` went through `deref_mut`, which we didn't count. §4 gives the exact algorithm.

**Flexible parameters: `AsRef` and `Into`.** Chapter 3.3 said to take `&str` for reading and `String` for keeping.
Two std traits let one signature accept several forms, with different costs. Measured with the counting allocator
(listing `ch01-04-asref-into.rs`; the allocator module is omitted here):

```rust,ignore
struct Tenant {
    name: String,
}

impl Tenant {
    /// `impl Into<String>`: take ownership when the caller has a String, convert when it has a &str.
    fn new(name: impl Into<String>) -> Tenant {
        Tenant { name: name.into() }
    }
}

/// `impl AsRef<Path>`: accept &str, String, &Path, PathBuf... without forcing a conversion on the caller.
fn log_file_for(dir: impl AsRef<Path>, tenant: &Tenant) -> String {
    dir.as_ref().join(format!("{}.log", tenant.name)).display().to_string()
}

fn main() {
    let owned = String::from("acme");
    let (a, allocs_owned, _) = measure(|| Tenant::new(owned)); // moves the String in: no new allocation
    let (b, allocs_borrowed, _) = measure(|| Tenant::new("globex")); // &str -> String: one allocation
    println!("Tenant::new(String): {allocs_owned} allocation(s); Tenant::new(&str): {allocs_borrowed} allocation(s)");

    println!("{}", log_file_for("/var/log/meridian", &a));
    println!("{}", log_file_for(String::from("/tmp"), &b));
    println!("{}", log_file_for(Path::new("logs"), &a));
}
```

```text
Tenant::new(String): 0 allocation(s); Tenant::new(&str): 1 allocation(s)
/var/log/meridian/acme.log
/tmp/globex.log
logs/acme.log
```

`Into<String>` is the right bound when the function will **store** an owned `String`. A caller who already has one
moves it in for free (0 allocations, measured), and a caller with a `&str` pays the one copy they'd have paid anyway.
`AsRef<Path>` is the right bound when the function only **reads**. Where these impls come from is itself a trait story:
`Into` is implemented through a *blanket impl* over `From` (Chapter 6.2).

---

## Pass 2 · Systems level — *What the compiler does with a trait*

### 4. Under the hood

**Bounds are assumptions, call sites are obligations.** [RUSTC] Inside `simulate`, the bound `L: RateLimiter` is an
*assumption* the type checker may use. At each call site, `simulate(&mut bucket, 3)` creates an *obligation*,
`TokenBucket: RateLimiter`, which the **trait solver** must prove by finding an impl, unifying through generic impls
where needed. After type checking, every trait-method call is resolved to one concrete function,
`<TokenBucket as RateLimiter>::admit_batch`. Monomorphization then generates code per concrete type. That's Part VII,
Chapter 7.1, so this chapter only needs the consequence: **static dispatch is decided before any code exists.**

**Default methods are instantiated per impl.** [RUSTC] There's no single compiled `admit_batch`. Each impl that uses the
default gets its own instance, `<FixedWindow as RateLimiter>::admit_batch`, in which `self.try_acquire` is a direct call
to `FixedWindow`'s version, open to inlining. A default method is closer to a *macro over the implementor* than to
Java's single shared bytecode body.

**Method resolution, precisely** [LANG]. For a call `recv.method(args)` where `recv` has type `R`:

```text
 1. Build the candidate receiver types by dereferencing repeatedly:
        R, *R, **R, …   (built-in derefs and Deref::deref), then a final unsizing step ([T; N] → [T]).
 2. For each candidate U, in order, try the receiver forms  U, &U, &mut U.
 3. At each form, INHERENT methods are checked before TRAIT methods, and only traits IN SCOPE count.
 4. The first form with a match wins. More than one trait match at the same form → E0034 (ambiguous).
```

For `boxed.len()` with `boxed: Box<String>`: `Box<String>` has no `len`, so step 1 derefs to `String`, and
`String::len(&self)` matches at the form `&String`. For `name.len()` with our `Tracked<String>`, the deref step calls
*our* `Deref::deref`, which is why the counter moved. **Deref coercion** is the call-site cousin: when a `&U` is passed
where `&T` is expected, the compiler inserts as many derefs as needed (`&Tracked<String> → &String → &str`) [LANG].

Two consequences architects run into:

- **Traits must be in scope for method syntax.** `file.write_all(b"…")` fails without `use std::io::Write;`. The
  2021 edition added `TryFrom`, `TryInto`, and `FromIterator` to the prelude for this reason [VERSION].
- **`Deref` is for pointer-like types.** Using it to make `struct Account(Inner)` "inherit" `Inner`'s methods works
  mechanically, and std's documentation cautions against it. Method lookup gets surprising, and the wrapper leaks its
  whole inner API, including methods that bypass the wrapper's invariants. Chapter 6.3 revisits this with newtypes.

**Fully qualified syntax is the real form.** Every trait-method call desugars to `<Type as Trait>::method(receiver,
args)`. Method syntax is sugar that resolution expands. When two traits in scope give a type the same method name,
the sugar is ambiguous, and you write the desugared form yourself (the debugging exercise).

**`#[derive]` is impl generation, with a blunt bound rule** [LANG]. `#[derive(Clone)]` on `struct Id<T>` writes
`impl<T: Clone> Clone for Id<T>`. It bounds *every* type parameter, whether or not a field needs it. For Part V's typed
IDs, where `T` is only a `PhantomData` tag, that's wrong (listing `ch01-08-derive-bounds.rs`):

```text
error[E0599]: the method `clone` exists for struct `Id<Merchant>`, but its trait bounds were not satisfied
16 |     let b = a.clone(); // derive generated `impl<T: Clone> Clone for Id<T>`: requires Merchant: Clone
   |               ^^^^^ method cannot be called on `Id<Merchant>` due to unsatisfied trait bounds
note: trait bound `Merchant: Clone` was not satisfied
 5 | #[derive(Clone, Copy, Debug, PartialEq)]
   |          ----- in this derive macro expansion
   = help: consider manually implementing the trait to avoid undesired bounds
```

The compiler's own help is the fix: hand-written `impl<T> Clone for Id<T> { fn clone(&self) -> Self { *self } }` and
`impl<T> Copy for Id<T> {}`, with no bound on the tag (listing `ch01-09-manual-impl.rs`, verified). Derive is a macro
that writes ordinary impls, and it can only guess the bounds.

**`impl Trait` in argument position is an anonymous generic** [LANG]. `fn describe(l: &impl RateLimiter)` is
`fn describe<L: RateLimiter>(l: &L)` with an unnameable `L`, so callers can't turbofish it. In *return* position,
`impl Trait` means something else: one concrete type chosen by the function and hidden from callers (Chapter 6.5 shows
the error when two branches return different types). [VERSION] In edition 2024, a return-position `impl Trait`
captures all in-scope generic parameters and lifetimes by default. The `use<..>` bound (stable since Rust 1.82) narrows
what it captures.

### 5. Memory

**A trait costs no bytes.** A struct doesn't grow when you add impls. `TokenBucket` is its four 8-byte fields whether
it implements zero traits or twenty, and there's no hidden vtable pointer in it. Compare:

```text
 Java object                  C++ object (virtual methods)   Rust struct with trait impls
 ┌────────────────────────┐   ┌────────────────────────┐     ┌────────────────────────┐
 │ mark word              │   │ vptr ──► vtable        │     │ capacity: f64          │
 │ klass ptr ──► class,   │   │ fields…                │     │ tokens:   f64          │
 │   vtable, itables      │   └────────────────────────┘     │ per_sec:  f64          │
 │ fields…                │                                  │ last_ms:  u64          │
 └────────────────────────┘                                  └────────────────────────┘
 every object pays            every object of a              nothing extra, ever
                              polymorphic class pays
```

In Rust, the vtable pointer lives in the **pointer**, not the object, and only when you ask for `dyn Trait`
(Chapter 6.4 measures the 16-byte fat pointer). That's why you can implement a trait for `u32` or `[u8]`: there's no
header to put anything in, and nothing is needed.

**`Tracked<T>`** is `T` plus a `Cell<u32>`. [LANG] `Cell<T>` has the same in-memory representation as `T`, so the
counter costs 4 bytes plus any padding. **`Tenant::new(owned)`** moves a 24-byte `String` header (pointer, capacity,
length) into the struct. The heap buffer doesn't move, which is why the allocator counted zero.

### 6. CPU / OS

Static dispatch is a **direct call** the optimizer can inline. `simulate::<TokenBucket>` calls
`<TokenBucket as RateLimiter>::admit_batch`, which calls `try_acquire`. All three addresses are known at compile time,
so LLVM can flatten them into one loop. Chapter 6.4 shows real assembly for the contrast: an inlined static loop next to a
`call qword ptr [rax + 24]` through a vtable.

**Each auto-deref step is a load.** `boxed.len()` on a `Box<String>` loads the box pointer, then the `len` field
behind it: two dependent memory accesses. `Rc<String>` is the same, with an offset past the reference counts. That's
cheap when it hits in cache. Inside a hot loop over many boxed values, it's a pointer chase per element, and Chapter 6.5
measures what that costs. Nothing here touches the OS: traits are purely compile-time, unless you choose `dyn`.

---

## Pass 3 · Architect level — *Contracts that survive evolution*

### 7. Trade-offs

**Required or default?**

| Choice | Adding it to a published trait | Risk |
|---|---|---|
| Required method | **Breaking**: every implementor must change | None silent: the compiler lists every impl that must be updated |
| Default method | Usually compatible | Silent wrong behavior if the default isn't right for some implementor (§10); possible E0034 ambiguity downstream if another trait has the same method name |
| Sealed trait (private supertrait) | Compatible: nobody outside can implement it | Users can't add their own impls, by design |

**Where to put bounds.** Put them on functions and `impl` blocks, not on struct definitions. `struct Gateway<L:
RateLimiter>` forces every `impl<L> Gateway<L>` and every type that mentions `Gateway<L>` to repeat the bound, because
[LANG] bounds on a struct aren't implied elsewhere (only supertrait bounds are). A bare `struct Gateway<L>` with
`impl<L: RateLimiter> Gateway<L>` says the same thing where it matters.

**Which spelling?** Use `impl Trait` arguments for short, local signatures. Use named generics in public APIs where a
caller might need to name the type (`parse::<u64>()`-style turbofish), or where two parameters must be the *same*
type (`fn merge<L: RateLimiter>(a: L, b: L)`: two `impl RateLimiter` arguments could be different types). Use `where`
for anything longer than one line.

**`AsRef`/`Into` aren't free.** Each is generic, so each caller type gets its own copy of the function (Part VII,
Chapter 7.3). std's usual answer is a thin generic shell around a non-generic inner function. This is an unverified
sketch of the pattern `std::fs::read` uses:

```rust,ignore
pub fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
    fn inner(path: &Path) -> io::Result<Vec<u8>> {
        /* the real work: compiled once */
    }
    inner(path.as_ref())
}
```

For internal code, `&str` and `&Path` parameters are often simpler and just as good.

### 8. Java comparison

| Aspect | Java interface | Rust trait |
|---|---|---|
| Where the implementation is declared | At the class (`implements`) | In any `impl` block the orphan rule allows (Chapter 6.3), including for types you didn't write |
| Default dispatch | Virtual (`invokeinterface`), devirtualized by the JIT when profiling allows | Static (monomorphized); `dyn` is opt-in |
| Default methods | Java 8+: one shared bytecode body; a diamond conflict must be overridden in the class | Instantiated per impl; conflicts between two traits are resolved at the **call site** |
| Static members | Static methods, not callable through a type variable | Associated functions, callable generically: `L::new()` works on a type parameter |
| Generic bounds | `<T extends Comparable<T>>`, checked in the body, erased at run time | `T: PartialOrd`, checked in the body, monomorphized |
| Retroactive implementation | Impossible: `Integer` can't implement your interface; you write an adapter | `impl MyTrait for u32` is fine |
| `Self` type | None (the F-bounded `Comparable<T>` idiom) | `Self`, usable in arguments and returns |

One row surprises people. **Java checks generic bodies against their bounds too.** On definition-site checking, Rust
is Java-like, not C++-like. What Java lacks is the rest of the table: retroactive impls, static dispatch by default,
and generic calls to associated functions.

> **Analogy limit.** "A trait is an interface" breaks in three places. (1) Implementation is *retroactive*: you can
> give `[u8]` a `checksum()` method (Chapter 6.3), which in Java takes a static utility class. (2) Dispatch is the
> caller's choice: the same `RateLimiter` serves `simulate::<TokenBucket>` (no vtable) and `&dyn RateLimiter` (vtable).
> A Java interface type is always a virtual-call site. (3) Some traits **can't be used as types at all**. A trait whose
> methods return `Self` or are generic has no `dyn` form (Chapter 6.4). Java has no such distinction, because every
> interface is a type.

### 9. Production scenario

**Meridian's limiter contract.** The gateway team's first Rust module is the limiter from §3, and the design choices
that made it into review are worth copying:

- **Time is a parameter** (`try_acquire(&mut self, now_ms: u64)`), not read from a clock inside. The simulation above is
  deterministic, so the output is a test assertion. In production the caller passes a monotonic timestamp it already has.
- **One required method, two defaults.** Every new algorithm must answer the one question that defines it, *admit
  this request now?*, and gets batching and naming for free. §10 shows the rule for when a default is safe.
- **Generic at the call site.** Per-key limiters live in a `HashMap<ApiKey, TokenBucket>`, a concrete type, so every
  `try_acquire` on the hot path is a direct call. Which algorithm a key class uses is a configuration question, and
  Chapter 6.5 decides between an enum and `dyn` for that.

If the team later wants an injectable clock, the Rust version of Java's `java.time.Clock` injection is a type
parameter, `TokenBucket<C: Clock>`, with a `SystemClock` in production and a `FakeClock` in tests. It costs nothing at
run time, because `C` is resolved at compile time.

### 10. Failure scenario

**The default that told clients to retry immediately.** When the gateway started returning 429s, the limiter trait
grew a `retry_after_ms` method. To avoid touching every existing impl, it got a default of `0`. Months later, another
team added a global concurrency limiter and never overrode it (listing `ch01-07-default-trap.rs`):

```rust
/// A limiter that rejects must tell the client when to retry.
trait RateLimiter {
    fn try_acquire(&mut self) -> bool;

    /// DEFAULT: "retry immediately". Convenient, and wrong for most implementations.
    fn retry_after_ms(&self) -> u64 {
        0
    }
}

struct PerKeyWindow {
    used: u32,
    limit: u32,
    ms_until_reset: u64,
}

impl RateLimiter for PerKeyWindow {
    fn try_acquire(&mut self) -> bool {
        self.used += 1;
        self.used <= self.limit
    }
    fn retry_after_ms(&self) -> u64 {
        self.ms_until_reset
    }
}

/// Added months later by another team. It compiles: the default fills the gap silently.
struct GlobalConcurrency {
    in_flight: u32,
    max: u32,
}

impl RateLimiter for GlobalConcurrency {
    fn try_acquire(&mut self) -> bool {
        if self.in_flight < self.max {
            self.in_flight += 1;
            true
        } else {
            false
        }
    }
    // forgot retry_after_ms: rejected clients are told to retry after 0 ms
}

fn reject_header(l: &dyn RateLimiter) -> String {
    format!("429 Too Many Requests; Retry-After-Ms: {}", l.retry_after_ms())
}

fn main() {
    let mut per_key = PerKeyWindow { used: 0, limit: 1, ms_until_reset: 750 };
    let mut global = GlobalConcurrency { in_flight: 1, max: 1 };
    for limiter in [&mut per_key as &mut dyn RateLimiter, &mut global] {
        limiter.try_acquire();
        if !limiter.try_acquire() {
            println!("{}", reject_header(limiter));
        }
    }
}
```

```text
429 Too Many Requests; Retry-After-Ms: 750
429 Too Many Requests; Retry-After-Ms: 0
```

Under overload, the global limiter rejected thousands of requests per second, and every rejected client was told to
retry *now*. Well-behaved SDKs obeyed, so the rejections turned into a retry storm that kept the gateway saturated
after the original spike was over. That's the self-sustaining shape of a *metastable* failure.

The type system did exactly what it was asked. The design error was the default. The rules the team adopted:

1. **A default must be correct for an implementor who has never heard of it.** `name() -> "limiter"` passes: a vague
   log label is harmless. `retry_after_ms() -> 0` fails: no single value is right for every limiter.
2. **If no default is universally correct, make the method required.** On an internal trait, the compile errors *are*
   the migration checklist. On a published trait, that's a breaking change, so do it before 1.0 or introduce a new
   trait.
3. **Or make "unknown" explicit.** `fn retry_after_ms(&self) -> Option<u64> { None }` lets the *caller* apply a policy
   (a jittered 1–2 s) instead of the trait silently choosing zero.
4. **Test the contract, not the impls.** A generic conformance test, `fn conformance<L: RateLimiter>(make: impl Fn() ->
   L)`, runs against every implementor and asserts that a rejection carries a non-zero retry hint.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part VI).*

1. What is the difference between a trait, an impl, and a bound? Where does each exist at run time?
2. Why does `largest<T>` fail to compile even though its only caller passes `i32`? Contrast Rust with C++ templates, and
   with Java generics.
3. Walk through method resolution for `boxed.len()` where `boxed: Box<String>`. Where do inherent and trait methods
   rank?
4. What does `impl Trait` mean in argument position, and how is it different from a named type parameter?
5. When should a parameter be `impl Into<String>`, `impl AsRef<str>`, or `&str`? What does each cost?
6. Is adding a method to a published trait a breaking change? Distinguish required and default methods, and name the
   subtle case.
7. Why is it a smell to put trait bounds on a struct definition?
8. A default method is compiled how many times? What does that enable that Java's default methods rely on the JIT for?

### 12. Exercises

- **Beginner.** Add a `SlidingLog` limiter (keep the timestamps of the last `limit` admissions in a `VecDeque<u64>`)
  and run it through `simulate`. Predict its output before running it.
- **Intermediate.** Write `fn largest<T: PartialOrd>(items: &[T]) -> Option<&T>`. Then call it with `&[f64]`
  containing `NaN`. Why does `PartialOrd` (not `Ord`) allow this, and what does your function return?
- **Advanced.** Implement `Deref<Target = str>` for a `TenantId(String)` newtype. List three things that became possible
  that you might not want, and rewrite it with an explicit `as_str()` instead.
- **Systems.** On the Playground, make `simulate` `#[inline(never)]`, call it with both limiters, and emit release
  assembly with `tools/emit.ps1`. How many copies of `simulate` exist? Is `admit_batch` a separate function in either?
- **Architecture.** Design the `Clock` trait for the limiter so that tests can advance time manually. Should `Clock` be a
  type parameter of `TokenBucket`, a parameter of `try_acquire`, or a `&dyn Clock` field? Argue from testability and
  hot-path cost.

### 13. Debugging exercise

A billing module has an invoice that's both auditable and billable. Both traits use the same method name (listing
`ch01-05-ambiguous-method.rs`):

```rust,compile_fail
trait Auditable {
    fn describe(&self) -> String;
}

trait Billable {
    fn describe(&self) -> String;
}

struct Invoice {
    id: u64,
}

impl Auditable for Invoice {
    fn describe(&self) -> String {
        format!("audit record for invoice {}", self.id)
    }
}

impl Billable for Invoice {
    fn describe(&self) -> String {
        format!("invoice #{} (billable)", self.id)
    }
}

fn main() {
    let inv = Invoice { id: 42 };
    println!("{}", inv.describe());
}
```

```text
error[E0034]: multiple applicable items in scope
28 |     println!("{}", inv.describe());
   |                        ^^^^^^^^ multiple `describe` found
note: candidate #1 is defined in an impl of the trait `Auditable` for the type `Invoice`
note: candidate #2 is defined in an impl of the trait `Billable` for the type `Invoice`
help: disambiguate the method for candidate #1
28 +     println!("{}", Auditable::describe(&inv));
```

1. At which step of the method-resolution algorithm in §4 does the ambiguity arise?
2. Write both disambiguated calls, one in the short form and one in the fully qualified `<Type as Trait>::` form. When
   is the long form *required*?
3. The same code compiled last month, before the `Billable` trait gained a `describe` method *with a default body*.
   What does this say about adding default methods to published traits?

### 14. Design exercise

**A limiter trait for a multi-tenant platform.** Meridian wants one limiter abstraction shared by the gateway, the
webhook dispatcher, and the batch export service. Requirements: per-tenant and global limits; a retry hint on
rejection; metrics (admitted/rejected counts); deterministic tests; and the gateway's hot path must not allocate or make
indirect calls per request.

Design the trait or traits: which methods are required, which have defaults and why each default is safe, how time is
supplied, and how metrics are exposed without forcing every implementor to write metrics code. Then list what you would
seal and what third teams may implement. Keep your design: Chapter 6.4 will ask whether it can be used as `dyn`.
