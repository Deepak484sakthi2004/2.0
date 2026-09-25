# Appendix A — Answer Key: Part II

> Model answers. Write yours first. Where several answers are defensible, the key says so.

---

## Chapter 2.1 — The Toolchain

### Interview & architecture questions

**1. Package, crate, module, workspace.** A **package** is a `Cargo.toml` plus the crates it builds: at most one
library and any number of binaries, examples, tests, and benches. It's close to a Maven module. A **crate** is one
compilation unit (one rustc invocation), a library or a binary. It's close to a JAR as a *distribution* unit, but it's
also the unit of privacy (`pub(crate)`), trait coherence, and incremental compilation, and generic code from a library
crate is compiled in the *user's* crate. A **module** is a namespace inside a crate. It's like a Java package, except
modules nest *for access* (children see their ancestors' private items). A **workspace** is several packages sharing one
`Cargo.lock` and one `target/`, like a multi-module Maven build.

**2. What's in an `.rlib`.** Object code plus **metadata**: types, signatures, trait impls, exported items, and the MIR
of generic and `#[inline]` functions. Downstream crates need the metadata to type-check against the library and to
**instantiate generics and inline code** inside their own compilation. A JAR ships bytecode that works for every type
argument (erasure), and it's linked at run time. An `.rlib` ships a recipe, because machine code can't be generic.

**3. Two major versions in one binary.** Cargo unifies each *semver-compatible* range to one version, but treats
different majors (or different `0.x` minors) as **different crates**, told apart by a metadata hash that also goes into
symbol names. The costs: bigger binaries and slower builds, **types that can't cross the boundary** (the "expected
`StdRng`, found `StdRng`" error), and duplicated global state (two registries, two caches). Java's classpath has one
class per fully qualified name, so conflicts resolve to a single version, often the wrong one, and fail at run time
with `NoSuchMethodError`. The workarounds there are shading or JPMS/OSGi.

**4. Editions.** An edition is a per-crate *source-level* dialect. It can reserve keywords, change defaults and
desugarings, and turn lints into errors. Crates of different editions link freely because edition differences are
resolved during parsing and lowering. After that, everything is the same compiler IR and the same type system. An
edition can't change the ABI, std's behavior, or anything that would make crates of different editions disagree about
a shared type.

**5. `target-cpu=native`.** It compiles for the build machine's instruction set extensions. If production CPUs lack
them (a heterogeneous fleet, older nodes), the binary executes an illegal instruction (`SIGILL`) whenever it reaches
code using them, which may be minutes after startup, on a subset of nodes. Safer: an explicit baseline that the whole
fleet supports (`x86-64-v2` or `-v3`), per-node-type builds, or runtime feature detection with multiversioned hot
functions.

**6. What `Cargo.lock` pins, and what it doesn't.** It pins exact dependency versions, checksums, and git revisions. It
**doesn't** pin the toolchain (use `rust-toolchain.toml`), `RUSTFLAGS` and target/CPU settings, what build scripts do
(network fetches, `pkg-config` finding system libraries), the system linker, C compiler, and libc, the environment
variables that influence builds, or path and timestamp nondeterminism (`--remap-path-prefix` helps). A reproducible
build pins all of them, ideally in a container image.

**7. Pipelined compilation and `cargo check`.** rustc emits a crate's metadata (`.rmeta`) before code generation
finishes, and cargo starts compiling dependents as soon as that metadata exists, overlapping upstream codegen with
downstream type-checking. `cargo check` stops after producing metadata. It skips LLVM codegen, usually the most
expensive phase, which is why it's much faster and ideal for the edit loop.

**8. musl static binaries.** Benefits: one self-contained file, `FROM scratch` images, no dependency on the host glibc
version, a smaller base-image CVE surface. Catches: musl's `malloc` is slow under multithreaded load (swap in mimalloc
or jemalloc); DNS and NSS behavior differs from glibc; you can't `dlopen` glibc-based libraries; some routines
(memcpy and similar) may perform differently. Measure.

### Debugging exercise (`gen` in edition 2024)

1. The colleague's crate declares `edition = "2021"` (or older), where `gen` isn't reserved.
2. **Rename** (`let generation = 5;`), the right fix for your own code, or use a **raw identifier** (`r#gen`) when the
   name is imposed from outside: an API field, a generated binding, a serialized field name.
3. The `let` failed to parse, so no binding was created. The `{gen}` in the format string then refers to a value that
   doesn't exist, and rustc's error recovery spells it `r#gen` because `gen` is now a keyword. **Cascading errors:** a
   compiler recovering from one error produces follow-on errors. Fix the first error first, then recompile.

