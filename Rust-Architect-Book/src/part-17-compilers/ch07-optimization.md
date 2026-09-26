# Chapter 17.7 — Optimization

> **Where this sits:** Part XVII · Compilers · chapter 7 of 8
> **Prerequisites:** Chapter 17.6 (CFGs, dominators, SSA); Chapter 1.1 (how C compilers exploit undefined behavior,
> `x + 1 < x`); Chapter 14.1 (a loop hoisted into `jmp` to itself, and a loop replaced by `memcpy`).
> **After this chapter you can:** implement constant folding, copy propagation, global value numbering, dead-code
> elimination, and CFG simplification on SSA; state the legality condition every transformation must meet and spot the
> classic violation (hoisting a trapping operation); read LLVM's output for the same optimizations; and explain what
> undefined behavior and data-race freedom *license* an optimizer to do.

---

## Pass 1 · User level — *Same meaning, less work*

### 1. Problem

Chapter 17.6's lowering is deliberately naive. Given

```text
fn demo(x: int) -> int {
    let scale = 4 * 25;
    let a = x * scale + 1;
    let b = x * scale + 1;
    let unused = a / 3;
    let debug = false;
    if debug { a } else { a + b }
}
```

it produces 19 instructions in 4 blocks: it multiplies `4 * 25` at run time, computes `x * scale + 1` twice, divides
for a result nobody uses, and branches on a constant. A person sees at once that the function is `(x * 100 + 1) * 2`.
An **optimizer** is the program that sees it too, and it has one job: rewrite the IR into a cheaper program that
*means the same thing*.

"Means the same thing" is the hard part. Deleting the unused `a / 3` is fine; deleting an unused `x / d` is not, if
`d` might be zero and the language says division by zero is an error. Moving `b / a` out of a loop is a big win,
unless the loop only divides when `a != 0`. Every optimization is an **analysis** (is this legal? is it profitable?)
plus a **transformation**, and every production optimizer bug is an analysis that said "legal" when it wasn't. This
chapter builds four passes over Ore's SSA with their legality checks, shows the classic illegal one, and then reads
LLVM doing the same things, correctly, to real Rust.

### 2. Mental model

```text
                    ┌──────────── repeat until nothing changes ────────────┐
 SSA from 17.6 ──►  simplify ──► gvn ──► dce ──► simplify ──► merge blocks ──► optimized SSA
                    constants,   same     unused   clean up      straight-line
                    copies,      value?   values?  what the      blocks joined
                    identities,  reuse    delete   others        by a jump
                    constant     it       (never   exposed       become one
                    branches              traps)
```

A small catalog covers most of what compilers do:

