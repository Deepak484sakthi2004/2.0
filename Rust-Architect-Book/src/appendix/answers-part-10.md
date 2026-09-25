# Appendix A — Answer Key: Part X

> Model answers. Write yours first. Where several answers are defensible, the key says so.

---

## Chapter 10.1 — Closures: Fn, FnMut, FnOnce, and Capture

### Interview & architecture questions

**1. What a closure is.** An anonymous struct with one field per captured place, plus compiler-generated
implementations of `FnOnce`, and of `FnMut` and `Fn` when the body allows. Its size is the size of that struct: 0 bytes
with no captures, 8 per captured thin reference (16 for a fat one such as `&str`), and the full size of every value
captured by move (a moved `String` makes it 24 bytes, measured in listing `ch01-01`). Closures are `Copy`/`Clone`
exactly when all their captures are.

**2. Capture inference and `move`.** For each place the body uses, the compiler picks the least powerful capture that
type-checks: a shared borrow if the body only reads, a unique borrow if it mutates, and by value if it moves out of the
place. Since edition 2021 the unit is the place (`cfg.name`), not the variable (`cfg`). `move` makes every capture by
value: a copy for `Copy` types, a move otherwise. It does **not** change which call traits the closure implements. A
`move` closure that only reads is still `Fn`.

**3. The three traits.** `FnOnce::call_once(self, args)` consumes the closure, so the body may move captured values
out. `FnMut::call_mut(&mut self, args)` may mutate captured state, one call at a time. `Fn::call(&self, args)` only
reads, so calls may overlap (shared or concurrent). The hierarchy is `Fn: FnMut: FnOnce`. Anything you can call through
`&self` you can call through `&mut self`, and anything you can call through `&mut self` you can call once by value.

**4. Choosing the bound.** Require the weakest capability your code uses: `FnOnce` if you call it at most once (thread
bodies, completion callbacks), `FnMut` if you call it repeatedly from one place (visitors, iterator adapters, retry
loops), and `Fn` only when calls may be shared or concurrent (middleware shared across threads, `Arc<dyn Fn>`). Weaker
bounds accept more closures. Also decide `Send`/`Sync`/`'static` at the same time if the closure crosses threads.

**5. Two closures, one `if`.** Every closure has a unique anonymous type, and `impl Fn` return position means "one
concrete type the caller can't name." Two capturing closures are two types, so `if`/`else` has incompatible branches
(E0308: *"no two closures, even if identical, have the same type"*). Options: `Box<dyn Fn>` (allocation + indirect
calls), an `enum` of the behaviors with a `match` (static, closed set), a single closure that takes the variant as
captured data (`move |c| if ceil { .. } else { .. }`), or, for non-capturing closures only, the function-pointer
coercion that happens automatically (listing `ch01-14`).

**6. Edition 2021 captures.** RFC 2229 made closures capture disjoint *places* instead of whole variables. A closure
that uses `cfg.name` captures only that field, so the rest of `cfg` stays usable (listing `ch01-06`, E0382 under 2018).
Because the closure now owns less, fields it no longer captures are dropped when their owner goes out of scope, not
when the closure is dropped, so the drop order can change. `cargo fix --edition` inserts `let _ = &cfg;` in affected
closures to preserve the old behavior where it matters (locks, guards, and other types with side-effecting drops).

**7. Machine code of the three calling styles.** Generic `F: Fn` is monomorphized per closure type and inlined. In the
verified assembly the closure's `* bps / 10_000` became a multiply-shift inside an unrolled loop. `&dyn Fn` loads the
`call` slot from the vtable (offset 40, after drop/size/align/`call_once`/`call_mut`) and does `call r13` per element.
A `fn` pointer does `call r12` per element. The cost of dynamic dispatch isn't the call (a predictable indirect call is
a few cycles): it's the optimizations the opaque call prevents (no inlining, no unrolling or vectorization across the
call, registers saved around every call).

**8. Java lambdas.** `invokedynamic` + `LambdaMetafactory` spin a hidden class implementing the functional interface.
Captured locals must be effectively final, and their values are copied into the lambda object. Non-capturing lambdas
are reused per call site (OpenJDK behavior; the JLS permits either). Capturing ones are allocated per evaluation unless
escape analysis removes them. The effectively-final rule exists because the lambda holds a *copy*: allowing assignment
would let code mutate a copy and expect the original to change. That's exactly the Rust `move`-counter bug.