### Selected exercises

- **Intermediate:** integration tests in `tests/` are **separate crates** that link your library like any other
  dependency, so they can only use its public API. That's exactly what makes them *integration* tests.

---

## Chapter 2.2 — Cargo.toml, Features, and Profiles

### Interview & architecture questions

**1. Why features must be additive.** A crate is compiled **once** per build, with the **union** of the features any
crate in the graph requests. If two features are mutually exclusive, two dependents choosing differently turn both on,
so you get conflicting code, `compile_error!`, or silent behavior changes for the dependent that expected the other one.
Features may only *add* capability.

**2. `debug_assert!` vs `assert!`.** `debug_assert!` documents internal assumptions that tests should exercise and that
are too expensive to check in production. It's **removed** in release. `assert!` guards invariants whose violation must
stop execution in every build, especially cheap checks that `unsafe` code relies on. **Neither** is for validating
external input or business rules. Those return `Result`.

**3. `lto = "thin"` + `codegen-units = 1`.** One codegen unit hands LLVM the whole crate as a single module, so there's
more inlining within the crate but no parallel codegen for it. Thin LTO adds cross-crate optimization at link time
(summary-based inlining across crates including std, internalization, dead-code elimination) and remains parallel. The
result is usually faster and smaller code. The cost is longer builds and more build memory. Measure both sides.

**4. Panic strategy.** (a) Multi-tenant Tokio gateway: **unwind**. Tokio turns a task's panic into a `JoinError`, so the
other in-flight requests survive. (b) A library loaded into a JVM: **unwind, plus `catch_unwind` at every exported
function**, converting panics to error codes. A panic must never cross into JVM frames (since 1.81 an unwind out of
`extern "C"` aborts the process, which is the JVM), and with `abort` nothing could catch it. (c) CLI: **abort**. It's a
bug, so fail fast with a smaller binary.

**5. Disabled `#[cfg]` code isn't type-checked.** `cfg` stripping happens during macro expansion, before name resolution
and type checking. The code only has to parse. So CI must build every relevant feature combination
(`cargo hack --each-feature` or `--feature-powerset`), treat `unexpected_cfgs` as an error (`-D warnings`), and build
platform-specific code on those platforms.

**6. Caret requirements.** `"1.2.3"` means `>=1.2.3, <2.0.0`. `"0.2.3"` means `>=0.2.3, <0.3.0`. `"0.0.3"` means
`>=0.0.3, <0.0.4`. Cargo treats the **leftmost non-zero** component as the breaking one. Pre-1.0 crates get to make
breaking changes in minor releases while still shipping compatible patch releases.

**7. A 40 MB binary.** First check whether debug info is included. It usually dominates, and stripping or splitting it
is free at run time. Then: duplicate dependency versions (`cargo tree -d`), features pulling in heavy dependencies,
generic bloat (`cargo llvm-lines`, `cargo bloat`), then `opt-level = "s"`/`"z"` if size really matters, then LTO with one
codegen unit, then `panic = "abort"`, then structural changes (non-generic inner functions, `dyn` at cold boundaries).

**8. Per-package `overflow-checks`.** For crates where silent wraparound is unacceptable (money, quotas) without paying
for checks in crates where wrapping is intended (hashing, compression). Use
`[profile.release.package.billing] overflow-checks = true`. Explicit `checked_*` arithmetic in the code is even better,
since it doesn't depend on the profile at all.

### Debugging exercise (the unchecked `metrics` feature)

1. The `#[cfg(feature = "metrics")]` items were removed during **macro expansion**, before name resolution and type
   checking. The feature was never on in CI, so the code was never checked.
2. The feature `metrics` **wasn't declared** in `Cargo.toml` at all ("no expected values for `feature`"). The code was
   guarded by a feature nobody could enable through Cargo's normal mechanism, a strong sign it was dead or misnamed.
3. CI: `cargo hack check --each-feature` (or a feature matrix), and `-D warnings` so `unexpected_cfgs` fails the build.
   Review rule: every new `cfg(feature = ...)` needs a declared feature and a CI job that enables it.

### Selected exercises

- **Beginner:** use `if pct > 100 { return Err(...) }` (or `assert!`), plus `checked_sub`/`checked_mul` for the
  arithmetic, so both profiles behave identically.

---

## Chapter 2.3 — Bindings and Scalars

### Interview & architecture questions

**1. Does `x` exist?** Not guaranteed. In debug builds rustc gives named locals **stack slots written for the
debugger** (`x.dbg.spill`), while the computation itself flows through SSA values. In release builds values are folded
into constants, live briefly in registers, or vanish. Things that force memory: an address that escapes (passed to a
non-inlined function, stored, printed with `{:p}`), aggregates too large for registers, and register pressure across
calls (spills).

