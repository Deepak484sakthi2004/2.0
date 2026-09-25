# Appendix A — Answer Key: Part IV

> Model answers. Write yours first. Where several answers are defensible, the key says so.

---

## Chapter 4.1 — Aliasing XOR Mutation

### Interview & architecture questions

**1. Places, loans, accesses.** A place is a path to memory: a local, a field, a dereference, an index. A loan is
(place, shared or mutable, region). A live **shared** loan forbids writing, moving out of, or mutably borrowing the place,
its parents, or its children, and allows reads and more shared borrows. A live **mutable** loan forbids *every* other
access to those places (reads, writes, moves, new borrows) except through the loan itself.

**2. Fields vs indices.** Two distinct fields are different places, and neither is a prefix of the other. The checker
does no arithmetic on index expressions, so a slice's elements are all the place `v[_]`. For a `Vec`, `v[i]` is a call
to `Index::index(&v, i)` that borrows all of `v`.

**3. Unique, not mutable.** `&mut T` guarantees there's no other live path to the value, and that exclusivity is what
makes mutation safe. `&T` guarantees shared access. Mutation through `&T` is possible, but only via `UnsafeCell`-based
types that keep it safe some other way.

**4. `UnsafeCell`.** It's the primitive that tells the compiler "this memory may change even though it's behind a `&`."
Without it, mutating through `&T` is undefined behavior, because rustc marks `&T` parameters `noalias readonly` and LLVM
optimizes on that basis. The verified IR: `&i32` carries `noalias ... readonly ... dereferenceable(4)`, while
`&Cell<i32>` has no `noalias`. So after an opaque call, the `Cell` version reloads (`add eax, dword ptr [rdi]`) and the
plain version doesn't (`add eax, eax`).

**5. How each re-establishes the rule.** `Cell`: no references into the interior (`get` copies, `set`/`replace` swap),
so nothing can observe a change mid-borrow. `RefCell`: a run-time borrow counter, and a conflict panics. `Mutex`/`RwLock`:
blocking makes access exclusive, or shared for readers. Atomics: each operation is indivisible in hardware, with
memory-ordering rules for everything around it (Part XIV).

**6. Iterator invalidation.** A slice iterator is two raw pointers into the buffer. `push` at capacity allocates a new
buffer and frees the old one, so the iterator's pointers dangle. C++: undefined behavior. Java: best-effort
`ConcurrentModificationException`. Rust: E0502 at compile time, because the iterator's loan is live across the loop body.

**7. Disjointness APIs.** `split_at_mut(mid)` is structural: two ranges `[0, mid)` and `[mid, len)` can't overlap, at the
cost of one bounds check. `get_disjoint_mut([i, j, ...])` checks at run time that every index is in bounds and all are
pairwise distinct (a few comparisons for small arrays), returning `Err` otherwise. Each wraps a small `unsafe` core
inside std.

**8. Indices vs references.** Indices fit when data is owned by one container (an arena) and references would tangle
lifetimes: graphs, deletions, long-lived handles. The cost is that correctness moves to you: stale indices, shifting after
`remove`, bounds panics. The compiler still guarantees memory safety, but not logical correctness.

### Debugging exercise (`prune`)

1. **E0502.** A: `scores.iter()` (the loop header, an immutable borrow). B: `scores.remove(i)` (a mutable borrow). C:
   the loop header again (the next `next()` call).
2. The iterator holds `ptr` and `end` into the buffer. `remove` shifts the tail left with a `memmove` and decrements the
   length. The iterator would skip the element that shifted into position, and its `end` would still point one past the
   *old* end, so it would read a slot that no longer holds a live element (a double drop for owning types like `String`).
3. `scores.retain(|&s| s >= threshold)`. It's O(n): one pass, compacting in place. A remove-based loop is O(n²) in the
   worst case, since each `remove` shifts the whole tail.

### Selected exercises

- **Beginner:** `&mut s.a` + `&s.b`: compiles (disjoint fields). `&mut s.a` + `&s`: error (`s` is a prefix of `s.a`).
  `&mut v[0]` + `&v[1]`: error (both are `v[_]`). `&mut *r` + `&r.x`: error (`r.x` is `(*r).x`, an extension of `*r`).
