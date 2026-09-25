# Appendix A — Answer Key: Part I

> Read an answer only after writing your own. These are **model answers**: what a strong senior or principal candidate
> would say. Where several good answers exist, the key says so. If yours differs, check whether it's *wrong* or just
> *different*. The second is fine if the reasoning holds.

---

## Chapter 1.1 — The Problem: Who Frees This Memory?

### Interview & architecture questions

**1. Why is a use-after-free harder to diagnose than a null dereference?**
A null dereference touches page 0, which is never mapped, so the MMU faults **at the faulting instruction** and the
stack trace points at the cause. A use-after-free touches memory that is **still mapped**, because allocators keep freed
chunks in user-space free lists. The read succeeds and returns stale data, allocator metadata, or another object's
bytes. A write corrupts whatever lives there now. The crash, if one comes, happens later and somewhere else (often
inside `malloc`/`free`), and it depends on allocation order and timing, so it's nondeterministic. That
**cause–symptom distance** is what makes it hard. ASan works by keeping freed chunks in quarantine and poisoned, so the
first bad access faults right away.

**2. Rust rejected `push` even with spare capacity. Flaw?**
It's a deliberate trade, not a flaw. To accept that program the checker would need to know Vec's *run-time* state
(`len < cap`), which means either interprocedural analysis of `push`'s body or a much richer signature that exposes
growth behavior in the type. Both are costly. Whole-program analysis is slow and non-modular. Encoding implementation
details in signatures means any change to `Vec`'s internals could break callers' compilation. Error messages would also
get less predictable. Rust keeps checking **local and signature-based**, which buys modularity, speed, and stable
library APIs, and accepts false positives. Those are handled with indices, reordering, or (rarely) `unsafe` with a
documented invariant.

**3. Can Java have the *logical* equivalent of use-after-free?**
Yes. The object stays alive, but its meaning is gone:
(a) using an iterator or a `subList` view after the backing list was structurally modified (CME, or a silent skip);
(b) holding a pooled connection or buffer after returning it to the pool, while another request is now using it (a
cross-request data leak); (c) using a cached entity after it was evicted or updated elsewhere; (d) using a closed
`ResultSet`/stream. GC guarantees **liveness**, not **validity**.

**4. What does UB permit, and why do compiler engineers want it?**
The standard places **no requirements** on a program that executes UB. The compiler may assume UB never happens and
optimize on that basis: delete null checks after a dereference, fold `x + 1 < x` to `false`, assume loop counters
don't wrap (which enables widening induction variables and vectorizing), reorder memory accesses under strict aliasing,
and hoist loads of non-atomic variables out of loops (assuming no data races). UB exists because it gives optimizers
freedom and lets one standard fit very different hardware. The price is that a program executing UB has no meaning at
all.

**5. "We run ASan and fuzzing in CI, so our C++ is memory-safe."**
That overstates it. ASan and fuzzing are **dynamic** tools. They find bugs only on paths that actually execute under
instrumentation, so their power is bounded by coverage, and complex state-dependent bugs often go unreached. ASan misses
some classes: uninitialized reads need MSan, data races need TSan, and intra-object overflows aren't caught. Production
binaries aren't instrumented, and UB-driven optimizations can behave differently in instrumented builds. The accurate
claim is: "we substantially reduce the *likelihood* of memory-safety bugs." The industry numbers (roughly 70% at
Microsoft and Chromium) come from organizations that use exactly these tools.

**6. Why is a data race a memory-safety issue in C++ and Go but not Java?**
In C++ a race is UB, so no guarantee survives it, including the compiler's own transformations. In Go, multi-word values
(interfaces are type pointer plus data pointer; slices are pointer, length, capacity) are written non-atomically, so a
racing reader can observe a **torn** value, such as a new type pointer paired with old data. That's type confusion, and
it leads to memory corruption. In Java the JMM guarantees that reference reads are never torn and that every reference
is to a properly typed object, so races give stale or inconsistent *values* but can't forge a pointer. (Non-`volatile`
`long`/`double` may tear, but they aren't pointers.)

**7. 200K untrusted protobufs/s: which classes matter?**
**Spatial** (length-prefixed fields driving reads and writes), **integer overflow in size arithmetic** (which leads to
undersized allocations and then spatial bugs), **temporal** (zero-copy views outliving their buffer), **resource
exhaustion** (deep nesting blowing the stack; huge declared lengths causing OOM), and **type confusion** in
oneof/union handling. Layer the defenses: a memory-safe parser language (Rust gives safety *with* zero-copy; Java is
safe but allocation-heavy at this rate, which is feasible but costs CPU and GC); checked arithmetic for sizes; recursion
depth and message size limits; fuzzing for panics and DoS; and process isolation or sandboxing if a C/C++ parser has to
stay.

