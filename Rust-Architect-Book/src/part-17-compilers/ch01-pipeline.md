# Chapter 17.1 — The Compiler Pipeline End to End

> **Where this sits:** Part XVII · Compilers · chapter 1 of 8
> **Prerequisites:** Chapter 2.3 (`let x = 10` through MIR, LLVM IR, and assembly); Chapter 1.4 (the anatomy of compile
> time). Parts II–IX are assumed.
> **After this chapter you can:** name every stage of a compiler and what it knows, decides, and forgets; predict which
> stage rejects a given mistake; read one function at four of rustc's levels; and choose between an interpreter, a VM,
> a JIT, and ahead-of-time compilation for a language you're asked to build.

---

## Pass 1 · User level — *A compiler is a chain of translations*

### 1. Problem

As a Java engineer you use two compilers every day and see neither. `javac` turns source into bytecode, and HotSpot's
JIT compilers turn hot bytecode into machine code while your service runs. In Rust there's one compiler, it runs before
deployment, and everything the JIT would have done at run time happens at build time. That's why this book has kept
asking rustc for its intermediate output: rustc's decisions *are* your program's performance, its error messages *are*
its correctness tooling, and its structure *is* your compile time (Chapter 1.4).

There's a second reason to learn this material. Sooner or later an architect is asked to build a language: a rule
engine for risk, a query filter, a configuration language with expressions, a templating system. Most of those are
built badly: as string splitting, a `match` on the first word, and a runtime evaluator that finds mistakes at 2 a.m.
The techniques in this Part are the difference between that and a small, correct compiler that finds mistakes when a
rule is uploaded.

This chapter builds the whole pipeline once, small enough to read in one sitting, and then maps it onto rustc.

### 2. Mental model

A compiler is a **chain of translations**. Each stage consumes one representation of the program and produces another
that is closer to the machine. Each stage **knows** something the previous one didn't, **decides** something, and
**forgets** something the next one doesn't need.

```text
  source text        "let mut i = 0; while i < 10 { ... }"
      │  LEXER        knows: character classes        decides: token boundaries       forgets: whitespace, comments
  tokens             Let Mut Ident("i") Assign Int(0) Semi ...
      │  PARSER       knows: the grammar              decides: nesting, precedence    forgets: parentheses, `;`
  AST                (while (< i 10) (set i (+ i 1)) ...)
      │  RESOLVER     knows: scopes, declarations     decides: which `i` each use is  forgets: names
  AST + side table   i -> slot 0, total -> slot 1
      │  TYPE CHECK   knows: types of every binding   decides: is each op legal       forgets: nothing yet
  typed AST          slot types [Int, Int]; result Int
      │  (Rust: BORROW CHECK on MIR: are the ownership rules respected?)
      │  LOWERING     knows: control flow             decides: jumps, evaluation order forgets: nesting, `if`, `while`
  IR (CFG / SSA)     basic blocks, explicit jumps
      │  OPTIMIZER    knows: data flow, dominance     decides: cheaper equivalents    forgets: dead code, redundancy
  optimized IR
      │  CODEGEN      knows: the target ISA and ABI   decides: instructions, registers forgets: variables
  machine code       lea rax, [rdi + 2*rdi]; inc rax; ret
      │  LINKER       knows: every object file        decides: addresses              forgets: symbol boundaries
  executable
```

Three terms structure the rest of the Part:

- The **front end** (lexer through type checking, and in Rust borrow checking) is about *meaning*: is this a valid
  program, and what does it mean? Almost every error message a user sees comes from here.
- The **middle end** (IR, optimization) is about *equivalence*: which cheaper program means the same thing?
- The **back end** (instruction selection, register allocation, emission, linking) is about *the machine*: which
  instructions, which registers, which addresses?

A useful rule for predicting errors: **each mistake is reported by the earliest stage that has the information to
detect it.** A stray `#` is a lexer error because the lexer is the first stage that looks at characters. An undefined
variable is a resolver error because names only get meaning there. Division by a variable that happens to be zero isn't
a compile-time error at all, because no stage has that information.

### 3. Rust code