**2. Mutability on the binding.** Values move between bindings (`let mut a = b; let c = a;`), so mutability as a
property of the value would be meaningless. It restricts what you can do with a *name*: reassign it, or take `&mut` to
it. In generated code, `mut` changes nothing. It's SSA either way. It changes which borrows the checker allows.

**3. SSA.** Every value is assigned exactly once. A reassignment creates a new version, and φ nodes choose among
versions at control-flow merges. A loop counter becomes
`%i = phi [0, %entry], [%i.next, %loop]`.

**4. Inference stops at signatures.** A signature is a contract readable without the body. That gives local reasoning,
errors near their cause, stable APIs (editing a body can't change a public type), bounded inference cost, and per-function
borrow checking. Java's `var` infers from the initializer only. Rust infers from *every use in the body*, and never across
signatures (`impl Trait` returns are opaque by design).

**5. Sorting `f64`.** IEEE 754 ordering isn't total: NaN is unordered, so `NaN < x`, `NaN > x`, and `NaN == x` are all
false. `sort` requires `Ord`, a total order, to be correct. Options: `sort_by(f64::total_cmp)` (IEEE totalOrder:
−NaN < −∞ < … < −0.0 < +0.0 < … < +∞ < +NaN); `partial_cmp(...).unwrap()` (panics on NaN); filter NaN out first;
or a newtype such as `ordered_float`.

**6. The four casts.** `300i32 as u8` = 44 (truncation mod 256); `-1i32 as u32` = 4294967295 (the same bits,
reinterpreted); `1e20 as i32` = `i32::MAX` (saturating); `NaN as i32` = 0. Before 1.45, an out-of-range float-to-int
`as` was **undefined behavior** (LLVM's `fptosi` produces poison), a soundness hole in safe Rust. 1.45 defined it as
saturating, at a cost of a few instructions. `to_int_unchecked` remains as an `unsafe` opt-out.

**7. `Option<bool>` vs `Option<u8>`.** `bool` has 254 invalid bit patterns, a niche, so `None` takes one of them and the
size stays 1 byte. Every `u8` pattern is valid, so the discriminant needs its own byte: 2 bytes.

**8. Money with JavaScript clients.** Inside the service: `i64` minor units (or a fixed-scale decimal) plus a currency
code, with checked arithmetic. At the JSON boundary: strings (or a units-plus-nanos pair), because JavaScript numbers are
doubles, exact only up to 2⁵³ − 1. At the database: `DECIMAL`/`NUMERIC` or `BIGINT` minor units. Conversions (`TryFrom`
with validation) happen only at those boundaries, since that's the only place representations change.

### Debugging exercise (sorting `f64`)

1. `f64` isn't `Ord` because comparisons involving NaN are all false: with `a = NaN` and `b = 1.0`, neither `a < b`,
   `a > b`, nor `a == b` holds. That violates totality, and a sort given an inconsistent comparator can produce garbage
   orderings.
2. `{float}` is an **inference variable** for a float literal whose concrete type hasn't been fixed yet (it would
   default to `f64`). The error surfaced before defaulting happened.
3. `total_cmp` put `-0.0` before `0.0` and positive NaN last. For prices, NaN is a **data error** and should have been
   rejected at the boundary. Sorting it last hides the problem.

### Selected exercises

- **Beginner:** `(-1i8) as u8` = 255; `256u16 as u8` = 0; `-3.7f64 as i32` = −3; `(-3.7f64) as u8` = 0 (saturates at
  the lower bound); `u16::try_from(-1i32)` = `Err(...)`.
- **Intermediate:** sum into `u128`. A slice holds at most `usize::MAX` elements, so a `u128` sum of `u32`s can't
  overflow. A `u64` sum *can*, once there are more than about 2³² maximal values, which is 16 GiB of input: rare, but
  not impossible. Then divide, and convert back with `u32::try_from` (the average is at most `u32::MAX`, so it always
  fits). Return `None` for an empty slice.

---

## Chapter 2.4 — Compound Types and Functions

### Interview & architecture questions

**1. The semicolon.** `;` turns an expression into a statement and discards its value. A block's value is its **tail
expression**. With `sum;` there's no tail, so the block's value is `()`, which doesn't match the declared `u64`, hence
E0308.

**2. `()` vs `void`.** `()` is a zero-sized type with exactly one value, so generic code needs no special case:
`Result<(), E>`, `HashMap<K, ()>` (literally how `HashSet` is built), futures and channels of `()`. Java needs `Void` plus
`null` and can't write `List<void>`.

**3. `!`.** The never type has no values. Expressions of type `!` never complete (`panic!`, `return`, `loop {}`,
`process::exit`). It coerces to any type, so match arms unify, and the compiler knows the code after it is unreachable.

**4. Returning `(u64, u64)` and `[u64; 8]`.** The pair comes back in `rax:rdx`. For the array, the caller reserves the
space and passes a hidden pointer in `rdi` (`sret`), the callee writes straight into it, and `rax` returns that pointer.
It's not a copy, so returning large values by value is cheap. (The Rust ABI is unspecified, so this is current
practice, not a guarantee.)

