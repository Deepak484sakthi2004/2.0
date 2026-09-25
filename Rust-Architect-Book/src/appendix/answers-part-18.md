# Appendix A — Answer Key: Part XVIII

> Model answers. Write yours first. Where several answers are defensible, the key says so. Claims about rustc's
> internals carry the same caveat as the chapters: they describe rustc 1.98.1 (and nightly 1.100.0 of 2026-09-24 where
> stated) and will drift.

---

## Chapter 18.1 — rustc's Architecture: Queries and Incremental Compilation

### Interview & architecture questions

**1. Queries.** A query is a memoized, dependency-tracked function from a key to a value, computed on demand through
`TyCtxt`: `type_of(def_id)`, `typeck(def_id)`, `mir_borrowck(def_id)`, `optimized_mir(def_id)`, `layout_of(ty)`,
`symbol_name(instance)`. "Demand-driven" means the driver asks for final products (check every body, generate code for
these codegen units) and everything else is computed only because something needed it. A generic function that is never
instantiated never gets code; a body that `cargo check` never needs to codegen never reaches LLVM.

**2. E0391.** Evaluating `LIMIT` required evaluating `BURST`, which required `LIMIT`, which was already on the stack of
in-progress queries. Each "...which requires" line is one query's description on that stack, innermost last, so you read
the cycle directly. One mechanism detects cycles among constants, type aliases (the review's Ticket 1), impls, and
opaque types.

**3. Errors appearing after fixes.** Borrow checking of a body (`mir_borrowck`) depends on that body's `typeck`. If
`typeck` is tainted by errors, the borrow checker isn't run for that body, because MIR built from ill-typed code would
produce nonsense. Fixing the type error lets borrowck run, and the three latent borrow errors appear. The privacy pass
and most lints run after a crate-wide gate ("did anything fail?"), so they appear only when every body type-checks and
borrow-checks (Experiment 3).

**4. `DefId` vs `DefPathHash`.** A `DefId` is (crate number, index) and is only meaningful within one compiler session:
insert an item and indices shift. A `DefPathHash` is a stable hash of the item's path (`ledger::accounts::balance::helper`)
that survives edits elsewhere. Incremental compilation compares results *across* sessions, so it keys dependency-graph
nodes and fingerprints on stable identities.

**5. Red-green and early cutoff.** At the start of a session, the compiler tries to mark each previous query result
green: all of its recorded dependencies are unchanged (inputs compared by fingerprint, derived queries marked
recursively). Green results are loaded from disk without running the provider. A red query is recomputed; if its new
fingerprint equals the old one, its dependents can still be green: early cutoff. Editing a body changes that body's HIR
and `typeck`, but the signature query's result is unchanged, so callers that read only the signature stay green.

**6. Back-end reuse.** The back end is incremental per codegen unit: if every mono item in a CGU is unchanged, its object
file from the previous session is reused and LLVM doesn't run for it. That's why incremental builds default to many
small CGUs (256): a change invalidates fewer items' worth of LLVM work.

**7. `cargo check`.** It runs the front end and analysis (resolution, type checking, borrow checking) and writes only
metadata, skipping monomorphization, LLVM, and linking. It can never report post-monomorphization errors (a generic
`const` assertion that fails only for a particular instantiation, Chapter 7.1) or link errors.

**8. One shared `target/` cache.** Don't share incremental state: it's per machine and per configuration, it grows without
bound across branches and feature sets, cross-branch hits are mostly misses, and it has had correctness bugs (the 1.52.1
episode). Cache what's stable and keyed well: `~/.cargo/registry` and compiled *dependencies*, keyed on `Cargo.lock`,
the toolchain version, and the target, with `CARGO_INCREMENTAL=0` in CI. Keep full release builds in a separate job.

### Debugging exercise (whack-a-mole)

1. **Run 1:** E0433 is a resolution error; E0308 is a type error in `settle`. `settle`'s `typeck` was tainted, so its
   borrow errors weren't looked for. The function that later showed E0382 is almost certainly the one containing
   `Invoce`: an unresolved type becomes an error type, which taints that body's type check, which suppresses *its*
   borrow checking too. **Run 2:** both bodies now type-check, so both are borrow-checked: two E0502s in `settle`, one
   E0382 elsewhere. **Run 3:** with no errors left in any body, the driver passes the gate, and the privacy pass (E0451)
   and late lints (`dead_code`) run.
2. None of run 2's errors could appear in run 1: each was in a body whose type check had failed. A borrow error in a
   third, clean body would have appeared in run 1 (Experiment 1).
3. Predict by phase: after resolution and type errors are gone, expect borrow errors in the same bodies; after those,
   expect privacy errors and lints. The habit: run `cargo check` (or watch rust-analyzer) continuously while editing,
   so each phase's errors surface while the change is small, instead of pushing to CI to find out.

### Selected exercises

- **Beginner.** (a) One error: E0308 (Experiment 2). (b) Two: E0308 in the new function and E0502 in the original,
  because they're now different bodies (Experiment 1). (c) Still those two. The E0451 is hidden until both are fixed
  (Experiment 3).
- **Advanced.** Modules, `impl` blocks, functions, and closures each get their own `DefId` [RUSTC]; statements and
  expressions don't (they're nodes inside their owner's HIR). Incremental reuse follows that granularity: results are
  keyed per item or body, so editing one function's body doesn't invalidate a sibling function's `typeck`.

---

## Chapter 18.2 — Macro Expansion, Name Resolution, and HIR Lowering

### Interview & architecture questions

**1. Between parser and type checker.** Parse to an AST with unexpanded macro calls; then iterate: collect invocations,
resolve their macro paths, expand the resolvable ones, parse and splice their output, update imports, repeat until
nothing changes; then resolve every path in every body (late resolution) and lower to HIR, desugaring control flow and
making elided lifetimes explicit. They're interleaved because they depend on each other: resolving a macro's path can
depend on items or imports that other macros generate.

**2. Mixed-site hygiene.** Local variables and labels in a `macro_rules!` body resolve at the macro's definition site;
everything else (functions, types, modules, other macros) at the call site; `$crate` names the defining crate. So a
macro can't see the caller's `request_id` (E0425 with the "macro hygiene" note), but a bare `mask(x)` in its body means
whatever `mask` the caller has in scope (the audit incident).

**3. Expanded output isn't source.** The printer drops syntax contexts. `scaled!(t + 1)` prints as
`{ let t = 2; t * (t + 1) }`, which computes 6 if you compile it, while the real program computes 22: the two `t`s were
different identifiers.

**4. Desugarings.** `for x in e { body }` → `match IntoIterator::into_iter(e) { mut iter => loop { match
Iterator::next(&mut iter) { None => break, Some(x) => body } } }`. `e?` → `match Try::branch(e) { Break(r) => return
FromResidual::from_residual(r), Continue(v) => v }`. `f.await` → `match IntoFuture::into_future(f) { mut a => loop {
match Future::poll(Pin::new_unchecked(&mut a), cx) { Ready(r) => break r, Pending => {} } cx = yield (); } }`. Each
targets a trait, so any type implementing `IntoIterator`, `Try` (unstable to implement, so in practice `Result`,
`Option`, `ControlFlow`, `Poll`), or `IntoFuture` gets the syntax.

**5. Lang items.** Std items marked `#[lang = "..."]` that the compiler knows by identity (`into_iter`, `next`, `branch`,
`Sized`, `Drop`, `Box`'s allocator hooks). Desugarings call them directly, so a user's `fn next` or `mod iter` can't
change what a `for` loop means, and the compiler doesn't have to resolve a path that might be shadowed.

**6. Format strings.** `format_args!` is a compiler built-in: the string is parsed at compile time, placeholders are
checked against the arguments and their traits (`Display`, `Debug`), and the HIR contains a pre-encoded template. Java's
`String.format` parses the string at run time and throws `IllegalFormatException` then.

**7. Proc macros on the critical path.** A proc-macro crate must be fully compiled *and linked* into a host dynamic
library before any crate using it can expand, and it's built for the host (twice when cross-compiling shared
dependencies). Pipelining only helps with dependencies whose metadata suffices; a proc macro's code must actually run.

**8. What derives can't see.** An annotation processor sees a typed element model (it can ask whether a field's type
implements an interface). A proc macro sees only tokens, because expansion happens before type checking. Derives work
around it by emitting impls with bounds (`where T: Serialize`) and letting the trait solver prove or reject them later.

