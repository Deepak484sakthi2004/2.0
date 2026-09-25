# Appendix A — Answer Key: Part VI

> Model answers. Write yours first. Where several answers are defensible, the key says so.

---

## Chapter 6.1 — Traits, Bounds, and Default Methods

### Interview & architecture questions

**1. Trait, impl, bound.** A trait is a named contract: methods, plus associated types and constants. An impl is the
evidence that one type meets it. A bound is a requirement on a type parameter. At run time, under static dispatch,
none of them exist: every call has been resolved to a concrete function (`<TokenBucket as RateLimiter>::try_acquire`).
With `dyn`, one vtable per (type, trait) pair exists in read-only data. Bounds never exist at run time.

**2. `largest<T>`.** Inside a generic function, `T` has exactly the capabilities its bounds list, and `largest` lists
none, so `>` is unavailable. The error is E0369 at the *definition*, whatever the callers pass. C++ templates check the
body per instantiation (C++20 concepts add optional up-front constraints), so the same mistake would surface inside the
first instantiation with a type lacking `>`. Java checks generic bodies against bounds too (`<T extends Comparable<T>>`),
so here Rust is Java-like. The benefit: a generic signature is a complete contract, so callers, reviewers, and semver
tooling can trust it.

**3. `boxed.len()` with `boxed: Box<String>`.** The candidate receiver types are `Box<String>`, then `String` (via
`Deref`), then `str` (via `String: Deref<Target = str>`). For each candidate `U`, resolution tries `U`, `&U`, `&mut U`,
checking inherent methods before trait methods (in-scope traits only) at each form. `Box<String>` has no `len` at any
form. At `String`, the inherent `String::len(&self)` matches at `&String`, and resolution stops there without reaching
`str::len`.

**4. `impl Trait` in argument position.** It's an anonymous type parameter: `fn describe(l: &impl RateLimiter)` equals
`fn describe<L: RateLimiter>(l: &L)`, and it's monomorphized the same way. The differences: callers can't name or
turbofish it, and two `impl Trait` arguments are two *independent* parameters, so they can't be forced to the same type.

**5. `Into<String>`, `AsRef<str>`, `&str`.** Use `impl Into<String>` when the function *stores* an owned `String`. A
caller with a `String` moves it in with zero allocations, and a `&str` caller pays one allocation (both measured). Use
`impl AsRef<str>` or `AsRef<Path>` when the function only *reads* and callers hold many different forms. Plain `&str` is
the default for internal read-only APIs: no monomorphization and no ceremony. The generic forms cost one copy per
caller type, which std mitigates by delegating to a non-generic inner function.

**6. Adding methods to a published trait.** A required method is breaking, since every implementor must change. A
defaulted method is usually compatible, with one subtle break. A downstream type implementing another trait that has a
method *of the same name*, with both traits in scope, now gets E0034 at call sites. Cargo's SemVer guide classes adding
a defaulted item as "possibly breaking" for this reason. A third break is covered in Chapter 6.4: adding a generic
method (without `where Self: Sized`) makes the trait dyn-incompatible. Sealing the trait avoids implementor-side breaks.

**7. Bounds on struct definitions.** They aren't implied elsewhere, so every `impl` block, every function mentioning
the struct, and every containing type must repeat them. They also restrict methods that don't need the bound
(`len`, `new`), and they spread through the codebase. Put bounds on the impls that need them. The exception is when
the bound is needed by the *type* itself, for example when a `Drop` impl requires it, since `Drop` impls must carry
exactly the struct's bounds.

**8. Default methods, compiled.** Once per impl that uses the default: `<FixedWindow as RateLimiter>::admit_batch` is its
own function, in which `self.try_acquire` is a static call to `FixedWindow`'s version, so it can be inlined and
specialized. Java compiles a default method once, as shared bytecode, and relies on the JIT to inline and specialize it
at hot call sites. Rust gets the per-type specialization ahead of time.

### Debugging exercise (E0034)

1. At the candidate type `Invoice`, form `&Invoice`, there's no inherent `describe`, and *two* in-scope trait methods
   match at the same form. Resolution can't prefer one, so it reports E0034.
2. `Auditable::describe(&inv)` (short form, trait-qualified) and `<Invoice as Billable>::describe(&inv)` (fully
   qualified). Listing `ch01-06-ambiguous-fixed.rs` prints `audit record for invoice 42` and `invoice #42 (billable)`.
   The long form is *required* when the trait path alone can't determine `Self`: associated functions without a receiver
   (`<T as Default>::default()`), calls where several `Self` types implement the trait and inference can't pick, and
   generic traits needing both `Self` and parameters (`<Cents as Convert<String>>::convert`, Chapter 6.2).
