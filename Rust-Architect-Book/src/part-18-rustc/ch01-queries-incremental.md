# Chapter 18.1 — rustc's Architecture: Queries and Incremental Compilation

> **Where this sits:** Part XVIII · How rustc Works · chapter 1 of 7
> **Prerequisites:** Chapter 17.1 (the textbook compiler pipeline), Chapter 2.1 (crates, `.rlib`, `cargo check`),
> Chapter 7.3 (codegen units and compile-time cost).
> **After this chapter you can:** describe rustc as a demand-driven query system rather than a pipeline; read a query
> cycle error; explain why fixing one error reveals others, using verified rules for which errors hide which; explain
> DefIds, fingerprints, and red-green incremental compilation; and set a sound incremental-compilation and caching policy
> for a team's builds.

---

## Pass 1 · User level — *A compiler you query, not a compiler you run*

### 1. Problem

Chapter 17.1 drew the classic compiler as a pipeline: source → tokens → AST → typed tree → IR → machine code, each
stage consuming the whole output of the previous one. Chapters 2.3 and 7.1 showed you rustc's outputs at several of those
stages. So you'd expect rustc to *be* that pipeline.

Some things you've already seen don't fit a pipeline:

- Chapter 2.7 put a type error (E0616) and a privacy error (E0451) in one file and rustc reported **only one**. Fixing
  it revealed the other.