### Debugging exercise (`timed!`)

1. `Instant` and `record` are items, so a `macro_rules!` body resolves them at the call site. In the defining crate, the
   call sites happened to have `use std::time::Instant;` and `record` in scope. In another crate, they don't.
2. Use absolute paths (verified in listing `answers-ch02-macros.rs`, where the calling module deliberately defines its
   own `Instant`, `record`, and `sleep`):

   ```rust,ignore
   #[macro_export]
   macro_rules! timed {
       ($name:expr, $body:block) => {{
           let start = ::std::time::Instant::now();
           let out = $body;
           $crate::metrics::record($name, start.elapsed());
           out
       }};
   }
   ```

   `record` must be reachable as `$crate::metrics::record` (public, possibly `#[doc(hidden)]`).
3. Making callers import names makes the macro's meaning depend on each caller's scope. A caller with its own `record`
   (or a future import that brings one in) silently changes what the macro does, which is exactly the audit-macro
   incident: it compiles and does the wrong thing. It also makes the macro's dependencies part of every caller's code.

### Selected exercises

- **Beginner.** Both survive lowering nearly unchanged: HIR has `let` *expressions* (the form `if let` and let chains
  use, as in `ch02-02`'s output) and `let` statements with an optional `else` block. Check with the Playground's HIR.
- **Intermediate.** The `retry!` macro in listing `answers-ch02-macros.rs` (verified): its `attempt` and `result` are
  macro-local by hygiene, so the caller's variables of the same names are untouched, and `::std::thread::sleep` can't
  be hijacked by the caller's `sleep`. The run prints `v=42 r=Ok(42) calls=2 (caller's attempt, caller's result)`.
- **Advanced.** `IntoFuture` lets builder types be awaited directly (`client.get(url).await` without a `.send()`),
  because the desugaring calls `into_future` first; every `Future` implements `IntoFuture` trivially.

---

## Chapter 18.3 — Type Checking and Trait Solving

### Interview & architecture questions

**1. `typeck`.** For one body: the type of every expression and pattern, every adjustment (autoref, deref, coercions),
which method each call resolved to, closure captures and kinds, and the hidden types of opaque types it defines. It
doesn't solve lifetimes (regions are recorded and left to borrowck) and never infers item signatures. The results
(`TypeckResults`) feed THIR and MIR building.

**2. Ambiguous.** "Can't decide yet with the types known so far." With `T` still an inference variable, `_: Clone` might
be true or false, so the obligation is parked and retried as inference makes progress. It's an error only if it's still
ambiguous after fallback at the end of the body (E0282/E0283).

**3. Reading an E0277 chain.** The solver built it top-down (goal → impl candidate → nested obligation → …) and the
message prints the failing leaf first, with notes back toward your code. Read the notes from the bottom (your call site)
up to the leaf. The actionable line is usually the first `help:` that points to an impl or type you own.

**4. E0275.** A proof whose sub-goals grow instead of shrinking (a where-clause about a bigger type than the impl's
self type) never terminates, so the solver stops at the recursion limit. Raising the limit makes the same failure
slower, or turns it into huge types and slow builds. The fix is a where-clause that reduces toward known goals.

**5. `x.borrow()` on `Rc<RefCell<T>>`.** Autoderef steps: `Rc<RefCell<T>>`, `RefCell<T>`, `T`, …; at each step try by
value, `&`, `&mut`; inherent before trait methods *at the same step*. Without the import: step 0 has no `borrow`, step 1
finds `RefCell::borrow`. With `use std::borrow::Borrow`: step 0 already finds `Borrow::borrow` (via `&Rc<…>`), and since
`Rc<T>` implements both `Borrow<Rc<T>>` and `Borrow<T>`, the result type is ambiguous: E0282.

**6. Never-type fallback.** A diverging inference variable (one only constrained by expressions of type `!`, such as
`?`'s `return` arm) falls back to `()` before edition 2024 and to `!` in edition 2024. Code whose meaning depended on
the old fallback would change meaning silently, so 1.98.1 rejects it in all editions (a deny-by-default lint in 2021)
and asks for an annotation.

**7. Local inference.** Signatures are the contracts between bodies. That locality gives: per-body type checking (errors
stay local), incremental early cutoff (a body edit doesn't re-check callers), and semver stability (a function's type
can't change because its body changed). The exception is auto-trait leakage through `impl Trait` returns.

**8. javac vs rustc.** javac looks up declared supertypes: whether `Invoice implements Comparable<Invoice>` is written on
the class. rustc *searches* for a proof: impls (including blanket and conditional ones), where-clauses in scope,
built-ins, and auto-trait rules, possibly with nested goals, which is why it can report a derivation or an overflow.

### Debugging exercise (`clone` on `&&Quote`)

1. `q: &&Quote`. Probe at step 0 (`&&Quote`) by value: `Clone::clone` takes `&self`, so the receiver `&&Quote` matches
   `&Self` with `Self = &Quote`, and `&Quote: Clone` holds (shared references are `Copy`). So it calls
   `<&Quote as Clone>::clone` and returns `&Quote`, before ever considering `Quote`.
2. `clone` succeeded; nothing is wrong at that expression. The mismatch appears only when `collect` requires
   `Vec<Quote>: FromIterator<&Quote>`, which fails, so the error is reported there, with the note tracing the item type.
3. Without changing `Quote`: build it explicitly (`Quote { symbol: q.symbol.clone(), px: q.px }`). Changing `Quote`: add
   `#[derive(Clone)]` and write `book.iter().map(|q| (*q).clone())` or `book.iter().copied().cloned()` (note that
   `q.clone()` would *still* clone the reference). On a hot path, question the snapshot itself: returning `&Quote`s,
   `Arc<Quote>`, or interned symbols avoids cloning a `String` per quote.
4. With `-> Vec<&'a Quote>` (it needs a named lifetime; elision can't choose between the slice's lifetime and the
   quotes'), it compiles, and the warn-by-default lint `suspicious_double_ref_op` reports "using `.clone()` on a double
   reference, which returns `&Quote` instead of cloning the inner type" (listing `answers-ch03-double-ref-clone.rs`,
   verified by denying the lint). It wasn't reported in the failing version because lints run after the crate-wide
   error gate (Chapter 18.1's table).

### Selected exercises

- **Beginner.** Without the turbofish, both bounds evaluate to `EvaluatedToAmbig` at the call site, `T: Sized` and
  `T: Clone` (listing `answers-ch03-no-turbofish.rs`, verified on nightly), and the program compiles once the argument
  fixes `T = String`.
- **Advanced.** `impl<T: Wire> Wire for Vec<T>`. Proving `Vec<u64>: Wire` now needs `u64: Wire`, a smaller type, which is
  an impl: the proof terminates.
- **Architecture.** A blanket `impl<T: Serialize> Auditable for T` makes `Auditable` impossible to implement manually for
  any serializable type (coherence), makes adding it later a breaking change for downstream impls (6.2), adds a
  candidate to every `Auditable` goal, and couples auditing to serialization format. Prefer a derive or an opt-in marker
  (`impl Auditable for Payment {}` with a default method using `Serialize`), which keeps the convenience with explicit
  opt-in.

---

## Chapter 18.4 — THIR and MIR

### Interview & architecture questions

**1. THIR vs HIR vs MIR.** THIR adds type-checking's results made explicit: types on every node, adjustments as explicit
deref/borrow nodes, method calls and overloaded operators as calls to specific functions. MIR loses the tree structure
(nesting, which expressions are inside which blocks) in exchange for explicit control flow. Checks: exhaustiveness and
unsafety on THIR; borrow checking, move checking, and drop elaboration on MIR.

**2. `call` terminators.** `bb9` is the unwind successor: a `(cleanup)` block that runs only while a panic unwinds
through this call. It drops the locals that are live at that point and ends with `resume` (continue unwinding), or
`unwind terminate` if a drop itself panics during cleanup.

**3. `let _` vs `let _l`.** `_` doesn't bind, so the initializer is a temporary in the statement's scope, dropped at the
`;`. `_l` is a binding whose scope is the enclosing block. In MIR, the first has `drop(_2)` as the terminator right after
the call, with no `debug` name; the second has `debug _l => _2` and the drop at the end of the block, plus a cleanup
block.

**4. Drop elaboration.** Using initialization dataflow: definitely initialized → a plain `drop`; definitely moved →
the drop is removed; maybe initialized (moved on some paths) → a drop flag set and tested at run time. Partially moved
aggregates get field-by-field drops. Only the maybe case costs a flag, and release builds often optimize it away.

**5. The Playground's MIR.** It's `optimized_mir`: after borrow checking, drop elaboration, and optimization (reborrows
simplified, calls inlined in release). To see what the borrow checker saw, dump MIR at the borrowck stage locally with
`-Z dump-mir=<fn>` (the output's own `HINT:` line) on a nightly toolchain.

**6. Const evaluation.** An interpreter (`rustc_const_eval`) executing `mir_for_ctfe` bodies on abstract memory with
per-byte initialization and provenance. It can run loops, calls to `const fn`s, indexing, and assertions; it can't do
I/O, call non-const functions, or (on stable) allocate on the heap; UB and invalid final values are compile errors. A
runaway evaluation trips the deny-by-default `long_running_const_eval` lint (listing `ch04-10`).

**7. Bounds checks in a hot loop.** Release assembly (or release LLVM IR). MIR shows the check as the source demands it
(an `assert` terminator); whether LLVM proved it redundant and removed it is only visible after LLVM runs.

**8. MIR vs bytecode.** Similar: both are typed, per-function, lower-level than source, produced by the language's
compiler. Different: bytecode is a stable distribution format verified by the JVM at load time and optimized at run time;
MIR is internal, unstable, unverified at run time, and optimized at build time. And cleanup: exception tables and
duplicated `finally` code versus scheduled drops and explicit cleanup blocks.

### Debugging exercise (lock held through a `match`)

1. `_4: MutexGuard<'_, State>` is the guard; it's dropped in `bb7`, after both arms. Temporaries in a `match` scrutinee
   live until the end of the `match` expression [LANG], because the arms may borrow from them (the scrutinee is a place
   the patterns inspect). `bb3` only reads the field; the guard stays alive.
2. With `lock()` in `bump`, the thread tries to lock a mutex it already holds. std's `Mutex` isn't reentrant: its
   documentation leaves the behavior unspecified except that the call won't return normally (it may deadlock or panic).
   In production, the worker hung. It was intermittent because only `status == 0` (the retry path) calls `bump`.
3. No. In edition 2024, an `if let` scrutinee's temporaries are dropped before the `else` block, not before the `then`
   block, and `bump` is in the `then` block. Listing `answers-ch04-if-let-guard.rs` (verified) prints "bump: lock is
   still held" for the `if let` version.
4. Copy the field out in its own statement (`let status = m.lock().unwrap().status;`, the fixed function in the same
   listing, which prints `retries = 1`), or pass the guard into the helper (`bump(&mut guard)`) so there's one lock
   acquisition by design. Review rule: never call anything that might lock while a scrutinee's guard is alive; read
   what you need into a local first. (Clippy has a lint for this pattern, `significant_drop_in_scrutinee`; check its
   group and status in your version, not verified here.)

### Selected exercises

- **Beginner.** The same shape: `discriminant`, `switchInt`, a downcast projection `(_1 as Some).0`. Recent rustc keeps
  `if let` as its own HIR form, but the MIR is essentially identical.
- **Systems.** The new arm doesn't move its payload, so on that path the `Vec<u8>` is still inside `_1` and must be dropped:
  drop elaboration adds a drop of `_1` (or of its `Blob` field) on that arm only. No flag is needed, because each arm
  statically knows which variant it's in.

---

## Chapter 18.5 — Borrow Checking on MIR

### Interview & architecture questions

**1. Steps.** Renumber regions; MIR type check (outlives constraints); liveness (regions contain points where values
are live, including drop-live); region inference (sets of points; universal-region checks); dataflow (loans in scope,
initialization). E0502 comes from the loans-in-scope check, E0382 from the initialization dataflow, and "lifetime may
not live long enough" from region inference checking a universal region.

**2. Before optimization.** So that acceptance depends only on language rules, never on the optimizer's power (which
varies by version and profile). Observable consequence: two conflicting `&mut` borrows inside `if false` are still E0499.

**3. Two-phase borrows.** Created for autoref'd method receivers, implicit reborrows of `&mut` arguments, and
overloaded compound assignment. The borrow is reserved (acts shared) until activated at the call. `v.push(v.len())`
autorefs `&mut v` as two-phase, so the argument can read `v`; `Vec::push(&mut v, v.len())` writes the borrow
explicitly: an ordinary `&mut`, live from creation, conflicting with `v.len()`. So does `(&mut v).push(v.len())`
(listing `answers-ch05-receiver-forms.rs`, verified E0502).

**4. Drop-live.** A value whose type has drop glue is live at its `drop` terminator, because `Drop::drop` gets `&mut
self` and might read anything the value holds, including borrowed data. So the loans inside it must survive to the
drop. An empty `Drop` impl still creates that use: the checker looks at the signature, not the body.

**5. Universal vs existential.** Universal regions come from the signature (named lifetimes, anonymous reference
lifetimes, `'static`): chosen by the caller, the body can't shrink or grow them. Existential regions are inside the body
and inferred. The `'1` in "let's call the lifetime of this reference `'1`" is universal.

**6. Closures.** Each closure is its own MIR body, borrow-checked first. When its body needs a relation between regions
that belong to the creator ("the strings outlive the vector's elements"), it can't prove it and propagates it as an
external requirement, which the creator must then prove from its own constraints (`where '?1: '?3` in the dump).

**7. Problem case #3.** NLL's outlives constraints are location-insensitive: the loan from `get` must outlive the
universal `'1` because of the `return v` path, so it's in scope everywhere after, including the fall-through path that
reaches `insert`. Polonius tracks which loans flow into which live origins at each point: on the fall-through path the
loan flows into nothing live, so there's no conflict. "Accepted on nightly" means nothing for code pinned to stable:
keep the NLL-compatible form until your pinned compiler accepts the simpler one.

**8. Comparisons.** Java's definite assignment (JLS 16) is the same dataflow family as Rust's initialization check (E0381,
E0382 for moves), and it rejects programs. HotSpot's escape analysis reasons about object lifetimes at run time,
speculatively, to optimize (scalar replacement, lock elision), and never rejects anything.

### Debugging exercise (loop back edge)

1. The loan is the shared borrow of `buf` taken by `buf.split(' ')` (through deref to `str`). Its reference flows into
   `method`, then into `methods: Vec<&str>`. `methods` is live at every point in the loop and after it (the `println!`),
   so by liveness (step 3) and the outlives constraint from the push (step 2), the loan's region contains every point in
   the loop, including `buf.clear()` in the next iteration via the back edge.
2. The conflict is reported at the access that violates the loan (`clear` needs `&mut buf` while a shared loan is in
   scope), with the creation and the later use as labels. The push itself is fine.
3. `to_string()` per method (one allocation per line: ten million allocations); borrow from the source instead of a
   reused buffer (`lines.iter().map(|l| l.split(' ').next().unwrap())`, zero allocations, if the input stays alive, for
   example a file read or mapped in full); or store a value that borrows nothing (an enum, or an interned ID). All three
   are verified in listing `answers-ch05-backedge-fixes.rs`. For ten million lines: the enum or an interner, since the
   number of distinct methods is tiny.
4. No. The program is genuinely wrong: `methods` would hold references into a buffer that `clear` and `push_str`
   overwrite. Polonius removes false positives; this is a true positive.

### Selected exercises

- **Intermediate.** Method syntax compiles; `Type::method(&mut x, x.len())` and `(&mut x).method(x.len())` are E0502. In
  the third, the explicit `&mut x` is an ordinary borrow, and the method call merely reborrows it.
- **Advanced.** Verified with a counting hasher (listing `answers-ch05-pc3-rewrites.rs`): `contains_key` then `insert`
  then `get` costs 2 hashes on a hit and 3 on a miss; the entry API costs 1 on both. The `contains_key` version is
  accepted because the only borrow it returns is created after the last mutation; the entry version because the entry
  owns the `&mut` borrow and hands out a reference derived from it.

---

## Chapter 18.6 — Monomorphization and Codegen Backends

### Interview & architecture questions

**1. Collector.** Roots: in a binary, `main` (through `lang_start`) plus `#[no_mangle]`/`#[used]` items; in a library,
the non-generic items other crates could call. Edges: calls with concrete generic arguments, unsizing casts to `dyn`
(the vtable and all its methods), drops (drop glue), statics, and generic constants (evaluated per instance). A `dyn`
cast pulls in every method because the vtable is a table of function pointers that must all exist; which slot will be
called is only known at run time.

**2. `noalias`.** The borrow checker proves exclusivity for `&mut` and immutability for `&T` without `UnsafeCell`
(language rules); `fn_abi_of` records `NoAlias`/`ReadOnly`/`NonNull`/pointee size and alignment in the argument
attributes (rustc); `rustc_codegen_llvm` emits them as LLVM attributes; LLVM's optimizer uses them (for example, one load
of `*src` instead of two).

**3. Debug IR.** rustc computes a smaller attribute set in debug builds (the dump shows `NonNull | NoUndef`) and the debug
IR carries only alignment (observed on 1.98.1): the attributes exist for an optimizer that isn't running. Benchmarks of
debug builds therefore measure code without these facts, besides being unoptimized in general: never draw performance
conclusions from them.

**4. The bracketed hash.** The crate disambiguator (a stable crate ID from the crate's name and metadata such as its
version). It keeps symbols from two versions of the same crate distinct, so both can be linked into one binary.

**5. CGUs.** Codegen units are the independent LLVM modules a crate is split into. More CGUs: more parallel LLVM work and
finer incremental reuse, at the cost of fewer cross-function optimizations (inlining across CGUs needs duplication or
LTO) and sometimes larger code. Release builds that need peak performance use `codegen-units = 1` or LTO.

**6. Function merging.** LLVM's MergeFunctions pass replaces functions with identical code by one body plus aliases.
rustc marks functions `unnamed_addr` because Rust doesn't guarantee distinct or stable function addresses, which permits
it. In debug, no merging: `fn_addr_eq` is false; in release, merged: true (and may even be constant-folded, as the
review shows).

**7. Other backends.** They share everything up to and including `rustc_codegen_ssa`'s translation of MIR (collector,
layouts, ABIs, symbols). Cranelift: faster compilation for debug builds, weaker optimization, nightly component. GCC:
reaching targets LLVM doesn't support. Both [VERSION]: check status before depending on them.

**8. Collector vs Native Image.** Both compute reachability from entry points in a closed world and compile only what's
reachable. rustc's analysis is exact for generics (all instantiations are visible in MIR) and conservative only for
vtables; Native Image must approximate reflection and dynamic loading and needs configuration for them.

### Debugging exercise (`Reply` over shared memory)

1. Rust's default layout: tag `u8` at offset 0 (values 0..=1), `Ok`'s `u8` at offset 1, `Value`'s `u32` at offset 4;
   bytes 1–3 are padding in `Value`, bytes 2–7 in `Ok`. The C++ reader takes a 4-byte tag from bytes 0–3, so the tag
   includes `Ok`'s payload at byte 1 (42 = 0x2A becomes a tag of 0x2A00) and whatever the padding bytes contain.
   `Value` decodes correctly only when bytes 1–3 happen to be zero (for example, in a freshly zeroed ring slot); its
   payload offset, 4, happens to match. `Ok`'s payload is read from offset 4, which is padding. There's also a Rust-side
   problem: copying an enum's padding bytes into the ring reads uninitialized memory (Chapter 5.2's `bytemuck` lesson).
2. The default representation is `[RUSTC]`: field order, tag size, and offsets may change between compiler versions,
   and the padding is uninitialized regardless.
3. `#[repr(C, u32)]` (RFC 2195) gives a defined layout: a `u32` tag at offset 0 and each payload at offset 4, 8 bytes
   total (listing `answers-ch06-repr-reply.rs`, verified with `rustc_dump_layout` on nightly). You'd still zero the
   padding of the `Ok` variant before copying. Or write an explicit encoder (`fn encode(&self, out: &mut [u8; 8])` with a
   `u32` tag and payload in a declared byte order). Chapter 5.2's rule favors explicit encoding for anything persisted
   or crossing machines. `repr(C, u32)` is justified for same-machine, same-architecture shared memory where both sides
   compile the same declaration.

### Selected exercises

- **Beginner.** The supertrait's methods come first: `[MetadataDropInPlace, MetadataSize, MetadataAlign,
  Method(<Circle as Debug>::fmt), Method(<Circle as Shape>::area)]` (listing `answers-ch06-supertrait-vtable.rs`,
  verified on nightly). A prefix laid out like the supertrait's own vtable is what makes upcasting to the first
  supertrait cheap (Chapter 6.4).
- **Advanced.** With `Wallet::BPS = 250`, the two instances differ (`imul rax, rdi, 250`), so there's nothing to merge:
  release prints `same address? false`. Identically laid-out types in generic helpers are the typical source of real
  merges (Chapter 7.3's sort helpers).

---

## Chapter 18.7 — Tracing `let x = foo();` Through the Compiler

### Interview & architecture questions

**1. The trace.** Tokens; expansion (prelude injected); HIR (elided lifetimes explicit, attributes parsed); type check
(`x: String`, a deref-coercion adjustment at `log(&x)`); THIR (the adjustment as an explicit deref); MIR (local `_1`,
drops and cleanup blocks, loans checked, regions erased); optimized MIR (inlined field reads in release); mono items
(the three functions plus std instances); LLVM IR (`alloca [24 x i8]`, `sret`, `invoke`/`landingpad` in debug); assembly
(a 24-byte stack area, two calls, a conditional free).

**2. The deref coercion.** Created as an adjustment by type checking (a `&String` where `&str` is expected), written out in
THIR, visible in debug MIR as `<String as Deref>::deref(copy _4)`, and gone in release: inlined into two field reads and
type-only `Transmute`s, and in the asm two register moves.

**3. Cleanup vs no landing pad.** Debug MIR is built for possible unwinding from every call made while `x` is alive. In
release, the only such calls are `log` and the deallocator, and LLVM marked both `nounwind` (`log` only runs inline
assembly), so the calls became plain calls and the landing pad was deleted.

**4. `sret`.** The `Indirect` pass mode chosen by `fn_abi_of` for return values too large for registers: the caller
allocates the space and passes its address as a hidden first argument. IR: `call @foo(ptr sret([24 x i8]) %x)`; asm:
`mov rdi, rsp` before `call foo`, and `foo` writes through `rbx` (the saved `rdi`).

**5. 18 vs 3 functions.** The collector reached drop glue, `String::from` and its `to_vec`, `black_box::<&str>`, local
copies of `#[inline]` std functions (`String::len`, `Deref::deref`), and debug-only precondition checks. In release, all
were inlined into `foo`, `log`, and `caller`, and the UB checks don't exist.

