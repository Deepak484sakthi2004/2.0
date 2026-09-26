# Chapter 17.6 — Intermediate Representations, CFGs, and SSA

> **Where this sits:** Part XVII · Compilers · chapter 6 of 8
> **Prerequisites:** Chapter 17.5 (a checked AST); Chapter 2.3 (`let x = 10` through MIR and LLVM IR; SSA and φ nodes
> introduced); Chapter 4.2 (liveness and NLL as dataflow).
> **After this chapter you can:** lower an AST to basic blocks with explicit control flow; compute dominators,
> dominance frontiers, and loops; build SSA form with φ nodes; run dataflow analyses to a fixpoint and know which way
> to start them; and read rustc's MIR and LLVM's IR as two different answers to the same design question.

---

## Pass 1 · User level — *Making control flow explicit*

### 1. Problem

An AST is a good representation for checking, and a bad one for everything that comes after. Three questions an
optimizer asks constantly are hard to answer on a tree:

- **"Which code can run after this?"** On a tree, `&&`, `||`, `if`, `while`, `return`, `break`, and `?` all hide
  jumps. In `a > 0 && b / a > 2`, the division runs only on one path, and nothing in the tree's shape says so.
- **"Where did this value come from?"** A variable assigned in a loop body, in both branches of an `if`, and before
  the loop has several definitions. Every use must be traced back through all of them.
- **"Is this true on every path, or on some path?"** Is `x` assigned before every use (Rust's E0381)? Might this value
  be used later, so it needs a register (Chapter 17.8)? Both are questions about *all paths through the program*.

Compilers answer these with an **intermediate representation** (IR): a flat list of simple instructions grouped into
**basic blocks**, connected by explicit edges, the **control-flow graph** (CFG). Most go one step further, to **static
single assignment** (SSA) form, where every value is defined exactly once. This chapter lowers Ore to a CFG, builds
its dominator tree, converts it to SSA, runs two dataflow analyses, and then reads rustc's MIR (a CFG that is *not*
SSA) and LLVM IR (which is) for the same loop.

### 2. Mental model

```text
fn sum_to(n):                              the same function in SSA
  bb0:  i = 0                               bb0:  i#1 = 0
        total = 0                                 total#1 = 0
        jump bb1                                  jump bb1
  bb1:  %0 = i < n          <- loop header  bb1:  i#2     = phi(bb0: i#1,     bb2: i#3)
        branch %0 ? bb2 : bb3                     total#2 = phi(bb0: total#1, bb2: total#3)
  bb2:  %1 = i + 1                                %0 = i#2 < n
        i = %1                                    branch %0 ? bb2 : bb3
        %2 = total + i                      bb2:  %1 = i#2 + 1
        total = %2                                i#3 = %1
        jump bb1            <- back edge          %2 = total#2 + i#3
  bb3:  return total                              total#3 = %2
                                                  jump bb1
                                            bb3:  return total#2
```

The vocabulary, all visible in that picture:

- A **basic block** is straight-line code with one entry and one exit: instructions, then one *terminator* (`jump`,
  `branch`, `return`). Control flow happens only between blocks.
- **Three-address code**: every instruction has at most one operator (`%1 = i + 1`); nested expressions become
  temporaries (`%0`, `%1`, ...).
- Block A **dominates** block B if every path from the entry to B passes through A. The entry dominates everything;
  `bb1` dominates `bb2` and `bb3`. Each block's closest strict dominator is its **immediate dominator** (idom), and the
  idoms form the **dominator tree**.
- An edge B → A where A dominates B is a **back edge**, and A is a **loop header**. That's how a compiler finds loops
  without knowing that the source said `while`.
- The **dominance frontier** of A is where A's dominance stops: the blocks A can reach but doesn't dominate. Those are
  the merge points, and that's where **φ (phi) nodes** go in SSA.
- In **SSA**, each variable has exactly one definition. A φ at the top of a block chooses a value according to which
  predecessor control came from: `i#2 = phi(bb0: i#1, bb2: i#3)` is `0` on the first entry and the incremented value
  on every later iteration.