| Optimization | What it removes | Legality question |
|---|---|---|
| Constant folding / propagation | computations on known values | would the folded operation have trapped? |
| Copy propagation | `a = b` moves | none in SSA (one definition per name) |
| Algebraic simplification | `x + 0`, `x * 1`, `x * 0` | is the identity exact for this type (floats: `x + 0.0` isn't)? |
| Branch folding | branches on constants | none |
| Common subexpression elimination / GVN | recomputing the same value | is the first computation available on every path to the second? |
| Dead-code elimination (DCE) | values nobody uses | does the instruction have effects, or can it trap? |
| CFG simplification | jumps between straight-line blocks | none, if φs are updated |
| Loop-invariant code motion (LICM) | work repeated on every iteration | would it have executed at all (guards, zero-trip loops)? |
| Induction-variable simplification | loops that compute a formula | exact wraparound semantics |
| Inlining | calls | code size, recursion (it's the *enabler*: most other wins happen after inlining) |

Three ideas organize the rest of the chapter:

- **SSA makes most of these local.** A value has one definition, so "is `%3` a constant?" is a question about one
  instruction, and replacing `%3` everywhere is always safe.
- **Dominance makes reuse safe.** A computation in block A can be reused in block B only if A dominates B: A then
  runs on every path to B (Chapter 17.6).
- **Legality is about observable behavior**, and in a safe language *errors are observable*. An optimizer may remove
  work; it may not remove, add, or move a run-time error, unless the language defines that situation as undefined
  behavior, which is the double-edged tool §4 examines.

### 3. Rust code

Listing `ch07-01-optimizer.rs` runs Ore source through Chapter 17.6's lowering and SSA construction, then four passes.
The legality predicate that every pass consults is short:

```rust,ignore
/// Can executing this instruction fail? Division by a value that might be zero can.
fn may_trap(i: &Inst) -> bool {
    match i {
        Inst::Bin { op: BinOp::Div | BinOp::Rem, b, .. } => !matches!(b, Val::Const(c) if *c != 0),
        Inst::Call { .. } => true, // calls may trap or not terminate: never delete or merge them
        _ => false,
    }
}
```

**Constant folding** refuses one case explicitly:

```rust,ignore
fn fold(op: BinOp, x: i64, y: i64) -> Option<i64> {
    Some(match op {
        BinOp::Add => x.wrapping_add(y), BinOp::Sub => x.wrapping_sub(y), BinOp::Mul => x.wrapping_mul(y),
        BinOp::Div | BinOp::Rem if y == 0 => return None, // keep the run-time error; never fold it away
```

A compiler that folds `10 / 0` to anything, or reports it as a compile error in dead code, has changed the program.
Keeping it means the program still fails at run time exactly where the source says. (Ore's arithmetic wraps, so folding
uses `wrapping_*` to compute exactly what the running program would.)

**Global value numbering** walks the dominator tree with a *scoped* table of expressions already computed. An
expression computed in a block is available in every block that block dominates; on the way back up, the table is
truncated so siblings don't see each other's entries:

```rust,ignore
        for inst in &f.blocks[b].insts {
            let (dst, key) = match *inst {
                Inst::Bin { dst, op, a, b } if !may_trap(inst) => {
                    let (a, b) = (canon(a, merged), canon(b, merged));
                    let commutative = matches!(op, BinOp::Add | BinOp::Mul | BinOp::Eq | BinOp::Ne);
                    let (a, b) = if commutative && rank(b) < rank(a) { (b, a) } else { (a, b) };
                    (dst, (op as u8, a, b))
                }
```

Commutative operations are put in a canonical order first, so `x * scale` and `scale * x` get the same key.

**Dead-code elimination** is mark-and-sweep. The roots are the terminators' operands *and every instruction that may
trap*; everything reachable from the roots through def-use edges is live, and the rest is deleted:

```rust,ignore
    for b in &f.blocks {
        match &b.term { Term::Branch { cond, .. } => work.push(*cond), Term::Return(v) => work.push(*v), Term::Jump(_) => {} }
        for i in &b.insts {
            if may_trap(i) { work.extend(operands(i)); }
        }
    }
```

(Excerpts of listing 17.7-1.) Real output: `demo` before optimization (the SSA from Chapter 17.6's pipeline), the pass
statistics for four functions, and the results:

```text
fn demo (SSA):
  bb0:
      %0 = 4 * 25
      scale = %0
      %1 = x * scale
      %2 = %1 + 1
      a = %2
      %3 = x * scale
      %4 = %3 + 1
      b = %4
      %5 = a / 3
      unused = %5
      debug = 0
      branch debug ? bb1 : bb3
  bb1:
      %6#1 = a
      jump bb2
  bb2:
      %6#2 = phi(bb1: %6#1, bb3: %6#3)
      return %6#2
  bb3:
      %7 = a + b
      %6#3 = %7
      jump bb2
demo       19 ->   4 instructions, 4 -> 1 blocks  (simplify 10+0, gvn 2, dce 1, merge 2)
guarded    11 ->   8 instructions, 4 -> 4 blocks  (simplify 3+0, gvn 0, dce 0, merge 0)
sum_to     13 ->   9 instructions, 4 -> 4 blocks  (simplify 4+0, gvn 0, dce 0, merge 0)
classify   32 ->  22 instructions, 13 -> 13 blocks  (simplify 8+0, gvn 0, dce 2, merge 0)

fn demo (SSA):
  bb0:
      %1 = x * 100
      %2 = %1 + 1
      %7 = %2 + %2
      return %7
```

Follow `demo` through the passes. `simplify` folded `4 * 25` to `100`, propagated the copies (`scale`, `a`, `b`,
`unused`, `debug` all disappear as names), and folded the branch on `debug = 0` into a jump, which made `bb1`
unreachable and turned the φ into a copy of `%7`. `gvn` found that `%3 = x * 100` and `%4 = %3 + 1` repeat `%1` and
`%2`. `dce` deleted `%5 = %2 / 3`: division by the constant 3 can't trap, and nothing used the result. `merge` joined
the remaining straight-line blocks. What's left is the person's version: `(x * 100 + 1) * 2`, written as `%2 + %2`.

Now `guarded`:

```text
fn guarded(x: int, d: int) -> int {
    let unused = x / d;
    if d != 0 { x / d } else { 0 }
}
```

```text
fn guarded (SSA):
  bb0:
      %0 = x / d
      %1 = d != 0
      branch %1 ? bb1 : bb3
  bb1:
      %3 = x / d
      jump bb2
  bb2:
      %2#2 = phi(bb1: %3, bb3: 0)
      return %2#2
  bb3:
      jump bb2
identical results on 676 runs; dynamic IR instructions 14687 -> 9667
guarded(1, 0) = Err("division by zero") (the dead `x / d` is kept: removing it would hide a run-time error)
```

`%0 = x / d` is dead as a *value*: nothing reads it. It's kept anyway, because it's the program's behavior when
`d == 0`: the first line divides before the guard is checked, so `guarded(1, 0)` must fail, and it does, before and
after optimization. (The guard is useless in this program, which is exactly the kind of bug a *linter* should report,
and exactly what an *optimizer* must not silently "fix".) The last test ran every function on 676 inputs through the
unoptimized and optimized SSA: identical results everywhere, with 34% fewer IR instructions executed (14,687 to
9,667).

**The classic illegal optimization.** Listing `ch07-02-licm-trap.rs` implements loop-invariant code motion on this
loop:

```text
acc = 0; i = 0; while i < n { if a != 0 { acc = acc + b / a }  i = i + 1 }  return acc
```

`b / a` is loop-invariant (neither `a` nor `b` changes in the loop), so hoisting it into the preheader computes it
once instead of `n` times. The naive pass checks only invariance; the safe pass also checks `may_trap`:

```rust,ignore
            let hoist = match &p[b][k] {
                I::Op(_, x, _, y) => {
                    let invariant = !defined_in_loop(p, x) && !defined_in_loop(p, y);
                    // Legal to hoist: the instruction cannot trap, OR it would have executed anyway (its block runs
                    // on every iteration AND the loop provably runs at least once). This pass checks only the first.
                    invariant && (!safe || !may_trap(&p[b][k]))
                }
                _ => false,
            };
```

(Excerpt of listing 17.7-2.) Real output:

```text
naive LICM hoists: ["Op(\"nz\", \"a\", '!', \"zero\") from bb2", "Op(\"q\", \"b\", '/', \"a\") from bb3"]
safe  LICM hoists: ["Op(\"nz\", \"a\", '!', \"zero\") from bb2"]

(n, a, b)          original                   naive LICM                 safe LICM
(1000, 2, 10)      5000 (1000 divisions)      5000 (1 divisions)         5000 (1000 divisions)
(1000, 0, 10)      0 (0 divisions)            ERROR: division by zero    0 (0 divisions)
(0, 0, 10)         0 (0 divisions)            ERROR: division by zero    0 (0 divisions)
(0, 5, 10)         0 (0 divisions)            0 (1 divisions)            0 (0 divisions)
```

The naive version is 1,000 times faster on the first input and *wrong* on the next two: the original program never
divides when `a == 0`, or when the loop runs zero times, and the hoisted division fails anyway. The last row is
subtler: no error, but work that the original never did (harmless for a division, fatal for a remote call or a
memory access that might fault). The safe pass gives up the big win to stay correct. §4 shows how LLVM gets both.

---

## Pass 2 · Systems level — *What LLVM does, and what licenses it*

### 4. Under the hood

Listing `ch07-03-llvm-does-it.rs` contains one small function per optimization, compiled in release mode and read with
`tools/emit.ps1` (real output, excerpts).

**Constant folding with exact semantics.**

```rust,ignore
pub fn wraps(x: i64) -> bool {
    x.wrapping_add(1) < x
}
```

```text
playground::wraps:
	movabs	rax, 9223372036854775807
	cmp	rdi, rax
	sete	al
	ret
```

`x + 1 < x` with wrapping arithmetic is true for exactly one input, `i64::MAX`, and LLVM computed that. Compare
Chapter 1.1: in C, signed overflow is undefined behavior, so the compiler may assume `x + 1` never wraps and fold
`x + 1 < x` to `false`, silently deleting an overflow check. Rust never makes integer overflow undefined (it panics in
debug builds and wraps in release, Chapter 2.2) [LANG], so the optimizer must preserve the one input where the answer
is `true`.

**Common subexpression elimination.** `(a * b + 1) * (a * b + 2)` multiplies `a * b` once:

```text
playground::cse:
	imul	rdi, rsi
	lea	rax, [rdi + 1]
	lea	rcx, [rdi + 2]
	imul	rax, rcx
	ret
```

**Loop-invariant code motion, then vectorization.** In `for x in xs.iter_mut() { *x += a * b; }` the product is
computed once, before the loop (`imul rcx, rdx`), and the loop adds it to four elements per iteration with two 16-byte
SSE2 additions (`paddq`), followed by a scalar loop for the remainder:

```text
playground::scale_all:
	test	rsi, rsi
	je	.LBB6_7
	lea	r8, [8*rsi]
	imul	rcx, rdx
	...
.LBB6_3:
	movdqu	xmm1, xmmword ptr [rdi + 8*r9]
	movdqu	xmm2, xmmword ptr [rdi + 8*r9 + 16]
	paddq	xmm1, xmm0
	paddq	xmm2, xmm0
	movdqu	xmmword ptr [rdi + 8*r9], xmm1
	movdqu	xmmword ptr [rdi + 8*r9 + 16], xmm2
	add	r9, 4
	cmp	rdx, r9
	jne	.LBB6_3
```

**Induction-variable simplification: the loop that disappears.** `triangle(n)` sums `1..=n` in a loop:

```text
playground::triangle:
	test	rdi, rdi
	je	.LBB4_1
	lea	rax, [rdi - 1]
	lea	rcx, [rdi - 2]
	mul	rcx
	shld	rdx, rax, 63
	lea	rax, [rdx + 2*rdi]
	dec	rax
	ret
```

There's no loop. LLVM's *scalar evolution* analysis recognized `s` as a sum over an arithmetic progression and replaced
the loop with a closed form [LIB]. The formula is `(n - 1)(n - 2) / 2 + 2n - 1`, which is algebraically `n(n + 1) / 2`.
It's computed with a 128-bit product (`mul` leaves the high half in `rdx`) and a 128-bit shift (`shld ..., 63` divides
by 2), so the result is exactly what the wrapping loop computes for every `n`, including values where `n(n + 1)`
overflows 64 bits.

**Dead-store elimination.** `*x = 1; *x = 2;` is one store: `mov qword ptr [rdi], 2`.

**A trapping operation, and legal LICM.** Rust's `/` on integers checks for a zero divisor and for `i64::MIN / -1` before
dividing [LANG]. The `divide` function shows both checks (panic calls to `panic_const_div_by_zero` and
`panic_const_div_overflow`), plus a test of whether both operands fit in 32 bits as unsigned values (`or rax, rsi` then
`shr rax, 32`), in which case it uses the much faster 32-bit unsigned `div` instead of the 64-bit `idiv` (an x86 tuning
LLVM applies to 64-bit division) [LIB]. Then `guarded_sum`,
listing 17.7-2's loop written in Rust:

```rust,ignore
pub fn guarded_sum(n: u64, a: i64, b: i64) -> i64 {
    let mut acc = 0i64;
    for _ in 0..n {
        if a != 0 {
            acc = acc.wrapping_add(b / a);
        }
    }
    acc
}
```

```text
playground::guarded_sum:
	xor	eax, eax
	test	rdi, rdi
	je	.LBB0_7
	test	rsi, rsi
	je	.LBB0_7
	mov	rax, rsi
	not	rax
	movabs	rcx, -9223372036854775808
	xor	rcx, rdx
	or	rcx, rax
	je	.LBB0_8
	...
	cqo
	idiv	rsi
	jmp	.LBB0_6
	...
.LBB0_6:
	imul	rax, rdi
.LBB0_7:
	ret
```

The loop is gone, and the division runs once. That's what the naive LICM wanted, done legally. LLVM first tests
exactly the conditions under which the original program would have divided at least once: `n != 0`
(`test rdi, rdi`) and `a != 0` (`test rsi, rsi`). Only then does it check `MIN / -1`, divide once, and multiply the
quotient by `n` (`imul rax, rdi`), which equals adding it `n` times with wrapping. The error behavior is preserved
precisely: the overflow panic can happen only on inputs where the original loop would have panicked on its first
iteration. The general technique is *versioning*: specialize the code under guards that make the transformation legal.

**What undefined behavior and data-race freedom license** [LANG]. Some optimizations are legal only because the
language declares certain situations impossible:

- **Undefined behavior.** An optimizer may assume UB never happens. In C that includes signed overflow (Chapter 1.1's
  deleted check). In Rust *safe* code, UB is unreachable by construction; in `unsafe` code, the programmer promises it
  (Part XV), and the optimizer relies on that promise: `&mut` means no aliasing (LLVM's `noalias`, Chapter 4.1), a
  reference is never null, a `bool` is 0 or 1.
- **Data-race freedom.** Chapter 14.1 showed a loop waiting on a plain `static mut` flag compiled to `jmp` to itself:
  LICM hoisted the load out of the loop. From the optimizer's side, hoisting a load requires that no store *in this
  thread* can change the location inside the loop. Stores by *other threads* are ignored, because a concurrent
  unsynchronized write would be a data race, and a data race is UB. The same license lets LLVM replace a copy loop with
  one `memcpy` call (Chapter 14.1's second example; LLVM's loop-idiom recognition) and change the order and size of
  the stores. Atomics and locks are how a program tells the optimizer that another thread *is* watching.

**rustc's own optimizations** [RUSTC]. rustc optimizes MIR before handing it to LLVM (inlining, simplification, a
GVN pass, dead-code removal), mostly to shrink what LLVM has to process and to help compile times. The heavy
optimization happens in LLVM's pass pipeline, where many passes run several times because each one exposes
opportunities for others (Chapter 18.6).

### 5. Memory

Optimizations change a program's memory behavior in both directions:

- **SROA and DSE remove memory traffic.** Chapter 17.6 showed SROA turning stack slots into registers; `overwrite`
  showed a store deleted outright.
- **LICM and GVN can *increase* register pressure.** A value hoisted out of a loop is live across the whole loop, and a
  reused value stays live from its first computation to its last reuse. With more values live than registers, the
  allocator spills to the stack (Chapter 17.8). Compilers therefore *rematerialize* cheap values (recompute a constant
  instead of keeping it in a register) and limit how far they hoist.
- **Inlining grows code.** Every inlined call duplicates the callee's body; that's the monomorphization bloat trade-off
  of Chapter 7.3 in another form, with instruction-cache effects.

The optimizer's *own* memory matters for compile time. Listing 17.7-1's `replace_uses` scans the whole function for
every replacement, so a function with many replacements costs quadratic time. LLVM's use lists make the same rewrite
proportional to the number of uses (Chapter 17.6 §5).

### 6. CPU / OS

The measured effects in this chapter, from IR instruction counts to machine code:

| Function | Before | After | Mechanism |
|---|---|---|---|
| Listing 17.7-1, all four functions, 676 runs | 14,687 IR instructions executed | 9,667 (−34%) | folding, GVN, DCE, block merging |
| `demo` | 19 instructions, 4 blocks | 4 instructions, 1 block | as above |
| `triangle(n)` | n iterations | constant time | scalar evolution |
| `guarded_sum(n, a, b)` | n iterations, n divisions | constant time, at most 1 division | versioning + LICM + induction simplification |
| `scale_all` | 1 multiply per element | 1 multiply total, 4 elements per iteration | LICM + vectorization |

(The IR counts are exact; the machine-code rows are read from the release assembly, not timed.) The asymptotic wins
(`triangle`, `guarded_sum`) dwarf everything else, which is why benchmarks must stop the optimizer from recognizing
the computation: listing 17.6-5 used `black_box` for exactly that reason, and Chapter 20.2 returns to it.

Optimization is also where release builds spend their compile time. LLVM's `-O2`/`-O3` pipelines run dozens of passes,
several of them repeatedly, and the order matters: inlining exposes constants, which enable branch folding, which
makes code dead, which enables more inlining. Finding a good order is the *phase-ordering problem*, and production
pipelines are tuned empirically. To see LLVM's pipeline locally (not verified here: requires a local toolchain):

```text
cargo rustc --release -- -C llvm-args=-print-after-all 2> passes.txt   # IR after every pass (very large)
cargo rustc --release -- --emit=llvm-ir -C opt-level=0                 # the unoptimized input, for comparison
```

---

## Pass 3 · Architect level — *Optimizers must be conservative, or verified*

### 7. Trade-offs

| Strategy | Wins | Risk | Where it's used |
|---|---|---|---|
| No optimization, good evaluation strategy | simple, predictable | leaves performance on the table | most DSLs, config languages |
| A few safe passes (fold, DCE, CSE) with a `may_trap` rule | most of the easy wins | low, if legality is tested | rule engines, query planners |
| Full AOT pipeline (LLVM) | near-optimal code | compile time; relies on UB contracts | rustc, clang, Swift |
| Speculative JIT with deoptimization | optimizes for *observed* behavior | complexity; warm-up | HotSpot, V8, LuaJIT |

For an in-house language, the question isn't "how much can we optimize?" but "how much *difference* between source and
executed program can we afford to test?". Each pass is a new way for the compiled rule to disagree with what its author
wrote. The price of every optimization is a **differential test**: the optimized and unoptimized programs, on many
inputs, compared (listing 17.7-1's 676-run check is the minimal version).

> **Why not optimize aggressively?** Because the benefit is usually small and the failure mode is silent. A rule
> engine spends its time on feature fetches and allocation, not on arithmetic; folding constants in a rule saves
> nanoseconds. A wrong fold, or a hoisted division, changes decisions without any error. Optimize what profiling says
> matters, and test legality harder than speed.

A useful rule for legality, stated once: **an optimizer may remove, add, or reorder work only when no observable
behavior changes, and in a safe language, run-time errors are observable behavior.** Undefined behavior is the one
exception, and it's why languages that want aggressive optimization without UB in safe code (Rust) put the UB
contracts in `unsafe` and in the type system (`&mut` uniqueness) instead of in arithmetic.

### 8. Java comparison

HotSpot's C2 performs the same catalog, and adds **speculation** [LIB]. It optimizes for what it has *observed*: a
virtual call that has only ever seen one receiver class is inlined, a branch never taken is compiled as an *uncommon
trap*, and if the assumption breaks, the method is *deoptimized* back to the interpreter and recompiled. That changes
legality: C2 can hoist or delete work that is valid for the observed behavior, protected by a cheap guard that
triggers deoptimization.

Java's exception semantics constrain it the same way Rust's panics constrain LLVM. The JLS requires exceptions to be
*precise*: when an `ArithmeticException` for division by zero is thrown, every effect before it must have happened and
none after. So C2 can move a division only when it can prove the divisor nonzero, or keep an explicit check (which it
turns into a deoptimization point). One classic trick has no Rust equivalent: C2 turns explicit null checks into
*implicit* ones by letting the access to address zero fault, and handling the `SIGSEGV` in the JVM's signal handler.
Rust doesn't need it, because references are never null (Chapter 5.2's niches).

| | HotSpot C2 | rustc + LLVM |
|---|---|---|
| When | at run time, per hot method | at build time, per codegen unit |
| Assumptions | observed profiles, with guards and deoptimization | proven facts and the language's UB contracts |
| Division by zero | precise `ArithmeticException` | panic (`panic_const_div_by_zero`), checked explicitly |
| Null checks | implicit, via a fault handler | not needed for references |
| Escape analysis / SROA | scalar replacement of non-escaping objects | SROA of stack aggregates |

> **Analogy limit.** "C2 and LLVM run the same optimizations" is true of the catalog and false of the contract. A JIT
> needs its transformation to be right only for the executions it guards, because it can undo it. An AOT compiler must
> be right for every possible execution, forever, because the binary ships. That's why LLVM's `guarded_sum` tests the
> exact legality conditions up front, while C2 might compile the loop body assuming `a != 0` (if it always has been)
> and deoptimize the first time it isn't.

### 9. Production scenario

**Sieve's optimizer is deliberately small.** It has four passes, each with a legality rule written next to it:

- **Checked constant folding.** Constant subexpressions are folded with *checked* arithmetic, and an overflow is a
  compile error with a span (the Part XVII review shows why: an unchecked fold crashed the first compiler). Money
  arithmetic folds in exact minor units.
- **CSE of feature reads.** Within one evaluation, a feature has one value, so two reads of `velocity_1h` are one read,
  and a remote feature is fetched at most once.
- **Dead-condition warnings, not deletions.** When the schema proves a condition always true (`amount >= EUR 0.00`,
  because amounts are non-negative), Sieve warns the author instead of silently deleting it. The author probably meant
  something else.
- **Nothing moves across a guard.** Divisions and remote calls are never hoisted out of the condition that protects
  them (§10), and the optimizer has a `may_trap` predicate identical in spirit to listing 17.7-1's.

Every change to the optimizer runs a differential test over a replay of about two million recorded evaluations: each
stored rule is evaluated optimized and unoptimized, and any difference fails the build.

### 10. Failure scenario

**The ratio that divided by zero.** Sieve v0.7 (August 2026) added a "compute derived values once" pass: any
arithmetic over features was computed at the start of the evaluation, so rules that used the same ratio in several
places didn't recompute it. It was LICM for rules, and it checked invariance but not trapping, exactly listing 17.7-2's
naive version.

One onboarding rule read:

```text
merchant_age_days > 0 && volume_30d / merchant_age_days > 5000
```

The guard exists because new merchants have `merchant_age_days == 0`. The pass hoisted the division to the start of
the evaluation, before the guard. For every merchant onboarded that day, evaluating the rule failed with a division by
zero. The rule's run-time policy was **fail closed** (Chapter 17.1's per-rule policy): an evaluation error blocks the
payment. For 47 minutes, until the rule was disabled, first-day payments at newly onboarded merchants were declined:
about 2,300 payments across roughly 180 merchants.

The fix restored the legality rule (no trapping operation moves out of its guard), but the lasting changes were about
testing and policy. The differential test's replay data had almost no zero-age merchants, so a generator now adds
boundary values (zero, negative, maximum) for every numeric feature a rule divides by. And fail-closed rules now need a
second reviewer who asks one question: "what happens to a brand-new merchant?"

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVII).*