**6. `range(i64 0, -9223372036854775808)`.** The return value is in `[0, i64::MIN)` as an unsigned-wrapping range, i.e.
`0 ..= isize::MAX`. It comes from `Vec::len`'s `assume(len <= isize::MAX)` in MIR, carried to the function's return
attribute, so callers of `caller` can use it too.

**7. Artifacts.** "When is this lock released?": debug MIR. "Does this loop allocate?": release asm (look for
`__rust_alloc`) or a counting-allocator test. "Is this call devirtualized?": release asm (a direct call or inlined code
instead of `call qword ptr [reg + offset]`).

**8. Lifecycles.** Rust: 24 bytes in the caller's frame, filled by `foo` through `sret`; an 8-byte heap block from
`__rust_alloc`; freed by an inlined conditional `__rust_dealloc` at the end of `caller`; frame reclaimed by `add rsp, 24`.
Java: a `String` object with a header (and its byte array) on the heap; a reference in a local slot; nothing at the end of
the method; reclaimed by the GC at some later collection, or never allocated if escape analysis scalar-replaced it.

### Debugging exercise (does `&x` copy?)

1. Debug MIR: `_3 = <String as Deref>::deref(copy _4)`, a call that returns a `&str` pointing into `x`'s existing buffer.
   Release MIR: two field reads (pointer and length) and two `Transmute`s that change only types. Release asm:
   `mov r14, qword ptr [rsp + 8]; mov rbx, qword ptr [rsp + 16]; mov rdi, r14; mov rsi, rbx`. There's no `__rust_alloc`
   in `caller`: nothing is copied.