3. Adding a **defaulted** method to a published trait can break downstream code that already calls a same-named method
   of another trait on the same type. The code didn't change, and the call became ambiguous. Treat new defaulted methods
   with common names (`describe`, `name`, `id`) as risky. Prefer distinctive names, or sealed traits.

### Selected exercises

- **Beginner (SlidingLog):** a correct sliding log never admits more than `limit` in *any* window-length span. The fixed
  window can admit up to 2× the limit across a boundary. The regular 3-per-tick load in §3 makes them look alike, so
  design a load that separates them: nothing until 750 ms, then 4 requests at 750 ms and 4 at 1,000 ms. The fixed
  window admits all 8, and the sliding log admits 4.
- **Intermediate (NaN):** `PartialOrd` allows `f64` because it doesn't require a total order. Every comparison with NaN
  is false, so NaN never *replaces* the best, but a NaN in the *first* position is never replaced either, and
  `largest(&[NaN, 1.0])` returns NaN. The answer depends on position, which is a bug. Use `total_cmp`, or filter NaNs
  first.
- **Advanced (`Deref` for `TenantId`):** (1) deref coercion lets `&TenantId` pass wherever `&str` is expected, including
  a `customer_id: &str` parameter, which erases the type distinction the newtype existed for; (2) callers start depending
  on `str` methods, so the representation can't change later (for example, to an interned `u32`); (3) comparisons and
  formatting silently operate on the raw string. Expose `as_str()` and implement only the traits you mean (`Display`,
  `Hash`, `Eq`).
- **Systems:** two copies of `simulate`, `simulate::<TokenBucket>` and `simulate::<FixedWindow>`. `admit_batch` is
  usually inlined into each, so there's no standalone symbol, unless it's marked `#[inline(never)]`.