**9. "Closures are zero-cost."** True for closures passed to generic parameters (or returned as `impl Fn`) in optimized
builds: the closure is a struct of captures, calls are static, and inlining removes them. False when the closure is
type-erased (`dyn Fn`, `Box<dyn Fn>`), coerced to a `fn` pointer, or compiled without optimization. Also not free in
compile time or code size: one instance per closure type (Chapter 7.3). And "zero-cost" still means the captures are
copied or moved when the closure is built.

**10. Configuration-driven rule engine.** Parse rules into an AST once, validate them (a bad rule is rejected at
load time with a precise error, never at evaluation), then either interpret the AST or compile it to a tree of
`Box<dyn Fn(&Record) -> bool + Send + Sync>` (closure compilation: 1.8× faster than the tree walk in listing `ch01-15`).
Publish the compiled rule set atomically (an `ArcSwap` or `RwLock<Arc<_>>`) so evaluation never sees a half-updated
rule set. Measure evaluation cost against request volume before choosing code generation. At the gateway's volume, the
interpreter was already under 0.5% of a core.

### Debugging exercise (loop-variable capture)

1. **E0597** (verified, listing `ch01-16`), *"`shard` does not live long enough"*. It points to: `shard` declared by the
   `for` pattern; `value captured here` (the closure's use of `shard`); `borrowed value does not live long enough`;
   `borrow later used here` at `handlers.push(..)`; and `` `shard` dropped here while still borrowed `` at the end of the
   loop body. This compiler version doesn't suggest `move` here, so you have to know the fix.
2. The body only *reads* `shard`, so capture inference picks the least powerful mode: a shared borrow. Being `Copy`
   doesn't change the inference. Capture by value is chosen only when the body needs ownership, or when you write
   `move`. The borrow can't outlive the iteration, but `Box<dyn Fn() -> u32>` means `+ 'static` by default.
3. `handlers.push(Box::new(move || shard * 10));` Each closure copies its own `u32`. With
   `let shard = format!("shard-{i}");` inside the loop, `move` moves each iteration's `String` into its closure. There's
   no extra cost beyond the `String` you already created per iteration. The closure is 24 bytes, and the `Box` holds
   those 24 bytes on the heap. If the `String` were created *outside* the loop and shared, you'd need `clone()` per
   closure, or an `Arc<str>` (one allocation, refcount increments per closure).

### Selected exercises

- **Beginner (sizes).** Measured in listing `ch03-08-exercise-sizes.rs`: nothing → 0; a `&str` variable read through a
  method (`s.len()`) → **16**, not 8, because the closure captured the `str` place through the reference (a `&str`, fat
  pointer), not the variable (`&&str`); a moved `&str` → 16; a moved `Vec<u8>` → 24; a mutably borrowed `[u8; 1024]` → 8;
  a moved `[u8; 1024]` → **1,024**. Moving large arrays into closures copies them, and every adapter that holds the
  closure grows by that much.
- **Intermediate (`compose`).** `fn compose<A, B, C>(f: impl Fn(A) -> B, g: impl Fn(B) -> C) -> impl Fn(A) -> C
  { move |a| g(f(a)) }`. With `FnMut`, the returned closure must call `f` and `g` through `&mut`, so it's `FnMut` itself:
  `-> impl FnMut(A) -> C { move |a| g(f(a)) }` with `mut f, mut g` parameters. A closure is at most as capable as the
  least capable thing it calls.
- **Advanced (`memoize`).** The returned closure inserts into its cache, so it is naturally `FnMut`. To make it `Fn`,
  put the cache behind interior mutability: `RefCell<HashMap<..>>` (single-threaded, run-time borrow checks, `!Sync`) or
  `Mutex<HashMap<..>>` (thread-safe, a lock per call). You pay a borrow-flag check or a lock on every call, and the
  closure becomes non-reentrant (`RefCell`) or can contend (`Mutex`).

---

## Chapter 10.2 — The Iterator Trait and Laziness

### Interview & architecture questions

**1. `next()`.** `fn next(&mut self) -> Option<Self::Item>`. One call both answers "is there more?" and delivers the
item, so there's no split state between `hasNext` and `next` to get out of sync. `hasNext` implementations that must
pre-fetch (to know whether more exists) are a classic source of bugs in Java iterators over I/O.

**2. Laziness.** Adapters only build structs. Work happens when a consumer pulls. Each item travels through the whole
pipeline before the next item is read: `filter` pulls from the source until an item passes, `map` transforms it,
`take(2)` counts it, and after the second result `take` stops pulling, so later items are never read (verified in
listing `ch02-01`: 3,000 was never touched).

**3. The three forms.** `impl IntoIterator for &Vec<T>` (items `&T`, borrow; the default), `for &mut Vec<T>` (items
`&mut T`; edit in place), and `for Vec<T>` (items `T`; consumes the vector, moving items out without cloning; use it
when the loop is the vector's last user).

**4. `Some` after `None`.** Yes, the trait allows it. Iterators over growing sources (a tail-follow reader, a queue
polled without blocking) may do it. `fuse()` wraps any iterator so that after the first `None` it always returns `None`.
`FusedIterator` is a marker trait an iterator implements to promise it already behaves that way, which lets `fuse()`
skip its bookkeeping.

**5. `size_hint`.** It gives `(lower, Option<upper>)` bounds on the remaining items, for optimizations such as
pre-allocation in `collect` and `extend`. It's safe to implement incorrectly (it's a safe method), so unsafe code that
relied on it could be made unsound by any buggy iterator. That's why std's own "exact length" guarantee is a separate
`unsafe trait` (`TrustedLen`), and why the docs say not to trust `size_hint` in unsafe code.

**6. Lending from `self`.** `Self::Item` is chosen once per impl and can't depend on the lifetime of each `&mut self`
borrow in `next`. If it could, two successive items could alias one buffer that the second call overwrote. Every
adapter that holds more than one item at a time (`collect`, `max_by_key`, `zip`, `windows`, `peekable`) would then
observe mutated or dangling data.

**7. GATs.** `type Item<'a> where Self: 'a;` plus `fn next(&mut self) -> Option<Self::Item<'_>>` ties each item to that
call's borrow. The borrow checker then enforces "one item at a time" (E0499 on a second `next()` while the first item
is alive, listing `ch02-12`). You give up `collect`, all std adapters, `for` loops, and ecosystem compatibility. You
write `while let` loops and your own adapters, and you convert to owned items where consumers need to keep data.