**5. Zero-sized function items.** Each function item has its own unique type that identifies the function, so the
value needs no storage, calls are **direct**, and a generic function receiving it is monomorphized for that exact
function, which makes the call inlinable (static dispatch). A function *pointer* erases that identity, so calls become
indirect. Closures are also unique types, plus their captures.

**6. `[T; N]` vs `Vec<T>` vs `&[T]`.** An array is `N` elements inline, with no header; its length lives in the type,
and it lives wherever its owner lives. A `Vec` is a 24-byte handle (pointer, capacity, length) plus a growable heap
buffer. A `&[T]` is a 16-byte fat pointer (pointer, length) that borrows contiguous data. Use arrays for small fixed
data, `Vec` for owned dynamic collections, and `&[T]` for parameters.

**7. No guaranteed tail calls.** Destructors run at scope exit, *after* the call, so many "tail" calls aren't really
tail calls. Guaranteed TCO also interacts badly with debugging and backtraces, ABI constraints, and backend support.
(`become` is reserved and unstable.) Instead: iteration with an explicit heap-allocated stack or queue, depth limits on
untrusted input, or a larger thread stack for depth that's known to be bounded.

**8. The 4 MiB array.** The main thread's stack is OS-configured (commonly 8 MiB on Linux), so it fits there. Spawned
threads default to 2 MiB, so the frame runs into the guard page. The OS delivers `SIGSEGV`, and Rust's handler recognizes
a guard-page hit, prints "has overflowed its stack," and **aborts**. It can't unwind as a panic would, because the stack
is exhausted: there's no safe place to run unwinding code, and it's an OS signal, not a Rust-level panic.

### Debugging exercise (the trailing semicolon)

1. `sum;` is a **statement** (an expression plus `;`), so its value is discarded. The block then has no **tail
   expression**, so its value is **unit**, `()`.
2. The contract being broken is the **signature**: the block's type `()` doesn't match the declared return type `u64`.
   The help points to the semicolon as the likely fix.
3. `items.iter().sum()`. The release assembly of both versions should be essentially identical (Chapter 1.3 showed the
   same effect). Verify it yourself.

### Selected exercises

- **Beginner:** `fn grade(score: u32) -> char { if score >= 90 { 'A' } else if score >= 75 { 'B' } else { 'F' } }`.
- **Intermediate:** in a public API, return a named struct (`Stats { min, max, mean }`), which is self-documenting and
  can grow fields. A tuple is fine for a private helper.

---

## Chapter 2.5 — Control Flow and Pattern Matching

### Interview & architecture questions

**1. Exhaustiveness.** Every value of the scrutinee's type matches some arm. rustc's usefulness algorithm (after
Maranget, 2007) asks whether a trailing `_` would still be useful. If it would, the values it would catch are the
missing cases. Integer types are split into disjoint ranges (0–9, 10–99, 100–254, 255), and the uncovered piece is
reported as a **witness**: `u8::MAX`.

**2. Refutable vs irrefutable.** Irrefutable patterns are required in `let`, function parameters, closure parameters,
and `for` patterns. Refutable ones are allowed in `match`, `if let`, `while let`, and `let ... else`. `let-else` binds
at the *current* scope level with an early exit on mismatch, which avoids the rightward drift of nested `if let` and
`match`.

**3. Guards.** A guard is an arbitrary boolean expression the checker can't reason about (undecidable in general), so an
arm with a guard counts as covering nothing. You need a fallback arm.

**4. Match lowering.** LLVM chooses: dense cases with constant results become a **lookup table**; dense cases with
different code become a **jump table**; sparse cases become a **comparison chain** or binary search. The choice depends
on density, case count, and whether arms are constants, and profile data can shift it.

**5. Match ergonomics.** When a non-reference pattern matches a reference, the compiler dereferences automatically and
switches bindings to borrow (`ref`). So `s` is `&String`.

**6. `#[non_exhaustive]`.** Downstream crates must add a wildcard arm, which lets the library add variants in a minor
release without breaking anyone. Downstream code loses compile errors for new variants. It's worth it for enums that
will genuinely grow, such as error kinds and protocol messages.