- **Intermediate:** `retain` is O(n). A reverse index loop (`for i in (0..n).rev()`) is correct but O(n²) worst case.
  `into_iter().filter(..).collect()` is O(n) plus an allocation.

---

## Chapter 4.2 — NLL, Liveness, and Reborrowing

### Interview & architecture questions

**1. NLL.** Loans end at their last use, per control-flow path, instead of at the end of the lexical scope.
`let f = &v[0]; println!("{f}"); v.push(4);` was rejected under lexical lifetimes (`f`'s scope runs to the end of the
block) and is accepted under NLL.

**2. Liveness.** A loan is live at P if some path from P reaches a use of the reference, or of anything derived from it.
It must be per path because the future depends on control flow: a reference used only inside an `if` is dead after the
`if` on the other path.

**3. Reborrows.** `&mut *r` creates a new loan derived from `r`, and `r` is suspended until the new one is dead. When the
parameter type is known to be `&mut T`, the argument position is a coercion site and the compiler inserts the reborrow.
For a generic `U`, the argument's type is inferred as `&mut T` itself, which isn't `Copy`, so it's **moved**.

**4. NLL steps, applied** to `let r = &v; if c { use(r) } v.push(1);`. Region variable `'r` for `r`'s type. Liveness adds
the points from the borrow up to `use(r)` inside the `if`. No outlives constraints extend it further. The solution: `'r`
= {borrow point … use point} on the `c` path, and just {borrow point} on the other. At `push`, no loan is in scope on
either path, so it's accepted.

**5. Problem case #3.** The returned reference must satisfy `'1` (the caller's region, covering the whole rest of the
function from the caller's view). Because one path returns the `get` loan, that loan's region must include `'1`, and
NLL's regions are plain point sets, so the loan is considered live at `insert` on the other path too. Polonius tracks
*which loans* can reach each origin *at each point*, and sees that on the insert path the `get` loan never flows into the
return value.

**6. Two-phase borrows.** For autoref'd method calls, the `&mut` is created in a *reserved* state that acts like a shared
borrow while the arguments are evaluated, then *activated* for the call. It's special-cased because `v.push(v.len())`
and the like are extremely common, and the soundness argument depends on that exact shape.

**7. Definite assignment vs liveness.** Definite assignment (JLS chapter 16) is a forward "must" analysis: assigned on
*all* paths before a read, protecting against reading uninitialized locals. Liveness is a backward "may" analysis: used
on *some* path later, protecting aliasing and lifetimes. Same dataflow family, different questions.

**8. Ranking workarounds.** (1) Entry API or a similar purpose-built API: often *fewer* hash lookups. (2) Restructure:
compute the decision first, borrow after. (3) Two lookups (`contains_key` then `get`): an extra hash. (4) Return owned
data or an index: an allocation or a later lookup. (5) `unsafe`: never, for sound-but-rejected code with a safe
alternative.

### Debugging exercise (`record(r)`)

1. `bump`'s parameter type is `&mut u32`. The expected type is known, so the compiler inserts `&mut *r`. `record<T>`'s
   parameter is generic, so `T` is inferred as `&mut u32` and `r` itself is passed, which is a move.
2. At the call site: `record(&mut *r)` or `record(&*r)`. At the definition: `fn record(value: &impl Debug)` (or
   `<T: Debug + ?Sized>(value: &T)`). The definition fix is better for a function that only reads: every caller then
   works with `&x` or a reborrow.
3. `record(r.clone())` compiles, but not the way you might think. `&mut u32` doesn't implement `Clone`, so method
   resolution auto-derefs and calls `u32::clone`, passing a `u32` copy. `record` prints the number, not a reference, and
   `r` isn't moved. It compiles and means something different.

### Selected exercises

- **Intermediate:** `words.sort();` uses a short reborrow of `*words` that ends at the statement. Then
  `words.iter().max_by_key(|w| w.len()).map(|w| w.as_str()).unwrap_or("")` takes a shared reborrow for `'a`. Returning a
  shared borrow derived from `&'a mut` is allowed. The mutable reference is effectively "frozen" for `'a`.
- **Advanced:** The entry API does 1 lookup (plus 0 or 1 allocations for a new value). `contains_key` + `insert` + `get`
  does up to 3 lookups. Returning an owned `String` does 1 or 2 lookups plus 1 allocation for the clone on every call.

---

## Chapter 4.3 — Lifetime Annotations and Elision