**8. Return types.** `impl Iterator` by default for internal APIs (zero cost, lazy, but unnameable). A named type for
public library APIs whose users must store the iterator in a struct or name it. `Box<dyn Iterator>` when the concrete
source is chosen at run time and the per-item cost doesn't matter (one allocation, indirect `next` per item). `Vec`
when callers need random access or several passes, or when the producer must release a resource (a lock, a
connection) before returning.

**9. `zip` losing items.** `Zip::next` pulls from the first iterator before the second. When the second is exhausted,
the item already taken from the first is dropped, which the docs allow ("at most one" extra advance of the first
iterator). With exact-length sources and internal iteration (`collect` over a `Vec` source), std precomputes the pair
count and doesn't over-pull, so tests that feed `Vec`s pass. `for` loops, or sources without an exact length (filters,
channels, readers), lose the item (listings `ch02-15`, `ch02-16`).

### Debugging exercise (`next()` inside `for`)

1. **E0499** (verified, listing `ch02-17`). The `for` loop's hidden iterator borrows `lines` mutably via
   `lines.by_ref()` for the entire loop. `lines.next()` in the body is a second mutable borrow. The first borrow is
   "later used here" at the loop header, because every iteration calls `next()` on it. The compiler adds a note
   (*"a for loop advances the iterator for you"*) and suggests `while let`.
2. Both fixes are verified in listing `ch02-18-next-inside-fixed.rs` (output `["a", "b"] ["a", "b"]`):
   ```rust,ignore
   let mut lines = input.lines();
   while let Some(line) = lines.next() {
       if line == "#continued" {
           lines.next(); // a separate, short borrow: the previous next() call has already returned
           continue;
       }
       out.push(line);
   }
   ```
   Each `lines.next()` call is a separate, short mutable borrow that ends when the call returns (the item is a `&str`
   into `input`, not into `lines`), so the two calls in one iteration never overlap.
3. With state instead of a second `next()` (same listing):
   ```rust,ignore
   let mut skip = false;
   for line in input.lines() {
       if std::mem::take(&mut skip) {
           continue;
       }
       if line == "#continued" {
           skip = true;
           continue;
       }
       out.push(line);
   }
   ```
   `scan` with a boolean state followed by `flatten` also works. The flag version is the clearest.

### Selected exercises

