# Appendix A — Answer Key: Part VII

> Model answers. Write yours first. Where several answers are defensible, the key says so. Predictions that the book did
> not verify are marked **(predicted)**. Check them on the Playground with `tools/emit.ps1` or a run.

---

## Chapter 7.1 — Generics from Call Site to Binary

### Interview & architecture questions

**1. `process::<i32>` vs `process::<String>`.** Both come from one definition that was type-checked once. After
monomorphization they are two unrelated functions with two symbols. They differ in: the argument (a 4-byte `i32` passed
in a register, versus a 24-byte `String` passed by pointer to the caller's copy); the `Debug::fmt` they call (each is a
direct call to the concrete implementation, and inlinable); the constants `size_of::<T>()` and `type_name::<T>()`,
folded to 4/"i32" and 24/"alloc::string::String"; and the end of the function. The `String` instance runs drop glue that
frees the heap buffer if its capacity is non-zero, and the `i32` instance has nothing to drop.

**2. The collector.** It is the compiler pass that decides which concrete functions exist. It starts from roots (`main`
in a binary; in a library, the public non-generic functions, statics, and other items that must be emitted). Each time
it finds a use of a generic item with concrete arguments, it records a mono item such as `largest::<u8>`, then walks
that instance's body with the substitution applied to find more. It stops when no new items appear. A generic function
that is never reached with concrete types is still type-checked, but it never becomes a mono item, so no code exists
for it.

**3. Definition checking vs instantiation checking.** Rust type-checks a generic body with `T` abstract. The only
operations available on `T` are those its bounds promise, so a mistake in the body is reported at the body (E0369), and
a caller whose type lacks a bound is reported at the call (E0277). C++ templates are checked at instantiation: the body
is re-checked with the concrete type substituted, so an unsupported operation is reported inside the template's
implementation, often deep in a library, through a chain of instantiation notes. C++20 concepts check constraints at the
call site and give better messages there, but a constrained template's body is still not checked against its concept.
The body may use operations the concept doesn't promise, and those fail only for types that lack them.