### Interview & architecture questions

**1. What annotations do.** They state relationships between regions: which inputs an output may borrow from, and which
lifetimes must outlive which. Callers are checked against them and bodies must satisfy them. Nothing changes at run
time, and the referent's *owner* alone decides how long it lives.

**2. Elision.** Rule 1: each elided input lifetime is distinct (`fn f(a: &str, b: &str)`). Rule 2: exactly one input
lifetime goes to all outputs (`fn first(v: &[u32]) -> &u32`). Rule 3: `&self`/`&mut self` goes to all outputs
(`fn name(&self) -> &str`). None applies: `fn longest(a: &str, b: &str) -> &str` (E0106).

**3. `longest` needs both inputs alive.** The signature says the output may borrow from either input, and the checker
uses only the signature, never run-time values.

**4. The tokenizer.** Elided, rule 3 makes the output `Option<&'s str>` where `'s` is the `&mut self` borrow, so each
token extends the exclusive loan on the tokenizer, and the second `next_token` conflicts (E0499). With `-> Option<&'a
str>`, tokens borrow from the input, and the `&mut self` loan ends when the call returns.

**5. `&'static T` vs `T: 'static`.** `&'static T` is a reference whose referent lives until the program ends: literals,
statics, leaked data. `T: 'static` means `T` contains no borrows shorter than `'static`. An owned `String` contains no
borrows at all, so it qualifies, even though it can be dropped at any moment.

**6. Erased lifetimes.** Code generated for a function is identical whatever the lifetimes, so there's nothing to
specialize and it's compiled once. Inside the body, a lifetime parameter is a *universal region*, an unknown region the
caller picks, so the body must be valid for every choice. That's why you can't return a reference to a local through
`-> &'a T`.

**7. Borrowing vs owning structs.** Put lifetime parameters on short-lived views: parsers, tokenizers, iterators, guards
(`Tx<'db>`). Own data in long-lived domain types, and anything that crosses a thread, task, queue, or storage boundary.

**8. `Box::leak`.** It's safe because leaked memory stays valid forever, and leaking can't cause UB (Chapter 1.3). Use it
for data created once that lives for the whole process: configuration, static lookup tables, a *bounded* set of interned
labels.

### Debugging exercise (elided tokenizer)

1. Rule 3 fires: `fn next_token<'s>(&'s mut self) -> Option<&'s str>`.
2. `method` holds an `Option<&'s str>` whose `'s` is the first call's `&mut tokens` loan. That loan must stay live until
   `method`'s use in `println!`, so the second call's `&mut tokens` overlaps it: two exclusive loans on `tokens`.
3. Fix: `-> Option<&'a str>`. A legitimately lending API: `fn next_line(&mut self) -> Option<&str>` on a reader that
   reuses an internal buffer, overwritten by every call. Tying the line to `&mut self` is *exactly* right there, because
   it forbids holding a line across the next read.

### Selected exercises

- **Beginner:** `first(v: &[u32]) -> &u32`: rule 2, no annotation needed. `pick(a: &str, flag: bool) -> &str`: rule 2
  (one reference input). `name(&self) -> &str`: rule 3. `join(a, b) -> String`: no output lifetime at all.
- **Advanced:** the result borrows only from `s`. With a single `'a` for both parameters, `sep`'s loan would have to last
  as long as the result, so a caller passing a temporary or short-lived separator (`&format!(...)` dropped early) fails.
  Two lifetimes state the truth: `'b` is unrelated to the output.

---

## Chapter 4.4 — Variance and Subtyping

### Interview & architecture questions

**1. Only lifetimes.** Subtyping exists only between lifetimes (and the higher-ranked function types built from them).
There's no inheritance, so `Dog` isn't a subtype of `Animal` and `Vec<Dog>` can't become `Vec<Animal>`. Polymorphism
comes from traits and generics (Part VI).