Listing `ch01-01-pipeline.rs` is a complete compiler for a subset of Ore (this Part's running language): `let`,
`let mut`, assignment, `while`, `if`/`else`, integer and boolean arithmetic. It has six stages in about 600 lines, and
each stage is a function from one representation to the next. The driver shows the chain (excerpt with the stage
comments added; the full listing is verified):

```rust,ignore
fn compile(src: &str, verbose: bool) -> Result<Compiled, CompileError> {
    let toks = lex(src)?;                                  // stage 1: bytes -> tokens
    let program = Parser { toks, at: 0 }.program()?;       // stage 2: tokens -> AST
    let mut res = Resolver { scopes: vec![HashMap::new()], slots: Vec::new(), uses: HashMap::new() };
    res.stmts(&program.stmts)?;                            // stage 3: names -> slots (a side table)
    res.expr(&program.result)?;
    let mut checker = Checker { res: &res, slot_ty: vec![None; res.slots.len()] };
    checker.stmts(&program.stmts)?;                        // stage 4: types
    let t = checker.expr(&program.result)?;
    let mut low = Lowerer { res: &res, code: Vec::new() };
    low.stmts(&program.stmts);                             // stage 5: AST -> stack bytecode
    low.expr(&program.result);
    low.code.push(Op::Halt);
    Ok(Compiled { code: low.code, nslots: res.slots.len() })
}
```

(The real `compile` also prints each stage's output when `verbose` is set; that code is elided here.) Stage 6 is the
"machine": a stack VM that runs the bytecode. Compiling this program:

```text
let mut i = 0;
let mut total = 0;
while i < 10 {
    i = i + 1;
    if i % 2 == 0 { total = total + i; } else { total = total - 1; }
}
total
```

prints every intermediate representation (real output, trimmed in the middle):

```text
[lexer]    49 tokens: Let Mut Ident("i") Assign Int(0) Semi Let Mut Ident("total") Assign Int(0) Semi ...
[parser]   AST:
    (let mut i 0)
    (let mut total 0)
    (while (< i 10)
      (set i (+ i 1))
      (if (== (% i 2) 0)
        (set total (+ total i))
       else
        (set total (- total 1))
      )
    )
    result: total
[resolver] i -> slot 0 (mut), total -> slot 1 (mut); 12 name uses resolved
[types]    ok; slot types [Int, Int]; result Int
[lowering] 30 instructions:
     0: Push(0)
     1: Store(0)
     ...
     4: Load(0)
     5: Push(10)
     6: Bin(Lt)
     7: JumpIfFalse(28)
     ...
    27: Jump(4)
    28: Load(1)
    29: Halt
[vm]       result = 25 after 205 instructions
```

Read the stages as successive loss of structure. The AST still has `while` and `if`. The bytecode has only jumps:
`while` became a test at instruction 4 and a `Jump(4)` at 27, and `if` became a `JumpIfFalse(23)` and a `Jump(27)`. The
names `i` and `total` are gone, replaced by slots 0 and 1.

The same driver on six broken programs shows the "earliest stage with the information" rule (real output):

```text
let x = 5 # 3; x           => [lexer] error at byte 10: unexpected character '#'
let x = 5 let y = 6; x     => [parser] error at byte 10: expected `;`, found Let
let x = 5; y + 1           => [resolver] error at byte 11: cannot find `y` in this scope
let x = 1; x = 2; x        => [resolver] error at byte 11: cannot assign twice to immutable `x`
let x = 5; x + true        => [types] error at byte 13: Add needs Int operands, found Int and Bool
let z = 0; 10 / z          => compiled fine; [vm] division by zero at pc 4
```

The last line is the important one. Nothing in this compiler tracks values, so a division by a variable that holds zero
passes every stage and fails when it runs. A compiler with constant propagation (Chapter 17.7) could see that `z` is 0
here. In general, though, "will this ever divide by zero?" is undecidable: Rice's theorem says no algorithm answers any
non-trivial question about what every program does. Compilers work with safe approximations, and each language decides
which approximation to impose. Rust's borrow checker is a famous example of one: it rejects some correct programs so that
it can accept only safe ones (Chapter 4.2).

> **What actually happens?** The Ore VM in this listing is the same *kind* of machine as the JVM: a stack machine
> interpreting bytecode. `Load(0) Push(10) Bin(Lt) JumpIfFalse(28)` corresponds to JVM bytecode like `iload_0 bipush 10
> if_icmpge L28`. Java stops here and hands the bytecode to a JIT at run time. Rust keeps going at build time: it lowers
> to MIR, then LLVM IR, then machine code, as §4 shows.

---

## Pass 2 · Systems level — *rustc's version of the chain*

### 4. Under the hood

rustc's pipeline has the same shape with more stages and more precise names. This is the map this Part and Part XVIII
use (internal crate names are [RUSTC] and change between versions; the stages are stable concepts):

| Stage | rustc (as of 2026) | Representation | What it adds |
|---|---|---|---|
| Lexing | `rustc_lexer` (standalone), wrapped by `rustc_parse` | tokens | kinds, lengths; then spans and interned symbols |
| Parsing | `rustc_parse` (hand-written recursive descent) | AST (`rustc_ast`) | nesting, precedence, spans |
| Expansion + resolution | `rustc_expand`, `rustc_resolve` | expanded AST | macros expanded; every path resolved to a definition |
| Lowering | `rustc_ast_lowering` | HIR | desugaring (`for`, `?`, `async`, `while let` become simpler forms) |
| Type checking | `rustc_hir_typeck`, trait solver | HIR + `TypeckResults` | a type for every expression; method resolution; coercions |
| THIR, MIR building | `rustc_mir_build` | THIR, then MIR | explicit control flow, drops, and temporaries |
| Borrow checking | `rustc_borrowck` | MIR | the ownership proof (Part IV) |
| MIR optimization | `rustc_mir_transform` | MIR | inlining, simplification, constant propagation |
| Monomorphization | collector + partitioning | mono items in CGUs | concrete instances of generics (Part VII) |
| Codegen | `rustc_codegen_llvm` (or Cranelift, GCC) | LLVM IR | then LLVM's optimizer and instruction selection |
| Linking | the system linker (`cc`, or `lld`) | executable | addresses, relocations (Part XIX) |

The front end isn't a straight pipeline. rustc is **demand-driven**: a *query* such as "the type of this function" or
"the optimized MIR of this function" runs the stages it needs on demand and caches the result, which is what makes
incremental compilation possible [RUSTC] (Chapter 18.1). For one function, though, the conceptual order above holds.

Listing `ch01-02-four-levels.rs` is one tiny function, compiled by rustc 1.98.1 and viewed at four levels with
`tools/emit.ps1`:

```rust,ignore
#[inline(never)]
pub fn scale(x: i64) -> i64 {
    let y = x * 3;
    y + 1
}
```

**HIR** (nightly `-Zunpretty=hir`; macros expanded, attributes normalized, otherwise close to the source):

```text
#[attr = Inline(Never)]
fn scale(x: i64) -> i64 { let y = x * 3; y + 1 }
```

**MIR, debug build.** Names became numbered locals (`_1` is `x`, `_2` is `y`), and the arithmetic became checked
operations with explicit control flow:

```text
fn scale(_1: i64) -> i64 {
    debug x => _1;
    let mut _0: i64;
    let _2: i64;
    let mut _3: (i64, bool);
    let mut _4: (i64, bool);
    ...
    bb0: {
        _3 = MulWithOverflow(copy _1, const 3_i64);
        assert(!move (_3.1: bool), "attempt to compute `{} * {}`, which would overflow", copy _1, const 3_i64) -> [success: bb1, unwind continue];
    }
    bb1: {
        _2 = move (_3.0: i64);
        _4 = AddWithOverflow(copy _2, const 1_i64);
        assert(!move (_4.1: bool), "attempt to compute `{} + {}`, which would overflow", copy _2, const 1_i64) -> [success: bb2, unwind continue];
    }
    bb2: {
        _0 = move (_4.0: i64);
        return;
    }
}
```

**MIR, release build.** Overflow checks are off in the release profile (Chapter 2.2), so the three blocks became one:

```text
    bb0: {
        StorageLive(_2);
        _2 = Mul(copy _1, const 3_i64);
        _0 = Add(copy _2, const 1_i64);
        StorageDead(_2);
        return;
    }
```

**LLVM IR, release.** SSA values with the source names kept as hints:

```text
define noundef i64 @_RNvCsjau7DNBNlby_10playground5scale(i64 noundef %x) unnamed_addr #0 {
start:
  %y = mul i64 %x, 3
  %_0 = add i64 %y, 1
  ret i64 %_0
}
```

**Assembly, release.** Instruction selection found that `x * 3` is `x + 2x`, which fits one `lea`:

```text
playground::scale:
	lea	rax, [rdi + 2*rdi]
	inc	rax
	ret
```

**Assembly, debug.** The same function is 21 instructions with spills to the stack, two `seto`/`jo` overflow tests, and
two calls to `panic_const_*_overflow` (excerpt):

```text
playground::scale:
	sub	rsp, 40
	mov	qword ptr [rsp + 24], rdi
	mov	eax, 3
	imul	rdi, rax
	mov	qword ptr [rsp + 16], rdi
	seto	al
	jo	.LBB0_2
	...
```

Each level forgot something. MIR forgot the names (they survive only as debug info), LLVM IR forgot Rust's types beyond
their machine representation (`i64`), and the assembly forgot the variable `y` entirely: it never exists in a register of
its own. Chapter 2.3 made the same observation for `let x = 10`. This Part explains *how* each translation is done.

### 5. Memory

A compiler is a memory-heavy program because every stage builds a new representation, and many of them stay alive at
once. For one function rustc may hold the AST, the HIR, the typeck results, THIR, several versions of MIR, and LLVM IR.
Three design choices keep that manageable, and all three appear in this Part's listings:

- **Side tables instead of mutating trees.** Listing 17.1-1's resolver doesn't rewrite the AST. It records
  "the name at byte offset *p* means slot *s*" in a `HashMap<usize, usize>`, and later stages look answers up there.
  rustc does the same at scale: type-check results are tables keyed by node id (`TypeckResults`, keyed by `HirId`)
  [RUSTC]. Chapter 17.4 discusses why.
- **Arenas.** Trees of `Box`es cost one allocation per node. rustc allocates HIR, types, and many other structures in
  arenas that live for the whole compilation session, and refers to them with lifetimes like `'tcx` [RUSTC]. Chapter 17.3
  measures the difference: 1,500,001 allocations for a `Box` tree against 21 for a `Vec` arena, for the same 1.5 million
  nodes.
- **Interning.** Identifiers become 4-byte symbols and types become pointers into an interned table, so comparisons are
  integer or pointer comparisons (Chapter 17.4 measures a 4-byte `Symbol` against a 24-byte `String`).

### 6. CPU / OS

Compilation is CPU-bound and has an unusual profile: a lot of pointer chasing (walking trees and graphs), many
unpredictable branches (a `match` on node kinds per node), and hash-table lookups for every name. Chapter 17.3 shows that
where nodes live affects parse time by about 2× (one Playground run).

Parallelism comes late in the pipeline. rustc's front end is largely single-threaded per crate on stable (a parallel
front end exists behind the nightly `-Z threads` flag [VERSION]), while code generation is split into codegen units that
LLVM compiles on separate threads (Chapter 7.3). From the operating system's point of view, `cargo build` is a tree of
processes: cargo, one `rustc` per crate (in parallel across independent crates), LLVM threads inside each rustc, and
finally a linker process. `cargo build --timings` draws that tree, and `-Z time-passes` (nightly) prints time per stage.
Both need a local toolchain, so they aren't verified here:

```text
not verified here: requires a local toolchain
cargo build --release --timings          # HTML report of crate-level parallelism
cargo +nightly rustc --release -- -Z time-passes
```

---

## Pass 3 · Architect level — *where to put each check*

### 7. Trade-offs

When you build a language, the first architectural decision is **when each translation happens**. The options form a
spectrum:

| Strategy | Translation happens | Per-evaluation cost (the 10.1 benchmark) | Build complexity | Where it's used |
|---|---|---|---|---|
| Tree-walking interpreter | parse once, walk the AST per evaluation | 11.45 ns/record | lowest | config expressions, first prototypes |
| Closure compilation | walk the AST once into nested closures | 6.28 ns/record | low | rule engines, filters (Meridian's log sampler) |
| Bytecode VM | compile to bytecode, interpret it (listing 17.1-1) | not measured here | medium | Lua, Python, the JVM's interpreter |
| JIT | compile hot code to machine code at run time | close to native after warm-up | high | HotSpot, V8, LuaJIT |
| Ahead-of-time compilation | compile everything before running | 0.89 ns/record (hand-written Rust) | highest | rustc, clang, Go, GraalVM native-image |

(The three measured numbers are Chapter 10.1's, one Playground run, noisy; the other rows are qualitative.)

The second decision is **where each check lives**. The earlier a mistake is caught, the cheaper it is: at upload time
it's an error message, at 2 a.m. it's an incident. But earlier checks need more information up front. A type checker
needs a schema, and a name resolver needs to know every feature that exists. For a rule language this is the central
design question, and Meridian's answer is §9.

> **Why not just interpret JSON?** JSON gives you a free parser, so the whole compiler looks optional. But a JSON
> "rule" still has names, types, and nesting. With JSON, name resolution and type checking just happen at evaluation
> time, on every evaluation, with whatever error behavior the evaluator happens to have. §10 is what that looks like in
> production.

### 8. Java comparison

Java splits the pipeline across time. `javac` is a front end: parse, *enter* (build symbol tables for classes), process
annotations, *attribute* (resolve names and type-check), *flow* (definite assignment, reachability), *desugar* (generics
erasure, inner classes, enhanced `for`), and *generate* bytecode. It does almost no optimization. The optimizer and code
generator live in the JVM. HotSpot starts in its bytecode interpreter, profiles, compiles warm methods with C1 (fast,
lightly optimizing), and recompiles hot methods with C2 (slow, aggressively optimizing, speculative). If a speculation
turns out wrong (a new class loads and makes a call site polymorphic), it *deoptimizes* back to the interpreter.

| | Java | Rust |
|---|---|---|
| Front end | `javac`, at build time | rustc, at build time |
| Borrow checking / ownership | — | rustc, on MIR |
| Optimization | C1/C2 (or Graal), at run time, using live profiles | LLVM, at build time; profiles only via PGO (Part XX) |
| Code generation | at run time, per hot method | at build time, per codegen unit |
| Cost paid at startup | interpretation and warm-up | none (the binary is already native) |
| Can re-optimize after deployment | yes (deoptimization, recompilation) | no |

> **Analogy limit.** "rustc is javac plus C2 at build time" is close, but it misses two things. First, C2 optimizes with
> knowledge rustc will never have: the actual receiver classes at a call site, the branch probabilities of *this*
> deployment. Second, rustc's front end does work javac doesn't do at all: borrow checking and monomorphization. GraalVM
> native-image is the closer Java analogy to rustc (AOT, closed world), and it gives up dynamic class loading to get there.

### 9. Production scenario

**Meridian's Sieve.** After the onboarding rule engine's crash loop (the Part IX interlude) and a string of rule bugs,
Meridian's risk platform team started **Sieve** in 2026: a small, typed rule language shared by onboarding, fraud, and
payments risk. A rule looks like `amount > EUR 1000.00 && country == "DE"`. The design was shaped by one decision: **the
compiler runs at upload time, in a rule service, not in the services that evaluate rules.**

```text
 rule editor ──► rule service: lex ─► parse ─► resolve against the feature catalog ─► type-check ─► fold ─► store
                    (all errors reported with spans, before the rule is saved)                    compiled rule + catalog version
                                                                                                         │
 fraud / onboarding / payments services ◄─── load compiled rules (Arc swap, Chapter 11.4) ◄──────────────┘
    evaluate with closure compilation (Chapter 10.1): no parsing, no name lookup, no type checks at run time
```

The consequences are architectural, not just technical:

- **Every error class has an owner stage**, and the review of each Sieve feature asks "which stage catches misuse of
  this?" A typo in a feature name is a resolver error at upload. A string compared with an amount is a type error at
  upload. Only genuinely dynamic failures (a feature service timing out) remain for evaluation time, and they have an
  explicit policy (fail open or closed, per rule).
- **The feature catalog is the symbol table** (Chapter 17.4). Rules are stored with the catalog version they were
  compiled against, so a catalog change can't silently change a stored rule's meaning.
- **Evaluation services never see source text.** They load an already-checked representation, so a malformed rule
  can't crash them. That's the lesson of the Part IX crash loop, applied by construction.

### 10. Failure scenario

**The rule that compared strings.** Before Sieve, Meridian's payments risk team kept rules as JSON evaluated by a
Java service. One rule sent large payments to manual review:

```json
{ "gt": ["amount_minor", "100000"] }
```

The rule editor had saved the threshold as a *string*. The evaluator had a convenience coercion: if either side of a
comparison is a string, compare both as strings. So the check became a lexicographic comparison:

```text
"95000"   > "100000"   → true   ('9' > '1')      a €950 payment goes to review
"250000"  > "100000"   → true                     correct, by accident
"1000"    > "100000"   → false  (a prefix)        correct, by accident
"1500000" > "100000"   → true                     correct, by accident
```

Most large payments still went to review, so the rule looked like it worked. What changed was that many small payments
whose amounts started with a high digit went to review too. The review queue grew by about 3,100 cases over nine days
before an analyst noticed that €950 payments were being flagged as "large".

The post-incident review didn't blame the coercion so much as the *pipeline*: there was no stage between "the editor
saved some JSON" and "the evaluator ran it" that knew types. Its fixes became Sieve's requirements: a type checker that
runs at upload (Chapter 17.5), no implicit conversions between strings and numbers ever, and typed money literals.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVII).*

1. Name the stages of a compiler from source text to executable. For each, say what it knows, what it decides, and what
   it forgets.
2. "Each mistake is reported by the earliest stage that has the information." Apply it to: an unknown character, a
   missing semicolon, an undefined variable, `1 + true`, a use of a moved value in Rust, and division by a zero that
   comes from user input.
3. Why can't a compiler detect every division by zero? What *can* it do?
4. What's the difference between the front end, the middle end, and the back end? Which one produces most user-facing
   error messages, and why?
5. Walk through `scale` at four levels (HIR, MIR, LLVM IR, assembly). What disappears at each step?
6. Why does rustc use side tables keyed by node id instead of adding fields to the AST?
7. Compare a tree-walking interpreter, closure compilation, a bytecode VM, a JIT, and AOT compilation for a rule engine
   evaluating 2 million rules per second. What would you choose, and what would change your mind?
8. What can HotSpot's C2 do that rustc can't, and what does rustc do that javac doesn't?
9. What does "demand-driven" (query-based) compilation mean, and why does it matter for incremental builds?
10. As an architect, where would you put name resolution and type checking for a user-facing rule language, and why?

### 12. Exercises

- **Beginner.** Add `let z = 1; 7 % (z - 1)` and `let b = !5; b` to the bad-programs list of listing 17.1-1. Predict
  which stage reports each one, then run it and check.
- **Intermediate.** Add a constant-folding stage between type checking and lowering: fold `Binary` nodes whose operands
  are both literals. Show the bytecode before and after for `let x = 2 * 3 + 4; x`. Don't fold a division by zero; explain
  why.
- **Advanced.** Replace the stack VM with a tree-walking interpreter over the AST and measure both on the loop program
  with `i < 10_000_000` in release mode (one run, noisy). Explain the difference in terms of dispatch per operation.
- **Systems.** Emit listing 17.1-2 at `-Target llvm-ir -Mode debug`. Find where `x` and `y` live (hint: `alloca`) and
  explain how the release IR got rid of them. Which LLVM pass is responsible? (Chapter 17.6 answers this.)
- **Architecture.** Pick a configuration or rule language at your company (or Meridian's JSON rules). Draw its pipeline
  as in §2. Which stages exist, at which time (build, upload, load, evaluation), and which error classes are caught
  late because a stage is missing?

### 13. Debugging exercise

A teammate reorders `compile` in listing 17.1-1 so the type checker runs before the resolver ("types are more
important"; an unverified sketch of the change):

```rust,ignore
let program = Parser { toks, at: 0 }.program()?;
let res = Resolver { scopes: vec![HashMap::new()], slots: Vec::new(), uses: HashMap::new() };
let mut checker = Checker { res: &res, slot_ty: vec![None; res.slots.len()] };
checker.stmts(&program.stmts)?;   // moved up
// ... the resolver runs here now
```

1. It compiles. What happens at run time on the good program, and why? (Look at how `Checker::expr` finds a variable's
   type.)
2. What does this tell you about the *dependencies* between stages? Draw them as a graph for this compiler.
3. rustc interleaves some stages (macro expansion and name resolution) instead of running them in a fixed order. Why
   might two stages need each other's results?

### 14. Design exercise

**Design Sieve's compilation service.** Rules are written by about 60 analysts, change a few hundred times a week, and
are evaluated by three services at a combined 2 million evaluations per second (fraud's 50K scores/s × about 40 rules
each). Decide:

- Which stages run at upload, which at service load, and which per evaluation? What is stored: source, AST, or a
  compiled form? What version information goes with it?
- How a feature catalog change (a feature renamed, retyped, or removed) is handled for already-stored rules.
- How you roll back a bad rule, and how a rule's *compiler* is rolled back if a compiler bug is found.
- How you'd test the compiler itself (Chapter 17.3's shadow mode and Chapter 17.8's differential testing are two
  answers).

Then compare your design with the "JSON evaluated at run time" design of §10 on three axes: when errors are found, what
an evaluation costs, and what can take a production service down.