- **Intermediate (Fibonacci).** Store `(a, b)` and stop when `a` would overflow (`checked_add` returns `None`). The
  exact remaining count isn't a closed form you want to compute. Precompute it once (there are 94 Fibonacci numbers that
  fit in a `u64`, from F(0) = 0 to F(93)) and decrement a counter, or compute it by iterating a clone. Implementing
  `ExactSizeIterator` requires `size_hint` to return `(n, Some(n))`.
- **Advanced (`DoubleEndedIterator` for `Frames`).** A length-prefixed format can only be parsed front to back: you
  can't find the last frame's start without scanning every header before it. A `next_back` would have to scan the
  whole remaining buffer each call (O(n²) overall) or pre-index all frames (memory and a full pass up front). The right
  answer is usually *don't implement it*, and let callers `collect` if they need reverse order. Implement it only if
  the format has a trailer or index.
- **Architecture (Ferrite range scans).** An owned `Iterator<Item = Result<(Vec<u8>, Vec<u8>), E>>` lets the engine
  release segment buffers freely, but copies every key and value. A lending iterator over borrowed slices avoids copies,
  but pins the current block (and the memtable snapshot) for the item's lifetime and blocks std adapters. A visitor gives
  zero copies and lets the engine control buffer lifetimes, but callers can't stop and resume. Project L10 uses owned
  items for the public API and a borrowed-slice fast path internally for merges and compaction.

---

## Chapter 10.3 — How an Iterator Chain Compiles

### Interview & architecture questions

**1. The type of a chain.** `Map<Filter<slice::Iter<'a, T>, P>, F>`, where `P` and `F` are the closures' anonymous
types. It's an ordinary value that lives wherever you bind it, usually on the stack. Its size is the sum of the
fields: 16 bytes for the slice iterator plus each closure's captures plus adapter state (8 per counter). Listing
`ch03-01` measured 16/16/24/32/40 bytes along one chain.

**2. The four ingredients.** (1) Adapters are structs, so the whole pipeline is one value and needs no allocation. (2)
Monomorphization makes every `next`/closure call static. (3) Inlining collapses those calls into one loop. (4) Internal
iteration (`fold`/`try_fold`) lets adapters choose their own loop structure. Without (2) (`dyn`), calls stay indirect
and nothing inlines across them (~20× on a tight loop). Without (3) (debug builds), every adapter is a real call
(10–40× slower). Without (4) (`for` over `chain`), the loop carries per-element state checks and may not vectorize (3×
in the measurement).

**3. Internal iteration.** The consumer hands a closure to `fold`. Each adapter overrides `fold` to wrap the closure and
forward it inward, and the innermost source runs a tight loop. `Chain::fold` folds the first half and then the second:
two independent loops, both vectorized in the verified assembly. A `for` loop calls `Chain::next`, which must check
which half is active on every element, and LLVM left it as a scalar loop (0.41 vs 0.14 ns per element).

**4. `sum` over `dyn`.** `sum` and `fold` are generic methods, so they aren't in the `dyn Iterator` vtable (they aren't
dyn-compatible). Calling `sum` on a `Box<dyn Iterator>` uses the default implementation, a loop over `next()`, and
`next()` is a vtable call (`[rsi + 24]`). Nothing inside the concrete iterator is visible to the loop.

**5. In-place `collect`.** When the source is a `vec::IntoIter` (a `Vec` consumed by `into_iter()`), every adapter is
`InPlaceIterable` (yields no more items than it consumes), and the output element fits (on rustc 1.98.1: same
alignment, no larger size), std writes the output into the source buffer. The risk: the result keeps the source's
**capacity**. Filtering 1% of a million-element vector keeps a million slots, and shrinking the element type re-expresses
the same bytes as more capacity (1,000,000 pairs → capacity 2,000,000 `u32`s). Fix: `shrink_to_fit()` (one allocation
and copy), or build the result from a borrowing iterator.

**6. Bounds checks.** Not always. In `add_indexed`, LLVM computed the largest prefix provably in bounds for all three
slices, vectorized it without checks, and kept checks only in a scalar tail that exists to panic at the right index.
Semantics differ: `add_indexed` writes part of `out` and then panics if `a` or `b` is short, `add_resliced` panics
before writing anything, and `add_zipped` silently stops at the shortest slice. Choose by intended behavior.

**7. Debug timings.** Debug builds don't inline, so every adapter, closure, and `next` is a real call. The relative
costs are unrelated to release (the chain was 25× slower, the index loop 37×, a mid-pipeline `collect` 30×), so no
release decision can be based on them.