2. `x.clone()` into a `log_owned(String)` costs an allocation, a `memcpy` of the bytes, and a second free: a
   `__rust_alloc` (or an inlined clone that calls it) plus a `memcpy`, and two `__rust_dealloc` calls in the release asm,
   one for the clone inside `log_owned` or its drop glue and one for `x`.
3. An owned parameter is right when the callee must keep the data after returning (store it, send it to another thread
   or task, which needs `'static` data) or when the caller is done with it (then move `x`, don't clone). Reply: "`&str`
   is the idiomatic parameter here (Chapter 3.3); the coercion is two register moves in release (asm attached). If
   `log` ever needs to keep the string, we'll change it to take `String` and move `x` in."

### Selected exercises

- **Beginner.** `log(x.as_str())` produces release asm identical to `log(&x)` (listing `answers-ch07-variants.rs`,
  `caller_as_str`); in debug MIR the call is `String::as_str` instead of `Deref::deref`.
- **Intermediate.** With `Box<str>` (same listing, `caller_boxed`), `x` is two words returned in `rax:rdx` (a
  `ScalarPair`, no `sret`, no stack slot). LLVM even propagated the constant length 8 from `foo_boxed` into the caller
  (`mov esi, 8`), and the free is unconditional, because the length is known to be non-zero.

---

## Part XVIII Review — Capstone: the compiler detective

### Part A

1. `ensure!` was expanded (18.2) into `if !acct.open { return Err(LedgerError::Closed); };`. The `$cond:expr` fragment
   stays one unit, so `!$cond` with `next >= 0` must mean `!(next >= 0)`, and the printer adds the parentheses: the same
   reason `scaled!(t + 1)` printed as `t * (t + 1)`.
2. `match branch(…) { Break(residual) => return from_residual(residual), Continue(val) => val }`: `Try::branch` and
   `FromResidual::from_residual` (lang items). For `Result<i64, LedgerError>` from a `Result<_, LedgerError>`,
   `from_residual` returns `Err(From::from(e))`, and `From` is the identity here.
3. `cmp byte ptr [rsi + 16], 0` / `je` is `ensure!(acct.open)` (the `Closed` exit writes tag 2). `add rdx, qword ptr
   [rsi + 8]` is `balance.checked_add(delta)`, and `jo` is its overflow test; `ok_or(LedgerError::Overflow)?` compiled
   into that branch, whose target writes tag 0. `mov [rsp + 8], rdx` keeps `next` for the formatter. `js` is `next >= 0`
   (sign bit), whose exit writes tag 1 plus the payload. Then `r13` saves the `sret` pointer and `r12` saves `next`.
4. `Account`: `balance` at 8, `open` at 16 (so `id` at 0; its address, `rsi`, is passed to the formatter as `&acct.id`).
   `Journal`: the `&mut dyn Sink` fat pointer at 0 and 8 (data pointer, then vtable), `entries` at 16. None of these
   offsets is guaranteed: default representation [RUSTC].
5. `rbp` holds the vtable pointer, loaded from `[rcx + 8]` (the fat pointer's second word). Offset 24 is the fourth
   8-byte slot, after drop, size, and align: `Method(<Stdout as Sink>::write)` in Artifact 3's entry list.
6. The 16-byte `Result<i64, LedgerError>`: word 0 is a tag, with 0 = `Err(Overflow)`, 1 = `Err(Negative)` (payload in
   word 1), 2 = `Err(Closed)`, and −1 (all ones) = `Ok` (value in word 1). The `Result`'s discriminant lives in the niche
   of `LedgerError`'s tag word: values outside 0..=2 are free, and `Ok` takes one. None of it is a promise; only specific
   cases like `Option<&T>` are guaranteed [LANG].
7. `format!` expands to `alloc::fmt::format(format_args!(…))`, which calls `format_inner` to build a `String` (a heap
   allocation inside). The temporary `String` is dropped after `write` returns: `test rbx, rbx` on its capacity and a
   conditional `__rust_dealloc`. With a reused buffer (`buf.clear(); write!(buf, …)`, Chapter 9.2's access-log fix),
   there'd be no allocation per posting after warm-up.
8. `NoAlias` applies to pointers: `&mut Account` and `&mut Journal` are exclusive for the call. An `i64` is a value, so
   only `NoUndef` (it must be initialized) applies. The unwinding call is `call qword ptr [rbp + 24]`: `Sink::write` can
   panic (`println!` can). While it runs, the formatted `String` is alive, so the landing pad frees it (if its capacity is
   non-zero) and ends with `call _Unwind_Resume` to continue unwinding.

### Part B

**Ticket 1.** The query system's cycle detection (18.1): expanding the alias `Config` requires expanding `Config`. A type
alias is only a name for its expansion, so the expansion never ends. `struct Config(HashMap<String, Config>)` is a new
nominal type that refers to itself by name; its size is finite because the map's entries live on the heap. Run-time
cost: none beyond the `HashMap` it wraps (a newtype, Chapter 5.3).

**Ticket 2.** Without the braces, `j` lives to the end of `main`. `Journal` implements `Drop`, so `j` is drop-live at the
end, and the `&mut out` loan inside it must survive until then (18.5, step 3). `out.write("done")` needs a second
`&mut out` while that loan is in scope: E0499. The braces ended `j`'s scope, and its drop, before the call. Without a
`Drop` impl, the loan would end at `j`'s last use and the program would compile, but there'd be no "journal closed" line
at all. Fixes that keep the output order: restore the braces, or call `drop(j);` explicitly before `out.write("done")`.

**Ticket 3.** `to_major::<Usd>` compiles to the same code as `to_major::<Eur>` (both divide by 100), and LLVM merged them
in release (18.6). The comparison didn't even survive: `mov byte ptr [rsp + 6], 1` stores a constant `true`, because
after merging, both pointers are the same symbol. The compiler is right and the design is fine: the **test** asserts
something the language doesn't promise. Test what you care about: `assert_eq!(eur(12_345), 123)`, and if the point is
that EUR and USD are configured separately, assert on the configuration (`Eur::MINOR`, `Usd::MINOR`) or a currency code,
never on function addresses.

---

## Part XVIII Review — Interview mode

**1. Guarantees vs details.** Guaranteed: drop order (Reference), that `&mut` is exclusive (which is what licenses
`noalias`, though the attribute itself is rustc's choice), never-type fallback per edition (edition guide). Details
[RUSTC]: default-repr layout, the `noalias` attribute and when it's emitted, two-phase borrow creation sites (the
*accepted programs* are stable; the mechanism isn't), and function addresses (explicitly *not* unique or stable).

**2. Hygiene.** Guaranteed: macro-local variables and labels can't collide with the caller's, and `$crate` always names the
defining crate. Not guaranteed: item names in the body resolve at the call site. Robust macros take everything
caller-specific as parameters and write every item path as `$crate::…` or `::std::…`, tested from a module with colliding
names.

**3. No inferred signatures.** See Chapter 18.3, question 7: local checking, incremental early cutoff, semver. If
signatures were inferred, a body edit could change a public type and break downstream crates, errors would surface far
from their cause, and every caller would need re-checking after any change.

**4. Queries for a javac engineer.** javac runs phases over all classes; rustc answers questions about items on demand
and caches them with their dependencies. Errors are results, so a failed query quietly stops its dependents (errors
hide each other by dependency, not by phase). Recorded dependencies let the next build reuse any result whose inputs
didn't change, down to per-CGU object files: incremental compilation inside the compiler, where Java does it in the
build tool.

**5. Life of a function.** HIR (resolved, desugared; well-formedness) → typeck (types, adjustments, method resolution,
trait obligations) → THIR (explicit tree; exhaustiveness, unsafety) → MIR (CFG; borrowck, drop elaboration, MIR
optimizations, const eval) → mono items (collector; instances, vtables, drop glue) → LLVM IR (layouts, ABIs, symbols,
attributes) → LLVM passes → object file.

**6. Borrow checking.** Loans with regions as sets of points, computed from outlives constraints (subtyping, calls,
returns) and liveness (including drop-liveness), then a dataflow of loans in scope checked against each access. Runs on
unoptimized MIR so acceptance doesn't depend on the optimizer. Polonius tracks origins as sets of loans and asks which
loans flow into live origins at each point, removing false positives like problem case #3.

**7. Collector and `dyn`.** Reachability from roots over MIR, creating instances, vtables, drop glue, and evaluating
generic constants. A `dyn` cast materializes a vtable whose every slot must point to real code, so every method of the
trait is compiled for that type, whether or not it's ever called.

**8. Where compile time goes.** Trait-heavy crates spend it in the front end (type checking and trait evaluation, serial);
generic-heavy crates in codegen (many instances, parallel LLVM work). Tools: `cargo build --timings` for the crate
graph, `-Z self-profile` for queries in one crate, IR function counts or `cargo llvm-lines` for instance counts.

**9. The `len()` myth.** Show release MIR (`len` is a field read) and release asm (the length is a register loaded once).
Debug artifacts show calls because nothing is inlined in debug. Performance questions need release artifacts.

**10. `noalias`.** It lets LLVM assume memory reached through one pointer isn't reached through others during the call:
fewer reloads, more vectorization. It comes from the borrow checker's guarantees via `fn_abi_of`. It's absent for
`&T` where `T` contains `UnsafeCell` (Chapter 4.1), for raw pointers, and in unoptimized builds.

**11. Breaking changes through the checker.** Borrow checking: adding `Drop` to a type that holds borrows (18.5).
Inference: adding a trait impl that creates ambiguity for callers relying on inference (18.3's `Borrow` import, or a new
`From` impl breaking `.into()`). Coherence: adding a blanket impl that overlaps downstream impls (Chapter 6.2).

**12. Macro policy.** Functions and generics by default; `macro_rules!` with `$crate` paths and collision tests; a short
proc-macro allowlist with build-time review (they run code on build machines); checked-in generated code for schemas
where hermetic builds matter; `--timings` budgets tracked in CI; one major version of `syn` across the workspace.

**13. Release-only differences.** Run tests in release as well as debug (at least nightly); keep overflow checks on where
arithmetic is money (Chapter 2.2's profile per system); never rely on function addresses; treat `debug_assert!` as
documentation, not protection; deny correctness lints (`unpredictable_function_pointer_comparisons`,
`suspicious_double_ref_op`) and review any suppression.

**14. Artifact per question.** Lock release: debug MIR. Loop allocation: release asm or a counting-allocator test. Method
resolution: the type checker's view (rust-analyzer's go-to-definition, or THIR/MIR showing the called function). Match
exhaustiveness: the language (an exhaustive `match` with no wildcard compiles, so it is), plus MIR's `unreachable`
arm. Slow compilation: `cargo build --timings`, then `-Z self-profile` on the slow crate.

**15. Three environments.** The Playground: quick, shareable reproductions on current stable, beta, and nightly with every
artifact type; no custom flags, no multi-crate builds. A local nightly: `-Z` flags (`dump-mir`, `self-profile`,
`unpretty=thir-tree`, `print-mono-items`), alternate backends, internal attributes. The pinned stable toolchain: the only
source of truth for what your product compiles to; confirm every conclusion there before acting on it.