**7. Rust vs Java 21.** Java checks exhaustiveness over sealed types and enums at compile time. But because Java links
at run time, a class or constant added later in a separately compiled module reaches a javac-inserted default that throws
`MatchException`. Rust links statically and recompiles against changed dependencies, so the proof is redone. Only
`#[non_exhaustive]` makes you write the fallback up front.

**8. `for` desugaring.** `IntoIterator::into_iter(expr)`, then `loop { match iter.next() { Some(p) => body, None =>
break } }`. `for x in vec` calls `Vec::into_iter`, which consumes the vector and yields `T`. `for x in &vec` calls
`<&Vec<T>>::into_iter`, which yields `&T` and leaves the vector usable.

### Debugging exercise (`u8::MAX` not covered)

1. The three fixes are `255 => ...`, `100..=u8::MAX => ...`, and `_ => ...`. **`100..=u8::MAX` is best**: it states the
   intent ("three digits: 100 up to the maximum"). The wildcard is the worst, because it also swallows anything a future
   change introduces.
2. With `n: u16`: the explicit-255 version fails with E0004 (`256_u16..=u16::MAX` not covered); the `100..=u8::MAX`
   version fails with a type mismatch (`u8::MAX` isn't a `u16`); the `_` version **compiles** and silently files 1000
   as "three digits." None of them is still correct, and only the wildcard hides that.
3. Write patterns precise to the domain and avoid wildcards on types that might change. Then a type change becomes a
   compile error instead of a silent behavior change.

### Selected exercises

- **Advanced:** the `match` gives compile-time exhaustiveness (a new state forces decisions), but changes need a deploy.
  The table is data (changes need no deploy, it can be audited as data), but a missing row is a run-time miss and a
  malformed table is a startup-time failure at best.

---

## Chapter 2.6 — Structs, Enums, and Methods

### Interview & architecture questions

**1. Receivers.** `&self`: a shared, read-only borrow, and the caller keeps ownership (`Vec::len`, `HashMap::get`).
`&mut self`: an exclusive borrow for mutation (`Vec::push`, `String::push_str`). `self`: takes ownership and consumes
the value (`Vec::into_iter`, `String::into_bytes`, `Option::unwrap`, a builder's `build`).

**2. No constructors.** Struct literals are the construction primitive. That avoids the problems constructors bring in
other languages: partially initialized objects, constructor chains under inheritance, exceptions mid-construction.
Associated functions (`new`, `try_new`) return `Self` or `Result<Self, E>`, and **private fields** force all
construction through them, so invariants are established at creation ("parse, don't validate").

**3. `acct.deposit(5)`** becomes `Account::deposit(&mut acct, 5)`. Method lookup finds `deposit` with receiver
`&mut Self`, and the compiler auto-borrows `acct` mutably, which requires `acct` to be a `mut` binding. It's a direct
static call.

**4. 16 vs 24 bytes.** The default repr lets rustc reorder the fields: `u64` first, then the two `u8`s, then 6 bytes of
padding, for 16. `repr(C)` keeps declaration order: `u8` + 7 padding, `u64`, `u8` + 7 padding, for 24. Use `repr(C)`
when the layout is an interface: FFI, binary file formats, shared memory, hardware.

**5. Niches.** `Shape`'s tag uses 3 of the values available in its (padded) tag byte, so `Option<Shape>` stores `None`
as a fourth tag value, and the size stays 24. `MessageBoxed` already uses the `Box`'s only niche (null) for `Ping`, and
rustc doesn't use alignment bits, so `Option` needs a real tag and padding: 16.

**6. Derived `PartialEq`.** It compares every field in declaration order (`&&`) and adds the `StructuralPartialEq`
marker. It's wrong when some fields are caches or metadata (a `last_accessed` timestamp, a memoized hash), when semantic
equality differs from structural equality (case-insensitive emails, `1.0` vs `1.00`), and with floats (a NaN field makes
`x != x`). Derived `Debug` is wrong for secrets such as tokens, passwords, and card numbers.

**7. Enum vs `Option` fields vs trait objects.** An **enum** for a closed set of alternatives you own, each with its own
data, where invalid combinations must be unrepresentable. A **struct with `Option` fields** for attributes that are
genuinely independent and optional (a nickname, a phone number), and for easy schema evolution. **Trait objects** for
open sets that other crates extend (plugins), where the set of operations is fixed.

**8. Java enum vs Rust enum.** A Java enum is a fixed set of **singleton objects of one class**. Every constant has the
same fields, set once, and they're heap references. A Rust enum is a **sum type**: each variant has its own data,
values are created freely (they aren't singletons), they're stored inline as a tagged union, and they're consumed by
pattern matching.

### Debugging exercise (the consumed builder)

1. `timeout_ms(self)` takes the builder by value. The call **moves** `builder` into the method, the configured builder it
   returns is dropped because nothing binds it, and every later use of `builder` is a use-after-move. The same happens
   again with `retries`, hence two errors.
2. Chaining: `let config = ClientBuilder::new().timeout_ms(250).retries(3).build();`. Rebinding:
   `let builder = builder.timeout_ms(250); let builder = builder.retries(3);`.
3. Make the setters `&mut self -> &mut Self`. Then `build` can't move fields out of a borrow: it takes `&self` and
   **clones** them (or takes `&mut self` and uses `mem::take`). That's cheap for integers and costly if the builder holds
   `Vec`s or `String`s.
4. E0596: cannot borrow `builder` as mutable, because the binding isn't `mut`. It's the same error as §3's `Counter`, and
   the fix is `let mut builder`.

### Selected exercises

- **Advanced (predictions):** `Option<Option<bool>>` = 1 (nested niches); `Result<u32, ()>` = 8; `Option<(u8, bool)>` = 2
  (the `bool` field provides the niche); `enum E { A(u32), B(u16), C }` = 8; `Option<Box<str>>` = 16 (a fat pointer; the
  null niche makes `Option` free). Verify all of them with `size_of`.

---

## Chapter 2.7 — Modules and Visibility

### Interview & architecture questions

**1. What private means.** Visible within the module where the item is declared **and all its descendants**. Java's
`private` is per *class*. Package-private is per package and excludes subpackages. In Rust the unit is the module, so two
types in one module see each other's private fields.

**2. Privacy flows downward.** A child module is part of its parent's implementation, and a parent shouldn't depend on
its children's internals. This encourages putting shared private helpers in ancestors, implementation details in
leaves, and a facade at the top.

**3. Struct literals need every field.** To name a field you must be able to see it, so one private field blocks all
external construction, and the module's functions become the only way to create values. Invariants are established in
one place.

**4. Privacy and memory safety.** `Vec`'s `unsafe` code relies on `len <= cap` and on `ptr` being valid for `cap`
elements. If safe code outside the module could change `len`, it could make a later `v[i]` read out of bounds with no
`unsafe` in sight, which would make the API **unsound**. Privacy restricts the code that can break the invariants to the
module, so the safety argument is local.

**5. Facades.** `pub use` re-exports items at chosen paths. The public API is decoupled from the internal layout, so
you can reorganize internals without breaking users, present a flat and discoverable API, and keep implementation
modules private.

**6. Hexagonal workspaces.** Domain crate(s) with no infrastructure dependencies; adapter crates depending on the domain;
a binary that wires them together. The acyclic graph forbids domain-to-adapter imports, and adding a forbidden dependency
means editing a `Cargo.toml`, which is visible in review and enforceable with `cargo deny` bans. Costs: more manifests,
the orphan rule, `pub` surfaces between crates, version coordination, and less inlining across crates without
`#[inline]` or LTO.

**7. Public dependencies.** A dependency whose types or traits appear in your public API. Its **major version becomes
part of your semver**: upgrading it forces a major release of your crate, and users must use a matching version to
interoperate. Exposing `reqwest::Client` ties you to reqwest's release cadence, so wrap it.

**8. `#[inline]` across crates.** A non-generic function in another crate can't be inlined into your crate without
`#[inline]` (which ships its MIR) or LTO. The exception is small leaf functions, which rustc has made automatically
inlinable since 1.75. It matters for tiny hot accessors called in loops, where the lost inlining can also block
vectorization. It's unnecessary for generic functions (already instantiated in the caller's crate), large functions,
code within one crate, and builds using LTO.

### Debugging exercise (three privacy errors)

1. **E0616:** reading a field outside its visibility scope (the field is private to `billing`). **E0451:** a struct
   literal naming a private field, since constructing requires every field to be visible. **E0603:** calling a private
   function from outside its module.
2. Field-access privacy is checked during **type checking**. The type-checking error stopped compilation before the
   later privacy pass that reports E0451. In a large change, errors come in layers: fixing one phase's errors can reveal
   the next phase's.
3. (a) A test-only constructor inside `billing`:
   `#[cfg(test)] pub(crate) fn with_total(customer: &str, cents: i64) -> Invoice`. (b) Put the tests *inside* the
   module (`#[cfg(test)] mod tests` as a child of `billing`), since children see their ancestors' private items.

---

## Project Level 1 — `logstat` architecture review

**1. Bottlenecks.** Probably CPU in parsing and hashing: UTF-8 validation, splitting, two integer parses, a SipHash per
line, and a hash-map probe. A laptop SSD delivers GB/s, so I/O limits only if input comes through a single-threaded
decompressor. Find out with `perf stat` and a flame graph (`perf record`), and by comparing against
`cat file > /dev/null` throughput with a warm cache.

**2. Allocations in steady state.** New path strings on first sight, plus the path map's occasional rehash growth; the
line buffer only when a longer line appears; the report at the end. **Unbounded:** the path map, when cardinality is
unbounded.

**3. Unbounded cardinality.** (a) **Normalize** paths with rules (`/api/orders/{id}`), bounded by the number of routes;
the risk is wrong rules and lost per-ID detail. (b) A **heavy-hitters** algorithm with fixed capacity (Space-Saving or
Misra–Gries; or Count-Min Sketch plus a top-k heap): approximate counts with provable error bounds and constant memory.
(c) A simple cap: after N distinct paths, count the rest as `(other)`.

**4. Microsecond resolution up to 5 minutes.** A linear 1 µs histogram would need 300 million buckets, about 2.4 GB. Use
**log-linear** buckets (HdrHistogram-style): a bounded *relative* error (say 1%) across a huge dynamic range in a few
thousand buckets, a few KB.

**5. File unreadable mid-run.** Today `?` propagates the error, `main` prints it, exits 1, and discards the partial
summary. A reasonable improvement: print the partial report to stdout, clearly marked `PARTIAL: stopped at <file>`, and
the error to stderr, and still exit 1 so scripts see the failure. Or add `--keep-going`. Either way, decide explicitly
and document it.

**6. A continuous 200 MB/s stream.** Single-core parse throughput may fall short (measure it). "All-time" percentiles
lose meaning over a stream, so you need windows (tumbling or sliding) and periodic output. Path cardinality grows memory
without bound. A slow `logstat` backpressures the producer through the pipe. Memory from cardinality and single-core CPU
break first.

**7. Parallel design.** Split the input at newline boundaries (per file, or byte ranges within a file), and let each
thread build its **own `Summary`** (no sharing). Merge by summing `lines`, `malformed`, and `invalid_utf8`,
element-wise summing `by_class` and `latency_buckets`, summing `latency_count`, and merging `path_hits` maps by summing.
Compute top-N after the merge. **Histograms merge exactly.** Exact percentiles over stored samples would need all
samples in one place, which is a strong architectural reason to prefer mergeable summaries.

**8. Ownership boundaries.** The line buffer is owned by `ingest`; `&str` and `Record<'a>` borrow from it; `Summary`
copies a path into an owned `String` only on first sight, and counters are `Copy`. The boundary sits exactly where data
must outlive the buffer, because the buffer is cleared on the next iteration.

**9. `unsafe` and zero-copy.** No `unsafe` is needed. Zero-copy *paths* would require the input to live for the whole
run: memory-map the file (Part XIX) or read it all into memory, then use `HashMap<&str, u64>` borrowing from it. That
doesn't work for streaming stdin. An arena or interner that stores each distinct path once is the streaming equivalent,
which is essentially what the `String` keys already do.

**10. Priorities.** *Time-to-first-result:* periodic incremental output, sampling early (the first N MB, or random-seek
sampling on files), a progress indicator. *Throughput:* parallel chunked ingestion with mergeable summaries, a faster
hasher for trusted input, `memchr`-based line splitting, SIMD UTF-8 validation or byte-level parsing, `mmap` for files.
*Developer velocity:* `clap`, `anyhow`, `serde_json`, the `hdrhistogram` crate, `regex` for flexible formats, accepting
more dependencies and compile time.

---

## Part II Review — the quota-service PR

| # | Location | Problem | Production consequence | Fix |
|---|---|---|---|---|
| 1 | `[features]` | `memory-backend` vs `redis-backend` are "choose one" features | Feature unification turns both on, so behavior is undefined by design | Make the backend a run-time choice (config) with additive features, or split into crates |
| 2 | `tokio` features | `["full"]` | Slower builds, bigger binary, more attack surface | List the features actually used |
| 3 | `panic = "abort"` | Whole-process abort on any panic in a multi-tenant service | One bad request kills every tenant's in-flight work | `unwind` with task isolation, or justify `abort` in writing with a restart story |
| 4 | `debug = false` | No line tables | No symbolized backtraces or profiles in production | `debug = "line-tables-only"`, split debug info, a symbol store |
| 5 | `.cargo/config.toml` | `target-cpu=native` | `SIGILL` on nodes lacking the CI runner's CPU features | An explicit fleet baseline; re-measure the 8% against it |
| 6 | `[package]` | No `rust-version`, `[lints]`, or toolchain pin | Unreproducible builds; lints not enforced | Add `rust-version`, `rust-toolchain.toml`, `[lints]` |
| 7 | `Quota` | `pub used`, `pub limit` | Anyone can bypass `consume` and break `used <= limit` | Private fields plus getters |
| 8 | `consume` | `debug_assert!` is the only enforcement | **No quota enforcement in release builds** | Return `Result`, rejecting over-quota in every build |
| 9 | `consume` | `n as u32` truncates | `n = 2³² + 1` counts as 1: quota bypass | `u32::try_from(n)` and reject |
| 10 | `consume` | `self.used += ...` wraps in release | `used` wraps to near zero: quota bypass | `checked_add`, or `overflow-checks` for this crate |
| 11 | `Command` | `Snapshot([u8; 65536])` is inline | Every `Command` is ~64 KiB; queues balloon; every move copies 64 KiB | `Box<[u8]>`, `Vec<u8>`, or `Bytes` |
| 12 | `validate` | `_ => Ok(())` wildcard on an owned enum | `Reset`/`Snapshot` never validated; future variants auto-approved (fail open) | List the variants; fail closed |
| 13 | API | `pub mod quota` exposes internals; `tenant: String` per command | Semver surface; stringly typed IDs; an allocation per command | A facade plus a `TenantId` newtype (Part V) |

**Verdict:** *request changes.* In release builds the service doesn't enforce quotas at all (issues 8–10), and the
build configuration creates production risk (issues 3–5).

---

## Part II Review — Interview mode

1. **No storage guarantee.** A binding is a name for a value, and storage is the compiler's decision (Chapter 2.3's
   table). A JVM local is *specified* as a frame slot, even though the JIT optimizes it.
2. **`()` and `!`.** Without `()`, generic code would need special cases for "no value," as with Java's `Void`. Without
   `!`, diverging expressions couldn't unify with other branch types, and the compiler couldn't know code is
   unreachable.
3. **Exhaustiveness.** Every `match` must cover every value, which the usefulness algorithm checks. Guards count as
   covering nothing. Wildcards satisfy the checker, including for future variants, which is why they should fail closed.
4. **Receivers.** `&self` borrows to read, `&mut self` borrows exclusively to mutate, `self` consumes. They're
   ownership contracts that the borrow checker enforces at every call site.
5. **cargo and `.rlib`.** One rustc invocation per crate, with edition, profile flags, `--extern` paths, and a metadata
   hash. An `.rlib` holds object code plus metadata, *including the MIR of generic and inline functions*, which a JAR has
   no equivalent for.
6. **MIR for `x + 1`.** `_n = AddWithOverflow(copy _x, const 1)`, then `assert(!move (_n.1: bool), "attempt to compute
   ... overflow") -> [success: bbK]`. The check comes from the `overflow-checks` setting, which is on in `dev`.
7. **Match lowering.** A lookup table for dense arms that are constants, a jump table for dense arms with different
   code, a comparison chain for sparse values.
8. **Hidden errors.** The compiler stops at the first failing phase, and errors from later phases (such as E0451 behind
   E0616) only appear once earlier ones are fixed.
9. **Debug builds.** Idiomatic Rust depends on inlining (iterators, combinators, small trait methods), so at
   `opt-level = 0` everything is real calls with stack spills and overflow checks. The standard mitigation is
   `[profile.dev.package."*"] opt-level = 2`.
10. **LTO and codegen units.** Cross-crate and whole-crate optimization (inlining, dead-code elimination), in exchange
    for longer, more memory-hungry builds.
11. **Large enums.** The largest variant sets the size of every value, so `Vec`s and channels store and copy that size
    for every element. Box or `Bytes` the large payload, and assert sizes in tests.
12. **Panic strategy.** Tokio gateway: unwind, for task isolation. JVM-loaded library: unwind plus `catch_unwind` at
    every FFI entry. CLI: abort.
13. **Workspaces.** The acyclic crate graph enforces dependency direction (domain can't import I/O). It can't enforce
    things like "no blocking calls in async code," "no `unwrap` on request paths," or performance budgets. Those need
    lints, reviews, and tests.
14. **`pub` and semver.** Anything reachable publicly is API. Changing it breaks dependents. Types from other crates that
    appear in your API make *their* major version part of yours.
15. **Build flags describe the fleet.** Chapter 2.1's `SIGILL` incident: `target-cpu=native` on AVX-512 CI runners
    crashed pods on older nodes, only when requests hit auto-vectorized code paths.