**8. What does a tracing GC cost, and when does it dominate?**
Costs: **memory headroom** (the heap must exceed the live set; historically 2–5x for good throughput), **CPU** for
concurrent marking and relocation, **barriers** on reads and writes, **allocation stalls** or pauses when the allocation
rate outruns collection, cache pollution from tracing, and interaction with JIT warm-up. It dominates for: large
long-lived heaps under tail-latency SLOs; high allocation rates; memory-constrained containers where headroom costs
money; many small instances (per-instance overhead multiplied); and sub-millisecond latency targets. It barely matters
for I/O-bound services with small heaps.

### Debugging exercise (the `Registry`)

**Error:** E0502. The borrow checker cites: the immutable borrow at `registry.find(1)`, the mutable borrow at
`registry.add(...)`, and the later use at `println!("found {:?}", ada)`.

**The ownership argument.** `fn find(&self, id: u64) -> Option<&User>` elides to "the returned reference borrows from
`self`." So while `ada` is live, `registry` is shared-borrowed. `add(&mut self)` needs exclusive access, which conflicts.
If it were allowed, `push` could reallocate `users` and `ada` would dangle. That's Chapter 1.1's bug, one level of
abstraction up.

**Fixes and trade-offs:**
1. **Copy out what you need.** Take `let name = registry.find(1)?.name.clone();`, then `add`, then print `name`. Costs one
   `String` allocation. Simple.
2. **Reorder.** Build the new `User` (using `ada.name`) and print `ada` *before* calling `add`. Zero cost, but only
   works when the logic allows the reordering.
3. **Use IDs.** Keep `1`, look the user up again after `add`. Costs a second lookup, and it survives any number of
   mutations. It's the scalable pattern.
4. **Shared ownership.** Store `Arc<User>` (or `Rc<User>`) in the registry and have `find` return a clone of the `Arc`.
   The handle then outlives the borrow. Costs a refcount and an indirection, and users can be held across registry
   mutations.

(Two-phase borrows don't help here, because `ada` is used *after* the mutable call.)

### Selected exercises

- **Beginner classification:** (a) Heartbleed: spatial (over-read). (b) Pointer to a local: temporal (dangling). (c)
  `memcpy` with a packet-supplied length: spatial. (d) Two threads incrementing an `int`: concurrency (data race). (e)
  Field read before construction: initialization. (f) Wrong `Derived*` cast: type confusion. (g) Double `free` on an
  error path: temporal. (h) Erasing from a `std::map` while iterating: temporal (iterator invalidation).
- **Intermediate (`Vec<Order>`, 200 bytes, not `Copy`):** Fix 1 no longer compiles as written (`let first = v[0]`
  tries to move out of the vector). It becomes `v[0].clone()`, which copies 200 bytes plus any heap fields. Fix 2
  (index) works unchanged and costs nothing. Fix 3 (reorder) works unchanged. Prefer the index or the reorder. Clone
  only if you need a snapshot that's independent of later mutation.

---

## Chapter 1.2 — Five Languages, Five Bets

### Interview & architecture questions

**1. Why did Rust remove its GC, green threads, and segmented stacks?**
Segmented stacks had the **hot-split** problem: a call at a segment boundary in a hot loop allocated and freed a
segment on every iteration. Go hit the same thing and moved to copyable contiguous stacks. Green threads needed a
mandatory runtime scheduler that made FFI and blocking calls awkward and stopped Rust from being embedded in C programs,
kernels, browsers, or other language runtimes. `@` GC pointers added a runtime collector while ownership plus a library
`Rc` covered the need. **Gained:** zero mandatory runtime, C-level embeddability, predictable performance, and "pay only
for what you use." **Lost:** built-in lightweight concurrency (later regained as library-level async/await, at the cost
of function coloring and several competing runtimes) and GC's convenience for graphs.

**2. Why does a Go binary contain a scheduler and a GC when Rust's doesn't?**
In Go they're part of the **language semantics**. There's no `free`, so memory reclamation must be automatic. The `go`
statement and channels assume a scheduler, and escape analysis moves values to a heap that the GC manages. In Rust,
ownership makes the compiler insert frees, so no GC is needed. Threads are OS threads, and async executors are
libraries, so no scheduler is needed. Go buys simplicity and fast development. Rust buys predictability, small binaries,
embeddability, and control, at the cost of more concepts and choosing a runtime yourself.