- Chapter 7.1 showed that an error in a generic function can appear only when the function is *used* ("while
  instantiating `fn first_byte::<0>`"), and that `cargo check` can miss it.
- A Java engineer's intuition says a failed type check stops a compiler. Yet rustc routinely reports name-resolution,
  type, and borrow errors from one run.
- Touch one function in a 50,000-line crate and a debug rebuild takes seconds, not minutes.

This chapter explains the architecture that produces all four behaviors: **rustc is a database of memoized,
dependency-tracked queries**. The pipeline still exists as the order in which queries usually run, but the unit of work
is a question ("what is the type of this item?"), not a phase.

### 2. Mental model

**A query is a pure function from a key to a value, computed on demand, cached, and dependency-tracked.** [RUSTC]
(rustc-dev-guide, *Queries: demand-driven compilation*.) All of them hang off one central context, `TyCtxt` (the
"typing context"):

```text
 tcx.type_of(def_id)          what is the type of this item?
 tcx.typeck(def_id)           type-check this function body; return the types of all its expressions
 tcx.mir_built(def_id)        build MIR for this body
 tcx.mir_borrowck(def_id)     borrow-check this body
 tcx.optimized_mir(def_id)    the optimized MIR codegen will use
 tcx.layout_of(ty)            size, alignment, field offsets, niches of a type
 tcx.symbol_name(instance)    the linker symbol for a monomorphized function
```

The compiler driver asks for the final products ("check the crate", then "generate code for these codegen units"), and
answering those forces everything they depend on. A query that nothing asks for never runs.

```text
  THE TEXTBOOK PIPELINE                      rustc
  ─────────────────────                      ─────
  parse all → check all → IR all → emit     "codegen CGU 3"  ──needs──► optimized_mir(foo)
                                                                          │
  each phase runs once, over everything       optimized_mir(foo) ──needs──► mir_borrowck(foo)
                                                                          │
                                              mir_borrowck(foo) ──needs──► typeck(foo) ──needs──► type_of(bar)
                                                                                                   fn_sig(bar) ...
                                              every result is cached; every "needs" edge is recorded
```

Four consequences, each of which you've already met or will meet in this Part:

1. **Errors are results.** A query that fails returns an error value. Anything that depends on it sees the error and
   usually stays quiet instead of cascading. That's why errors "hide" each other (§3).
2. **Cycles are detectable.** If answering query A requires A, the compiler notices and reports E0391, naming the
   queries on the cycle (§3).
3. **Memoization gives "compute once."** A constant used in three places is evaluated once (§3).
4. **Recorded dependencies give incremental compilation.** Next build, a cached result can be reused if nothing it read
   has changed (§4).

**Items are named by `DefId`s.** [RUSTC] Every definition (module, function, struct, closure, const) gets a `DefId`: a
crate number plus an index within that crate. The dump in §3 shows real ones. A `DefId` is only meaningful within one
compiler session, so incremental compilation, which must match things *across* sessions, keys on a **`DefPathHash`**: a
stable hash of the item's path (`ledger::accounts::balance::helper`).

**Where the stages live.** [RUSTC] rustc is itself a workspace of crates. The ones this Part visits, in the order a
function passes through them (names as of 2026; they're refactored often):

| Stage | rustc crates | Representative queries | Artifact you can see |
|---|---|---|---|
| Lex, parse | `rustc_lexer`, `rustc_parse` | (driven by a few coarse, crate-wide queries) | — |
| Expand macros, resolve names | `rustc_expand`, `rustc_resolve` | `resolutions` | `-Target expand` (18.2) |
| Lower to HIR | `rustc_ast_lowering` | `hir_crate`, owner nodes | `-Target hir` (18.2) |
| Collect items, check well-formedness | `rustc_hir_analysis` | `type_of`, `fn_sig`, `predicates_of` | errors, `rustc_dump_*` (18.3) |
| Type-check bodies | `rustc_hir_typeck`, `rustc_trait_selection` | `typeck` | errors, `rustc_*` dumps (18.3) |
| Build THIR, check patterns and `unsafe` | `rustc_mir_build`, `rustc_pattern_analysis` | `thir_body`, `mir_built` | `-Zunpretty=thir-tree` (18.4) |
| Borrow-check | `rustc_borrowck` | `mir_borrowck` | errors, `rustc_regions` (18.5) |
| Optimize MIR | `rustc_mir_transform` | `optimized_mir` | `-Target mir` (18.4) |
| Collect mono items, partition | `rustc_monomorphize` | `collect_and_partition_mono_items` | `-Zprint-mono-items` (18.6) |
| Generate code | `rustc_codegen_ssa`, `rustc_codegen_llvm` | `codegen_unit`, `fn_abi_of_instance` | `-Target llvm-ir`, `asm` (18.6) |

### 3. Rust code

**A query cycle, reported with the queries' own names** (listing `ch01-01-query-cycle.rs`, verified):

```rust,compile_fail
const LIMIT: usize = BURST * 2;
const BURST: usize = LIMIT / 2;

fn main() {
    println!("{LIMIT}");
}
```

```text
error[E0391]: cycle detected when simplifying constant for the type system `LIMIT`
 --> src/main.rs:4:1
  |
4 | const LIMIT: usize = BURST * 2;
  | ^^^^^^^^^^^^^^^^^^
  |
note: ...which requires const-evaluating + checking `LIMIT`...
note: ...which requires simplifying constant for the type system `BURST`...
note: ...which requires const-evaluating + checking `BURST`...
  = note: ...which again requires simplifying constant for the type system `LIMIT`, completing the cycle
  = note: cycle used when simplifying constant for the type system `LIMIT`
  = note: for more information, see <https://rustc-dev-guide.rust-lang.org/overview.html#queries> and <https://rustc-dev-guide.rust-lang.org/query.html>
```

(Note spans trimmed.) Each "requires" line is a **query description**, and the error links to the rustc-dev-guide's
query chapter. You're reading the compiler's stack of in-progress queries: evaluating `LIMIT` asked for `BURST`, which
asked for `LIMIT`, which was already running. A pipeline compiler would need a special-purpose cycle check for
constants, another for type aliases, another for trait impls. rustc gets them all from one mechanism.

**Memoization, visible** (listing `ch01-02-memoized-const.rs`, verified). One broken constant, used three times:

```rust,compile_fail
const MAX_RETRIES: u8 = 200 + 100; // overflows u8: const evaluation fails

fn gateway() -> u8 {
    MAX_RETRIES
}
fn payments() -> u8 {
    MAX_RETRIES
}
fn ledger() -> u8 {
    MAX_RETRIES
}
```

```text
error[E0080]: attempt to compute `200_u8 + 100_u8`, which would overflow
 --> src/main.rs:4:25
  |
4 | const MAX_RETRIES: u8 = 200 + 100; // overflows u8: const evaluation fails
  |                         ^^^^^^^^^ evaluation of `MAX_RETRIES` failed here

note: erroneous constant encountered
 --> src/main.rs:7:5
note: erroneous constant encountered
  --> src/main.rs:10:5
note: erroneous constant encountered
  --> src/main.rs:13:5

error: could not compile `playground` (bin "playground") due to 1 previous error
```

**One** error, three notes. The evaluation ran once; each use asked the query again and got the cached failure.

**Which errors hide which.** Three verified experiments, run on 1.98.1, that let you predict what a build will report.

*Experiment 1: errors in different bodies are independent* (listing `ch01-03-per-body-errors.rs`). A name-resolution
error in one function, a type error in a second, a borrow error in a third:

```rust,compile_fail
fn resolve_error() -> u32 {
    retries + 1 // E0425: no such name
}

fn type_error() -> u32 {
    "three" // E0308: wrong type
}

fn borrow_error() -> u32 {
    let mut v = vec![1u32];
    let first = &v[0];
    v.push(2); // E0502
    *first
}
```

```text
error[E0425]: cannot find value `retries` in this scope
error[E0308]: mismatched types
error[E0502]: cannot borrow `v` as mutable because it is also borrowed as immutable
error: could not compile `playground` (bin "playground") due to 3 previous errors
```

All three phases reported. A resolution error didn't stop type checking, and type errors elsewhere didn't stop borrow
checking of `borrow_error`.

*Experiment 2: within one body, a type error suppresses borrow checking* (listing `ch01-04-same-body-gating.rs`):

```rust,compile_fail
fn both_in_one_body() -> u32 {
    let mut v = vec![1u32];
    let first = &v[0];
    v.push(2); // would be E0502...
    let label: u32 = "three"; // ...but this body also has E0308
    *first + label
}
```

```text
error[E0308]: mismatched types
 --> src/main.rs:8:22
  |
8 |     let label: u32 = "three"; // ...but this body also has E0308
  |                ---   ^^^^^^^ expected `u32`, found `&str`
  |                |
  |                expected due to this

error: could not compile `playground` (bin "playground") due to 1 previous error
```

The borrow error is still there. It's just not reported: [RUSTC] a body whose type-check results are *tainted by
errors* isn't borrow-checked, because MIR built from an ill-typed body would produce nonsense errors.

*Experiment 3: some checks run only if everything before them succeeded.* Listing `ch01-05-driver-gate.rs` has a borrow
error in one function and a struct literal with a private field (E0451, Chapter 2.7) in another. Only the borrow error is
reported:

```text
error[E0502]: cannot borrow `v` as mutable because it is also borrowed as immutable
error: could not compile `playground` (bin "playground") due to 1 previous error
```

Remove the borrow error (listing `ch01-06-privacy-alone.rs`) and E0451 appears, together with two warnings (an unneeded
`mut` and a never-read field) that were also silent before:

```text
warning: variable does not need to be mutable
warning: field `id` is never read
error[E0451]: field `total_cents` of struct `Invoice` is private
```

The rules these experiments establish (observed on 1.98.1; the gating is [RUSTC], not a language promise):

| Error class | Reported when | Hidden by |
|---|---|---|
| Parse, macro, name resolution (E0425, E0433, E0659) | First | Almost nothing (a parse error can hide the rest of the file) |
| Type errors (E0308, E0277, E0599) | Per body | Resolution errors *in the same expression* (they become error types) |
| Borrow-check errors (E0499, E0502, E0505, E0382) | Per body, only for bodies with no type errors | Any type error in the same body |
| Privacy pass (E0451), many lints and warnings | After a crate-wide "anything failed?" gate | **Any** earlier error, anywhere in the crate |
| Post-monomorphization errors (Chapter 7.1) | Only in full builds, only for instances that are used | Everything above, and `cargo check` never reaches them |

That table is the practical payoff of this chapter. It explains why "I fixed the last error and got five new ones" is
normal, and it's why this book puts one intended compile error in each listing.

**The DefIds of a nested item** (listing `ch01-07-def-ids.rs`, nightly-only, verified). `#[rustc_dump_def_parents]` is
one of the compiler's internal testing attributes (the README's caveat applies). Applied to a function nested inside
another function inside two modules:

```text
error: rustc_dump_def_parents: DefId(0:6 ~ playground[7766]::ledger::accounts::balance::helper)
note: DefId(0:5 ~ playground[7766]::ledger::accounts::balance)
note: DefId(0:4 ~ playground[7766]::ledger::accounts)
note: DefId(0:3 ~ playground[7766]::ledger)
note: DefId(0:0 ~ playground[7766])
```

(Spans trimmed.) `0:6` means crate 0 (the crate being compiled; dependencies get other numbers) and index 6. After the
`~` is the item's **def path**, the stable name that the `DefPathHash` hashes. Insert a new function above `ledger`
and every index shifts, but the def paths don't. That's why incremental compilation uses paths, not indices.

---

## Pass 2 · Systems level — *Inside the query engine*

### 4. Under the hood

**Anatomy of a query.** [RUSTC] Each query is declared once (its key type, value type, and description, the string you
saw in E0391) and implemented by a **provider** function registered by the crate that owns that stage. Calling
`tcx.typeck(def_id)`:

1. Looks up the key in the query's cache. On a hit, it records a dependency edge (the caller read this query) and
   returns.
2. On a miss, it pushes the query onto the active-query stack (the stack E0391 printed), runs the provider, and records
   every query the provider calls as a dependency.
3. Stores the result (types are interned in arenas, so results are mostly cheap pointers), pops the stack, returns.

Some callers need a query's *side effects* (its errors) but not its value, and call it through `tcx.ensure()`, which
skips loading the result if it's already known to be valid.

**The driver.** [RUSTC] `rustc_driver` parses arguments and hands off to `rustc_interface`, which runs the front of the
compiler (parse, expand, resolve, lower to HIR), then an **analysis** step that forces the checks: well-formedness, type
checking of every body, MIR building, borrow checking of every body. Experiment 3 shows what comes next: a gate that
stops if any error has been emitted, then crate-wide passes such as privacy checking and linting. Only then does the
driver ask for codegen. `cargo check` stops after analysis and writes only the crate's **metadata** (`.rmeta`, Chapter
2.1), which is why it's several times faster than a build and why it never reaches monomorphization-time errors.