**2. The three variances.** Covariant: `&'a T` in `'a`, so a `&'static str` is usable as `&'a str`. Contravariant:
`fn(T)` in `T`, so an `fn(&'a str)` that works for all `'a` is usable as `fn(&'static str)`. Invariant: `&mut T` in `T`,
so a `&mut &'static str` can't be used as `&mut &'a str`.

**3. The `overwrite` counterexample.** If `&mut &'static str` could be treated as `&mut &'a str`, `overwrite` could store
a pointer to a short-lived `local` into `label`. The block ends, `local` is freed, and reading `label` is a
use-after-free. Invariance forces `'a = 'static`, which rejects `&local`.

**4. `Cell` vs `Vec`.** `Cell<T>` allows writes through a shared reference, so it must be invariant for the same reason as
`&mut`. `Vec<T>` is covariant because writing into it requires `&mut Vec<T>`, and `&mut` is invariant in `Vec<T>`. The
covariance applies only to an owned `Vec` being moved or shared read-only, where shortening the lifetime harms nobody.

**5. Function contravariance.** A function that accepts any lifetime can replace one that needs only `'static`, because
it accepts more. The reverse would pass it references it can't handle. "One type is more general than the other": the
expected type is higher-ranked (`for<'a> fn(&'a _)`), and the provided function only works for `'static`.

**6. Variance inference.** rustc computes each parameter's variance from how it's used in the fields, combining uses: any
invariant use, or covariant plus contravariant, makes it invariant. You adjust it with `PhantomData<T>` (covariant),
`PhantomData<fn(T)>` (contravariant), or `PhantomData<fn(T) -> T>` / `PhantomData<Cell<T>>` (invariant).

**7. Java arrays vs `&mut`.** Java arrays are covariant, so it checks every reference store at run time
(`ArrayStoreException`), paying on every write and failing in production. Rust makes the mutable case invariant: rejected
at compile time, at some cost in flexibility.

**8. #25860.** Implied bounds combined with the variance of function types let contrived safe code extend a lifetime to
`'static`, causing a use-after-free. Lesson: soundness is a process, and even the type system has had gaps. Keep the
trusted base small, and prefer well-trodden patterns.

### Debugging exercise (`overwrite`)

1. `label: &'static str` → `&mut label: &mut &'static str` → **invariance**: `T` in `&mut T` must be exactly
   `&'static str` → `overwrite<'a>` needs `slot: &mut &'a str`, so `'a = 'static` → `value: &'static str` → `&local` must
   be `'static`, and it isn't.
2. No. `fn overwrite<'a: 'b, 'b>(slot: &mut &'b str, value: &'a str)`: invariance still forces `'b = 'static`, and then
   `'a: 'static`, so it's the same requirement on `local`.
3. Make `label` a `String` and assign `label = local.clone()` (owned, so the label outlives `local` independently), or
   keep `label` inside the block (`let mut label: &str = "default";` declared *in* the block, so its lifetime can be
   short). The first changes ownership. The second changes scope.

### Selected exercises