**8. Rust vs HotSpot.** Rust removes the abstraction at compile time, driven by types: every build, every run, from the
first call. HotSpot removes it at run time, driven by profiles: inlining lambda bodies into the pipeline when call
sites are monomorphic, escape analysis to avoid allocating pipeline objects, range-check elimination and superword
vectorization in the compiled loop. HotSpot fails when call sites go megamorphic (shared helpers called with many
lambdas), when inline budgets are exceeded (deep pipelines), before warm-up, and after deoptimization. Rust fails with
`dyn`, in debug builds, and in the specific shapes above.

**9. Reviewing an "index loops for performance" PR.** Ask for release-mode measurements on realistic data (best of N,
with `black_box` on inputs), the assembly if the claim is about bounds checks, and the semantic change (the index
version panics where the zip version truncated). Point out the shapes where the right fix is a different consumer
(`fold` instead of `for` over `chain`) or moving `dyn` to a coarser boundary, not a loop.

### Debugging exercise (the 25× claim)

1. `cargo test` builds with the `test` profile, which inherits `dev`: no optimization. Debug-build numbers say nothing
   about release performance.
2. Their numbers show the **iterator** as faster (10.24 ns vs 15.28 ns). They've misread their own table, or
   inverted the label. In debug, the index loop pays for a bounds check and an overflow check per access, as calls into
   unoptimized code.
3. Put the benchmark in a release build (`cargo bench`, or a `--release` binary with best-of-N timing, `black_box` on
   inputs and outputs). Prediction, from this chapter's table: 0.41 ns per element for both.
4. `sum()` over a `Box<dyn Iterator>` measured 1.63 ns vs 0.08 when devirtualized. The fix is to move dynamic dispatch
   to a coarser boundary (dispatch once per batch or per source, then run a generic inner loop), not to write an index
   loop.

### Selected exercises

- **Beginner (size prediction).** Measured in listing `ch03-08-exercise-sizes.rs`: `v.iter().zip(w.iter())` is **48
  bytes** (two slice iterators, 32 bytes, plus 16 bytes of index/length state that std's `Zip` keeps for its
  random-access fast path [LIB]), and `.skip(3).step_by(2)` brings it to **72 bytes**.
- **Advanced (`MyChain` with only `next`).** You reproduce the external-iteration shape: one loop with a state check.
  Adding a `fold` that folds `a` then `b` reproduces the two-loop, vectorized structure. That's the whole reason
  adapters override `fold`.
- **Systems (in-place rows).** Predict with the rule "same alignment and output no larger → in place." `u8 → u8`: in
  place. `[u8; 3] → u8`: same alignment (1), smaller → in place, capacity ×3. `u64 → (u32, u32)`: same size (8), but
  alignment 4 vs 8 → not in place. Listing `ch03-09-in-place-exercise.rs` confirms all three on rustc 1.98.1
  (`same buffer=true cap=1000`; `same buffer=true cap=3000`; `same buffer=false`). These are implementation choices that
  can change between releases.

---

## Chapter 10.4 — Rust Iterators vs Java Streams

### Interview & architecture questions

**1. Push vs pull.** Java's terminal operations push elements through a sink chain (`forEachRemaining`), and switch to
pulling (`tryAdvance`) for short-circuiting operations. Rust's trait is pull-based (`next`), but every consumer that
doesn't need to stop early uses `fold`/`try_fold`, which is push-based internal iteration. Both languages use both
models. The difference is that Rust decides at compile time (types, monomorphization) and Java at run time (objects,
JIT).

**2. `collect::<Result<..>>`.** std wraps the iterator in an internal shunt adapter that yields `Ok` values to the inner
`FromIterator` and, at the first `Err`, stores it and ends the iteration by returning `None`. The partially built
`Vec` is then dropped (its elements dropped, its buffer freed), and the stored error is returned. The rest of the
input is never pulled (verified: 2 of 5 parsed).

**3. Checked exceptions vs `?`.** Java's functional interfaces (`Function`, `Predicate`) don't declare checked
exceptions, so a lambda can't throw one. Rust has no exceptions: the equivalent restriction is that `?` returns from
the *closure*, so it only works inside a closure whose return type is `Result`/`Option`. `for_each` takes a closure
returning `()`, hence E0277. Use `try_for_each`, `map` + `collect`/`sum` into `Result`, or a `for` loop.

