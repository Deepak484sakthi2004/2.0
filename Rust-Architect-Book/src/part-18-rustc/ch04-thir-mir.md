# Chapter 18.4 — THIR and MIR

> **Where this sits:** Part XVIII · How rustc Works · chapter 4 of 7
> **Prerequisites:** Chapter 18.3 (type checking and its results), Chapter 17.6 (IRs, CFGs, SSA), Chapter 3.1 (drop
> flags in MIR), Chapter 3.5 (drop order and temporaries), Chapter 8.3 (unwinding and landing pads).
> **After this chapter you can:** say what THIR adds to HIR and which checks run on it; read MIR fluently (locals,
> places, statements, terminators, cleanup blocks); predict where drops land, including the `let _ =` and `match`
> scrutinee cases; explain how the MIR pipeline turns built MIR into the optimized MIR codegen sees; use const
> evaluation, the compiler's MIR interpreter, as a build-time validation tool; and choose which artifact (MIR, LLVM IR,
> or assembly) answers a given question.

---

## Pass 1 · User level — *The IR you've been reading since Chapter 2.3*

### 1. Problem

This book has quoted MIR since Chapter 2.3: `debug x => const 10_i32`, drop flags in Chapter 3.1, cleanup blocks in
Chapter 3.5, the `?` desugaring in Chapter 8.1. You've been reading it as "what the compiler does with my function."
This chapter explains where it comes from, what it guarantees, and what it's for.

Several things this book has asserted come down to decisions made on the way from HIR to MIR:

- `let _ = acquire();` drops the value immediately, but `let _lease = acquire();` keeps it to the end of the scope
  (Chapter 1.3's debugging exercise). Where is that decided, and how can you *see* it?
- A lock taken in a `match` scrutinee is still held inside the arms (Chapter 3.5). Why the arms, and not just the
  scrutinee?
- `const _: () = assert!(...)` can fail your **build**. Something must be running Rust code inside the compiler.
- A missing `match` arm (E0004) and a call to an `unsafe fn` outside `unsafe` (E0133) are rejected before borrow
  checking, and neither is a type error.
- The Playground's MIR output starts with *"subject to change without notice. Knock yourself out."* How much of it can
  you rely on?

The answer to all five is the stretch of the compiler between type checking (18.3) and code generation (18.6): **THIR**,
a typed tree, and **MIR**, a control-flow graph.

### 2. Mental model

```text
 HIR  +  TypeckResults (18.3: types, adjustments, resolved methods, captures)
   │  thir_body(def)                                    [RUSTC] rustc_mir_build
   ▼
 THIR   a typed, fully explicit TREE: every auto-ref/deref, coercion and overloaded operator written out;
   │    method calls resolved to DefIds; patterns typed
   │    checks that run here: pattern exhaustiveness (E0004), unsafety (E0133)
   │  mir_built(def)
   ▼
 MIR (built)   a control-flow GRAPH of basic blocks: explicit drops, explicit unwind edges,
   │           every temporary a numbered local
   │  mir_promoted → mir_borrowck (18.5) → drop elaboration → optimizations
   ▼
 optimized_mir(def) ───► codegen (18.6)          mir_for_ctfe(def) ───► const evaluation (the MIR interpreter)
```

Why two IRs after HIR, and why these two?

- **THIR is for questions about the program's *shape*.** Exhaustiveness is a question about a `match` and its patterns;
  unsafety is a question about which expressions sit inside which `unsafe` blocks. Both are easier on a tree. THIR is
  also the last place where the source's nesting still exists: MIR building consumes it and throws it away (it's built
  one body at a time and freed afterward). [RUSTC]
- **MIR is for questions about *execution order and control flow*.** Borrow checking (18.5), drop elaboration, move
  checking, and optimization all need "what can happen before what, on which path." A graph of basic blocks answers
  that directly. MIR is also what gets stored in crate metadata and instantiated downstream (Chapters 2.1 and 7.1), and
  what the compiler *executes* for constants.

**The MIR vocabulary**, enough to read everything in this Part:

| MIR | Meaning |
|---|---|
| `_0` | The return place. `_1 … _n` are the arguments, then user variables and temporaries |
| `debug x => _3` | Debug info only: the source name `x` lives in local `_3` |
| `bb0: { … }` | A basic block: straight-line **statements**, then exactly one **terminator** |
| `_4 = &_3`, `_2 = discriminant(_1)`, `_8 = AddWithOverflow(copy _6, const 1_usize)` | Statements: assign an **rvalue** to a **place** |
| `(_1 as Get).0`, `(*_1)`, `(_4.0: Vec<u8>).1` | Places with projections: downcast to a variant, deref, field |
| `copy _6`, `move _7` | Operands: whether the use copies the bits or moves ownership out |
| `switchInt(move _2) -> [0: bb4, 1: bb3, otherwise: bb1]` | Terminator: multi-way branch |
| `_0 = f(…) -> [return: bb5, unwind: bb9]` | Terminator: a call, with a normal successor and an **unwind** successor |
| `drop(_3) -> [return: bb10, unwind continue]` | Terminator: run `_3`'s drop glue (Chapter 3.1) |
| `assert(!move (_8.1: bool), "attempt to compute …") -> [success: bb7, unwind: bb8]` | Terminator: panic unless the condition holds (overflow, bounds) |
| `bb8 (cleanup): { … }` | A block that only runs while unwinding (Chapter 8.3) |
| `unwind continue` / `unwind terminate(cleanup)` / `resume` | Keep unwinding / abort (a panic *during* cleanup) / re-raise |
| `unreachable` | This point can't be reached (the optimizer may assume so) |