- **Beginner:** `&'a u8` covariant. `&'a mut &'a u8` invariant (the inner `&'a u8` sits in `&mut`'s invariant position).
  `Option<&'a str>` covariant. `Cell<&'a str>` invariant. `fn(&'a str)` contravariant. `Box<dyn Fn(&'a str)>` invariant
  (a trait object's generic parameters are invariant).

---

## Chapter 4.5 — Higher-Ranked Trait Bounds

### Interview & architecture questions

**1. Caller-chosen vs for-all.** `<'a, F: Fn(&'a str)>`: the caller picks one `'a` that outlives the whole call, so the
callee can't pass references to its own locals. `F: for<'a> Fn(&'a str)`: the callback must work for every `'a`, so the
callee can lend short-lived data on each call.

**2. No retention.** Storing the reference requires its lifetime to outlive the storage, which is unprovable for an
arbitrary `'a`, hence E0521. The invariance notes: the captured `&mut Vec<&'x str>` fixes `'x`. Invariance stops it from
adjusting, and the placeholder `'a` can't be proven to outlive it.

**3. Implicit HRTBs.** `F: Fn(&str)` bounds, `Box<dyn Fn(&str)>` trait objects, `fn(&str)` pointer types, closure
parameters of iterator adapters (`filter(|x: &&T| ..)`), and serde's `DeserializeOwned`.

**4. Standalone closures.** Without an expected signature, each elided reference lifetime is inferred independently, so
the returned reference isn't related to the parameter. Fixes: a function item (normal elision), or passing the closure
where an HRTB bound supplies the signature (or through a helper like `fn constrain<F: for<'a> Fn(&'a str) -> &'a str>(f:
F) -> F { f }`).

**5. `for<'de> Deserialize<'de>`.** Deserializable from input of any lifetime, so it can't borrow from the input, so it
owns all its data. serde names it `DeserializeOwned`.

**6. Checking HRTBs.** The solver replaces the bound lifetime with a fresh placeholder region, about which nothing is
known beyond the bound itself, and checks the impl or closure against it. Any requirement relating the placeholder to an
outside region fails.

**7. Lending iterators.** `Iterator::next(&mut self) -> Option<Self::Item>`, where `Item` is fixed per iterator type and
can't depend on the `&mut self` borrow of each call. Use visitor or callback APIs with HRTB bounds, inherent
`next_line(&mut self) -> Option<&str>` methods, or GAT-based lending-iterator traits from crates.

**8. When bugs are found.** Rust: at compile time (E0521). Netty: partly at run time (reference-count exceptions, a
sampling leak detector), otherwise as silent corruption or data exposure.

### Debugging exercise (E0521)

1. Each line's `String` is freed at the end of its iteration. Storing its `&str` and reading it later reads freed memory,
   a use-after-free.
2. Copy: a `Vec<String>` and `failures.push(line.to_owned())`, one allocation per *kept* line. Keep the input alive: read
   everything into one `String` and iterate `input.lines()`, which yields `&str` borrowed from it. `Vec<&str>` then works,
   with zero per-line allocations but the whole input resident in memory.
3. No. `Vec<String>` has no lifetime parameter, so there's nothing to be invariant over.

---

## Chapter 4.6 — Reading Errors as Proofs

### Interview & architecture questions

**1. A–B–C.** A: the loan (or move) is created. B: a conflicting action. C: a later use that keeps the loan live at B.
Chapter 1.1's E0502: A `let first = &v[0]`, B `v.push(4)`, C `println!("{first}")`. The proof: `push` may reallocate the
buffer `first` points into, and `first` is read afterwards.

**2. Translations.** E0505: moving would relocate or free the value while a loan still points at it. E0506: assigning
changes a value that a live shared loan promised was frozen (and drops the old value). E0597: the owner goes out of scope
while a loan on it is still going to be used. E0716: the same, where the owner is an unnamed temporary freed at the end
of its statement.

**3. Temporary lifetime extension** applies to specific syntactic positions, notably `let x = &temp;`, and not to a
temporary used as the receiver of a method whose result borrows from it.

**4. Strategies, with examples.** Shorten: Chapter 1.1's Fix 3, using `first` before `push`. Split: disjoint fields
(3.3), `split_at_mut`, `get_disjoint_mut` (4.1). Re-API: `entry` (4.2), `retain` (4.1), `mem::replace` (3.2).
Re-own: `move` into threads (4.3), `to_owned()` at queue boundaries (4.3, 4.5), `Arc` (3.1). Restructure: the index-linked
LRU and arenas (3.6).

**5. `clone()`.** Right when an independent copy is semantically required: a snapshot, divergent modification, or small
data crossing a boundary. A smell when it avoids an ownership decision, copies large data in hot paths, or creates copies
that should have stayed in sync.

**6. "Later used here."** The diagnostics walk the constraint graph the checker used and pick the constraint that forced
the loan's region to include B, typically the nearest later use ("best blame"). It's a derivation from the same facts as
the verdict.

**7. The UB program printed `1`** because freed memory usually stays mapped and wasn't overwritten before the read. UB
permits any behavior, including the correct-looking one, so the output is no evidence of correctness.

**8. `unsafe` to silence.** It deletes the compiler's proof obligation for that reference for all *future* edits too, so a
later change that makes the code genuinely unsound (the eviction step in §10) goes undetected.

### Debugging exercise (`cache_key`)

1. The original returned `&key`: E0515. A: the borrow `&key`. B: `key` is dropped when the function returns. C: the
   caller uses the returned reference.
2. Every call leaks one heap allocation (the boxed string): unbounded memory growth, as measured in Chapter 4.3 (1,000
   calls, 1,000 blocks leaked).
3. Return an owned `String` (strategy **re-own**): `fn cache_key(user: &str) -> String`. The key is computed per call
   anyway, so there was never anything to borrow from.

---

## Part IV Review — Capstone triage

**Error 1 (`expire_idle`, E0502).** A: `&self.sessions`, the iteration loan. B: `self.sessions.remove(id)`. C: the loop
header (the next `next()`). At run time, removing from the map during iteration changes the table the iterator walks, and
`id` is a reference *into* the key being removed, so it would dangle. **Re-API:**
`self.sessions.retain(|_, s| s.hits > 0)`.

**Error 2 (`rename`, E0506).** A: `let old = &s.user`. B: `s.user = user.to_string()`, which drops the old `String` and
frees its buffer. C: returning `old` (it must be borrowed for `'1`). At run time, `old` would point into the freed
buffer. **Re-own:** `std::mem::replace(&mut s.user, user.to_string())`, returning the old value as an owned `String`.

**Error 3 (`main`, E0499).** A: `m.touch(1)`. Rule 3 ties the returned `&Session` to the `&mut m` borrow, so `m` stays
mutably borrowed while `s` lives. B: `m.expire_idle()`, a second `&mut m`. C: `s.user` in the `println!`. At run time,
`expire_idle` could remove the session `s` points to (the compiler can't know that session 1's hit count is now 1).
**Shorten or re-API:** return a copy of what the caller needs (`hits: u32`), or print before calling `expire_idle`.

**Error 4 (`main`, E0502, the subtle one).** A: `m.rename(1, ..)` returns `&str` tied, by rule 3, to the `&mut m` borrow,
so `m` is *exclusively* borrowed while `old` lives. B: reading `m.log`. C: `old` in the `println!`. The field is disjoint
from anything `rename` touches, but the **signature** says the result borrows all of `*m` mutably, and the checker reads
signatures, not bodies. **Re-own:** return the old name as a `String`, and the borrow ends at the call.

**Design questions.** `touch` compiles because `s` is a loan on the place `self.sessions` (via `get_mut`), and `self.log`
is a **disjoint field**: a split borrow (Chapter 4.1). Should `touch` return `&Session`? Returning a reference into the
manager **locks the whole manager exclusively** (it came from `&mut self`) for as long as the caller holds it, which is
exactly error 3. Return what callers need (a copy, an ID, a small snapshot struct) and keep the manager usable.

---

## Part IV Review — Interview mode

1. **What's checked:** accesses to places are checked against live loans. A shared loan permits reads, and a mutable
   loan permits nothing else, both extending to parents and children of the place (Chapter 4.1).
2. **Unique references:** Chapter 4.1, questions 3 and 4.
3. **What a lifetime describes:** the region of program points where a loan must stay valid, from its creation to its
   last uses. It isn't a duration, it doesn't exist at run time, and it never keeps anything alive.
4. **`T: 'static`:** Chapter 4.3, question 5.
5. **How borrow checking works:** Chapter 4.2, question 4, plus Chapter 4.1's conflict table.
6. **Problem case #3 and Polonius:** Chapter 4.2, question 5.
7. **Lifetime compilation:** Chapter 4.3, question 6.
8. **Checking HRTBs:** Chapter 4.5, question 6.
9. **`&mut` invariance:** Chapter 4.4, question 3.
10. **Caller-chosen vs higher-ranked:** Chapter 4.5, question 1.
11. **`noalias` vs `Cell`:** `&i32` lets LLVM reuse a loaded value across an opaque call (`add eax, eax`), and
    `&Cell<i32>` forces a reload (`add eax, dword ptr [rdi]`). Chapter 3.3's `add_twice` shows the same for `&mut`
    against raw pointers.
12. **Run-time cost:** none. Lifetimes and borrow checking are erased. What they buy at run time is the freedom not to
    copy (zero-copy parsing and visitors) and pervasive `noalias` optimization.
13. **API boundaries:** a lifetime view for scope-contained processing; owned data for storage and crossing threads;
    refcounted buffers (`Bytes`, `Arc<[u8]>`) for shared data across tasks (Chapters 4.3 and 4.5).
14. **Zero-copy callbacks:** a visitor with a higher-ranked bound over a reused buffer, `ControlFlow` for early exit, and
    `to_owned()` as the documented way for consumers to keep data (Chapter 4.5 §9).
15. **Team policy:** A–B–C in PR descriptions, named fix strategies, a justifying comment for every `clone()` of a
    collection in a hot path, `Rc<RefCell<_>>` only for genuinely dynamic sharing, no `unsafe` to fix borrow errors, and
    Miri in CI for crates with `unsafe` (Chapter 4.6 §9).