**Dataflow analysis** computes a fact at every block boundary ("variables assigned on every path so far", "variables
possibly used later") by applying a *transfer function* per block and a *meet* where paths join, and iterating until
nothing changes (a *fixpoint*). Two dimensions classify every analysis: **forward or backward**, and **must**
(meet = intersection: true on *all* paths) **or may** (meet = union: true on *some* path).

### 3. Rust code

**Lowering.** Listing `ch06-01-cfg.rs` lowers Ore's AST to basic blocks. `while` becomes a header block holding the
test, a body block, and an exit block:

```rust,ignore
            ExprKind::While(c, body) => {
                let header = self.new_block();
                self.terminate(Term::Jump(header));
                self.cur = header;
                let vc = self.expr(c);
                let (body_b, exit) = (self.new_block(), self.new_block());
                self.terminate(Term::Branch { cond: vc, then: body_b, els: exit });
                self.cur = body_b;
                self.block(body);
                self.terminate(Term::Jump(header));
                self.cur = exit;
                Val::Const(0)
            }
```

The short-circuit operators are the reason a CFG exists. `a && b` doesn't compute both operands: it's a branch.

```rust,ignore
            // Short-circuit operators become control flow: the right operand runs only when needed.
            ExprKind::Binary(op @ (BinOp::And | BinOp::Or), a, b) => {
                let r = self.temp();
                let va = self.expr(a);
                let (rhs, short, join) = (self.new_block(), self.new_block(), self.new_block());
                let (then, els) = if *op == BinOp::And { (rhs, short) } else { (short, rhs) };
                self.terminate(Term::Branch { cond: va, then, els });
                self.cur = rhs;
                let vb = self.expr(b);
                self.emit(Inst::Copy { dst: r, src: vb });
                self.terminate(Term::Jump(join));
                self.cur = short;
                self.emit(Inst::Copy { dst: r, src: Val::Const((*op == BinOp::Or) as i64) });
                self.terminate(Term::Jump(join));
                self.cur = join;
                Val::Var(r)
            }
```

(Excerpts of listing 17.6-1.) The listing then computes dominators with the Cooper–Harvey–Kennedy iterative
algorithm ("A Simple, Fast Dominance Algorithm", 2001), dominance frontiers, and back edges. For `sum_to` the output is
the left column of the diagram above, plus (real output):

```text
  idom      bb1:bb0 bb2:bb1 bb3:bb1
  frontiers DF(bb1)={bb1} DF(bb2)={bb1}
  back edge bb2 -> bb1: bb1 is a loop header
```

`bb1` is in its own dominance frontier: it dominates `bb2`, which is a predecessor of `bb1`, but it doesn't *strictly*
dominate itself. It's also in `bb2`'s frontier. Those frontiers are where `i` and `total`, assigned in `bb0` and `bb2`,
will need φ nodes. The more
interesting function is `classify`:

```text
fn classify(a: int, b: int) -> int {
    if a > 0 && b / a > 2 { 1 } else if a == 0 || b < 0 { 2 } else { 3 }
}
```

Its CFG has 13 blocks (real output, trimmed to the first four):

```text
fn classify(a, b):
  bb0:
      %1 = a > 0
      branch %1 ? bb1 : bb2
  bb1:
      %2 = b / a
      %3 = %2 > 2
      %0 = %3
      jump bb3
  bb2:
      %0 = 0
      jump bb3
  bb3:
      branch %0 ? bb4 : bb6
  ...
```

The division `b / a` lives in `bb1`, reachable only through the `true` edge of `a > 0`. The IR interpreter confirms it
(real output):

```text
sum_to[10] = Ok(55)
fib[10] = Ok(55)
classify[0, 5] = Ok(2)
classify[2, 9] = Ok(1)
classify[3, 5] = Ok(3)
(1444 IR instructions executed; classify(0, 5) never divided by zero)
```

**SSA.** Listing `ch06-02-ssa.rs` converts the CFG to SSA with the algorithm of Cytron, Ferrante, Rosen, Wegman, and
Zadeck (1991), in two steps. Step 1 places φ nodes at the *iterated* dominance frontier of every multiply-defined
variable (a φ is itself a new definition, so it can require more φs further down):

```rust,ignore
    let mut phi_vars: Vec<Vec<u32>> = vec![Vec::new(); n];
    for (v, blocks) in defs.iter().enumerate() {
        if blocks.len() < 2 { continue; } // one definition dominates all its uses: no phi needed
        let mut work = blocks.clone();
        let mut has_def = blocks.clone();
        while let Some(b) = work.pop() {
            for &d in &df[b] {
                if !phi_vars[d].contains(&(v as u32)) {
                    phi_vars[d].push(v as u32);
                    if !has_def.contains(&d) { has_def.push(d); work.push(d); }
                }
            }
        }
    }
```

(Excerpt of listing 17.6-2.) Step 2 renames: it walks the dominator tree keeping a stack of the current version of
each variable, gives every definition a new version (`i#1`, `i#2`, ...), rewrites every use to the version on top of
the stack, fills in the φ arguments of each successor, and pops its versions on the way back up. `sum_to` becomes the
right column of the diagram. `classify` shows φ nodes for the short-circuit results (real output, trimmed):

```text
  bb3:
      %0#3 = phi(bb1: %0#1, bb2: %0#2)
      branch %0#3 ? bb4 : bb6
  ...
  bb5:
      %4#2 = phi(bb4: %4#1, bb11: %4#3)
      %5#1 = phi(bb4: 0, bb11: %5#4)
      %8#1 = phi(bb4: 0, bb11: %8#3)
      return %4#2
  ...
SSA and CFG interpreters agree on 270 runs; 9 phi nodes in total
```

`%0#3` is the value of `a > 0 && b / a > 2`: `%0#1` if control came through the division, `0` (false) from the short
path. Look at `%5#1` and `%8#1` in `bb5`. Nothing uses them, and their `bb4` argument is `0`, the listing's marker for
"undefined on this path". They're correct but useless: this is **minimal SSA**, which places a φ wherever the dominance
frontier says a merge happens, whether or not the variable is still needed. **Pruned SSA** consults liveness (below)
and skips φs for dead variables; dead-code elimination (Chapter 17.7) would also delete them. The last line is the
test oracle: an SSA interpreter (φs read their inputs on block entry) and the CFG interpreter agree on 270 runs.

**Dataflow.** Listing `ch06-03-dataflow.rs` runs the two classic analyses, with each variable a bit in a `u64`.
*Definite assignment* is forward and *must*: a variable is assigned at a point if it's assigned on **every** path
there, so paths meet by intersection. *Liveness* is backward and *may*: a variable is live if it **might** be used
later, so paths meet by union. On a diamond where only one branch assigns `x` (real output):

```text
(a) definite assignment on the diamond:
  error: `x` is possibly unassigned when used in bb3
  IN(bb0) = {c}
  IN(bb1) = {c}
  IN(bb2) = {c}
  IN(bb3) = {c}
  fixpoint after 2 rounds
```

And liveness on `sum_to`:

```text
(b) liveness:
  live-in(bb0) = {n}
  live-in(bb1) = {n,i,total}
  live-in(bb2) = {n,i,total}
  live-in(bb3) = {total}
  fixpoint after 3 rounds (the back edge bb2 -> bb1 needs a second pass)
  pruned SSA: bb1 needs phis only for its live-in, multiply-defined variables: i, total
```

`n` is live everywhere in the loop because the header tests it on every iteration; `total` is the only thing live at
the exit. Liveness is the input to register allocation (Chapter 17.8): a variable that is live needs a home.

**rustc runs the same analysis.** Definite assignment is part of borrow checking on MIR (Chapter 4.2). Listing
`ch06-04-rust-e0381.rs`:

```rust,compile_fail
fn main() {
    let x: i32;
    let c = std::env::args().count() > 5;
    if c {
        x = 1;
    }
    println!("{}", x + 1);
}
```

```text
error[E0381]: used binding `x` is possibly-uninitialized
  --> src/main.rs:10:20
   |
 5 |     let x: i32;
   |         - binding declared here but left uninitialized
 6 |     let c = std::env::args().count() > 5;
 7 |     if c {
   |        - if this `if` condition is `false`, `x` is not initialized
 8 |         x = 1;
 9 |     }
   |      - an `else` arm might be missing here, initializing `x`
10 |     println!("{}", x + 1);
   |                    ^ `x` used here but it is possibly-uninitialized
   |
   = note: when checking initialization, the compiler describes possible control-flow paths without evaluating whether branch conditions can actually have the values shown
```

The note states the defining property of dataflow analysis: it reasons about **paths**, not **values**. Listing
`ch06-07-rust-if-true.rs` takes that literally with `if true { x = 1; }`, and rustc still reports E0381 for the use,
with the same note: a constant `true` condition still has a `false` edge in the CFG.

---

## Pass 2 · Systems level — *MIR is a CFG; LLVM IR is SSA*

### 4. Under the hood

Listing `ch06-05-rustc-ir.rs` is Ore's `sum_to` written in Rust (with `black_box` so LLVM keeps the loop), emitted at
two levels with `tools/emit.ps1`.

**MIR (debug)** is a CFG of basic blocks over **mutable locals**. Excerpt (real output):

```text
    bb0: {
        _2 = const 0_i64;
        _3 = const 0_i64;
        goto -> bb1;
    }

    bb1: {
        _5 = copy _2;
        _4 = Lt(move _5, copy _1);
        switchInt(move _4) -> [0: bb6, otherwise: bb2];
    }

    bb2: {
        _6 = AddWithOverflow(copy _2, const 1_i64);
        assert(!move (_6.1: bool), "attempt to compute `{} + {}`, which would overflow", copy _2, const 1_i64) -> [success: bb3, unwind continue];
    }

    bb3: {
        _2 = move (_6.0: i64);
        _8 = copy _2;
        _7 = std::hint::black_box::<i64>(move _8) -> [return: bb4, unwind continue];
    }
```

Compare it with Ore's CFG: same shape (a header with the test, a body, a back edge), same three-address style, but
`_2` (the variable `i`) is assigned in `bb0` *and* in `bb3`. **MIR is not SSA** [RUSTC]. It's built around *places*,
memory locations with projections like `_6.0`, because its main consumers need them: borrow checking asks which
*place* a loan covers and whether a later access *to that place* conflicts (Part IV), and drop elaboration asks
whether a place is initialized on each path. In SSA a variable would dissolve into many values, and those questions
would have to be reconstructed. Notice also what MIR makes explicit that Ore's IR didn't: the overflow check is an
`assert` terminator with its own successor block, and every call has an `unwind` edge (the panic path of Chapter 8.3).

**LLVM IR (debug)** keeps MIR's model: each local is an `alloca` (a stack slot) accessed by `load` and `store`
(excerpt, real output; the `#dbg_declare` records and `!dbg` location attachments are removed):

```text
start:
  %n.dbg.spill = alloca [8 x i8], align 8
  %total = alloca [8 x i8], align 8
  %i = alloca [8 x i8], align 8
  store i64 %n, ptr %n.dbg.spill, align 8
  store i64 0, ptr %i, align 8
  store i64 0, ptr %total, align 8
  br label %bb1

bb1:                                              ; preds = %bb5, %start
  %_5 = load i64, ptr %i, align 8
  %_4 = icmp slt i64 %_5, %n
  br i1 %_4, label %bb2, label %bb6
```

**LLVM IR (release)** is SSA. The `alloca`s for `i` and `total` are gone, replaced by φ nodes (real output, trimmed):

```text
define noundef i64 @_RNvCsjau7DNBNlby_10playground6sum_to(i64 noundef %n) unnamed_addr #0 {
start:
  %_5 = alloca [8 x i8], align 8
  %_34 = icmp sgt i64 %n, 0
  br i1 %_34, label %bb2, label %bb3

bb3:                                              ; preds = %bb2, %start
  %total.sroa.0.0.lcssa = phi i64 [ 0, %start ], [ %2, %bb2 ]
  ret i64 %total.sroa.0.0.lcssa

bb2:                                              ; preds = %start, %bb2
  %total.sroa.0.06 = phi i64 [ %2, %bb2 ], [ 0, %start ]
  %i.sroa.0.05 = phi i64 [ %0, %bb2 ], [ 0, %start ]
  %0 = add nuw nsw i64 %i.sroa.0.05, 1
  ...
  %2 = add i64 %1, %total.sroa.0.06
  ...
  %exitcond.not = icmp eq i64 %0, %n
  br i1 %exitcond.not, label %bb3, label %bb2
}
```

`%i.sroa.0.05 = phi i64 [ %0, %bb2 ], [ 0, %start ]` is exactly listing 17.6-2's `i#2 = phi(bb0: i#1, bb2: i#3)`. The
names tell you which LLVM passes produced them [LIB]: **SROA** (scalar replacement of aggregates, which also does what
`mem2reg` does) promoted the stack slots `%i` and `%total` into SSA values, and `lcssa` marks *loop-closed SSA*, a
normal form that gives each value leaving a loop its own φ at the exit. Two more loop transformations are visible:

- **Loop rotation.** The test `i < n` moved: a guard `icmp sgt i64 %n, 0` runs once before the loop, and the loop
  itself tests at the bottom (`icmp eq i64 %0, %n`), a `do-while` shape that saves a branch per iteration.
- **Flags from facts.** `add nuw nsw` promises no unsigned or signed wrap. Overflow checks are off in release, so the
  flags aren't from Rust semantics; LLVM proved them: `i` starts at 0 and stays below `n`, so `i + 1` can't overflow.
  Those flags then license further optimizations (Chapter 17.7).

The one `alloca` left (`%_5`) is `black_box`'s argument, which must live in memory so the empty inline-assembly
barrier can "observe" it.

**SSA construction in production** [LIB]. Cytron et al.'s algorithm (listing 17.6-2) needs dominance frontiers
computed up front. Braun, Buchwald, Hack, Leißa, Mallon, and Zwinkau's "Simple and Efficient Construction of Static
Single Assignment Form" (CC 2013) builds SSA *on the fly* while lowering from the AST, without dominance frontiers,
and Cranelift's frontend builder uses that approach. LLVM front ends (rustc and Clang among them) take a third route:
emit memory (`alloca`) and let `mem2reg`/SROA build SSA afterwards, which keeps front ends simple.

### 5. Memory

An IR's representation decides what its optimizations cost:

- **Dense indices.** Listing 17.6-1 stores blocks in a `Vec<BasicBlock>` and variables as `u32` indices into a name
  table. MIR does the same with typed indices (`IndexVec<BasicBlock, BasicBlockData>`, `IndexVec<Local, LocalDecl>`),
  so `BasicBlock` and `Local` are 4-byte newtypes that can't be mixed up (Part V's newtype argument) [RUSTC].