**4. Per-instance checks.** Constant evaluation that depends on generic parameters, such as an inline `const { assert!(N
> 0) }` or an associated constant computed from `T` (verified: E0080, "the above error was encountered while
instantiating `fn first_byte::<0>`"). Some layout errors, such as an array type too large for the target, also appear
only for a concrete instance. `cargo check` stops before monomorphization, so it can miss these. CI must run a real
build (or `cargo test`, which builds).

**5. The symbol.** `_R` means v0 mangling. `I … E` wraps a generic instantiation. `Nv` begins a value-namespace path.
`Cs…_10playground` is the crate root `playground` with its disambiguator. `8byte_len` is the identifier. The generic
argument is `RSh`: `R` (shared reference), `S` (slice), `h` (`u8`), so `&[u8]`. `B2_` is a back-reference that names
the instantiating crate. Read aloud: `playground::byte_len::<&[u8]>`. In production, this means profilers, flame graphs,
debuggers, crash reports, and size tools show *which instance* is hot or large, rather than one shared name.

**6. Chapter 1.3's assembly.** Monomorphization turned every adapter (`Filter`, `Map`, `Sum`, `fold`) and every closure
into a concrete instance whose callees are known. Every call in the tower is a static call to a known body. Inlining
then folded the whole tower into its caller, leaving one loop that the ordinary loop optimizations handled (verified: 22
functions in debug IR, 2 in release IR). Monomorphization alone gives static calls but keeps the calls (the debug build
executes about ten calls per element). Inlining alone can't work through indirect calls (`dyn Fn`) unless something
devirtualizes them first, which is what the JVM's profile-guided speculation does at run time.

**7. Lifetimes.** They are not monomorphized. Lifetimes exist only for borrow checking and are erased before
monomorphization. They cannot change a value's size, layout, or the code that operates on it, so one copy serves every
region. (This is Chapter 4.3's statement, now with its reason.)

**8. Cross-crate instances.** The instance is compiled in the crate that uses the generic item with concrete types,
usually downstream. The library ships generic MIR in its metadata, and your crate instantiates and optimizes it. For
build times, this means generic-heavy dependencies cost time in *your* crate, often the final binary crate, which is on
the critical path. [RUSTC] In unoptimized builds, rustc can reuse instances that upstream crates have already exported
(shared generics), which removes some duplicates.

### Debugging exercise (`mean` over `u64`)

1. **E0277**, "the trait bound `f64: From<u64>` is not satisfied", pointing at the argument `&[1u64, 2, 3]` on line 9,
   with a note that the requirement comes from the bound `Into<f64>` in `mean` (verified in
   `listings/part-07/ch01-10-no-from-u64.rs`). By the E0369/E0277 split, the *caller* is at fault: the bound says
   "losslessly convertible to `f64`", and `u64` isn't.
2. **Three fixes.**
   - *Bound:* define a trait with an explicit, documented lossy conversion (`trait AsF64 { fn as_f64(self) -> f64; }`
     with a doc comment about 2^53). This makes the loss a stated contract instead of an accident.
   - *Caller:* convert at the call site, `counts.iter().map(|&c| c as f64)`, and call a non-generic `mean(&[f64])`. The
     `as` is visible in review.
   - *Algorithm:* sum exactly in an integer type (`u128` for safety), then divide once: `sum as f64 / n as f64`. There
     is one rounding at the end instead of one per element.

   For request counts, the algorithm fix is best: counts are far below 2^53 per element, but the integer sum is exact
   and faster. For account IDs, none applies. Averaging IDs is meaningless, and the exercise exposes a design error.
   IDs should never be converted to `f64` at all (§10).
3. The compile error surfaces the loss before any data flows. The `ToF64` version compiled, ran, and silently merged
   distinct users. The standard library's choice not to implement `From<u64> for f64` is a policy (`From` means lossless),
   and a bound on `Into<f64>` inherits that policy for free.

### Selected exercises

- **Beginner.** `size_of::<&str>()` is 16 (pointer and length) and `size_of::<Vec<u8>>()` is 24 (pointer, capacity,
  length) on 64-bit targets. Both types need `Debug`, which they have.
- **Advanced.** A `const { assert!(N.is_power_of_two()) }` in `Ring::new` turns `Ring::<u32, 6>::new()` into a build
  error: E0080, with a note that it was encountered while instantiating the `new` instance for `N = 6` **(predicted from
  the verified `first_byte` case)**. Whether it belongs in the type depends on whether the wrong value is a programmer
  error that should never ship (then yes, the check is free at run time) or a configuration value (then the size
  shouldn't be a const parameter at all).
- **Systems.** **(predicted)** `u16` uses unsigned compares (`cmova`), `i8` signed ones (`cmovg`), and `f32` the
  single-precision SSE equivalents of the `f64` sequence (`maxss`, `cmpltss`, and a blend). Emit the release assembly
  to confirm, and note the unrolling factor, which depends on element size.

---

## Chapter 7.2 — Monomorphization vs Java Type Erasure

### Interview & architecture questions

**1. Homogeneous vs heterogeneous.** Homogeneous translation compiles one body for all type arguments, using a uniform
representation (in Java, a reference to an object) and inserting casts where values of `T` flow out. Heterogeneous
translation compiles a body per type argument, each with the type's natural representation. Java is homogeneous
(erasure). It chose erasure for **migration compatibility**: generics arrived in Java 5 without JVM changes, erased code
interoperates with pre-generics bytecode and raw types, and libraries could adopt generics independently. Rust is
heterogeneous (monomorphization), because it wanted zero-cost abstraction and had no legacy bytecode to stay compatible
with.

**2. Bridge methods.** When a class overrides or implements a generic method with a more specific signature, for example
`compareTo(Money)` for `Comparable<Money>`, its method doesn't match the erased signature `compareTo(Object)` that
callers of the interface invoke. `javac` generates a synthetic bridge `compareTo(Object)` (flags `ACC_BRIDGE` and
`ACC_SYNTHETIC`) that casts its argument to `Money` and calls the real method. At run time, every call through the
interface goes through the bridge. If a wrong type arrives (through raw types or heap pollution), the bridge's cast throws
`ClassCastException` from a line with no visible cast.

**3. `new T[n]`.** Java arrays are reified and covariant: an array knows its component type at run time and checks every
store (`ArrayStoreException`). Creating one needs the component type at run time, which erasure has removed. So `new
T[n]` is illegal, and the workarounds are an `Object[]` with an unchecked cast, or a `Class<T>` token with
`Array.newInstance`. In Rust, each instance knows `T`'s size and layout at compile time. `vec![T::default(); n]` needs
`T: Default` (to make one value) and `T: Clone` (the macro clones it `n - 1` times). The signature must state both
bounds.

**4. JIT recovery.** A *type profile* is the JVM's record, per bytecode location, of the receiver classes seen at a
virtual or interface call, or the classes seen at a `checkcast`/`instanceof`. A *monomorphic* site has seen one class.
C2 inlines that class's method behind a class check. A *bimorphic* site has seen two, and gets two inlined paths. A
*megamorphic* site (more than two) stays a real virtual or interface dispatch. An *uncommon trap* is the path taken when a
guard fails: the compiled code is deoptimized back to the interpreter and later recompiled with the new profile. *Profile
pollution* happens because profiles belong to the shared bytecode of a generic method, not to its callers. Every caller's
types accumulate in one profile, so a site becomes megamorphic even for a caller that only ever passes one type.
Inlining the method into that caller can recover precision, but only if the method is small and shallow enough.

**5. Vectorized `u32`, sequential `f64`.** Integer addition (wrapping in release) is associative and commutative, so the
compiler can split the sum into SIMD lanes and several accumulators and combine them at the end. Floating-point addition
is not associative: reordering changes rounding. The compiler must keep the source order, so it emits one sequential
chain of `addsd`, unrolled but not parallel. A single shared body could do neither. It would operate on boxed or erased
values through dynamic calls, so it could not see that the operation is an integer add, and could not vectorize at all.

**6. .NET, Go, Java, Rust.**
- *.NET* reifies type arguments. It shares one compiled body for all reference-type instantiations (they're all
  pointers) and compiles a separate body per value-type instantiation, at JIT or AOT time.
- *Go* (1.18+) compiles one body per GC shape, with all pointer types sharing a shape. It passes a dictionary for
  type-specific operations, so method calls on type-parameter values can be indirect.
- *Java* shares one body for everything and boxes primitives.
- *Rust* specializes every instantiation, including every pointer type, at build time.

**7. Heap pollution.** A variable of a parameterized type ends up referring to an object that isn't of that type (via raw
types, unchecked casts, or varargs of generic types). Nothing fails at the insertion. A `ClassCastException` appears
later, at some `checkcast`, far from the cause. The closest Rust equivalent is erasure you build yourself: values stored
as `Box<dyn Any>` and downcast later, where a failed downcast returns `None` and is often turned into a default (§10's
unlimited refunds). The other equivalent is `unsafe` transmutes. In review, look for `dyn Any`, `downcast_ref`, and
`TypeId` in business logic, and for maps from strings to erased values.

**8. What erasure makes easy.**
- *Loading new types at run time into existing generic code:* Rust uses `dyn Trait`, or a plugin interface over a C ABI.
- *Binary compatibility and compact libraries* (one compiled body, usable by future types): Rust ships MIR and
  recompiles. For a stable binary interface, it exports concrete `extern "C"` functions.
- *Small code and fast compiles:* Rust uses thin generic shells over non-generic cores, and `dyn` at cold boundaries.
- *Heterogeneous collections* (`List<Shape>`): Rust uses `Vec<Box<dyn Shape>>`, or an enum when the set is closed.

### Debugging exercise (`fresh_batch`)

1. **E0599**: "no associated function or constant named `new` found for type parameter `T` in the current scope"
   (verified in `ch02-05-no-new.rs`). Java's "cannot instantiate the type T" is about erasure: there is no `T` at run
   time to construct. Rust's error is about the *contract*. Every instance knows its `T`, but the body is checked once,
   against bounds, and no bound promises a `new`.
2. `String::new` is an *inherent* method of `String`, not a method of any trait in `T`'s bounds. The generic body can't
   see it, because `fresh_batch` must type-check for every `T` that satisfies the bounds (here, every `T`), and most
   types have no `new`.
3. The fixes:
   - *Standard bound:* `fn fresh_batch<T: Default>(n: usize) -> Vec<T> { (0..n).map(|_| T::default()).collect() }`.
   - *Own trait:* `trait Fresh { fn fresh() -> Self; }` with `fn fresh_batch<T: Fresh>`.

   The own trait lets a type participate without implementing `Default`. That matters for types whose "fresh" value
   is not a sensible default, such as a new random ID, a timestamped record, or a type whose `Default` would be
   misleading.

### Selected exercises

- **Beginner.** **(predicted)** `Vec<Option<i64>>`: 1 allocation of 16,000,000 bytes (each `Option<i64>` is 16 bytes,
  verified in the layout line of §3). `Vec<Option<Box<i64>>>`, all `Some`: 1,000,001 allocations. The vector is
  8,000,000 bytes, because the niche keeps `Option<Box<_>>` at 8 bytes. The boxes are another 8,000,000.
- **Advanced.** **(predicted)** `sum::<i64>` vectorizes (`paddq`), and `sum::<f32>` stays sequential (`addss`). An
  order-insensitive `f64` sum with four or eight independent accumulators vectorizes or at least pipelines, and differs
  from the sequential sum by rounding error. Bound the difference in a test with a relative tolerance, not equality.
- **Architecture.** Typical finds: `Class<T>` parameters on repository and deserialization APIs, `TypeReference`
  tokens, primitive collections on hot paths, and `IntFunction`-style interface explosions. The Rust equivalents need no
  token (the type is known per instance) and no primitive variants. The cost is an instance per type used: estimate the
  count, and apply Chapter 7.3's shell pattern to large bodies.

---

## Chapter 7.3 — Code Bloat, Compile Time, and How to Control Them

### Interview & architecture questions

**1. Body × instantiations.** Everything after the monomorphization collector (IR generation, LLVM optimization, and
codegen) runs per instance, so cost scales with the number of instances. "Body" is larger than the source, because an
instance also instantiates every generic item it uses with its own `T`. In the growth table, `top_n`'s five lines cost
104 debug IR functions and about 15,000 IR lines per type (sorting, cloning, formatting, iterators, and drop glue), and
the growth was exactly linear.

**2. Why 88 → 26, not 4 → 1.** Each fat instance duplicated not just `load` but everything `load` instantiated: 22
functions per path type, including instances of `map_err`, iterator adapters, `extend_trusted`, `fold`, and drop glue,
all parameterized by the closures' types. The thin version has 4 one-line shells, plus one `inner` family of 22
functions compiled once. The saving is 22 × (4 − 1) functions minus the shells' own cost.

**3. Closures in generic functions.** A closure's type is defined inside its enclosing function, so it is implicitly
parameterized by all of that function's generic parameters. `fat::load::<&str>::{closure#0}` and
`fat::load::<String>::{closure#0}` are different types, even though the closure never mentions `P`. Every generic item
that takes the closure is therefore instantiated again. You stop it by moving the code into a non-generic inner
function, or by using named non-generic functions instead of closures.

**4. Definition in crate A, type in crate B.** The instance is compiled where it's needed, typically crate B or a crate
downstream of both, from A's MIR. Splitting crates lets definitions type-check in parallel and be cached, but instances
still compile downstream, often in the final binary crate. So crate splits help front-end and non-generic codegen time
much more than they help monomorphization.

**5. `cargo check`.** It skips monomorphization, LLVM, and linking, and runs only the front end plus metadata emission.
It can miss post-monomorphization errors (constant evaluation depending on generic parameters, some layout errors) and
link errors.

**6. RPIT.** `-> impl Trait` returns one concrete type chosen by the function body, hidden from callers. Callers get
static dispatch and no allocation, but can only use the trait's interface. A generic return type `-> T` means the
*caller* chooses the type (as with `parse::<T>` and `collect::<T>`). `-> Box<dyn Trait>` returns a heap-allocated value
behind a vtable, which can be a different concrete type on different paths.

**7. Edition 2024 capture.** Up to edition 2021, an RPIT captured the function's type parameters but not lifetimes
unless they appeared in its bounds. Edition 2024 captures all in-scope generic parameters, including elided lifetimes,
so callers must assume the returned value may borrow from every reference argument (verified: E0502 under 2024, OK under
2021). The new default is safer for library evolution: the author can later change the hidden type to one that really
borrows without breaking callers, because callers were never allowed to assume it didn't. `use<..>` (stable since 1.82)
lists exactly what the hidden type may capture. The author uses it to promise *less*, for example `use<T>` to say "holds
no borrow of the argument", so callers may mutate or drop the argument while the returned value is alive.

**8. A CI code-size budget.** Measure, for each binary: total LLVM IR lines (emitted with `--emit=llvm-ir`, or via
`cargo llvm-lines`), the top generic functions by IR lines × copies, the `.text` size of the release binary, and build
time from `cargo build --timings`. Fail the check when totals grow beyond a threshold (say 5%) in one PR without a
justification label. Allow exceptions for generics on measured hot paths, where the PR shows a benchmark improvement.
Track the trend on a dashboard, because gradual growth is the common failure.

### Debugging exercise (`log_field` 1.4.1)

1. Downstream builds fail with **E0107**: "function takes 0 generic arguments but 1 generic argument was supplied", with
   the note "`impl Trait` cannot be explicitly specified as a generic argument" (verified in
   `ch03-07-apit-turbofish.rs`).
2. The generated code is identical, but the *API* changed. An explicit type parameter is part of the callable surface:
   callers may name it with a turbofish. Replacing it with `impl Trait` removes that ability and breaks existing,
   valid call sites. Under semver, that requires a major version, and a patch release (1.4.1) promises no breaking
   changes at all.
3. Argument-position `impl Trait` suits new functions where the argument alone determines the type and no caller will
   ever need to name it, such as `fn read_config(r: impl Read)`. Prefer an explicit parameter when callers might need a
   turbofish (for example, when the argument is a literal whose type must be chosen), when the same type appears in
   several places (`fn f<T>(a: T, b: T)`), or when bounds relate it to other parameters.

### Selected exercises

- **Beginner.** The only generic line is the conversion `inner(p.as_ref())`. Everything else, including opening,
  reading, and error mapping, moves into `fn inner(p: &Path) -> io::Result<Vec<String>>`.
- **Intermediate.** **(predicted)** A named non-generic function is a single type regardless of `P`, so the
  `map_err::<…, fn item>` and iterator instances it feeds become shared across path types. Most of the 22 per-type
  functions should disappear, leaving roughly `fat::load::<P>` itself per type, plus whatever still depends on `P`.
  Re-count to confirm. The difference measures how much of the cost the closures caused.
- **Systems.** **(predicted)** LLVM merges only functions whose generated code is identical. Types with different sizes
  or comparison code produce different sort helpers, so the merging observed for identical `(u32, String)` rows should
  mostly disappear. Rows that share a layout and comparison code may still merge.

---

## Part VII Review — Capstone instance accounting

**1. Count.** In a production binary, `T`, `C`, `R`, and `M` are fixed to one combination. `V` ranges over 40 event
types, and the topic parameter over two types (`&str` and `String`). So the 60-line `publish` body is compiled once per
(`V`, topic type) pair actually used: between 40 and 80 instances. Each instance also instantiates its closures'
dependencies (error mapping, retry helpers, iterator adapters) and the codec's serializer for `V`. The other 11 methods
don't depend on `V`, so they compile once per combination, which is 11 instances. The test build adds a second
combination (Memory, Json, NoRetry, Noop): another 40–80 `publish` instances, 11 more method instances, and a JSON
serializer instance per event type.

**2. Classification.**

| Parameter | Changes hot-path machine code? | More than one type at a time in production? | Decision |
|---|---|---|---|
| `V` (event) | Yes: encoding is per type | Yes | Keep generic, behind a *thin* method |
| Topic | No | No (just string types) | Concrete `&str` |
| `T` (transport) | No: network I/O dominates | No | `Box<dyn Transport>` |
| `C` (codec) | Yes, but it is per service | No | Fix it per service: events implement `Encode` for the chosen format, or use an enum if a service truly needs two |
| `R` (retry) | No: it is configuration | No | A value (enum or config struct) |
| `M` (metrics) | No | No | `Arc<dyn Metrics>` |

**3. Redesign.** See the verified listing in the review: `publish<E: Encode + ?Sized>` encodes into a reused buffer and
calls the non-generic `send_encoded`, which owns retry, metrics, and error mapping. The count becomes 40 three-line
shells plus one body.

**4. What was given up.**
- Inlining `transport.send` and the metrics calls into `publish`. These are dwarfed by system calls and network I/O.
- Compile-time selection of the retry policy. A `match` on an enum per retry is negligible.
- Type-level distinctions such as "this publisher can't retry". If that matters, express it as a constructor
  (`Publisher::without_retry`) or a type-state (Chapter 5.4).

To prove it doesn't matter, measure publish latency (p50/p99) and CPU per event in the service benchmark before and
after, alongside IR lines and incremental build time.

**5. A review comment.** "This compiles the 60-line `publish` body once per event type × topic type × collaborator
combination: about 80 copies in each service and 80 more in tests. Only the event type changes the machine code on the
hot path. Transport, retry, and metrics are configuration, so please make them values or `dyn`, and keep one thin
`publish<E: Encode>` that encodes and hands off to a non-generic `send_encoded`. Testability doesn't need type
parameters: the `Memory` fake works through `dyn Transport`. Please add IR line counts and build time for one service,
before and after, to the PR description."

---

## Part VII Review — Interview mode

1. A bound lets the body use exactly the bound's methods and associated items, nothing else. It requires every caller's
   type to implement the bound. A body that uses more than the bounds allow is reported at the definition (E0369, E0599).
   A caller type that lacks a bound is reported at the call (E0277).
2. `fn f<T: Display>(x: T)` and `fn f(x: impl Display)` both generate one instance per type, with static dispatch and
   identical code. Callers can turbofish only the first, and switching from the first to the second breaks callers
   (semver-major). `fn f(x: &dyn Display)` generates one instance and dispatches through a vtable, so there is no
   inlining across the call. It accepts any type without new code, and the binary stays smaller.
3. RPIT promises "some type implementing the trait" and hides which one. The body chooses one concrete type, and calls
   are static. In edition 2024, the hidden type is assumed to capture all in-scope generic parameters, including
   lifetimes. `use<..>` lists the captures explicitly, which lets the author promise that the value holds no borrow of
   an argument.
4. The collector walks from roots, recording each concrete use of a generic item as a mono item and walking its
   substituted body for more. Mono items are partitioned into codegen units, and LLVM compiles those in parallel. Each
   item gets a symbol; v0 names encode the type arguments. A dependency's generic functions are instantiated in the
   crate that uses them, from MIR shipped in the dependency's metadata.
5. Constant evaluation that depends on generic parameters, and some layout checks. `cargo check` doesn't monomorphize,
   so it can report success for code that `cargo build` rejects. CI needs a real build.
6. A closure's type is nested in the generic function, so it is parameterized by all of that function's generic
   parameters. Every generic item that takes the closure is therefore instantiated again for each instance of the
   enclosing function. Move the code into a non-generic inner function, or use named non-generic functions.
7. *To a Rust engineer:* Java compiles one body for all `T`, replacing `T` by `Object` or its bound. It inserts
   `checkcast` where values of `T` flow back into typed code, and generates bridge methods so that erased interface
   calls reach type-specific overrides. *To a Java engineer:* Rust does at build time what C2 does at best for a
   monomorphic call site. It creates a copy of the method specialized to the exact types, with the calls devirtualized
   and inlined. It does this for every type combination, without profiling and without deoptimization, and it never
   boxes.
8. Profile pollution: HotSpot type profiles belong to bytecode locations, so a shared generic method's call sites
   accumulate the receiver types of all its callers. They become megamorphic even for callers that only use one type.
   A Rust instance is specialized to one set of types by construction. There is no shared site to pollute.
9. Java shares one body for everything and boxes. C# shares one body across reference types and specializes value
   types (at JIT or AOT time). Go shares by GC shape (all pointers share) and passes dictionaries. C++ specializes
   every instantiation (checked per instantiation). Rust specializes every instantiation (checked once at the
   definition).
10. Integer addition is associative, so the `u32` sum is split into SIMD lanes. Floating-point addition isn't, so the
    `f64` sum keeps its order and stays a sequential chain of `addsd`. If order doesn't matter, write it explicitly
    with several accumulators or a chunked or pairwise sum, and test with a tolerance.
11. `Vec<i64>` of 1M: 1 allocation of 8,000,000 bytes. `Vec<Box<i64>>`: 1,000,001 allocations and 16,000,000 bytes
    requested, plus allocator overhead (measured). Java's `ArrayList<Long>` is typically 20–28 MB (headers). At the
    CPU level, iteration becomes pointer chasing: each element may be a cache miss (on the order of 100 ns from DRAM)
    instead of a sequential, prefetched, vectorizable stream.
12. Per parameter, ask whether the type changes the machine code on a hot path, and whether production uses more than
    one type at a time. If both are yes, keep it generic behind a thin method. If it varies but is cold, use `dyn`. If
    it is a closed set, use an enum. If it is configuration, use a value. If it is a convenience conversion
    (`AsRef`, `Into`), take the concrete target type or use a thin shell. Then count instances across the services and
    set a budget.
13. Measure: `cargo build --timings` for the critical path, IR lines per generic function, and binary `.text` size.
    Act: `cargo check` in the edit loop, thin shells, `dyn` at cold boundaries, fewer type parameters on widely used
    types, and crate splits for non-generic code. Enforce: a CI budget on IR lines or binary size, with a justification
    label for exceptions, and a trend dashboard.
14. "This is Java's `Map<String, Object>` without the `ClassCastException`: a wrong type becomes `None`, and here, a
    default. Please use a typed configuration struct deserialized and validated at startup. If you need an open
    registry, use typed keys (`Key<T>` with `PhantomData`), so the compiler checks each lookup. `dyn Any` in business
    logic needs a justification like `unsafe`."