**3. How can a JIT beat AOT, and how do you close the gap?**
A JIT **speculates** from live profiles. It inlines interface calls it has seen to be monomorphic, prunes branches that
are never taken, specializes for observed types, and targets the exact CPU. When a speculation breaks, it deoptimizes.
To close the gap: make dispatch **static by default** (Rust's monomorphized generics leave nothing to speculate about),
use **PGO** and **BOLT** for layout and branch information, **LTO** for cross-crate inlining, and target-CPU settings or
function multiversioning for ISA features.

**4. `ArrayList<Point>` vs `Vec<Point>` layout.**
Java: the `ArrayList` object points to an `Object[]` (16-byte header plus 4-byte compressed references), and each
reference points to a separate `Point` (12-byte header + 16 bytes of data + 4 bytes of padding = 32 bytes). Rust: a
24-byte handle points to one contiguous buffer of 16-byte `Point`s. **When it matters:** tight loops over large
collections (bandwidth, cache misses, prefetching), memory-bound footprints, and GC scan cost. **When it's noise:**
small collections, heavy per-element work, I/O-bound services, and cases where TLAB allocation order already gives
near-sequential layout.

**5. Stackful vs stackless concurrency.**
**Stackful** (goroutines, virtual threads): each task has a real, growable stack of a few KB and up. A blocking call
parks the task and the runtime reuses the OS thread (with some pinning cases in Java). Deep recursion just grows the
stack, up to a limit (Go's default maximum is 1 GB on 64-bit). **Stackless** (Rust futures): the task is a state
machine whose size is the largest set of values live across any `.await`, known at compile time and often a few hundred
bytes. A blocking call **blocks the executor's worker thread** and starves other tasks, so blocking work goes to
`spawn_blocking`. Recursion needs boxing (`Box::pin`), because a recursive future would otherwise have infinite size.
Suspension happens only at `.await`.

**6. How can a Go data race break memory safety?**
An interface value is two words: a type/itab pointer and a data pointer. If one goroutine assigns `x = &A{}` and then
`x = &B{}` while another calls `x.M()`, the reader can see **B's itab with A's data pointer**, and it will run B's method
on A's memory, treating A's fields as B's. That's type confusion, and it gives arbitrary reads and writes. Slices tear
the same way (a pointer from one value, a length from another, and you get out-of-bounds access).

**7. "Modern C++ is as safe as Rust in practice." Steelman, then evaluate.**
*Steelman:* RAII everywhere, `unique_ptr`, no raw `new`/`delete`, `span`, hardened standard-library modes, sanitizers,
fuzzing, static analysis, and Core Guidelines checkers together eliminate most bugs, and many C++ codebases have
excellent records. Rust has `unsafe` too. *Evaluation:* in C++ all of that is **opt-in and unenforced**. References,
iterators, `string_view`, and `span` can still dangle because lifetimes aren't tracked. Data races aren't prevented, UB
is pervasive (overflow, uninitialized reads, aliasing), and the tools are coverage-limited. Discipline erodes with scale
and turnover, and the 70% data comes from teams that already follow these practices. **Verdict:** disciplined modern
C++ is much safer than C, but it isn't Rust-equivalent as an *organizational guarantee*, because Rust makes safety the
default and makes the exceptions (`unsafe`) searchable and auditable.

**8. Where does Rust deliberately pay a cost C++ doesn't?**
**Bounds checks** on indexing (C++ `operator[]` is unchecked), **overflow checks** in debug builds, and runtime borrow
flags in `RefCell`. Justification: a bounds check turns spatial UB into a deterministic panic at the faulting line, costs
one well-predicted branch, is frequently removed by the optimizer (Chapter 1.3), is avoided entirely by iterators, and
has an explicit opt-out (`get_unchecked` in `unsafe`) for hot spots that have been measured and proven.

### Debugging exercise (`largest` + E0597)

**Ownership argument:** `scores` owns a `Vec` whose heap buffer holds the `9`. At the inner block's closing brace,
`scores` is dropped and the buffer is freed. `top` is an `Option<&i32>` pointing *into* that buffer, so at the
`println!` it would dangle.

**Fixes:** (1) Move `let scores = ...` out of the inner block so it lives as long as `top`. (2) Make `top` own its value:
`largest(&scores).copied()` gives an `Option<i32>`. For non-`Copy` types use `.cloned()`, or better, consume the vector:
`scores.into_iter().max()` returns the owned element **without any clone**. (3) Return a *position*: an index of the
maximum, which is valid for as long as you keep the vector.

**Trade-offs:** `u64`: copying is 8 bytes, so `.copied()` is ideal. `String`: `.cloned()` allocates. That's fine once
and bad in a loop; consuming with `into_iter().max()` avoids it. `[u8; 4096]`: cloning copies 4 KB, so keep the borrow,
return an index, or box it.

### Selected exercises

- **Beginner (absence):** C uses `NULL` or sentinel values, and forgetting to check is UB. C++ uses `nullptr` or
  `std::optional`; dereferencing null or an empty `optional` with `*` is UB, while `.value()` throws. Java uses `null` or
  `Optional`; forgetting gives an NPE at run time. Go uses `nil`, the `(v, ok)` idiom, and zero values; a nil pointer
  dereference panics, and writing to a nil map panics. Rust uses `Option<T>`, and forgetting is a **compile error**
  because an `Option<T>` isn't a `T`. You can only `unwrap()` explicitly, which panics on `None`. `Option<&T>` is the same
  size as `&T` (a guaranteed niche).
- **Advanced (10M orders):** Rust `Order { id: u64, price: f64, qty: u32, side: u8 }`: 8+8+4+1 = 21 bytes, padded to
  alignment 8 = **24 bytes**, so 10M × 24 B = **240 MB**. Java: 12-byte header + 21 bytes of fields = 33, rounded to 8 =
  **40 bytes** per object, plus a 4-byte reference in the backing array = 44 B, so about **440 MB** (plus `ArrayList`
  slack from 1.5x growth). **Struct-of-arrays** (`long[]` + `double[]` + `int[]` + `byte[]`): 80 + 80 + 40 + 10 =
  **210 MB**, which is *smaller than Rust's array-of-structs*, because SoA removes padding. Rust gets the same 210 MB if
  it uses SoA. In Java you give up object identity, encapsulation, and ordinary collection APIs, and you manage indices
  by hand.
- **Systems (idle threads):** An idle JVM typically shows well over a dozen threads: main, GC workers, C1/C2 compiler
  threads, VM thread, Reference Handler, Finalizer, Signal Dispatcher, service threads, Common-Cleaner, and so on. The
  exact count depends on the JDK version, the collector, and the core count. Go shows a handful (the main M, `sysmon`,
  plus threads created as needed for GC and syscalls). Rust shows **1**. The exact numbers vary, and what matters is
  that you can name every thread you see.

---

## Chapter 1.3 — Rust's Bet

### Interview & architecture questions

**1. Destructive vs non-destructive moves.**
**Rust (destructive):** the moved-from name is statically dead, so no destructor runs for it and types don't need an
"empty" state. Exactly-once freeing needs no runtime flags, except drop flags on conditional paths, which are usually
optimized out. Moves are plain `memcpy`s. Costs: self-referential types are hard (they need `Pin`, Part XII), and you
can't move out of borrowed content or out of an index without leaving something behind (`mem::take`, `mem::replace`,
`mem::swap`). **C++ (non-destructive):** the moved-from object stays in a "valid but unspecified" state and its
destructor still runs, so every movable type needs a cheap valid empty state (this is why `unique_ptr` is nullable),
destructors grow extra branches, and use-after-move compiles. The upside is that custom move constructors can fix up
self-pointers (for example, small-string optimization buffers).

**2. How library signatures enforce thread safety, and what breaks if `Rc` were `Send`.**
`Send` and `Sync` are **auto traits**: the compiler implements them structurally, and types with thread-unsafe insides
(`Rc`'s plain counters, raw pointers, `Cell`) don't get them unless someone writes an `unsafe impl` taking
responsibility. **Lifetime bounds** (`'static`, `'scope`) guarantee that captured borrows outlive the threads that use
them. `thread::spawn` combines both: `F: FnOnce() -> T + Send + 'static`. If `Rc` were `Send`, clones in two threads
would do concurrent **non-atomic** increments and decrements of the strong count. Updates would be lost, the count would
hit zero while references still existed (use-after-free and double free), or never hit zero (a leak).

**3. Why is leaking safe, and what's the API lesson?**
A leak can't cause UB: memory that's never freed stays valid. Leaks are also unavoidable in safe code anyway (`Rc`
cycles, process exit). In 2015 the pre-1.0 `thread::scoped` returned a guard whose destructor joined the thread.
Leaking the guard let the thread outlive the stack data it borrowed, a use-after-free in safe code. Resolution (RFC
1066): `mem::forget` is safe, and **no safe API may rely on a destructor running for soundness**. API lesson: enforce
invariants with **scopes and closures** that do the cleanup before returning (`thread::scope`), or design guards so
that leaking one only leaks or poisons state rather than exposing it. `Vec::drain` sets the vector's length up front, so
a leaked `Drain` leaks elements instead of exposing moved-out slots ("leak amplification").

**4. Three classes safe Rust doesn't prevent, with countermeasures.**
(a) **Deadlock.** Two transfers locking accounts in opposite order. Countermeasures: a global lock order (lower ID
first), `try_lock` with back-off, or single-owner actors. (b) **Race condition / TOCTOU.** Check-then-act across two
lock acquisitions, as in Chapter 1.3's overdraft, or across a database read and write. Countermeasures: an atomic
check-and-act (one guard, CAS, a conditional `UPDATE ... WHERE`, database constraints), plus idempotency keys for
retries. (c) **Leaks.** `Rc` cycles in an observer graph, or an unbounded cache. Countermeasures: `Weak` back-edges,
arenas, bounded caches with eviction, and memory alerts. (Also **panics**: `unwrap()` on unexpected input.
Countermeasures: `Result` at trust boundaries, a panic boundary per request or `panic = "abort"` with supervisor
restarts, and fuzzing.)

**5. Why `Relaxed` was enough, and what would break it.**
`fetch_add` is an atomic read-modify-write. Every RMW works on the latest value in that variable's modification order,
whatever the ordering argument, so no increment is lost. The final read comes after `thread::scope` has **joined** every
thread, and joining creates a **happens-before** edge, so every increment is visible. It becomes **insufficient** when
the counter *publishes* other data: for example, a producer fills a buffer and then increments `ready` with `Relaxed`,
and a consumer sees `ready == N` and reads the buffer. Nothing then orders the buffer writes before the buffer reads,
so the consumer may see stale data (on weakly ordered CPUs like ARM, and from compiler reordering on any CPU). The fix
is a `Release` increment paired with an `Acquire` load (Part XIV).

**6. What "zero-cost abstraction" does and doesn't promise.**
It promises that *in optimized builds* the abstraction's code is as good as the hand-written equivalent, and that
features you don't use cost nothing. It does **not** promise zero compile time (monomorphization and inlining cost
build time), small binaries (each instantiation is separate code), good performance in debug builds (no inlining means
deep call chains), that the equivalent hand-written code is cheap (`collect()` still allocates), or that the optimizer
always succeeds (some chains fail to vectorize or keep bounds checks, so measure).

**7. What `&mut T` tells LLVM.**
rustc marks `&mut T` parameters `noalias` (and shared references to types without interior mutability as `readonly`
plus `noalias`): during the call, no other pointer accesses that memory. LLVM can then keep values in registers across
stores through other pointers, eliminate redundant loads, reorder, and vectorize. In C a `T*` may alias any other
compatible pointer (and `char*` aliases everything), so the compiler has to reload after stores unless it can prove
otherwise. `restrict` exists but is rarely used and unchecked, and it's UB if the promise is false. Example:
`fn f(a: &mut i32, b: &i32) { *a += *b; *a += *b; }` can load `*b` once.

**8. "Safety is a property of an API boundary": explain with `Vec`.**
`Vec` is built from raw pointers, manual allocation, and uninitialized spare capacity. Its soundness rests on invariants:
`ptr` is valid for `cap` elements, the first `len` are initialized, and `len <= cap`. The fields are private, so only
code inside the module can break those invariants, and every public method preserves them. That makes the API sound for
**every possible safe caller**. The same logic explains why `set_len` is an `unsafe fn` even though its body contains no
unsafe operation: calling it with a bad length would break an invariant that *other* code's `unsafe` blocks rely on.
Soundness belongs to the module, not the line.

### Debugging exercise (`Timer`)

1. `let _ = expr;` **doesn't bind**. `_` is a pattern that matches without taking ownership, so the temporary `Timer`
   is dropped at the end of the statement. It measures nothing, which is where the ~190 ns comes from. `let _timer = ...`
   *is* a binding (the leading underscore only silences the unused-variable warning), so the value lives until the end
   of the scope.
2. Change `_` to `_timer`.
3. `let_underscore_lock` is a **lint**, a heuristic that recognizes std lock guard types, for which dropping immediately
   is almost always a bug. For an arbitrary user type the compiler can't tell whether an immediate drop is intended
   (`let _ = result;` to ignore a `Result` is idiomatic). The allow-by-default `let_underscore_drop` lint covers types
   with significant destructors, if you opt in. `#[must_use]` wouldn't help either, because `let _ =` counts as
   explicit use. **The type system guarantees soundness. Lints guess at intent.** The timer bug is a logic bug, not a
   safety bug. Design lesson: when dropping early would be a bug, prefer closure-scoped APIs
   (`time("slow_query", || slow_query())`), which can't be misused this way.

### Selected exercises

- **Beginner (drop order):** For `Outer { a, b }`, `Outer`'s own `Drop::drop` runs first (if it implements one), then
  its fields in **declaration order** (`a`, then `b`). Locals drop in **reverse declaration order**. A value moved into
  a function is dropped when that function's parameter goes out of scope, which is when the function returns.
- **Intermediate (race-fix timing):** Expect *no sharing* ≪ *atomic* < *mutex*, with the gap widening as threads are
  added, because all atomic and mutex increments contend on one cache line. On a noisy shared VM such as the
  Playground, report ratios loosely and don't generalize beyond that machine.

---

## Chapter 1.4 — The Honest Cost

### Interview & architecture questions

**1. Where does Java do its optimization work, and what does it cost?**
At **run time**, in the JIT, on every process start, on production CPUs. Costs: warm-up latency (slow early requests and
elevated p99 for seconds to minutes), CPU spent on compiler threads, memory for the code cache and profiles, and
deoptimization storms when assumptions break. Mitigations include CDS/AOT caches (Project Leyden), CRaC, GraalVM Native
Image (which gives up peak JIT performance), and warm-up traffic. For autoscaling and serverless, where processes start
constantly, warm-up is a recurring cost.

**2. Why are parent pointers and doubly linked lists hard, and what are the designs?**
Ownership forms a tree. A back-edge is a second path to a node the parent owns and may mutate, which is aliasing plus
cyclic lifetimes, and the checker can't prove it's free of dangling. Designs: (1) **`Rc<RefCell<_>>` + `Weak`
back-edges**: independent node lifetimes, dynamic structure, small graphs. (2) **Arena + indices** (with generations,
e.g. `slotmap`): performance, big graphs, serialization, bulk lifetimes. (3) **Arena references** (`typed_arena`,
`bumpalo`: `&'arena Node` with `Cell` for mutation): every node lives as long as the arena, and cycles are allowed. (4)
**`unsafe` raw pointers behind a safe API**: only for foundational structures (std's `LinkedList`) with documented
invariants. Or use a library such as `petgraph`.

**3. Zero-copy `&'a str` fields: benefit, cost, and deciding per module.**
Benefit: no allocation or copy per field, less allocator pressure, better cache use. Cost: `'a` spreads into every
holder and signature; values can't outlive the buffer (no long-lived caches, no sending to other threads or tasks
without owning the buffer); refactors ripple. Decide per module: hot parsing and short-lived request processing use
**borrowed** data. Anywhere data is stored, queued, or crosses thread or task boundaries uses **owned** data, or
refcounted shared buffers (`bytes::Bytes`), or `Cow<'a, str>` when you only sometimes need to own.

**4. 1M req/s across 32 cores: does the language matter?**
That's about 31K req/s per core, or ~32 µs (~100K cycles) per request. It's a generous budget: GC barriers, virtual
calls, and some allocation all fit. The language probably doesn't dominate *unless* per-request CPU is heavy (TLS
handshakes, crypto, large JSON), the tail SLO is tight (a sub-ms p99.9 makes GC and allocation stalls matter), or
memory per instance is capped. You'd also need: measured CPU ms per request, the I/O share, heap size and allocation
rate, cost per core, instance count, and team skills.