- **Use lists.** Replacing every use of a value (`replace_uses` in Chapter 17.7's listing) scans the whole function in
  this Part's code. LLVM keeps a *use list* on every value (each `Value` knows its `Use`s), so `replaceAllUsesWith` costs
  time proportional to the number of uses [LIB]. It's more memory per value and much faster rewriting.
- **Bitsets for dataflow.** Listing 17.6-3 uses one `u64` per block for "the set of variables", and meets are single
  AND/OR instructions. rustc's dataflow framework uses bitsets over MIR locals and move paths (dense or chunked,
  depending on density) [RUSTC]. With thousands of locals per function, representation matters more than the
  algorithm.

SSA itself costs memory: φ nodes, and more names (one per definition instead of one per variable). Minimal SSA's
useless φs (the `%5#1` and `%8#1` above) are why production compilers build pruned or semi-pruned SSA.

### 6. CPU / OS

**Iteration counts.** Both dataflow runs converged quickly: definite assignment on the diamond in 2 rounds (the second
round confirms nothing changed), liveness on `sum_to` in 3, because the back edge `bb2 → bb1` carries information into
a block that was already processed. Visiting blocks in *reverse postorder* for forward problems (postorder for backward
ones) means that, for the bit-vector problems here, the number of rounds is bounded by the loop-nesting depth plus a
small constant (Kam and Ullman, 1976), rather than by the number of blocks. The dominator algorithm uses the same
trick: it iterates in reverse postorder and typically converges in two or three passes on real code.