**4. Duplicate keys.** `Collectors.toMap(k, v)` throws `IllegalStateException` on a duplicate key. `collect::<HashMap<_,
_>>()` inserts in order, so the last value silently wins. Port the Java behavior with a `try_fold` into a map using the
entry API and returning an error on `Occupied` (listing `ch04-04`), or with an explicit merge function
(`and_modify`/`or_insert`) when a merge is intended.

**5. Reuse.** A Java stream may wrap a one-shot source, so the API forbids reuse for every stream (run-time
`IllegalStateException`). In Rust, consuming methods take the iterator by value, so reuse is a compile-time error
(E0382). An iterator over borrowed data is a small `Clone` value, and cloning it creates a second traversal of the same
borrowed collection. Its type proves that re-reading is possible.

**6. `sorted`/`distinct`.** Both are barriers in Java: `sorted` buffers every element before emitting any, and
`distinct` keeps a hash set of everything seen. Rust's std has no `sorted` adapter because an adapter is supposed to be
lazy and streaming. The buffering is written explicitly (`collect` then `sort`, or `HashSet`). itertools' `sorted`
allocates the same buffer behind adapter syntax.

**7. `Stream<Long>` vs `Vec<u64>`.** Java pays for each element's object header and a pointer chase (a `Long` is ~24
bytes on a typical 64-bit JVM, plus a 4-byte reference), for pipeline and sink objects, and for interface calls into
lambdas. The JIT can remove the pipeline objects (escape analysis) and the calls (inlining a monomorphic site), but not
the boxing of elements already in a `List<Long>`. Use `LongStream` over `long[]` for primitive paths. Rust has none of
these costs by default. The Rust-side model measured boxed elements at 1.16 ns vs 0.41 flat, and indirect stages at
1.91.

**8. Parallel `f64` sums.** Floating-point addition isn't associative, and a parallel sum's grouping depends on how work
was split and stolen, so the result can differ between runs (two runs of listing `ch04-08` differed in the last digits).
Money: integers only, never `f64`. Metrics: accept the variation and document it, or use a deterministic reduction
(fixed chunking, then combining in chunk order) or compensated summation, if reproducibility matters more than speed.

**9. Blocking in a shared pool.** Both pools size themselves to the CPU count and assume tasks are CPU-bound. Blocking
calls park pool threads, so unrelated parallel work (other requests' `par_iter`, Java's `CompletableFuture` async
defaults on the common pool) waits. Use a dedicated pool for blocking or long work, and keep data-parallel work off
latency-sensitive threads.

### Debugging exercise (`?` in `for_each`)

1. E0277 (verified): *"the `?` operator can only be used in a closure that returns `Result` or `Option`"*, pointing to
   the closure passed to `for_each` (*"this function should return `Result` or `Option` to accept `?`"*).
2. All three are verified in listing `ch04-12-question-fixes.rs` (each returns `Ok(42)` for `["1", "2", "39"]`):
   ```rust,ignore
   fn with_try_for_each(lines: &[&str]) -> Result<u64, ParseIntError> {
       let mut total = 0;
       lines.iter().try_for_each(|l| {
           total += l.parse::<u64>()?;
           Ok::<(), ParseIntError>(())
       })?;
       Ok(total)
   }

   fn with_sum(lines: &[&str]) -> Result<u64, ParseIntError> {
       lines.iter().map(|l| l.parse::<u64>()).sum()
   }

   fn with_loop(lines: &[&str]) -> Result<u64, ParseIntError> {
       let mut total = 0;
       for l in lines {
           total += l.parse::<u64>()?;
       }
       Ok(total)
   }
   ```
   The `try_for_each` closure needs its error type spelled out (`Ok::<(), ParseIntError>(())`), because nothing else
   in the closure fixes it.
   Ship `with_sum` for a pure computation (one expression, short-circuits), and `with_loop` when the body grows
   (logging, other side effects). `with_try_for_each` works, but it mixes a captured `&mut` counter with a fallible
   closure and needs the type annotation.
3. `unwrap_or(0)` turns malformed input into a *valid-looking* zero. A truncated or corrupted amount line silently
   under-reports a total, which is Chapter 8.1's "absence may default, malformed never does." Expect the incident at the
   next reconciliation, with no error anywhere to explain it.

### Selected exercises