**5. When is Java + ZGC the better choice?**
When the team and the ecosystem are Java; the SLO is in the milliseconds (p99 around 10–50 ms, where sub-millisecond
pauses are noise); allocation rates are manageable; memory headroom is affordable; processes are long-lived so warm-up
is amortized; the domain changes quickly; and JFR-level observability matters. Rust wins when p99.9 needs to be under a
millisecond, when there are huge live heaps with high allocation rates, when memory per pod is tight, or when cold
starts are frequent.

**6. Why new code in memory-safe languages beats rewriting old code.**
Vulnerability density falls with code age. Old code has been exercised, fuzzed, and patched, so most of its bugs have
already been found, while new and recently changed code has the highest density. Writing new code in a memory-safe
language removes most *future* vulnerabilities cheaply. Rewriting old code is expensive, introduces new bugs, and aims
at the lowest-density code. Strategy: new components and features in Rust behind FFI or network boundaries; rewrite
only code that is both high-risk *and* high-churn; track vulnerability and incident trends.

**7. "We rewrote it in Rust and it's slower." Likely causes, most likely first.**
(1) The workload is I/O-bound, or the benchmark measures the wrong thing. (2) The build is wrong: a debug build, no
`--release`, no LTO. (3) The design is Java-shaped: a global `Arc<Mutex<_>>` under contention, `clone()` everywhere,
`String`-heavy data paths. (4) Async misuse: blocking calls inside async tasks, needless task spawning, async where
threads would do. (5) Rust-specific gotchas: `HashMap`'s default **SipHash** hasher (DoS-resistant but slower than
FxHash or aHash for small keys, which are fine where inputs aren't attacker-controlled), and **unbuffered or
line-locked I/O** (`println!` in a loop locks stdout on every call; use `stdout().lock()` with a `BufWriter`). The JVM
may also have been using a better algorithm or data structure than the port.