**Why SSA pays for itself.** Many analyses that need iterative dataflow on a CFG become a single pass over SSA:
"where was this value defined?" is a pointer, and "is this value constant?" follows def-use edges instead of
iterating over blocks (sparse analysis). That's why almost every optimizing compiler converts to SSA before
optimizing, and why MIR's non-SSA design is a deliberate choice for *checking*, with optimization mostly left to LLVM.

**Where the IR gets built.** Lowering and SSA construction are linear-time passes, but they allocate: every block,
instruction, and name. rustc builds MIR per function on demand (the `mir_built` and `optimized_mir` queries,
Chapter 18.1), and LLVM builds its IR per codegen unit, which is what gets parallelized across threads (Chapter 7.3).

---

## Pass 3 · Architect level — *One IR per question*

### 7. Trade-offs

| IR form | Examples | Good at | Awkward at |
|---|---|---|---|
| AST / HIR | rustc's HIR, javac's trees | type checking, error messages with source structure | control-flow questions |
| Stack bytecode | JVM bytecode, listing 17.1-1's VM | compact, portable, easy to interpret | analysis (implicit stack) |
| CFG over mutable places | rustc's MIR | borrow checking, drop elaboration, initialization | value-level optimization |
| SSA CFG | LLVM IR, Cranelift IR, HotSpot C1's HIR | optimization: constants, GVN, code motion | memory-location questions |
| Sea of nodes (SSA without fixed order) | HotSpot C2, GraalVM | aggressive code motion, speculation | debugging, predictability |
| Machine IR | LLVM MIR (a *different* MIR), after instruction selection | register allocation, scheduling | anything target-independent |