1. Name five optimizations and the legality question each must answer.
2. Why does SSA make constant propagation and copy propagation simple? Why does GVN walk the dominator tree?
3. Listing 17.7-1 keeps the unused `x / d` in `guarded`. Why is deleting it wrong, and what would a linter say instead?
4. What is the legality condition for loop-invariant code motion of an operation that can trap? How does LLVM's
   `guarded_sum` satisfy it?
5. How did LLVM remove the loop from `triangle`, and why does the result use a 128-bit multiply?
6. Why does `x.wrapping_add(1) < x` compile to `x == i64::MAX` in Rust, while `x + 1 < x` may compile to `false` in C?
7. Explain how data-race freedom licenses hoisting a load out of a loop. What changes when the variable is an atomic?
8. Why can LICM or GVN make code slower? What do compilers do about it?
9. How does a JIT's legality contract differ from an AOT compiler's? Give an optimization C2 can do that LLVM can't.
10. You're adding an optimizer to a rule language. Which passes would you ship first, and what testing does each
    require?

### 12. Exercises

- **Beginner.** Add two algebraic identities to listing 17.7-1's `simplify`: `x - x → 0` and `x / x → 1`. One of them is
  illegal in Ore. Which one, and what input shows it?
- **Intermediate.** Extend GVN so that a trapping instruction can reuse an *identical, dominating* trapping instruction
  (in `guarded`, `bb1`'s `x / d` is dominated by `bb0`'s). Argue why this is legal, and show that the 676-run
  differential test still passes.