- **Beginner.** `let out: Vec<String> = { let mut v: Vec<String> = names.iter().filter(|n| n.chars().count() > 3)
  .map(|n| n.to_uppercase()).collect(); v.sort(); v };`. The output is `Vec<String>`, and it owns *new* strings
  (`to_uppercase` allocates, Chapter 9.2). Note what "length" means on each side: Java's `length()` counts UTF-16
  code units, Rust's `len()` counts UTF-8 bytes, and `chars().count()` counts Unicode scalar values. They agree for
  ASCII names and disagree for "Kristina Øberg" (Chapter 3.4), so choose deliberately.
- **Advanced (`StrictMap`).** `FromIterator::from_iter` returns `Self`, not `Result<Self, E>`, so collecting into
  the map itself can't fail. Record the duplicates inside the value and expose `into_result()`. What about implementing
  `FromIterator<(K, V)>` for `Result<StrictMap<K, V>, DupError>`, so that `collect()` returns a `Result` directly?
  Listing `ch04-11-strictmap-orphan.rs` shows the answer: **E0117**, *"only traits defined in the current crate can be
  implemented for types defined outside of the crate ... `Result` is not defined in the current crate"*. `Result` isn't
  a fundamental type, so `Result<LocalType, LocalType>` still counts as foreign, and `(K, V)` isn't local either
  (Chapter 6.3). The explicit `try_fold` is the idiomatic answer.
- **Systems (Kahan in parallel).** Compensated summation reduces the error of each partial sum, but the partial sums
  are still combined in a work-stealing-dependent tree, so results can still differ in the last bits, just less often.
  Determinism needs a fixed reduction order: fixed chunk boundaries, with partial results combined in chunk order.

---

## Part X Review — Capstone: the settlement report PR

**Defects** (listing `review-settlement.rs`; the output shows the ones marked ✔):

| # | Defect | Mechanism | Shown? | Impact |
|---|---|---|---|---|
| 1 | `load_fees` collects into `HashMap`: the duplicate `m-100` row silently wins (190 bps) | `FromIterator for HashMap` keeps the last value (10.4) | ✔ | **Money**: wrong fee tier |
| 2 | `load_fees` `unwrap`s the split and the parse | A panic on malformed partner input (8.1) | later | Job crash |
| 3 | `skipped_non_eur` captured by `move`: the closure counts a copy | `move` + `Copy` (10.1); the compiler warned | ✔ (0 vs 7,144) | Wrong report |
| 4 | `audited` filled by a side effect in `map`, consumed by `any` | Short-circuiting consumers stop pulling (10.2) | ✔ (4 of 50,000) | **Compliance** |
| 5 | `_suspicious` computed and discarded | The flagged transaction is never reported | later | **Compliance** |
| 6 | `by_ref().zip(0..size)` over the channel | Zip over-pulls and drops one payout per full batch (10.2) | ✔ (10 of 12) | **Data loss: merchants unpaid** |
| 7 | Fees in `f64`, truncated with `as u64` per transaction | Float money + truncation (2.3, 10.4) | ✔ (with a clean schedule: 30,375,887 vs 30,397,321 cents) | **Money**: 21,434 cents per run undercharged |
| 8 | `settled` built by `into_iter().filter().collect()` and kept in `Report` | In-place collect keeps source capacity (10.3) | ✔ (50,000 / 100,000) | Memory |
| 9 | `eur: Vec<&Txn>` collected, then iterated once | Mid-pipeline allocation (10.3) | later | Perf |
| 10 | `by_merchant: HashMap<String, Vec<&Txn>>` with `merchant.clone()` per transaction | An allocation per transaction plus materialized groups, where sums would do (9.3, 10.3) | later | Perf/memory |
| 11 | `fees[&m]` indexing | Panics if a merchant has no fee row (8.1) | later | Job crash |
| 12 | `fees_cents += fee` inside `map` | Side effect in a lazy adapter: correct only because the result is collected right away (10.2) | later | Fragile |
| 13 | Payouts in `HashMap` iteration order | Randomized per process (9.3): reports and bank batches differ between runs | later | Audit/reproducibility |
| 14 | CI accepted warnings | The compiler reported #3 (8.1: `-D warnings`) | ✔ | Process |