The architectural lesson is that **compilers use several IRs because each question has a natural representation**.
rustc lowers AST → HIR → THIR → MIR → LLVM IR → machine IR, and each stage exists because some analysis is easy on it
and hard on the others. A DSL rarely needs more than two (a tree for checking, a flat IR for evaluation), but the same
principle decides *which* two.

> **Why not do everything on the AST?** You can, for a while. Then someone adds `break`, or `?`, or short-circuit
> evaluation with a side-effecting right operand, and every analysis must reimplement the control-flow rules of every
> construct, identically. Lowering once to a CFG writes those rules down in one place (the lowering), and every later
> analysis inherits them.

Two smaller decisions come with every IR:

- **Must versus may, forward versus backward** is a *correctness* decision, not a performance one. §10 and this
  chapter's debugging exercise are both about choosing wrongly.
- **Minimal versus pruned SSA** trades construction cost (liveness first) for fewer φs.

### 8. Java comparison

`javac` goes straight from trees to **stack bytecode**: jumps are explicit (`ifeq`, `goto`), but there's no CFG
object, no SSA, and almost no optimization. The CFG work happens at run time [LIB]:

- **The verifier is a dataflow analysis.** When a class is loaded, the JVM checks that every instruction's operands
  have the right types on every path, a forward analysis over the bytecode's CFG. Since Java 6's class-file format,
  compilers emit `StackMapTable` frames that state the types at merge points, so verification becomes a single linear
  check instead of an iterative fixpoint (and since Java 7 the frames are mandatory).