**8. Introducing Rust into a 200-engineer Java organization.**
Technical bridge: a **new service** behind the network (lowest coupling), an **in-process library** through the C ABI
and Java's FFM API (for hot paths), or a **sidecar**. Organizational steps: pick one team and one component that the
workload arithmetic justifies; train a core group; set review norms (a fluent reviewer on every PR, `clippy`,
`#![forbid(unsafe_code)]` in application crates); provide a paved-road template (tracing, metrics, config, error
handling); supply-chain CI (`cargo deny`, `cargo audit`, `cargo vet`); agree success metrics and exit criteria up
front; expand based on results; rotate engineers through to spread knowledge. Don't mandate it.

### Debugging exercise (`Request` with `&str`)

1. E0106 asks, **"how long is `path` valid for?"** A struct holding a reference has to name the data it borrows from,
   so that every instance's validity can be checked against that data.
2. At 1.2M req/s with ~12 small allocations each, that's ~14M allocations (and frees) per second. With a fast
   thread-caching allocator, that's on the order of tens of nanoseconds each, which is plausibly a significant fraction
   of a core or more. It's worse with cross-thread frees or a contended allocator, and every new allocation touches
   cache lines that weren't warm. *Measure it; the estimate only tells you it's worth measuring.* Zero-copy views
   allocate nothing and point into a request buffer that's already cache-hot. Choose **borrowed** for the parser's
   internal representation. For business logic, borrowed works if processing finishes within the request's lifetime;
   otherwise convert at the boundary.