- **Advanced.** Make listing 17.7-2's safe LICM hoist a trapping instruction when it's legal: its block must execute on
  every iteration (it dominates the loop's latch) and the loop must provably run at least once (or you insert a guard,
  as LLVM did). Test it on a loop without the `if a != 0`.
- **Systems.** Emit listing 17.7-3 in debug and release and compare `triangle` and `scale_all`. Then add
  `std::hint::black_box` around the loop counter in `triangle` and show that the closed form disappears. What does that
  tell you about benchmarking loops (Chapter 20.2)?
- **Architecture.** Write the review checklist for a new Sieve optimizer pass: legality, trapping, remote calls,
  timeouts, fail-open/closed policies, and the differential-test data it needs.

### 13. Debugging exercise

Listing `ch07-04-dce-trap-bug.rs` is listing 17.7-1 with one change in `dce`: instructions that may trap are no longer
roots, and only calls are kept unconditionally. The optimizer runs, and `guarded` now optimizes to (real output):

```text
guarded    11 ->   7 instructions, 4 -> 4 blocks  (simplify 3+0, gvn 0, dce 1, merge 0)
...
fn guarded (SSA):
  bb0:
      %1 = d != 0
      branch %1 ? bb1 : bb3
```

Then the differential test fails (real output):

```text
thread 'main' (13) panicked at src/main.rs:1340:17:
assertion `left == right` failed: guarded[-6, 0]
  left: Err("division by zero")
 right: Ok(0)
```

1. Which instruction did DCE delete, and why did the modified pass consider it dead?
2. Which side of the assertion is the unoptimized program, and which result is correct according to Ore's semantics?
3. Why is `guarded[-6, 0]` the *first* failure, and why would a test suite with only "normal" inputs never notice?
4. Rust's own compiler has the same obligation. What does LLVM do with an unused `let _ = x / d;` in Rust, and why?

### 14. Design exercise

**Design the optimizer for Meridian's support-console query language** (Chapter 17.3's design exercise). Queries run
against a payments database of about 2 billion rows through an index-aware planner. Decide:

- Which rewrites belong in your compiler (constant folding of dates like `7d ago`, predicate simplification,
  contradiction detection such as `amount > 100 && amount < 50`) and which you leave to the database's planner.
- The legality rules for each, including time-dependent expressions (`ago` must be evaluated once per query, not per
  row) and anything that can fail at run time.
- How you'd show a user what their query was rewritten into, and when a rewrite should be a warning instead.
- The differential test: what oracle, what data, and how often.