- **HotSpot's C1** builds an SSA-form high-level IR from bytecode. **C2** builds a *sea of nodes* (Click and
  Paleczny, 1995): an SSA graph in which instructions float free of blocks except where dependencies pin them, which
  makes code motion natural. GraalVM's compiler uses the same idea.
- **Definite assignment** is specified in JLS chapter 16 as rules per statement and expression form, the same
  forward must-analysis as listing 17.6-3 and Rust's E0381 (Chapter 4.2 compared them).

> **Analogy limit.** "Java's definite assignment is Rust's E0381" holds for most programs, but the JLS rules are
> written over syntax and treat constant expressions specially: after the constant `true`, "when false" facts hold
> vacuously, so `if (true) x = 1;` definitely assigns `x` in Java (per JLS chapter 16; not verified here: requires a
> JDK). rustc computes the property on MIR paths and ignores values: listing 17.6-7's `if true { x = 1; }` is rejected
> with E0381. Same analysis family, different edge cases, because one is specified on syntax and the other on a CFG.

### 9. Production scenario

**Sieve's rule IR.** Sieve compiles each rule to a small CFG before evaluation, lowered like listing 17.6-1: `&&` and
`||` become branches, feature reads are explicit instructions, and each remote feature read (`graph_score`, a call to
the graph service) is its own instruction. Two consumers use it:

- **The evaluator** walks the CFG, so a remote feature is fetched only on a path that reaches its read, which is the
  short-circuit behavior the Part XVII review's first draft got wrong.
- **The prefetch planner** decides which remote features to fetch *in parallel at the start* of an evaluation, to cut
  latency. It runs a backward **must** analysis (a feature is *anticipated* at the entry if every path from the entry
  reads it) and prefetches exactly the anticipated features. Everything else is fetched lazily when reached.

The planner's output is shown in rule review ("prefetched: `velocity_1h`; lazy: `graph_score`, reached on about half
of evaluations in shadow mode"), so authors see what their rule costs other services.

### 10. Failure scenario

**The prefetcher that took every path.** The first version of the prefetch planner, in Sieve v0.6 (July 2026), used
a **may** analysis: it prefetched every remote feature that *some* path read. For a rule like
`amount > 100 && graph_score > 80`, that meant fetching `graph_score` for every evaluation, although the rule reads it
only when `amount > 100` (a bit under half of evaluations for this rule, the same ratio the Part XVII review measures).

Across the fraud rule set, calls to the graph service roughly doubled within an hour of rollout. The graph service is
shared, its p99 latency rose from about 9 ms to about 31 ms, and fraud scoring, whose budget is p99 under 5 ms for the
whole score (Chapter 1.2), started timing out on the graph call and falling back to its degraded mode. The rollout was
reverted after 25 minutes.