3. **Hybrid:** borrow inside the parsing and routing layers. Convert to owned data exactly where the data's lifetime
   stops being "within this request buffer": where a command is queued, cached, persisted, or handed to another task.
   Convert only the fields that cross that line (often 2 of the 12). Use `bytes::Bytes` if the buffer itself can be
   refcounted and shared.

### Selected exercises

- **Beginner (removing from the arena):** `Vec::remove` shifts every later element, which invalidates every index
  handed out after the removed slot. Instead, tombstone the slot (`Option<Node>`) and keep a free list for reuse. But
  then a stale index can point at a **new** node that reused the slot. Fix: give each slot a **generation** counter and
  make handles `(index, generation)`. Lookups compare generations, so a stale handle returns `None` instead of the wrong
  node. That's what `slotmap` implements.

---

## Part I Review

### Architecture review: model points

1. **Bottlenecks.** Profile first. TLS handshake and signature cost is mostly **cryptography**, about the same in any
   language using comparable libraries. Session-resumption rates change it far more than the language does. JWT
   verification is also crypto (cache verified tokens or keys). Header parsing, routing, and logging are where language
   and allocation design matter. Expect the language change to affect only part of the 0.5 ms.
2. **Allocations.** In Java: per-request objects for headers, strings, the route context, log records, and buffers.
   Rust can parse headers zero-copy, route over borrowed paths, and format logs into reusable buffers. The cost is
   lifetime parameters in the parsing and routing layers, converting to owned data only where requests are queued or
   logged asynchronously.