**Ranking.** Data loss (#6: merchants don't get paid) and compliance (#4, #5) first, then money (#1, #7), then crashes
on bad input (#2, #11), then reproducibility (#13), then memory and performance (#8, #9, #10), then fragility (#12) and
process (#14). The ranking is about *consequence*. #6 and #4 both look like iterator trivia, and they're the two that
would reach a regulator.

**The fixed design** (listing `review-settlement-fixed.rs`, 5 tests):

- `load_fees` returns `Result<HashMap<..>, FeeError>` with `Malformed` and `Duplicate`, built with `try_fold` + the
  entry API.
- `build_report` borrows `&[Txn]` (nothing moved or retained), and makes **one pass**: a `for` loop over
  `txns.iter().filter(settled)` that counts, screens *every* settled transaction (no short-circuit decides), records
  suspicious IDs, skips non-EUR with a real counter, and accumulates integer gross and fees per merchant in a
  `BTreeMap<&str, Acc>` (borrowed keys: no clone per transaction; ordered output).
- Fees are integer basis points with half-up rounding (`(cents * bps + 5_000) / 10_000`), tested at the boundary.
- A missing fee row is an `Err`, not a panic.
- Batching uses `by_ref().take(size)`, and the test feeds a **channel**, not a `Vec`.
- CI should add `-D warnings`.

**Design question 1: `for` loop vs chains.** A `for` loop over a filtered iterator is better when one pass produces
several outputs (counts, a list, a map) or has non-trivial control flow (`continue`, `?`). Forcing that into a chain
means side effects in `map`/`filter` closures (defects #3, #4, #12) or a `fold` with a large tuple accumulator. It's a
regression when the loop re-implements what a single consumer expresses (a `sum`, a `collect`, an `any`), or when it
reintroduces an index with bounds semantics you didn't intend.

**Design question 2: `settled: Vec<Txn>` for the dashboard.** No. The dashboard needs aggregates and maybe the
suspicious IDs, not 50,000 cloned transactions held for a day. Keep a summary struct. If drill-down is required, query
the ledger by ID on demand. If the full set must be kept, build it from a borrowed iterator (`iter().filter().cloned()`)
or `shrink_to_fit` after an in-place collect, and put its `capacity × size_of` on a gauge.

---

## Part X Review — Interview mode

1. **Closure type and size:** Chapter 10.1, questions 1–2.
2. **The three traits and the bound for a loop callback:** Chapter 10.1, questions 3–4. `FnMut`.
3. **`impl Fn` returns:** Chapter 10.1, question 5.
4. **The three `for` forms:** Chapter 10.2, question 3.
5. **Chain to loop:** Chapter 10.3, questions 1–3. Name all four ingredients and what breaks without each.
6. **Closure MIR:** the enclosing function builds an aggregate `{closure@..} { threshold: move _5, seen: move _6 }`. The
   body is a separate function `count_over::{closure#0}(_1: &mut {closure@..}, _2: u64)`, and each captured variable is
   a projection like `(*((*_1).1: &mut usize))`. Calls go through `FnMut::call_mut(closure, (args,))` with the
   arguments tupled.
7. **Lending:** Chapter 10.2, questions 6–7.
8. **Where zero-cost fails:** `dyn` in hot loops (move it to a coarse boundary), `for` over `chain`/`flatten` (use
   `fold`-based consumers), mid-pipeline `collect` (fuse the stages), debug builds (measure release). Also in-place
   `collect` for memory (`shrink_to_fit`).
9. **`dyn Iterator` cost:** Chapter 10.3, question 4. 1.63 vs 0.08 ns per element measured. The cost is the lost
   inlining and vectorization, not the `call`.
10. **In-place `collect`:** Chapter 10.3, question 5, and the blocklist failure scenario (8M capacity for 160K
    entries).
11. **Lambdas vs closures:** Chapter 10.1 §8 table and question 8.
12. **Streams vs iterators:** Chapter 10.4 §2 and questions 1, 5, 7.
13. **Literal translations that change behavior:** `Collectors.toMap` → `collect::<HashMap>` (duplicates),
    `forEach` with checked exceptions → `?` in closures, `groupingBy` → `HashMap` order assumptions, `parallelStream`
    `f64` sums, `count()` skipping `peek` side effects in Java 9+ (the reverse direction), and `sorted()` stability.
14. **Run-time configurable rules:** Chapter 10.1 question 10 and §9: parse + validate at load time, closure-compile,
    publish atomically, measure against volume.
15. **Hot-path policy:** Chapter 10.3 §7 rules. Iterators by default, `fold`-based consumers for `chain`/`flatten`,
    `dyn` only at coarse boundaries, `collect` only when the collection is needed, `shrink_to_fit` for long-lived
    filtered vectors. Overrule only with a release-mode, best-of-N measurement on realistic data, plus the assembly
    when the claim is about codegen.