The analysis wasn't wrong; it answered a different question. **May** answers "could this be needed?", the right question
for keeping a value in a register (liveness, Chapter 17.8) or for security checks that must consider every possible
path. **Must** answers "will this certainly be needed?", the right question for doing work *early*. The fix replaced
the union with an intersection (anticipated features only), added a per-rule "remote calls per evaluation" estimate
from shadow mode to rule review, and added a load test for the graph service that replays the planner's output rather
than the rule set.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVII).*

1. What is a basic block? Why do compilers lower an AST to a CFG, and which constructs force it?
2. Define dominance, immediate dominator, and dominance frontier. How do dominators find loops?
3. What is SSA, and what does a φ node mean operationally? Why are φs placed at dominance frontiers?
4. Minimal versus pruned SSA: what's the difference, and where did listing 17.6-2 produce useless φs?
5. Classify definite assignment and liveness as forward/backward and must/may, and explain why each meet operator
   (intersection or union) is the right one.
6. Why must a must-analysis start from "everything" and a may-analysis from "nothing"? What goes wrong otherwise?
7. Why is rustc's MIR not in SSA form? What would borrow checking lose if it were?
8. In LLVM's release IR for `sum_to`, what do `sroa`, `lcssa`, and `nuw nsw` tell you about the passes that ran?
9. Why does rustc reject `if true { x = 1; }` followed by a use of `x`, while Java accepts the equivalent?
10. A planner decides which remote calls to make eagerly. Which dataflow direction and meet does it need, and what
    happens if you pick the other one?

### 12. Exercises

- **Beginner.** Lower `if a > 0 { 1 } else { 2 }` by hand into listing 17.6-1's IR (blocks, terminators, the result
  temporary). Check your answer against the listing's lowering of `fib`.
- **Intermediate.** Add `break` to Ore's `while` in listing 17.6-1 (a stack of loop exit blocks). Show the CFG of a loop
  with a `break` inside an `if`, and its dominator tree.
- **Advanced.** Make listing 17.6-2 build **pruned** SSA: compute liveness (listing 17.6-3's algorithm, over the CFG's
  real instructions) and skip φs for variables not live-in at the φ's block. How many of `classify`'s 9 φs remain?
- **Systems.** Emit listing 17.6-5 as LLVM IR in release mode *without* `black_box` (edit a copy). What happens to the
  loop, and which pass is responsible? (Chapter 17.7 shows the answer in assembly.)
- **Architecture.** Sieve wants a "cost estimate" per rule: expected remote calls per evaluation. Design it as a
  dataflow analysis plus shadow-mode statistics (branch probabilities). What is static, what is measured, and how do you
  present uncertainty to rule authors?

### 13. Debugging exercise

Listing `ch06-06-dataflow-bottom-bug.rs` is listing 17.6-3 with one line changed in `definite_assignment`:

```rust,ignore
    let mut out = vec![0u64; n]; // the bug: a pessimistic start ("bottom") for a must-analysis
```

(Excerpt of listing 17.6-6.) The diamond still reports its real error. But on `sum_to`, which has no mistake, it now
prints (real output):

```text
(b) sum_to: definite assignment finds nothing wrong:
  error: `n` is possibly unassigned when used in bb1
  (no errors) fixpoint after 3 rounds
```

(The heading and the "(no errors)" label are fixed strings in the listing; the error line in between is the bug's
output.)

1. Trace the first round of the iteration for `bb1` with the new initial values. Why is `n`, a *parameter*, lost?
2. Why does the analysis never recover it, even though it iterates to a fixpoint? Which fixpoint did it find?
3. Why doesn't the diamond (no loop) show the problem? What property of `sum_to`'s CFG exposes it?

### 14. Design exercise

**Design the IR for Sieve v1's evaluator and analyzer.** Rules have `&&`, `||`, `if`-expressions, `let`, list
membership, remote features, and a per-rule timeout policy (fail open or fail closed). Decide:

- The instruction set and terminators, including how a remote call, its timeout, and the fail-open/closed policy appear
  in the CFG.
- Whether you need SSA, and for which analyses.
- The three analyses you would run at upload time (think: unreachable conditions, features read on no path, anticipated
  remote features), with direction and meet for each.
- How the evaluator and the analyzer stay consistent (one IR, or two with a differential test).