3. **Contention.** Options: shard counters by core with periodic aggregation (approximate limits); a shard per key hash
   (`Vec<Mutex<HashMap>>`); atomic token buckets per key in a concurrent map; or ownership partitioning, where each core
   owns a subset of keys and requests are routed there. The trade-off is precision against contention. Chapter 1.3's
   counter already showed that "don't share" wins.
4. **Ownership boundaries.** A connection task owns its socket and buffers. A request borrows from its connection's
   buffer until it's routed. The upstream pool owns upstream connections and hands out guards that return on `Drop`.
   Rate-limit state is owned by a limiter, and requests hold an API-key *ID* rather than a reference into it. Response
   bodies stream through owned chunks (`Bytes`).
5. **Failure model.** A panic in a handler: catch it at the request or task boundary (Tokio isolates task panics) and
   return a 500 without taking down the process, *or* choose `panic = "abort"` plus fast restarts. A hung upstream:
   timeouts, cancellation (drop the future), circuit breakers. Memory limit: Rust aborts on allocation failure by
   default, so bound everything (connections, buffers, queues) and apply backpressure. Java today: GC thrash and
   allocation stalls first, then `OutOfMemoryError`, with a handler that may or may not recover.
6. **`unsafe`.** None in the gateway's own crates (`#![forbid(unsafe_code)]`). The trusted base is the dependencies:
   Tokio, Hyper, rustls, and the crypto provider underneath rustls (which may include assembly or C). Audit with
   `cargo geiger`-style unsafe inventories, `cargo vet`, and `cargo audit`, and track advisories.
7. **10x / 100x.** CPU for TLS saturates first. Then file descriptors and upstream connection pools. Then the
   rate-limiter's shared state if it's centralized. Memory stays flat only if every queue and buffer is bounded.
8. **Benchmark before the rewrite.** Build a thin Rust prototype of the hot path (TLS + parse + route + proxy) and
   compare it with the Java gateway using the **same** traffic replay: real resumption rate, payload sizes, keep-alive
   behavior, and route mix. Compare CPU per 1K req/s, p50/p99/p99.9 under open-loop load, memory per pod, and behavior
   during bursts. Decide from that data.
9. **Observability.** You lose JFR, heap dumps, and live attach. Compensate with structured `tracing` plus metrics,
   continuous profiling (perf/eBPF-based), tokio-console for async runtime visibility, core dumps with symbols, and
   explicit memory accounting for caches and queues.
10. **Rollout.** Not a weekend cutover. Shadow traffic first (mirror requests and compare responses and latency), then
    canary by percentage and by tenant, with an automatic rollback on SLO regression, and keep the Java gateway
    deployable for a full release cycle.
11. **Changing priorities.** *Latency first:* no GC, pre-allocated buffers, a per-core ownership design, and tightly
    tuned TLS resumption. The rewrite case strengthens. *Throughput per dollar:* the benchmark's cores per 1K req/s
    decides it. Compare the Rust prototype against a *tuned* Java gateway, not the current one. *Developer velocity:*
    tune the Java gateway, and perhaps move only one hot component (TLS offload, a proxy in front) to Rust or an
    off-the-shelf proxy.

### Interview mode: model answers

1. **Ownership without saying "safety":** it gives every resource exactly one party responsible for releasing it, at a
   point the compiler knows statically. That makes cleanup deterministic (memory, files, sockets, locks), removes the
   need for a runtime collector, and gives the compiler precise aliasing facts to optimize with.
2. **Non-null references:** a reference must point to a live, valid value, so null isn't a possible value. Absence is
   expressed as `Option<&T>`, which costs nothing because of the guaranteed niche (`None` uses the null bit pattern).
3. **UB and panics:** UB means the language places no requirements on the program. Safe Rust guarantees none, relative
   to the trusted base. A panic is **defined** behavior: a controlled unwind (or abort) with destructors run. It's a
   failure, not an undefined one.