- **Architecture (`Clock`):** a time *parameter* (`try_acquire(now_ms)`) is simplest, most testable, and free on the hot
  path. The caller usually has a timestamp already. A type parameter `TokenBucket<C: Clock>` is also free and
  encapsulates time, but spreads `C` through types. A `&dyn Clock` field is flexible at run time and costs an indirect
  call per acquire, fine at gateway rates (Chapter 6.5's ratio test). Any of the three is defensible. The §9 design
  picked the parameter.

---

## Chapter 6.2 — Associated Types, Generic Traits, and Blanket Impls

### Interview & architecture questions

**1. Associated type vs parameter.** Ask whether a type can sensibly implement the trait more than once. If exactly
once, the type is an *output* determined by the impl, so make it an associated type. If many times, it's an *input*
chosen by the user, so make it a parameter. `Iterator::Item`: an iterator yields one item type, so it's associated.
`From<T>`: a type can be built from many sources, so it's a parameter. `Add<Rhs = Self> { type Output; }` has both: the
right-hand side is an input (`Money + Money`, `Money + i64`), and the result is determined by the pair.

**2. Inference.** In `decode_all(&CsvQuoteDecoder, …)` the argument fixes `D`, and the solver normalizes
`<D as Decoder>::Output` to `Quote` from the unique impl. Information flows outward from `Self`. In
`let x = price.convert();`, `Self = Cents` is known, but two impls (`Convert<f64>`, `Convert<String>`) remain and
nothing fixes `T`, hence E0282.

**3. `From` not `Into`, `Display` not `ToString`.** `impl<T, U> Into<U> for T where U: From<T>` derives `Into` from
`From`. Implementing `From` gives both directions of use, and `?` relies on `From` for error conversion. Implementing
`Into` alone gives only `Into`. `impl<T: Display + ?Sized> ToString for T` derives `ToString` from `Display`, so a
manual `ToString` on a `Display` type conflicts (E0119), and `Display` also plugs into `format!`.

**4. `?Sized`.** Type parameters are implicitly `Sized`. `?Sized` removes that, so the blanket covers unsized types
(`str`, `[T]`, `dyn Display`) as `Self`. Then `LogLine` is implemented for `str` itself and for `dyn Display`, not only
for references to them.

**5. Supertrait vs method bound.** `trait Auditable: Debug` requires *every* implementor to be `Debug`, lets every
`A: Auditable` bound imply `A: Debug`, and puts `Debug`'s method in `dyn Auditable`'s vtable. A `where Self: Debug` on
one method requires `Debug` only of types that call that method. Types that aren't `Debug` can still implement the
trait. Use the supertrait for genuine "is-a" requirements.

**6. Blanket impls are breaking.** A downstream crate had `impl LogLine for Money`, with `Money: Display`. After the
upstream crate adds `impl<T: Display> LogLine for T`, both impls cover `Money`, and the downstream crate gets E0119 on
upgrade, from a change it didn't make. Versioning the blanket as a minor release is the mistake.

**7. Java vs Rust.** After erasure, `Comparable<A>` and `Comparable<B>` are the same interface, with one
`compareTo(Object)` slot, so Java forbids implementing both. In Rust, `PartialEq<A>` and `PartialEq<B>` are distinct
traits with distinct, monomorphized impls, selected statically by the argument type. std relies on this:
`String: PartialEq<str>`, `PartialEq<&str>`, and `PartialEq<String>`.

**8. `D: Decoder<Output = Quote>`.** "Any decoder, as long as it produces `Quote`s," without adding a type parameter to
the function. Java would declare `<D extends Decoder<Quote, ?>>`, or carry `O` and `E` parameters through every
signature.

### Debugging exercise (E0282)

1. Method calls need the receiver's type to already be known in order to *search* for the method. Inference doesn't
   work backwards from "some type with a `len` method" to a type. The compiler says so: "type must be known at this
   point."
2. `let shown: String = price.convert();` · `let shown = <Cents as Convert<String>>::convert(&price);` ·
   `fn show(c: &impl Convert<String>) -> String { c.convert() }`, called as `show(&price)`.
3. With `type Output`, `Cents` could implement the trait only once, so there'd be no ambiguity and no annotation. You'd
   lose the ability to convert one type into several targets through one trait, and would need separate methods or
   traits (`to_f64`, `Display`).

### Selected exercises

- **Beginner:** `decode_all` doesn't change. `JsonQuoteDecoder` sets `Output = Quote` (with `Quote: Deserialize`) and
  `Error = serde_json::Error`. That's the point of associated types.
- **Intermediate:** std's documentation for `From` asks for conversions that are infallible, lossless, value-preserving,
  and *obvious*. "An `f64` means percent" isn't obvious. `let b: Basis = 1.75.into();` hides the unit, which is the
  2.2 incident's bug class. `Basis::from_percent(1.75)` puts the unit in the name.
- **Advanced:** `impl Add<Money> for i64` is legal: a foreign trait and foreign `Self`, but the local `Money` appears as
  a trait parameter and no uncovered type parameter precedes it (Chapter 6.3 §4).
- **Systems:** five instances of `log_line`, one per `Self` type. Erasure would compile one body taking `Object`,
  calling `toString()` virtually.
- **Architecture:** (1) Prefer an opt-in marker, `impl<T: Display + LogViaDisplay> LogLine for T`, which is additive and
  non-breaking. (2) If it must be a true blanket, it's a major version. (3) Before release, build every internal
  consumer against the pre-release (workspace-wide `cargo check` in CI) to find conflicting impls. (4) Release notes
  name the conflict pattern ("remove your `impl LogLine for X` where `X: Display`, or opt out by …") and flag
  behavioral changes, such as redaction.

---

## Chapter 6.3 — Coherence and the Orphan Rule

### Interview & architecture questions

**1. Coherence.** For any trait and type, at most one impl exists in the whole program. Without it, two crates could
each implement `Hash` for `Uuid`: a `HashMap` filled through one impl and queried through the other misses keys or
duplicates them. Two `Ord` impls would corrupt a `BTreeMap`'s ordering. Behavior would depend on which crate's code
happened to run.

**2. The orphan rule.** `impl<P..> Trait<T1..Tn> for T0` is allowed if `Trait` is local, or if some `Ti` is a local
type and no uncovered type parameter appears in `T0..Ti-1`. The rule forces every impl into a crate that owns the trait
or a type in the header. Any two crates able to write impls for the same (trait, type) pair are therefore related by a
dependency path, so the later one is checked against the earlier one's impls. The uncovered-parameter clause stops two
*unrelated* crates from writing generic impls that could meet on a type neither can see.

**3. `Box<dyn Rule>` vs `Vec<Box<dyn Rule>>`.** `Box` is `#[fundamental]` and `dyn Rule` is local, so `Box<dyn Rule>`
counts as local (verified). `Vec` isn't fundamental. `Vec<Box<dyn Rule>>` is a foreign type, whatever its argument, and
`Display` is foreign, so the impl is an orphan (E0117).

**4. `From<Cents> for f64`.** The local type `Cents` appears as a trait parameter, and no type parameter precedes it,
so it's allowed. RFC 2451 (Rust 1.41) went further, allowing type parameters before the local type when a foreign type
*covers* them: `impl<T: From<i64>> From<Ledger> for Vec<T>` (verified). A covered `T` can't overlap with another
crate's impl for `Vec<T>` *on this trait parameter*, because only this crate can name `From<Ledger>` for foreign types.
An uncovered `T` (`impl<T> From<Cents> for T`, E0210) would claim `From<Cents>` for every type in every downstream crate.

**5. The future-compatibility rejection.** The overlap check refuses negative reasoning about foreign types and traits.
It can't conclude "`Vec<u8>` isn't `Display`" because std can add that impl in a minor release. If it accepted, that
std release would break (or make ambiguous) every crate using the pattern, so adding impls, which Rust treats as
non-breaking, would become breaking. Rejecting now keeps ecosystem evolution possible.

**6. When each workaround wins.** A newtype when you need a *foreign* trait on a foreign type, or want invariants (a
`Serialize` format for a vendor type, `Display` for `Vec<u8>`). An extension trait when you want new *methods* on
foreign types (`checksum` on `[u8]`). An upstream feature when the impl is broadly useful and belongs with the type
(`uuid` with `serde`).

**7. `serde` features.** An impl of `Serialize` for a type can only live in `serde` or in the type's crate. `serde` can't
know every type, so type crates host the impls behind optional features. Coherence turns "who can connect trait A to
type B" into an ecosystem-coordination problem that's solved crate by crate.

**8. `Deref` on newtypes.** It's fine for presentation wrappers with no invariants (`Hex`): read-only access to the
bytes can't break anything. It's a bug for boundary types (`TenantId`, `Email`): it leaks the inner API, deref
coercion lets the wrapper pass where the raw type is expected, and `DerefMut` can break validation.

### Debugging exercise (E0119, future compatibility)

1. The overlap check may assume only what this crate controls. `Vec<u8>: Display` is decided by std, which may add the
   impl later, so the compiler must treat the two impls as potentially overlapping. The note says exactly that.
2. Every crate with this pattern would fail to compile after upgrading to that std version, and std couldn't add
   `Display for Vec<u8>` without breaking the ecosystem. Rust's rule is that adding impls is allowed in minor releases,
   and this check is what makes that promise keepable.
3. Keep the blanket and move the special case to a newtype: `impl Render for Hex`. Or drop the blanket for an explicit
   list of impls or an opt-in marker trait. `impl Render for Hex` is accepted because `Hex` is local. Only this crate
   could make `Hex: Display`, so the compiler may use the negative fact that it isn't.

### Selected exercises

- **Beginner:** only the newtype makes `format!("{}", hex)` work. The extension trait gives
  `format!("{}", bytes.to_hex())`, which formats a `String` returned by your method.
- **Intermediate:** `impl<T> From<Cents> for T` is E0210, with `T` uncovered before the local type
  (`ch03-05-uncovered-param.rs`). `impl<T: Into<i64>> Add<T> for Cents` compiles, since the local type comes first
  (`ch03-07-local-first.rs`). A trap worth knowing: `impl<T> From<T> for Cents` passes the orphan check but conflicts
  with core's reflexive `impl<T> From<T> for T` (E0119, `ch03-06-reflexive-from.rs`).
- **Advanced:** with the local type non-`Debug`, both impls are accepted: it's local, so the compiler may reason that it
  isn't `Debug`. Derive `Debug` and you get E0119, which is correct, because now two impls apply.
- **Systems:** sizes and alignment match either way. `repr(transparent)` additionally *guarantees* the same ABI (passed
  and returned exactly like the inner type), which matters for FFI signatures and for sound transmutes or pointer casts
  between `Hex` and `Vec<u8>`.
- **Architecture:** `AuditKey for OrderId` goes in `meridian-audit` (owns the trait; it must then depend on
  `meridian-types`) or in `meridian-types` (owns the type; it must then depend on the audit crate). Pick the direction
  of the dependency graph you want. `Serialize for OrderId` can only be in `meridian-types` (serde is third party),
  behind a `serde` feature. `ToSql for OrderId` can only be in `meridian-types` too, behind a `sql` feature. So the
  types crate grows features, and no "glue" crate can hold any of these impls.

---

## Chapter 6.4 — Trait Objects, vtables, and dyn Compatibility

### Interview & architecture questions

**1. Trait objects.** `dyn Trait` is a value of some concrete type implementing `Trait`, with that type erased.
Implementors have different sizes, so `dyn Trait` has no static size (it's unsized) and lives behind a pointer. The
pointer is fat: a data pointer plus a vtable pointer, 16 bytes on x86-64 for `&dyn`, `Box<dyn>`, `Rc<dyn>`, and
`Arc<dyn>` (measured).

**2. Layout.** In current rustc: a drop-glue pointer, size, align, then methods in declaration order, plus supertrait
data for upcasting. Guaranteed: that the metadata lets the program call methods, drop, deallocate, and compute
`size_of_val`/`align_of_val`. Not guaranteed: the order, the exact entries, uniqueness of vtables, or the null drop
entry. The IR showed `<Circle as Shape>`'s vtable starting with 24 bytes of data, eight zero bytes where the drop
pointer would be, then size 8 and align 8, because `Circle` has no drop glue.

**3. `call qword ptr [rax + 24]`.** `r14` walks the slice of fat pointers. `mov rdi, [r14]` loads the data pointer into
the first argument register (System V), becoming `self`. `mov rax, [r14 + 8]` loads the vtable pointer. The call reads
the function pointer at offset 24, the fourth 8-byte word, after drop, size, and align: `area`, the trait's first
method.

**4. Deriving the rules.** A vtable is a finite table of function pointers, each called with an erased `self`:
(a) a generic method is an unbounded family of monomorphized functions, which can't be listed; (b) `Self` by value
(arguments or return) needs the size at the call site, which is erased; (c) a `Sized` supertrait contradicts `dyn`
being unsized; (d) an associated function without a receiver has no `self` to find a vtable through; (e) `async fn` and
return-position `impl Trait` return a different hidden type per impl, of unknown size.

**5. `where Self: Sized`.** It removes the method from the vtable. It stays callable on concrete types and is
unavailable through `dyn`. Use an extension trait with a blanket impl over `T: Trait + ?Sized` when the convenience can
be written in terms of dyn-compatible methods and should work on trait objects too. It's then monomorphized at each
call site (listing `ch04-11-ext-trait.rs`).

**6. `Send` and `dyn`.** The object type's auto traits are exactly what it declares. Erasure hides the concrete type,
so the compiler can't inspect fields, and the set of implementors is open: tomorrow's implementor may hold an `Rc`.
Write `dyn Trait + Send + Sync`, or make them supertraits (`trait Rule: Send + Sync`) so every `dyn Rule` has them.

**7. Variance of trait objects.** A trait's type parameters may appear in method inputs (`put(&mut self, T)`) and
outputs, and the compiler doesn't track which for the object, so it treats them as invariant. Otherwise you could store
a short-lived value where the object's owner expects a long-lived one. The lifetime bound only states how long the
object is valid, and shrinking it just forgets information, which is safe (covariant).

**8. Placement of the dispatch pointer.** Java puts a class pointer in *every* object header, so references are one word
and any call can be virtual. Pervasive polymorphism is cheap to express, and the JIT makes it fast. Rust objects carry
nothing, and only `dyn` references carry a vtable pointer, so non-polymorphic code pays nothing and polymorphism costs
exactly one more word per reference. A dynamic call is a fixed-offset load (no itable search), but it's never
speculatively inlined.

### Debugging exercise (`Any` trap)

1. `&boxed` is `&Box<dyn Any>`. `Box<dyn Any>` is itself `'static`, so it implements `Any`, and the *unsizing*
   coercion `&Box<dyn Any>` → `&dyn Any` applies. The resulting object's data pointer points to the *box*, and its vtable
   is `Box<dyn Any>`'s `Any` vtable. `type_id` returns `TypeId::of::<Box<dyn Any>>()`, no downcast matches, and the
   result is "unknown type."
2. The first candidate is `Box<dyn Any>`. By value, no `type_id` takes `self` by value. At the form `&Box<dyn Any>`,
   `Any::type_id(&self)` with `Self = Box<dyn Any>` matches, so resolution stops *before* dereferencing to `dyn Any`
   and returns the box's `TypeId` (verified: "is Box<dyn Any>? true").
3. Fixes: pass `&*boxed` (or `&**item` from a `&Box<dyn Any>`), and call `(*boxed).type_id()`. Review rule: never let a
   `&Box<dyn Any>` flow where a `&dyn Any` is expected. Dereference explicitly. Clippy's lint is `type_id_on_box`.

### Selected exercises

- **Beginner:** add one registry line, `r.insert("geofence", |arg| Box::new(GeoFence::parse(arg)))`. No other code
  changes. That's the open-set property.
- **Intermediate:** `Auth` 0/1, `RateLimit` 4/4, `TenantTag` 0/1. `Box::new` of a zero-sized value doesn't call the
  allocator. It uses a dangling, well-aligned, non-null pointer [LIB].
- **Advanced:** an unverified sketch of the well-known pattern (the `dyn-clone` crate automates it):

  ```rust,ignore
  trait CloneBox {
      fn clone_box(&self) -> Box<dyn Middleware>;
  }
  impl<T: Middleware + Clone + 'static> CloneBox for T {
      fn clone_box(&self) -> Box<dyn Middleware> { Box::new(self.clone()) }
  }
  trait Middleware: CloneBox { /* … */ }
  impl Clone for Box<dyn Middleware> {
      fn clone(&self) -> Self { self.clone_box() }
  }
  ```

  Making `CloneBox` a *supertrait* puts `clone_box` in `dyn Middleware`'s vtable. The blanket impl writes it for every
  `Clone` implementor.
- **Systems:** expect the supertrait's methods to appear in the vtable before the subtrait's own methods. For a single
  supertrait, the upcast can reuse the same vtable pointer, since the supertrait's entries form a prefix. Both are
  implementation details: confirm them in your IR rather than relying on them.
- **Architecture:** open-ended. Any trait used as `dyn` should have a `fn _assert(_: &dyn Trait) {}` guard, and a
  semver policy that treats dyn-incompatibility as breaking.

---

## Chapter 6.5 — Static vs Dynamic Dispatch: The Architect's Decision

### Interview & architecture questions

**1. Expression problem.** Enum: adding an operation is one function with a `match`, while adding a variant means editing
the enum and every `match` (the compiler finds them). Only the enum's owner can add cases. Generics and `dyn`: anyone,
including a third party, can add a type. Adding an operation means changing the trait and every impl, unless it can be
a default method or an extension trait over existing methods. Between those two, `dyn` additionally lets the case be
chosen at run time.

**2. `impl Trait` returns.** Return-position `impl Trait` is one concrete type chosen by the function body, so two
branches with different types give E0308. Fixes: `Box<dyn Trait>` (an open set, one allocation and indirect calls), or
an enum implementing the trait (a closed set, no allocation, a `match`). An `Either`-style enum is the generic version
of the second.

**3. Grouped vs mixed `dyn`.** The instructions are identical, and both sets of boxes were allocated in iteration order.
What changed is the *sequence of call targets*: long runs of the same target make the indirect-branch predictor right
almost every time. To prove it, use `perf stat -e branch-misses,instructions` on both runs: instruction counts should
match, and misses should differ by roughly the time gap divided by the misprediction cost. Also shuffle allocation order
separately to rule out locality.

**4. Enum vs mixed `dyn`.** Both have unpredictable branches, but the enum's are ordinary conditional branches in a loop
with no call, no return, no function-pointer load, and no argument shuffling. All arms are visible to the optimizer
(the `Tiered` arm became branch-free), and the data is inline in the `Vec` instead of behind a pointer per element.

**5. The ratio test.** Overhead fraction ≈ extra ns per dynamic call ÷ ns of work per call. A gateway middleware chain
has ~10 calls against ~0.5 ms of CPU per request, so the share is ~0.005%: irrelevant. A per-byte decoder does ~1 ns of
work per call, so dispatch dominates, and it also blocks vectorization. Put dynamic dispatch *outside* the loops.

**6. Plugin ABI.** Rust has no stable ABI: default layouts, vtable layout, and the `extern "Rust"` calling convention may
change between compiler versions and flags, and nothing checks compatibility at load time. The three real boundaries are
the C ABI (hand-built vtable, fast, no isolation, `unsafe`), WebAssembly (a sandbox with a moderate crossing cost and
data copies), and out of process (full isolation, network latency, independent deploys).

**7. When `dyn` is the right move without being faster.** To cut compile time and binary size by erasing deep generic
stacks at module boundaries. To keep type parameters from spreading through every signature (API simplicity). For
heterogeneous collections and runtime configuration. In libraries, to compile shared logic once in the defining crate
rather than in every user's.

**8. HotSpot vs Rust.** HotSpot is dynamic by default and profiles call sites. Monomorphic and bimorphic sites get
guarded, inlined fast paths, megamorphic ones fall back to table dispatch, and newly loaded classes trigger
deoptimization. Java wins when a site is dynamic in the source but monomorphic in practice: it gets inlined with no
source change, while a Rust `dyn` site stays indirect. Rust wins on predictability (no warm-up, no deopt), on explicit
static dispatch, and on monomorphized generics over unboxed values, which erasure can't express.

### Debugging exercise (`Box<dyn Fee>` in the pricing loop)

1. Per call: one heap allocation and one free (typically tens of ns, order of magnitude), plus an indirect call (2–3 ns
   in this Part's measurement, against about 1 ns for the enum). But 40M line items per hour is only ~11K per second, so
   even ~50 ns per item is ~0.5 ms of CPU per second, about 0.05% of a core. **The ratio test says the CPU cost doesn't
   matter here.** The design problem is the next question.
2. The enum column. The set is closed and owned by one team, it changes with releases, and exhaustive `match` checking is
   worth more than open extension. There's no allocation per call as a bonus.
3. Resolve the dynamic decision once per configuration load: build a `HashMap<Country, FeeKind>` (or an
   `Arc<dyn FeeTable>` if other teams must plug in table *implementations*) when the config loads, and have each line
   item do a lookup plus an enum `match`. Dynamic dispatch happens once per load, static per item.

### Selected exercises

- **Beginner:** `Box<dyn Fee>` costs one allocation per call, and the enum zero. The counting allocator shows it.
- **Intermediate:** predicted: `FeeKind` grows to about 200 bytes plus the tag, so 1M elements take about 208 MB, and
  ns/fee rises toward memory-bandwidth limits, because every element drags its large slot through cache. Boxing the rare
  large variant brings elements back to about 32 bytes. Measure both.
- **Advanced:** predicted: `Vec<fn(i64) -> i64>` lands near mixed `dyn`, with one indirect call per element, one fewer
  dependent load (no vtable), and the same misprediction behavior when mixed. `Box<dyn Fn>` with captures behaves like
  `dyn`, plus the captured data behind the box.
- **Systems:** predicted: identical instruction counts, and a mixed-run miss rate on the order of 20–30% of elements
  against near zero for grouped. At roughly 15–20 cycles per miss and about 3.5 GHz, that covers the ~1.3 ns gap
  (about 4–5 cycles per element).
- **Architecture:** open-ended.

---

## Part VI Review — Capstone (fraud-rules SDK)

**1. The three error kinds.**

- **E0117 (Chapter 6.3 §2, §4):** `impl Display for Vec<Box<dyn Rule>>` is a foreign trait on a foreign, non-fundamental
  type. The orphan rule protects coherence: another crate could write the same impl. Fix: implement `Display` for a
  local type (the `Engine`, or a newtype `RuleSet(Vec<Box<dyn Rule>>)`).
- **E0038 (Chapter 6.4 §4):** `Rule: Clone` requires `Sized`, and `dyn Rule` is unsized. Once that's removed,
  `explain<W: fmt::Write>` is a generic method with no single vtable slot (verified with `review-rules-sdk-step2.rs`: the
  compiler reports one reason at a time).
- **E0308 (a consequence):** `dyn Rule` isn't a valid type, so the unsizing coercion `Box<LargeAmount>` →
  `Box<dyn Rule>` can't happen, and `vec![Box::new(LargeAmount{..})]` stays `Vec<Box<LargeAmount>>`. It disappears when
  E0038 is fixed.

**2. The silent bug.** `block_at` defaults to `0`, and `score >= 0` is always true for `u32`, so *every* payment is
blocked by *every* rule: `engine.blocked(&small)` returns `true` for a 12.00 payment. It breaks Chapter 6.1 §10's rule
(a default must be right for an implementor who never heard of it). Worse, it puts *policy* (the threshold) into each
rule. The threshold belongs to the engine's configuration.

**3. Threads.** Nothing requires `Send`/`Sync`. Sharing `Arc<Engine>` with `thread::spawn` fails with E0277, because
`Arc<T>` is `Send` only if `T: Send + Sync`, and `dyn Rule` is neither. It's the same error family as
`ch04-08-dyn-send.rs`. Fix: `trait Rule: Send + Sync`, which the redesigned listing uses to share the engine across
three threads.

**4. v0.2** (as in `review-rules-sdk-fixed.rs`):

```rust,ignore
pub trait Rule: Send + Sync {
    fn id(&self) -> &str;
    fn score(&self, txn: &Txn) -> u32;
    fn explain(&self, txn: &Txn, out: &mut dyn fmt::Write) -> fmt::Result;
}
pub struct Engine { rules: Vec<Box<dyn Rule>>, block_at: u32 }
impl fmt::Display for Engine { /* ids + threshold */ }
```

`explain` takes `&mut dyn fmt::Write`, which is dyn compatible, and a `String` works as the writer. `Clone` is gone,
since the engine is shared through `Arc`. If copies are needed, add `clone_box`. The engine sums scores and compares
against *its* threshold, and explains only the rules that fired.

**Design questions.**

- **Core rules and `dyn`:** by the ratio test, dispatch is irrelevant at the rule level (~30 µs of work per call). By the
  matrix, a closed set owned by the fraud team is the enum's column: exhaustive `match`, no allocation. Keep the core as
  an enum implementing `Rule`, and use `dyn` for the experimental slot. `dyn` everywhere is also defensible, since the
  cost is invisible, if uniformity matters more than exhaustiveness.
- **`.so` plugins:** no. `dyn Rule` across separately compiled binaries has no stable ABI, and nothing checks
  compatibility at load time. Offer a C-ABI interface (a hand-built vtable like `FfiRule`, with explicit versioning and a
  documented thread-safety contract), a WebAssembly sandbox, or the out-of-process scoring sidecar, which is what
  Chapter 6.5 §9 chose.
- **Breaking after 1.0:** adding `Send + Sync` supertraits (implementors holding `Rc` or `Cell` break), changing
  `explain`'s signature, removing `Clone` (users who cloned `T: Rule` break), removing `block_at`, adding required
  methods, and any change that makes `Rule` dyn-incompatible. Make these decisions before 1.0, and seal what you can.

---

## Part VI Review — Interview mode

**1.** `fn f<T: Trait>(x: T)` and `fn f(x: impl Trait)` are both static: one copy per concrete type, direct calls, often
inlined. The only difference is that the second can't be named or turbofished. `fn f(x: &dyn Trait)` is one copy that
takes a fat pointer and calls through the vtable.

**2.** Associated types for outputs determined by the impl, where there's one impl per `Self` (`Iterator::Item`).
Parameters for inputs, where there are many impls per `Self` (`From<T>`). Both in one trait when there are inputs and a
determined output (`Add<Rhs> { type Output }`).

**3.** At most one impl per (trait, type) in the program. An impl may be written only by the crate owning the trait or
a type in its header, with no uncovered type parameters before the first local type. A third crate owns neither, so
allowing it would let two unrelated crates write conflicting impls that no single compilation could detect.

**4.** A vtable is a finite table of function pointers, each called with an erased `self`. So no generic methods (an
unbounded family), no `Self` by value (unknown size), no `Sized` supertrait (contradiction), no receiver-less
associated functions (no vtable to reach), no `async fn` or RPITIT (a different hidden type per impl). The escape
hatch is `where Self: Sized`.

**5.** Candidate receiver types: `Rc<Box<String>>`, `Box<String>`, `String`, `str` (each step via `Deref`). For each,
try `U`, `&U`, `&mut U`, with inherent methods before in-scope trait methods. `len` is found as `String::len` at
`&String`. User-defined `Deref` impls take part in the chain exactly like std's, and our `Tracked<T>` counted its calls.

**6.** In "intercrate mode": it may not assume a foreign type lacks a foreign trait, because upstream may add the impl
in a minor release. Negative reasoning is allowed only for local types and traits. That keeps "adding an impl" a
non-breaking change for the whole ecosystem.

**7.** Drop glue (null in current rustc when there's none), size, align, then methods in order, plus supertrait data
for upcasting. Guaranteed: the capabilities (dispatch, drop, `size_of_val`). Everything about layout and uniqueness is
an implementation detail.

**8.** Java erases generics: a generic method is one method over `Object`, so it fits one slot. Rust monomorphizes: a
generic method is a separate function per type argument, and a vtable can't list an open-ended family.

**9.** Beyond the call: the two dependent loads, argument setup, a possible misprediction, and above all the lost
optimization. In the verified assembly, the dynamic loop spilled and reloaded its `f64` accumulator around every call
(all XMM registers are caller-saved in System V), while the static loop was unrolled ×8 with the arithmetic inlined.

**10.** Identical instructions, a different target sequence: grouped targets are predictable. Prove it with
`perf stat -e branch-misses,instructions`, and control for allocation order.

**11.** When many types times large generic bodies multiply compile time and code size, especially nested generic
types in libraries compiled into every user. Remedies: a non-generic inner function, `dyn` at boundaries, fewer type
parameters, and measuring instantiations (Part VII, Chapter 7.3).

**12.** Middleware pipeline: `dyn` (configuration-driven, per-request cost irrelevant), or the hybrid static core plus
dynamic `Plugins` slot. Rule engine: an enum for the closed core, `dyn` for the experimental slot, out of process for
code owned by other teams. Decoder inner loop: static only, with an enum for field kinds, and at most one `dyn` per
feed, called per message.

**13.** Add methods with defaults only when the default is correct for unknown implementors, and choose distinctive
names (E0034 risk). Treat new required methods, new blanket impls, and loss of dyn compatibility as major changes.
Seal traits you don't want implemented externally. Guard dyn compatibility with `fn _assert(_: &dyn Trait) {}`, and
check with `cargo-semver-checks` (Part XXII).

**14.** In-process plugins built with the host: a factory registry of `Box<dyn Plugin + Send + Sync>`. Separately built
plugins: a C ABI with a hand-built, versioned vtable, a plugin-provided drop, and a documented thread-safety contract.
Untrusted or independently deployed plugins: WebAssembly or out of process. Decide by isolation needs, per-call cost
(ratio test), and deployment ownership.

**15.** The JVM wins on dynamic-in-source, monomorphic-in-practice call sites, which get inlined automatically, and on
flexibility (class loading, a stable bytecode ABI for plugins). Rust wins on predictability (no warm-up, deopt, or GC
pauses), on explicit static dispatch, on unboxed monomorphized data, and on tail latency. For a latency-sensitive
service, Rust's model puts the performance decision in the source, where review can see it.