**Incremental compilation: red-green.** [RUSTC] (rustc-dev-guide, *Incremental compilation in detail*.) While a
session runs, every query execution becomes a node in a **dependency graph**, with edges to the queries it read. Each
result is hashed into a **fingerprint** using a stable hasher (stable across sessions, so it hashes `DefPathHash`es,
never `DefId`s or pointers). The graph, fingerprints, and serializable results go to `target/<profile>/incremental/`.

In the next session, before recomputing a query, the compiler tries to prove the old result is still valid:

```text
 try_mark_green(Q):
   for each dependency D that Q read last time:
       if D is an input (source text of an item, a compiler flag):  compare its fingerprint to last time
       else:                                                        try_mark_green(D), or recompute D
       if D changed → Q must be recomputed ("red"), unless...
   all dependencies green  → Q is GREEN: load its result from disk, don't run the provider

 recompute Q; if the new fingerprint EQUALS the old one → Q is still GREEN for its dependents ("early cutoff")
```

**Early cutoff** is what makes it work in practice. Edit a function body: its HIR changes, so `typeck` for that body
reruns. But if its *signature* didn't change, the signature query produces an identical fingerprint, and nothing that
only read the signature (every caller's type check) needs to rerun.

**Codegen reuse.** [RUSTC] The back end is incremental at the granularity of **codegen units** (Chapter 7.3's CGUs:
16 by default for non-incremental builds, 256 for incremental ones). If all mono items in a CGU are unchanged, its
object file from the last session (a "work product") is reused, and LLVM never runs for it. That's why incremental
builds use many small CGUs: a change invalidates less.

**Parallelism.** [VERSION] The front end (everything up to MIR) has historically been single-threaded per crate; LLVM
work is parallel across CGUs. A parallel front end exists on nightly behind `-Z threads=N`. The Rust compiler team
announced it in November 2023 and reported large reductions on some workloads (up to 50% in their post). Check the
current release notes before counting on it.

> **What actually happens?** Why doesn't a type error in `type_error()` stop borrow checking of `borrow_error()`?
> Because borrow checking isn't a phase that runs "after type checking"; it's the query `mir_borrowck(borrow_error)`,
> which depends on `typeck(borrow_error)`, which succeeded. Nothing in its dependency chain touches `type_error`. The
> "phase" is an illusion created by the driver asking for results in a particular order.

### 5. Memory

- **Interning.** [RUSTC] Types, lists of types, constants, and many other values are **interned**: each distinct value
  is stored once in an arena, and a `Ty<'tcx>` is a pointer to it. Type equality is pointer equality, which makes the
  enormous number of type comparisons during inference cheap. The `'tcx` lifetime you'll see all over rustc's source is
  the lifetime of those arenas: Chapter 3.6's arena-and-lifetime pattern, at compiler scale.
- **The query caches hold the whole crate's analysis in memory.** Compiling a large crate can take gigabytes of RAM
  (order of magnitude, labeled; measure your own with `/usr/bin/time -v cargo build`). Memory scales with the size of one
  crate, not the whole workspace, since each crate is a separate rustc process. That's one more argument for splitting a
  giant crate.
- **The incremental cache is disk, and it grows.** `target/debug/incremental/` keeps session directories per crate and
  per configuration. Cargo deletes stale ones for the current configuration, but switching branches, features, or
  toolchains multiplies them. Measure with `du -sh target/debug/incremental` (not verified here).

### 6. CPU / OS

A `cargo build` of a workspace is a DAG of rustc processes. Cargo runs independent crates in parallel (bounded by the
jobserver, `-j`), and **pipelining** (Chapter 2.1) lets a downstream crate start type-checking as soon as its dependency
has emitted metadata, before the dependency's codegen finishes. Inside one rustc process, the front end is mostly one
thread and LLVM uses several threads for CGUs. The link step at the end is one more process (lld by default on
x86-64 Linux since 1.90, Chapter 1.4).

That structure predicts where time goes:

| Symptom (in `cargo build --timings`) | Likely cause | Lever |
|---|---|---|
| One crate's long bar blocks everything downstream | A big crate on the critical path | Split it; move generic instantiation out (Chapter 7.3) |
| Long "codegen" portion of a bar | Many or large mono items | Fewer instances, `dyn` at boundaries, fewer CGUs in release |
| Long front-end portion, idle cores | Type checking / trait solving in one crate | `-Z self-profile` on that crate (§9), simpler trait machinery (18.3) |
| Linking dominates incremental rebuilds | Big binaries, debug info | Faster linker, `split-debuginfo`, less debug info in dev |

`cargo build --timings` is stable Cargo (since 1.60), not verified here because the Playground doesn't expose it.

---

## Pass 3 · Architect level — *Designing builds around the query engine*

### 7. Trade-offs

| Decision | Option A | Option B | What decides it |
|---|---|---|---|
| Incremental | On (dev profile default) | Off (release default; common in CI) | Warm local cache → on. Clean CI machine, or a cache you can't keep coherent → off |
| Crate size | Few big crates | Many small crates | Big: less metadata overhead, more incremental reuse *inside*. Small: parallel builds, less rebuilt per change, but instances compile downstream (Chapter 7.3) |
| Check vs build in CI | `cargo check` + tests | Full release build | Check is fast but misses post-mono errors and codegen. Do both, at different frequencies |
| Codegen units | Many (fast parallel builds) | 1 (best optimization, Chapter 2.2's gateway profile) | Release binaries you ship: fewer. Dev: many |

> **Why not compile each file separately, like javac or a C compiler?** Because the language doesn't allow it. Privacy
> is crate-relative (`pub(crate)`, Chapter 2.7), coherence reasons about all impls in a crate (Chapter 6.3), generic
> functions are instantiated where they're used, and inlining crosses modules freely. The query system gets most of
> separate compilation's benefit (only recompute what changed) *without* separate compilation's semantics. The price is
> that the crate is the unit of parallelism and memory, which is why crate boundaries are an architectural decision.

### 8. Java comparison

`javac` is much closer to the textbook pipeline. It runs phases over the classes of a compilation (parse, enter,
annotation processing, attribute, flow, desugar, generate), and writes one `.class` file per class. Incremental builds
happen *outside* the compiler: Gradle and Maven plugins track each class's ABI and recompile dependents only when an ABI
changes. That's "early cutoff" at class granularity, implemented by the build tool.

| | javac + Gradle | rustc + Cargo |
|---|---|---|
| Unit of compilation | Class / source file | Crate |
| Unit of incremental reuse | Class (build tool tracks ABI) | Query result (compiler tracks dependencies), CGU object file |
| Where the expensive optimization happens | At run time (C1/C2 JIT) | At build time (LLVM) |
| Cycle between definitions | Allowed (classes can reference each other freely) | Allowed between items, **not** between values the compiler must compute (E0391) |
| IDE engine | Separate (Eclipse JDT, IntelliJ's own model) | Separate (rust-analyzer, built on the salsa query framework, inspired by rustc's queries) |

> **Analogy limit.** "Cargo is Gradle" breaks exactly at incrementality. Gradle can skip recompiling a Java class
> because the JVM links classes at run time: a class file is a stable, separately loadable unit. Rust has no run-time
> linking of generic code and no stable ABI (Chapter 6.5), so the unit that can be skipped is a *compiler query result*
> or a *codegen unit's object file*, and only rustc itself can decide that.

### 9. Production scenario

**Meridian's Rust build policy.** By 2026 Meridian has several Rust codebases (the gateway workspace, payments-core,
the fraud library, and shared crates like `meridian-types` and `meridian-telemetry`), all pinned to 1.98.1 (Chapter
2.1). The platform team's build policy follows the query engine's economics:

- **Developer machines:** incremental on (the dev-profile default). rust-analyzer gives type errors on save. `cargo
  check` is the inner loop, `cargo test` the outer.
- **CI, per pull request:** `CARGO_INCREMENTAL=0`. A CI machine starts cold, and restoring an incremental cache that
  was produced by another branch costs I/O and buys little. Instead, CI caches `~/.cargo/registry` and the *compiled
  dependencies* in `target/` (which change only when `Cargo.lock` does), keyed on the lock file and the toolchain
  version.
- **CI, nightly:** a full `--release` build of every binary. This is the only job that reaches monomorphization-time
  errors (Chapter 7.1) and link errors, so it gates the release train.
- **When a build gets slow:** first `cargo build --timings` to find the crate on the critical path (that's how the
  gateway's workspace diet in Chapter 7.3 started). Then, for that one crate, a nightly
  `cargo +nightly rustc -- -Z self-profile`, summarized with the `measureme` tools, to see which *queries* dominate:
  `typeck` and trait evaluation point at trait-heavy code (18.3), `mir_borrowck` at giant generated functions (18.5),
  LLVM passes at instance counts (18.6). Not verified here: these flags need a local nightly toolchain.

### 10. Failure scenario

**The CI cache that made builds slower, and then broke them.** In early 2026, a Meridian team added a CI step that
cached the *entire* `target/` directory, keyed only on the branch name, with incremental compilation left on "because it's
faster locally." Three things happened:

1. **The cache grew without bound.** Each feature-flag combination and each branch left its own incremental session
   directories (§5). Within two months the cache archive was tens of gigabytes. Restoring and re-uploading it took longer
   than a clean build of the dependencies it contained.
2. **The hits were mostly misses.** Incremental reuse needs the *same* crate compiled with the same flags. A pull request
   restored the cache of a different branch, so the fingerprints mostly didn't match. The job paid the cost of loading a
   dependency graph and got little reuse.
3. **Then an internal compiler error.** The fuzzing job, which used a nightly toolchain, began failing intermittently
   with an ICE about **unstable fingerprints** in the incremental cache. Purging the cache made it go away.

The team didn't need to debug the ICE to fix the policy: incremental state is a *per-machine, per-configuration* cache,
not an artifact to share. They switched to the policy in §9.

That ICE class has history. [VERSION] In May 2021, Rust 1.52.0 added verification of incremental-compilation
fingerprints, which surfaced latent bugs as ICEs. Rust 1.52.1 responded by **disabling incremental compilation by
default** (per the Rust blog's "Announcing Rust 1.52.1"), and it was re-enabled in 1.54 after the bugs were fixed. The
lesson for an architect: incremental compilation is an optimization with its own correctness risk. Keep it where it
pays (warm local caches), keep it out of places where a wrong result is expensive (release builds, which disable it by
default), and make clearing it a one-command operation.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVIII).*

1. What is a query in rustc? Give three examples, and explain what "demand-driven" means for which code actually gets
   compiled.
2. Why does rustc report an E0391 "cycle detected" for two mutually defined constants, and what do the "...which
   requires" lines tell you?
3. A developer fixes a type error and three borrow errors appear in the same function. Explain why, using the query
   model. Then explain why a privacy error might appear only after *all* of those are fixed.
4. What's the difference between a `DefId` and a `DefPathHash`, and why does incremental compilation need the second?
5. Explain red-green marking and early cutoff. Why doesn't editing a function body force every caller to be
   re-type-checked?
6. How does incremental compilation reuse back-end work, and what does that imply for the number of codegen units?
7. Why is `cargo check` faster than `cargo build`, and what class of errors can it never report?
8. Your team wants to share one `target/` cache across all CI jobs. What do you recommend, and why?

### 12. Exercises

- **Beginner.** Predict how many errors each variant reports, then check on the Playground: (a) listing
  `ch01-04-same-body-gating.rs` as is; (b) with the `let label` line moved into a separate function; (c) with a private
  field literal (E0451) added to (b).
- **Intermediate.** Write three different E0391 cycles: two constants, a recursive type alias, and a trait impl whose
  where-clause requires itself through an associated constant. Record the query descriptions each one prints. Which
  queries appear in all three?
- **Advanced.** Using `#[rustc_dump_def_parents]` on nightly, dump the DefIds for items in a `mod`, an `impl` block, and
  a closure. Which of them get their own DefId? What does that suggest about the granularity of incremental reuse?
- **Systems.** On a local machine with a nightly toolchain, build a medium-sized crate twice with `-Z self-profile` (once
  clean, once after editing one function body) and compare the query counts and times. Which queries were "green"
  (loaded) the second time? (Not verifiable on the Playground.)
- **Architecture.** Sketch the crate graph of a service you know as it would look in Rust. Mark the critical path, the
  crates whose change rebuilds the most, and where you would split or merge crates. Justify each change with this
  chapter's mechanisms.

### 13. Debugging exercise

A pull request touches three functions in a crate. The first CI run reports:

```text
error[E0433]: failed to resolve: use of undeclared type `Invoce`
error[E0308]: mismatched types   (in fn settle)
```

The author fixes both and pushes. The second run reports two E0502 borrow errors *in `settle`* and one E0382 in another
function that the PR also touched. They fix those, and the third run reports E0451 and a batch of `dead_code` warnings.
The author says "rustc is making me play whack-a-mole."

1. For each run, explain why exactly those errors appeared and the others didn't, using §3's table.
2. Which of the second run's errors *could* have appeared in the first run, and why did or didn't they?
3. What would you tell the author about how to predict the next run? What one habit shortens this loop?

### 14. Design exercise

**A build platform for Meridian's Rust estate.** Thirty services depend on `meridian-telemetry` and `meridian-types`
(Chapters 5.3 and 7.3). A change to `meridian-types` rebuilds everything. CI is at 25 minutes per PR and rising.

Design the build architecture: crate boundaries (what goes in the shared crates, and what must never go there), caching
(what is cached, keyed on what, and whether incremental state is ever shared), the split between `check`, test, and
release builds, and the measurements you'd collect on every run (per-crate timings, query profiles on demand). State
which Rust mechanisms from this chapter each decision relies on, and what would make you change it.