4. **Data race vs race condition:** a data race is unsynchronized, conflicting (at least one write) concurrent access to
   the same memory. Safe Rust rules it out through `&mut` exclusivity plus `Send`/`Sync`. A race condition is a result
   that depends on timing, even when every access is synchronized (TOCTOU). Rust doesn't prevent those.
5. **Rejecting `push` without knowing its body:** it checks against the *signature*. `&mut self` demands exclusivity
   while a shared borrow is live, and that's a conflict whatever `push` does inside.
6. **Lifetimes erased:** lifetimes exist only during type and borrow checking and carry no run-time representation or
   cost. If they weren't erased, references would need run-time metadata and checks, which would be closer to a GC or
   borrow-flag scheme.
7. **Monomorphization:** it buys static dispatch, inlining, and specialization, so abstractions become as fast as
   hand-written code. It costs compile time and binary size, one copy per instantiation. `dyn Trait` is the opposite
   trade (Part VI).
8. **`Rc` vs `Arc`:** `Rc`'s count uses plain increments, so it isn't `Send`. `Arc` uses atomic RMWs, which are safe
   across threads but cost cache-line traffic under contention and are slower even uncontended.
9. **The lock owns the data:** with `Mutex<T>`, the guarded data can only be reached through the guard, so "forgot to
   lock" can't be written. `synchronized` associates the lock and the data only by convention.
10. **`Relaxed` for the counter:** RMW atomicity means no increment is lost, and the joins create happens-before for the
    final read. No other data is published through the counter.
11. **Async vs green threads:** async/await compiles each task into a state machine, a plain value that *any* executor
    (or none) can poll. It needs no special stacks, no scheduler in std, and no runtime baked into the language, so the
    runtime becomes a library choice. Green threads needed a runtime-managed stack and a scheduler inside std.
12. **Zero-cost:** in optimized builds, the abstraction's code is as good as the hand-written equivalent, and unused
    features cost nothing. Common misreadings: "it's free" (the hand-written equivalent still costs what it costs), "it
    compiles fast" (it doesn't), "debug builds are fast too" (they aren't).
13. **Java's optimization cost:** the JIT at run time, meaning warm-up on every instance plus compiler threads and code
    cache.
14. **100M ops/s on one core:** about 10 ns, or ~30 cycles, per operation. That rules out DRAM misses (one miss is
    5–10x the budget), allocation, locks, syscalls, and virtual dispatch in the hot loop. Data must be cache-resident,
    and the work often needs batching and SIMD.
15. **Choosing:** Rust over Java for no-GC tail latency, memory ceilings, CPU-bound fleet cost, untrusted parsers, and
    embedding. Rust over Go for the same reasons plus compile-time data-race freedom and zero-overhead abstraction in
    CPU-bound code. Neither when the service is I/O-bound, the domain is fast-changing, and the team is productive in
    what it has.
16. **Low-risk adoption:** see Chapter 1.4, question 8. One justified component, a clear boundary (a network service or
    FFM), written success metrics and exit criteria, a trained core group, and paved-road tooling.
17. **Slower rewrite:** see Chapter 1.4, question 7. Check what's being measured, the build profile, contention and
    cloning, async misuse, hasher and I/O buffering, and algorithmic differences.

### Capstone: model ADR outline

```text
ADR-001: Prototype a Rust data plane for the API gateway; decide rewrite by benchmark
Status:        proposed
Context:       400K req/s peak; ~0.5 ms CPU/req; ~330 cores; p99.9 = 45 ms at peak with spikes correlated to
               allocation stalls; 6 GB heaps; 2 GC-related SLO misses per quarter; team: Java experts, 2 engineers
               with Rust experience; gateway is stable, low feature churn.
Options:       A. Tune Java: reduce per-request allocation, raise TLS session resumption, ZGC tuning, right-size pods.
                  Low risk, weeks not months. Unknown ceiling on p99.9 improvement.
               B. Rust data plane (TLS + parse + route + proxy) behind the existing control plane; Java keeps admin/config.
               C. Adopt an off-the-shelf proxy (Envoy/Pingora-based) and keep custom logic as filters or plugins.
               D. Full Rust rewrite with weekend cutover (as proposed).
Decision:      Do A immediately (cheap, benefits regardless). In parallel, run an 8-week B prototype.
               Reject D (big-bang risk). Keep C as the fallback if the B prototype underperforms.
Consequences:  + measured decision; + A's gains bank early; − two efforts in parallel;
               − Rust skills concentrated in two people (bus factor), mitigated by pairing.
Validation:    Same traffic replay for all variants; metrics: cores per 1K req/s, p99.9 at peak, memory/pod,
               burst behavior. Go for B only if it shows ≥25% fewer cores AND p99.9 ≤ 15 ms versus the TUNED Java.
               Roll out by shadowing, then canary with automatic rollback.
Revisit when:  tuned Java meets the SLO with acceptable cost; traffic grows 3x or more; team Rust capacity changes.
```