### 3. Rust code

**A `match` as a graph.** Listing `ch04-01-match-mir.rs` (verified to build):

```rust,ignore
pub enum Cmd {
    Get(String),
    Del(String),
    Ping,
}

#[inline(never)]
pub fn cost(cmd: Cmd) -> usize {
    match cmd {
        Cmd::Get(k) => k.len(),
        Cmd::Del(k) => k.len() + 1,
        Cmd::Ping => 0,
    }
}
```

Its MIR in a debug build (`tools/emit.ps1 -Target mir -Mode debug`, rustc 1.98.1; the constructor functions rustc also
printed for `Cmd::Get` and `Cmd::Del` are trimmed, and we'll come back to them):

```text
fn cost(_1: Cmd) -> usize {
    debug cmd => _1;
    let mut _0: usize;
    let mut _2: isize;
    let _3: std::string::String;
    let mut _4: &std::string::String;
    let _5: std::string::String;
    let mut _6: usize;
    let mut _7: &std::string::String;
    let mut _8: (usize, bool);
    scope 1 {
        debug k => _3;
    }
    scope 2 {
        debug k => _5;
    }

    bb0: {
        _2 = discriminant(_1);
        switchInt(move _2) -> [0: bb4, 1: bb3, 2: bb2, otherwise: bb1];
    }

    bb1: {
        unreachable;
    }

    bb2: {
        _0 = const 0_usize;
        goto -> bb10;
    }

    bb3: {
        _5 = move ((_1 as Del).0: std::string::String);
        _7 = &_5;
        _6 = String::len(move _7) -> [return: bb6, unwind: bb8];
    }

    bb4: {
        _3 = move ((_1 as Get).0: std::string::String);
        _4 = &_3;
        _0 = String::len(move _4) -> [return: bb5, unwind: bb9];
    }

    bb5: {
        drop(_3) -> [return: bb10, unwind continue];
    }

    bb6: {
        _8 = AddWithOverflow(copy _6, const 1_usize);
        assert(!move (_8.1: bool), "attempt to compute `{} + {}`, which would overflow", move _6, const 1_usize) -> [success: bb7, unwind: bb8];
    }

    bb7: {
        _0 = move (_8.0: usize);
        drop(_5) -> [return: bb10, unwind continue];
    }

    bb8 (cleanup): {
        drop(_5) -> [return: bb11, unwind terminate(cleanup)];
    }

    bb9 (cleanup): {
        drop(_3) -> [return: bb11, unwind terminate(cleanup)];
    }

    bb10: {
        return;
    }

    bb11 (cleanup): {
        resume;
    }
}
```

Read it the way you'd read a small assembly function, top to bottom:

- **`bb0`** reads the enum's **discriminant** and branches. The `otherwise: bb1` arm is `unreachable`: the
  discriminant can only be 0, 1, or 2 [LANG: validity, Chapter 5.2], and this is the MIR form of the exhaustiveness
  proof. (Chapter 5.1 showed the same shape from the pattern side.)
- **`bb4`** is the `Get` arm. `((_1 as Get).0: String)` is a *downcast projection*: "treat `_1` as its `Get` variant,
  take field 0." The `String` is **moved** into `_3`, which is the binding `k` (see `scope 1`).
- **Every call has an `unwind` edge.** If `String::len` panicked (it can't, but the compiler hasn't looked inside it
  yet), control would go to `bb9 (cleanup)`, which drops `_3` and then `resume`s unwinding. That's Chapter 8.3's
  landing pad, before LLVM has seen it.
- **The drops are explicit terminators**: `drop(_3)` on the normal path, `drop(_3)` again on the unwind path. Nothing in
  MIR happens implicitly.
- **`bb6` is `overflow-checks = on`** (Chapter 2.2): `k.len() + 1` became `AddWithOverflow` plus an `assert` terminator.
- **`_1` itself is never dropped.** Each arm either moved the variant's only field out, or (for `Ping`) the variant has
  nothing to drop. Deciding that is **drop elaboration**'s job (§4).

**The same function, optimized.** The release MIR (trimmed to the `Get` and `Del` arms):

```text
    bb3: {
        StorageLive(_4);
        _4 = move ((_1 as Del).0: std::string::String);
        StorageLive(_5);
        _5 = copy ((_4.0: std::vec::Vec<u8>).1: usize);
        StorageLive(_6);
        _6 = Le(copy _5, const 9223372036854775807_usize);
        assume(move _6);
        StorageDead(_6);
        _0 = Add(move _5, const 1_usize);
        StorageDead(_5);
        drop(_4) -> [return: bb6, unwind continue];
    }

    bb4: {
        StorageLive(_3);
        _3 = move ((_1 as Get).0: std::string::String);
        _0 = copy ((_3.0: std::vec::Vec<u8>).1: usize);
        StorageLive(_7);
        _7 = Le(copy _0, const 9223372036854775807_usize);
        assume(move _7);
        StorageDead(_7);
        drop(_3) -> [return: bb5, unwind continue];
    }
```

Four MIR-level changes, before LLVM runs at all:

1. **`String::len` was inlined by the MIR inliner** (the release output lists `scope 6 (inlined String::len)` and
   `scope 7 (inlined Vec::<u8>::len)`). A call became a field read: the `len` of the `Vec<u8>` inside the `String`.
2. **The inlined code carried a promise**: `assume(len <= isize::MAX)`. [LIB] `Vec::len` tells the optimizer that a
   length never exceeds `isize::MAX` (allocations can't be larger). You'll see it again as LLVM's
   `range(i64 0, -9223372036854775808)` in Chapter 18.7.
3. **The overflow check is gone**: `Add`, not `AddWithOverflow`. That's the release profile's `overflow-checks = false`,
   not an optimization.
4. **The cleanup blocks for the calls are gone**, because nothing that remains can unwind.

And one change in the other direction: the release MIR has `StorageLive`/`StorageDead` statements and the debug MIR
has none. Those markers tell the backend when a local's stack slot is in use, so slots can be shared. They become LLVM
lifetime markers (Chapter 18.7 shows `llvm.lifetime.start` in release IR), which aren't emitted without optimization, so
the debug pipeline drops them early. [RUSTC, observed on 1.98.1]

**`let _ =` proven in MIR.** Chapter 1.3 had you debug `let _ = guard` from the outside. Listing
`ch04-02-lease-drop.rs` (verified) makes the lease's lifetime visible:

```rust,ignore
#[inline(never)]
pub fn pay_out_buggy(held: &Cell<bool>) {
    let _ = acquire(held); // BUG: `_` is not a binding; the Lease is dropped at the end of this statement
    println!("  paying out (lease held: {})", held.get());
}

#[inline(never)]
pub fn pay_out_fixed(held: &Cell<bool>) {
    let _lease = acquire(held); // a binding: dropped at the end of the scope
    println!("  paying out (lease held: {})", held.get());
}
```

```text
buggy:
  lease acquired
  lease released
  paying out (lease held: false)
fixed:
  lease acquired
  paying out (lease held: true)
  lease released
```

The debug MIR of the two functions (`-Target mir -CrateType bin`, trimmed to the first blocks):

```text
fn pay_out_buggy(_1: &Cell<bool>) -> () {            fn pay_out_fixed(_1: &Cell<bool>) -> () {
    let mut _2: Lease<'_>;                               let _2: Lease<'_>;
                                                         scope 1 {
                                                             debug _lease => _2;
    bb0: {                                               bb0: {
        _2 = acquire(copy _1) -> [return: bb1, …];           _2 = acquire(copy _1) -> [return: bb1, …];
    }                                                    }
    bb1: {                                               bb1: {
        drop(_2) -> [return: bb2, …];                        _7 = Cell::<bool>::get(copy _1) -> [return: bb2, unwind: bb7];
    }                                                    }
    bb2: {                                               …
        _7 = Cell::<bool>::get(copy _1) -> …                 bb5: { drop(_2) -> [return: bb6, …]; }
```

(Side-by-side layout and `…` are mine; the lines are verbatim.) In the buggy version, `_2` is an anonymous temporary with
no `debug` name, and `drop(_2)` is the very next terminator after the call, **before** `held.get()`. In the fixed
version, `_2` is the variable `_lease`, and its drop is at `bb5`, after the print. There's also a `bb7 (cleanup)` in the
fixed version that drops the lease if anything panics while it's held. The buggy version has no such block: there's
nothing left to clean up. [LANG] The rule behind it: `_` is a pattern that doesn't bind, so the initializer is a
temporary, and temporaries in a `let` statement are dropped at the end of the statement (Chapter 3.5's table).

**Const evaluation: the compiler runs your code.** Listing `ch04-03-const-table-bad.rs` validates a fee table at
compile time:

```rust,compile_fail
/// Fee tiers: (monthly volume threshold in cents, fee in basis points). Must be sorted by threshold.
const FEE_TIERS: [(u64, u32); 4] = [
    (0, 290),
    (1_000_000_00, 250),
    (500_000_00, 220), // BUG: out of order (a copy-paste from the old table)
    (10_000_000_00, 180),
];

const fn tiers_sorted(t: &[(u64, u32)]) -> bool {
    let mut i = 1;
    while i < t.len() {
        if t[i - 1].0 >= t[i].0 {
            return false;
        }
        i += 1;
    }
    true
}

const _: () = assert!(tiers_sorted(&FEE_TIERS), "FEE_TIERS must be sorted by threshold");
```

```text
error[E0080]: evaluation panicked: FEE_TIERS must be sorted by threshold
  --> src/main.rs:23:15
   |
23 | const _: () = assert!(tiers_sorted(&FEE_TIERS), "FEE_TIERS must be sorted by threshold");
   |               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ evaluation of `_` failed here
```

`tiers_sorted` ran *inside rustc*: a `while` loop, indexing, comparisons, an early `return`. The corrected table
(listing `ch04-04-const-table-ok.rs`) passes the same assertion, and the check costs nothing at run time:

```text
volume       1000000 cents -> 290 bps
volume      70000000 cents -> 250 bps
volume     200000000 cents -> 220 bps
volume    5000000000 cents -> 180 bps
```

The evaluator is not a simplified calculator. It interprets MIR and checks the same validity rules Miri checks (listing
`ch04-07-const-eval-ub.rs`, verified):

```rust,compile_fail
const FLAG_BYTE: u8 = 2; // e.g. a byte copied from a wire format

// SAFETY (deliberately violated): `bool` must be 0 or 1; the compiler's evaluator catches this.
const FLAG: bool = unsafe { std::mem::transmute::<u8, bool>(FLAG_BYTE) };
```

```text
error[E0080]: constructing invalid value of type bool: encountered 0x02, but expected a boolean
 --> src/main.rs:7:1
  |
7 | const FLAG: bool = unsafe { std::mem::transmute::<u8, bool>(FLAG_BYTE) };
  | ^^^^^^^^^^^^^^^^ it is undefined behavior to use this value
  |
  = note: the raw bytes of the constant (size: 1, align: 1) {
              02                                              │ .
          }
```

(Trimmed.) Compare Chapter 5.2's Miri report for an invalid enum tag: "constructing invalid value … expected a valid
enum tag." Same phrasing, same engine (§4).

**A check that runs on THIR.** Listing `ch04-05-unsafety-thir.rs` (verified):

```rust,compile_fail
unsafe fn read_raw(p: *const u64) -> u64 {
    // SAFETY: callers must pass a valid, aligned, initialized pointer.
    unsafe { *p }
}

fn main() {
    let x = 7u64;
    let v = read_raw(&x); // E0133
    println!("{v}");
}
```

```text
error[E0133]: call to unsafe function `read_raw` is unsafe and requires unsafe block
  --> src/main.rs:11:13
   |
11 |     let v = read_raw(&x); // E0133
   |             ^^^^^^^^^^^^ call to unsafe function
```

The unsafety checker walks THIR [RUSTC] (rustc-dev-guide, *The THIR*). It needs the resolved callee (is it an
`unsafe fn`?), which HIR doesn't have yet, and the nesting of `unsafe` blocks, which MIR no longer has. THIR has both.
The same reasoning puts exhaustiveness checking (E0004, Chapter 5.1) on THIR.

---

## Pass 2 · Systems level — *From tree to graph*

### 4. Under the hood

**Building THIR.** [RUSTC] The `thir_body` query combines a body's HIR with its `TypeckResults` (18.3) and produces a
tree in which nothing is implicit any more:

- Every **adjustment** recorded by type checking becomes a node: auto-refs, auto-derefs, reborrows, unsizing, deref
  coercions. In Chapter 18.7's MIR you'll see `&x` passed as a `&str` turn into an explicit call to
  `<String as Deref>::deref`. That call came from an adjustment made explicit here.
- **Method calls** become calls to a specific function (`DefId` plus generic arguments). **Overloaded operators**
  (`a + b` on a `Money` type, `v[i]` on a `Vec`) become calls to `Add::add` and `Index::index`, while primitive `+`
  and indexing stay built-in operations.
- **Patterns** carry their types and binding modes, which is what exhaustiveness checking (`rustc_pattern_analysis`,
  the usefulness algorithm Chapter 5.1 described) needs.

You can print THIR on a local nightly (`rustc +nightly -Zunpretty=thir-tree file.rs`, or `thir-flat`; not verified
here, since the Playground doesn't pass `-Z` flags). It's verbose, which is exactly the point: it's the fully explicit
program.

**Building MIR.** [RUSTC] `mir_built` walks THIR and emits basic blocks. Two mechanisms do most of the work:

- **Places and temporaries.** Every intermediate value gets a local. An expression like `k.len() + 1` becomes a
  `_7 = &_5` borrow, a call into `_6`, an `AddWithOverflow` into `_8`, and a move out of `_8.0`, which is why MIR has so
  many more locals than your code has variables.
- **Scopes and scheduled drops.** The builder keeps a stack of scopes (blocks, statements, `match` arms, temporaries'
  scopes). Each scope records the values that must be dropped when it's exited. Every way out (falling off the end,
  `break`, `return`, `?`, and **unwinding** from any call) gets drop terminators for everything in scope, in reverse
  order. The unwind exits become the `(cleanup)` blocks. This is where Chapter 3.5's rules become code: a temporary in
  a `let` statement is scheduled in the statement's scope (so `let _ = acquire()` drops at the `;`), and a temporary in
  a `match` scrutinee is scheduled in the **whole `match` expression's** scope (so it lives through the arms).

**The MIR pipeline.** [RUSTC] (rustc-dev-guide, *MIR queries and passes*; names as of 2026, and they change.) Built MIR
passes through a sequence of queries, each a snapshot:

```text
 mir_built        straight from THIR
 mir_promoted     constant-like temporaries (&5, &[1, 2, 3]) promoted to statics: "rvalue static promotion"
 mir_borrowck     (18.5) borrow checking reads the promoted MIR; it does not transform it
 drop elaboration drops of maybe-moved places become conditional (drop flags) or disappear
 optimizations    inlining, GVN, constant/copy propagation, jump threading, dead-store elimination,
                  SimplifyCfg, and more (only some run at opt-level 0)
 optimized_mir    what codegen and the Playground's "MIR" button show
 mir_for_ctfe     a separate copy for const evaluation, taken before the runtime optimizations
```

Two consequences you've already met. **Promotion** explains Part VIII's tooling note: `&[Duration::from_millis(5)]`
isn't promoted, because outside const contexts rustc promotes only expressions that can't fail or panic, and a function
call (even to a `const fn`) could [LANG] (RFC 3027, "infallible promotion"). So the temporary dies at the end of the
statement (E0716). **The Playground's MIR is the end of this pipeline**, *after* borrow checking and elaboration.
When you want to see what the borrow checker saw, you need `-Z dump-mir` locally (the output's own `HINT:` line), not
the Playground (not verified here).

**Drop elaboration.** [RUSTC] Built MIR contains a `drop` wherever a value *might* need dropping. Elaboration uses a
dataflow analysis of which places are initialized at each point:

- **Definitely initialized:** keep a plain `drop`.
- **Definitely moved:** delete the drop. In `cost`, `_1` was moved out of in every arm that had anything to drop, so no
  `drop(_1)` survives.
- **Maybe initialized** (moved on some paths only): add a **drop flag**, a hidden `bool` local set on initialization,
  cleared on move, tested before the drop. That's the `_5 = const true` you read in Chapter 3.1's MIR.
- **Partially moved** (a struct with some fields moved out): drop the remaining fields individually ("open drops").

**Const evaluation is a MIR interpreter.** [RUSTC] `rustc_const_eval` executes `mir_for_ctfe` bodies on a virtual
machine with its own memory model: every allocation is an abstract block of bytes with per-byte initialization state
and pointer **provenance** (Chapter 15.2). Miri is built on the same engine (Chapter 4.6 used it; Part XV relies on it),
which is why the invalid-`bool` error reads like a Miri report. You can see an interpreter allocation in Chapter
18.7's MIR: the string literal `"meridian"` printed as `alloc1 (size: 8, align: 1) { 6d 65 72 69 64 69 61 6e }`.

The debug output of `cost` also contained this, which we trimmed earlier:

```text
// MIR FOR CTFE
fn Cmd::Get(_1: String) -> Cmd {
    let mut _0: Cmd;

    bb0: {
        _0 = Cmd::Get(move _1);
        return;
    }
}
```

Enum-variant constructors are `const fn`s [LANG], so rustc keeps a separate body for const evaluation: `mir_for_ctfe`
from the pipeline above.

What the evaluator enforces [LANG, with [RUSTC] wording]: only `const fn` calls, no heap allocation on stable
[VERSION], no I/O, UB is an error rather than a behavior, and a const's final value must be valid for its type. What it *doesn't* enforce
is a fixed step count. On 1.98.1 an evaluation that runs very long trips a deny-by-default lint instead (listing
`ch04-10-const-eval-long.rs`, a 50-million-iteration loop):

```text
error: constant evaluation is taking a long time
  = note: this lint makes sure the compiler doesn't get stuck due to infinite loops in const eval.
          If your compilation actually takes a long time, you can safely allow the lint
  = note: `#[deny(long_running_const_eval)]` on by default
```

(Trimmed.)

> **What actually happens?** Why doesn't a type error in some *other* function stop MIR building for this one, and
> why does MIR exist for bodies that later fail borrow checking? Because each of these is a per-body query (18.1).
> `mir_built(cost)` depends on `thir_body(cost)`, which depends on `typeck(cost)`. If `typeck(cost)` is tainted by
> errors, the chain stops for `cost` only. That's Experiment 2 from Chapter 18.1, seen from the MIR side.

### 5. Memory

- **MIR is stored in crate metadata** [RUSTC] for generic functions and `#[inline]` functions, so downstream crates can
  instantiate or inline them (Chapter 2.1's `.rlib` contents, Chapter 7.1's instances). Everything a library exports
  generically ships as MIR. That's one reason a generic-heavy dependency makes *your* crate's compile slower: you
  compile its MIR.
- **THIR is transient.** It's built per body, consumed by MIR building (and the checks), then freed. MIR is kept: it's
  needed by borrow checking, optimization, codegen, const evaluation, and metadata.
- **Locals are stack slots, and storage markers let slots overlap.** Each MIR local that survives optimization gets a
  stack slot (an `alloca` in Chapter 18.7's LLVM IR). `StorageLive`/`StorageDead` bracket when a slot holds a value, so
  two locals never live at the same time can share memory. That's one reason (along with register allocation keeping
  many values out of memory entirely) why the recursive frame in the Part IX interlude was 96 bytes in release and 224
  in debug.
- **Const evaluation's memory is compile-time memory.** A `static` built by a `const fn` over a large table is
  evaluated in the compiler's interpreter (slower than native code by orders of magnitude, order of magnitude,
  labeled). The result is written into the binary's read-only data.

### 6. CPU / OS

MIR work is per body and runs in the front end's mostly single-threaded part (18.1). Two design choices trade compile
time between rustc and LLVM:

- **MIR optimizations reduce LLVM's input.** Inlining `String::len` in MIR means every generic instance that calls it
  reaches LLVM already simplified, once per MIR body rather than once per monomorphized copy (Chapter 7.3 counted how
  fast the copies multiply). Compile time was one of the motivations given for rustc's MIR inliner [RUSTC]. The release
  MIR of `cost` shows it at work.
- **Const evaluation moves work from run time to build time.** A validated table costs the build a few milliseconds of
  interpretation and costs the running service nothing. An accidentally huge const computation costs every build, on
  every developer's machine, until someone notices. The lint above is the backstop.

At run time, what MIR decided is simply the shape of the machine code: where the drop calls are, where the unwind
paths (landing pads, Chapter 8.3) go, which checks survived.

---

## Pass 3 · Architect level — *Using the middle of the compiler*

### 7. Trade-offs

**Which artifact answers which question?** The most practical lesson of this Part:

| Question | Best artifact | Why not the others |
|---|---|---|
| When exactly is this value dropped? Which path drops it? | **MIR** (debug) | LLVM IR spreads the drop across landing pads; asm inlines the drop glue |
| Is this match exhaustive, and how is it dispatched? | MIR (`switchInt`, `unreachable`) | Asm may turn it into a table or a chain (Chapter 2.5) |
| Did the overflow/bounds check survive? | MIR for the check, asm for whether LLVM removed it | MIR shows the check as written; only asm shows the final result |
| Did a function get inlined or merged? | Release **asm** (plus MIR for the MIR inliner) | MIR inlining is only the first round; LLVM inlines far more |
| Which attributes (`noalias`, `readonly`) does LLVM get? | **LLVM IR** | MIR has no attributes |
| What does a macro or desugaring produce? | Expansion / **HIR** (18.2) | By MIR the structure is gone |
| Is this constant computed at compile time? | A `const` item or `const { }` block (a language guarantee) | Asm can show folding, but folding isn't a promise |

**When to validate at compile time:**

| Mechanism | Runs | Can read | Fails | Good for |
|---|---|---|---|---|
| `const _: () = assert!(…)` over `const` data | In rustc's interpreter | Only constants in the source | The build | Invariants of tables and layouts that ship with the code |
| `build.rs` | Before compiling the crate | Files, environment | The build | Generated code, schemas, data files |
| Startup validation | At process start | Anything | The deployment (crash loop) | Configuration that changes without a rebuild |
| `LazyLock` | First use | Anything | The first request | Expensive tables you don't always need |

> **Why not make everything `const`?** Const evaluation is deliberately restricted: no I/O, no allocations that escape
> into the value, only `const fn`s. Those limits are what make its result reproducible and safe to bake into a binary.
> Data that changes more often than your deploys (limits, routing weights, fee schedules negotiated per merchant)
> belongs in configuration validated at startup, not in a constant.

### 8. Java comparison

javac compiles to **bytecode**, and the comparison with MIR is instructive because the two look alike (typed,
low-level, one method at a time) and have opposite roles:

| | Java bytecode | MIR |
|---|---|---|
| Role | The **distribution format**: the stable contract between compiler and JVM (JVMS) | An **internal** IR: unstable, never shipped as a program, only inside `.rlib` metadata |
| Shape | Stack machine: operands pushed and popped | Places and assignments on a CFG of basic blocks |
| Cleanup | `try`/`finally` compiled to exception tables, `finally` blocks duplicated per exit | Scheduled drops per exit, `(cleanup)` blocks per unwind edge |
| Who checks it | The JVM's **bytecode verifier**, at class-load time | The borrow checker and other passes, at compile time; nothing at run time |
| Optimized by | The JIT, at run time (C1/C2, Graal) | MIR passes, then LLVM, at build time |
| Compile-time evaluation | Constant folding of `static final` primitives and strings (JLS §15.29); `static {}` initializers run at class init | `const` items and `const fn`s run in the compiler's interpreter; `static` initializers are compile-time values |

The drop story is the deepest difference. In Java, `try (var lease = acquire()) { … }` makes the release explicit and
structured, and forgetting the `try` is a leak (Chapter 1.3's connection-pool incident). In Rust, the release is
scheduled for you, and MIR shows exactly where. The risk moves from *forgetting to release* to *releasing earlier than
you think*, which is the `let _ =` case.

> **Analogy limit.** "MIR is Rust's bytecode" is true for *what it looks like* and false for *what it's for*. Bytecode
> is a stable, verified, runnable artifact that outlives the compiler that produced it. MIR is a compiler-internal
> snapshot that exists only while rustc runs (and inside metadata for the same compiler version). You can't ship it,
> nothing verifies it at run time, and its printed form changes between releases, as its own header says.

### 9. Production scenario

**Fee tiers that can't ship broken.** Meridian's marketplace fees used to live in a Java YAML file loaded by the
billing service and validated at startup: tiers sorted by threshold, basis points between 0 and 1,000, no gaps. When
payments-core (Chapter 8.4) took over fee calculation, the team split the data by how often it changes:

- The **standard tier table** changes a few times a year, through a pull request reviewed by finance. It became a
  `const FEE_TIERS` table in payments-core with `const _: () = assert!(…)` checks for ordering, the 0–1,000 basis-point
  range (Chapter 2.2's basis-points lesson), and a first tier starting at 0: exactly listing `ch04-03`'s pattern. A
  mis-ordered table now fails the pull request's build with the message the author wrote.
- **Per-merchant negotiated rates** change daily. They stayed in the database, validated when loaded, with an alert
  on rejection, because a constant can't change without a deploy.

The team also adopted a review habit from this chapter's §7 table. When a question is about *when* something happens
(a drop, a lock release, a check), the reviewer asks for the debug MIR of the function, not an opinion. It takes one
Playground or local `--emit=mir` run and settles arguments that "it looks right" can't.

### 10. Failure scenario

**The payout lease that released itself.** Meridian's payouts service (Rust, introduced in 2026 to pay out marketplace
sellers) runs a scheduler on three replicas. For each seller, a replica takes a lease row in the database (a
compare-and-set, Chapter 5.4's pattern), computes the payout, and calls the bank's transfer API. The lease type
released its row in `Drop`, and was marked `#[must_use]`.

A refactor changed the call site from

```rust,ignore
let lease = leases.acquire(seller_id)?;
```

to

```rust,ignore
let _ = leases.acquire(seller_id)?;
```

because `lease` was no longer read after the payout logic moved into a helper, and the compiler had started warning
about an unused variable. The lease was now released at the end of the statement, before the payout computed.
Whenever two replicas' schedules overlapped, both paid the same seller. The bank-side idempotency keys (Chapter 8.4)
rejected most duplicates, because they were derived from the payout run. A handful of retries generated new keys and
went through. Reconciliation found the double transfers two days later.

What made it easy to write, in this chapter's terms:

- **`#[must_use]` doesn't catch it.** Explicitly discarding a value counts as using it. Listing
  `ch04-09-must-use-let-underscore.rs` shows that `let _ = acquire();` compiles silently, while `acquire();` warns, and
  the warning's own help text suggests the bug:

  ```text
  warning: unused `Lease` that must be used
   --> src/main.rs:13:5
    |
  13 |     acquire(); // warning: unused `Lease` that must be used
    |     ^^^^^^^^^
    |
  help: use `let _ = ...` to ignore the resulting value
  ```

- **`let_underscore_lock` doesn't catch it either.** That lint (deny by default, Chapter 1.3) knows about lock guards,
  not about your lease type.
- **The MIR shows it immediately.** `drop(_2)` as the terminator right after `acquire` (the listing in §3).

The fixes were three:

1. The call site went back to a named binding, `let _lease = …`, and the payout helper now takes `&Lease` as a
   parameter, so a leaseless call doesn't compile.
2. CI enables the allow-by-default lint `let_underscore_drop` (listing `ch04-06-let-underscore-lint.rs`), which catches
   exactly this:

   ```text
   warning: non-binding let on a type that has a destructor
     --> src/main.rs:24:5
      |
   24 |     let _ = acquire(&held);
      |     ^^^^^^^^^^^^^^^^^^^^^^^
      |
   help: consider binding to an unused variable to avoid immediately dropping the value
      |
   24 |     let _unused = acquire(&held);
      |          ++++++
   help: consider immediately dropping the value
      |
   24 -     let _ = acquire(&held);
   24 +     drop(acquire(&held));
      |
   ```

   It also flags harmless cases, so the team allows it locally where a drop is intended and writes `drop(x)` to say so.
3. The lease's correctness no longer depends on the process: leases expire server-side (Chapter 3.5's design exercise:
   a `Drop` backstop plus server-side expiry), and the payout run checks its lease is still held before the transfer.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVIII).*

1. What does THIR contain that HIR doesn't, and what does MIR lose that THIR had? Name one check that runs on each.
2. Read a MIR `call` terminator with `[return: bb5, unwind: bb9]`. What is `bb9`, and what happens there?
3. Why does `let _ = acquire();` drop immediately while `let _l = acquire();` doesn't? Point to where the difference
   appears in MIR.
4. What does drop elaboration do? Describe the three cases and the one that costs a run-time flag.
5. Why is the Playground's MIR not what the borrow checker saw? What would you run to see that?
6. Explain const evaluation as an interpreter. What can a `const` initializer do, what can't it do, and how does
   rustc stop one that runs forever?
7. You need to know whether a bounds check was removed in a hot loop. Which artifact do you read, and why not MIR?
8. Compare MIR with Java bytecode: two similarities and three differences that matter to an architect.

### 12. Exercises

- **Beginner.** Emit the debug MIR of a function with `if let Some(x) = opt { … } else { … }` and one with
  `match opt { Some(x) => …, None => … }`. Identify the `discriminant`, the `switchInt`, and the downcast projection in
  each. Are they the same MIR?
- **Intermediate.** Write a function that moves a `String` out of a local on one branch only, then emit its debug MIR.
  Find the drop flag, every place it's set and tested, and explain why the release asm doesn't need it (Chapter 3.1).
- **Advanced.** Write a `const fn` that validates a `[&str; N]` table of route prefixes (non-empty, starting with `/`,
  unique), wire it into a `const _: () = assert!(…)`, and make it fail with a useful message. What string operations
  can't you use in a `const fn` on 1.98.1, and how do you work around them?
- **Systems.** Take listing `ch04-01-match-mir.rs`, add a fourth variant with a `Vec<u8>` payload that the `match`
  ignores (`Cmd::Blob(_) => 0`). Predict the new `drop` terminators and cleanup blocks, then check with the Playground.
  What changed in drop elaboration, and why?
- **Architecture.** List five invariants of a service you know that could be checked at compile time, five that must
  be checked at startup, and five that can only be checked at run time. For each compile-time one, say whether it
  should be a `const` assertion, a type (Part V), or a `build.rs` check.

### 13. Debugging exercise

A retry path in the settlement batcher occasionally logs "lock is still held" and loses retry counts. The reduced
program is listing `ch04-08-match-guard.rs` (verified):

```rust
use std::sync::Mutex;

struct State {
    status: u8,
    retries: u32,
}

fn bump(m: &Mutex<State>) {
    match m.try_lock() {
        Ok(mut s) => s.retries += 1,
        Err(_) => println!("  bump: lock is still held"),
    }
}

#[inline(never)]
fn process(m: &Mutex<State>) -> &'static str {
    match m.lock().unwrap().status {
        0 => {
            bump(m);
            "retried"
        }
        _ => "done",
    }
}

fn main() {
    let m = Mutex::new(State { status: 0, retries: 0 });
    println!("{}", process(&m));
    println!("retries = {}", m.lock().unwrap().retries);
}
```

```text
  bump: lock is still held
retried
retries = 0
```

The debug MIR of `process`, trimmed:

```text
    bb0: { _5 = std::sync::Mutex::<State>::lock(copy _1) -> [return: bb1, unwind continue]; }
    bb1: { _4 = Result::<…>::unwrap(move _5) -> [return: bb2, unwind continue]; }
    bb2: { _3 = &_4; _2 = <std::sync::MutexGuard<'_, State> as Deref>::deref(move _3) -> [return: bb3, unwind: bb9]; }
    bb3: { switchInt(copy ((*_2).0: u8)) -> [0: bb5, otherwise: bb4]; }
    bb4: { _0 = const "done"; goto -> bb7; }
    bb5: { _6 = bump(copy _1) -> [return: bb6, unwind: bb9]; }
    bb6: { _0 = const "retried"; goto -> bb7; }
    bb7: { drop(_4) -> [return: bb8, unwind continue]; }
```

1. Which local is the `MutexGuard`, and where is it dropped? Why there and not after `bb3`?
2. The production code used `lock()` in `bump`, not `try_lock()`. What happened in production, and why was it
   intermittent (hint: `status`)?
3. Edition 2024 changed temporary scopes for `if let` (Chapter 3.5). Does rewriting the `match` as `if let 0 = …`
   help? Check your answer against the edition guide, then on the Playground.
4. Give two fixes, and say which one you'd make a review rule.

### 14. Design exercise

**A compile-time-checked routing table for the gateway.** The Meridian gateway (Chapter 1.2's ~120 upstreams) keeps
its route table in a YAML file reloaded every ten minutes (Chapter 3.1's refresh). A proposal says: "generate a Rust
`const` table from the YAML in `build.rs`, validate it with `const` assertions, and ship it in the binary: no bad route
can ever reach production."

Evaluate it. Cover what can and can't be validated in a `const` context (think about regexes, upstream health, and
cross-references between routes), what happens to the ten-minute refresh and to emergency changes, build-time cost and
the `long_running_const_eval` lint, and the rollback story. Propose the split you'd actually ship between compile-time
checks, `build.rs`, startup validation, and run-time validation, and name the failure mode your design accepts.
