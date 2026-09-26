# Chapter 17.8 — Code Generation and Register Allocation

> **Where this sits:** Part XVII · Compilers · chapter 8 of 8
> **Prerequisites:** Chapter 17.6 (liveness as dataflow); Chapter 17.7 (the optimized IR); Chapter 2.4 (the System V
> x86-64 calling convention, `sret`); Chapter 17.1 (`scale` in debug and release assembly).
> **After this chapter you can:** turn three-address code into two-address x86-style instructions; compute live
> intervals from liveness and allocate registers with linear scan, including spilling; test a code generator with an
> emulator as the oracle; read LLVM's register allocation and calling-convention decisions in real assembly; and choose
> between interpreting, closure-compiling, and generating machine code for a language you own.

---

## Pass 1 · User level — *From unlimited names to sixteen registers*

### 1. Problem

The optimized IR of Chapter 17.7 still assumes an imaginary machine: every value has its own name (`%1`, `%2`, `i#3`),
there are as many of them as the program needs, and every instruction has three operands. The real machine is
different in three ways that matter:

- **Registers are few.** x86-64 has 16 general-purpose registers, and some are spoken for (the stack pointer, the
  return value, arguments). A function with more values alive at once than free registers must keep some in memory:
  it **spills** them to its stack frame, and reloads them when needed.
- **Instructions have shapes.** `add rax, rbx` means `rax = rax + rbx`: x86 is mostly *two-address*, so `v = a + b`
  becomes "copy `a` into `v`'s register, then add `b`". Some operations have special forms worth finding
  (`lea rax, [rdi + 2*rdi]` computes `3x` in one instruction, Chapter 17.1).
- **Functions must agree on a protocol.** Where do arguments go? Which registers does a callee promise to preserve?
  That's the **calling convention** (the System V ABI on Linux x86-64, Chapter 2.4), and it's what lets separately
  compiled code, and code in different languages (Part XVI), call each other.

Code generation is three jobs done in order: **instruction selection** (IR operations to machine instructions),
**register allocation** (values to registers or stack slots), and **emission** (text or bytes, plus the symbols and
relocations the linker needs). This chapter implements the middle one properly, shows the other two in miniature, and
tests the whole thing without being able to run the generated code, by writing an emulator for it.

### 2. Mental model

```text
 three-address code        liveness (17.6)          live intervals               allocation (K = 4)
 v1 = x + 1                 at each instruction:      v1  [0 ........ 20]          v1 -> rcx
 v2 = y + 2                 which values might be     v2   [1 ....... 20]          v2 -> rdx
 ...                        used later?               ...                          v5 -> [rsp+0]   (spilled)
 loop:                                                v10          [11,12]         v10 -> rdi
   t1 = a * i                                         v11           [12,13]        v11 -> rsi
   ...                                                                            ...
   jump loop    <- back edge keeps loop-invariant values live to the jump
```

- A value's **live interval** runs from its definition to the last point where it's live (not just its last *textual*
  use: a value used at the top of a loop is live all the way to the loop's back edge).
- **Register pressure** at a point is the number of values live there. If the maximum pressure exceeds the number of
  registers **K**, something must spill.
- **Linear scan** (Poletto and Sarkar, 1999) sorts intervals by start, walks them once, keeps the *active* intervals
  in registers, frees a register when its interval ends, and when none is free, spills whichever interval ends last:
  it would block a register longest.
- **Graph coloring** (Chaitin, 1982) is the classic alternative: build an *interference graph* (an edge between two
  values live at the same time), then color it with K colors. It produces better code and costs more compile time.

### 3. Rust code

Listing `ch08-01-regalloc.rs` is a back end, end to end. The input is a small function in three-address code with
unlimited virtual registers, chosen to create pressure: seven values computed before a loop, all used inside it.

```text
f(n, x, y):  a = x + 1; b = y + 2; c = x * y; d = a + b; e = c - a; f = d * e; g = b + c;
             acc = 0; i = 0
             loop: if i >= n goto done
                   t1 = a * i; t2 = t1 + b; t3 = t2 + c; t4 = t3 - d; t5 = t4 + e; t6 = t5 - f; t7 = t6 + g
                   acc = acc + t7; i = i + 1; goto loop
             done: return acc
```

Liveness is the backward dataflow of Chapter 17.6, at instruction granularity, and each value's interval is the range
of instructions where it's defined, used, *or live-out*. Linear scan is 25 lines:

```rust,ignore
fn linear_scan(ivs: &[(u32, usize, usize)], k: usize) -> HashMap<u32, Loc> {
    let mut loc = HashMap::new();
    let mut active: Vec<(usize, u32, usize)> = Vec::new(); // (end, vreg, reg), kept sorted by end
    let mut free: Vec<usize> = (0..k).rev().collect();
    let mut slots = 0;
    for &(v, start, end) in ivs {
        // expire intervals that ended before this one starts
        active.retain(|&(e, _, r)| if e < start { free.push(r); false } else { true });
        if let Some(r) = free.pop() {
            loc.insert(v, Loc::Reg(r));
            active.push((end, v, r));
        } else {
            // spill heuristic: evict whichever interval ends LAST (it would block a register longest)
            let &(last_end, last_v, last_r) = active.last().unwrap();
            if last_end > end {
                loc.insert(last_v, Loc::Slot(slots));
                loc.insert(v, Loc::Reg(last_r));
                active.pop();
                active.push((end, v, last_r));
            } else {
                loc.insert(v, Loc::Slot(slots));
            }
            slots += 1;
        }
        active.sort();
    }
    loc
}
```

Instruction selection then rewrites each three-address instruction into x86-flavored two-address text, using the
allocation. The only subtlety is the shape of x86 instructions: `imul` can't write to memory, and `d = a - d` can't be
done as `mov d, a; sub d, d`. So a scratch register (`r11`, never allocated) absorbs those cases:

```rust,ignore
            Tac::Bin(d, op, a, b) => {
                let mnem = match op { '+' => "add", '-' => "sub", _ => "imul" };
                let (d, a, b) = (place(Opnd::V(*d), loc), place(*a, loc), place(*b, loc));
                // x86 is two-address: dst = dst op src. Work in the scratch register when the
                // destination is in memory (imul can't write memory), or when dst would clobber b.
                if is_mem(&d) || d == b {
                    out.push(format!("mov {SCRATCH}, {a}"));
                    out.push(format!("{mnem} {SCRATCH}, {b}"));
                    out.push(format!("mov {d}, {SCRATCH}"));
                } else {
                    if d != a { out.push(format!("mov {d}, {a}")); }
                    out.push(format!("{mnem} {d}, {b}"));
                }
            }
```

(Excerpts of listing 17.8-1.) The Playground can compile Rust but can't assemble or run this text, so the listing
includes a 35-line **emulator** for its own output (`mov`, `add`, `sub`, `imul`, `cmp`, `jge`, `jmp`, `ret`, registers
and memory operands in a `HashMap`). For each K, the generated code runs on four inputs, and every result must equal
the result of an interpreter running the original three-address code. Real output:

```text
16 virtual registers; register pressure (max simultaneously live) = 10
live intervals: v1:[0,20] v2:[1,20] v3:[2,20] v4:[3,20] v5:[4,20] v6:[5,20] v7:[6,20] v8:[7,22] v9:[8,20] v10:[11,12] v11:[12,13] v12:[13,14] v13:[14,15] v14:[15,16] v15:[16,17] v16:[17,18]

  K  spilled  instructions  memory operands executed (for n=10)
  2        9            51       164
  3        8            50       151
  4        7            49       138
  6        5            47       113
  8        3            45        90
 12        0            39        15
all allocations produce the interpreter's results: [-190, 0, -690, 765612]
```

Read the intervals first. `v1` (`a`) is last *used* at instruction 11 (`t1 = a * i`), but its interval runs to 20: the
loop's back edge at instruction 20 jumps to the loop header, and the next iteration will read `a` again, so `a` is
live-out of every instruction in the loop body. The same holds for `v2` through `v7`, and for `i` (`v9`). The loop
temporaries `v10` through `v16` live for one instruction each, which is why seven of them can share two registers.
Maximum pressure is 10: nine long-lived values plus one temporary.

Then the table. With 12 registers nothing spills, and the only memory operands executed are the three argument reads
per loop test (15 in total for `n = 10`). Each register removed spills one more value and adds memory traffic, up to
164 memory operands at K = 2. Every row produced the interpreter's results, so the allocator, the instruction
selector, and the spill code are correct for these inputs. The K = 4 code shows what spilling looks like (real output,
excerpt):

```text
        mov rcx, [args+8]
        add rcx, 1
        mov rdx, [args+16]
        add rdx, 2
        mov r11, [args+8]
        imul r11, [args+16]
        mov [rsp+48], r11
        ...
    .L0:
        mov r11, [rsp+32]
        cmp r11, [args+0]
        jge .L1
        mov rdi, rcx
        imul rdi, [rsp+32]
        mov rsi, rdi
        add rsi, rdx
        mov rdi, rsi
        add rdi, [rsp+48]
        ...
        mov r11, [rsp+24]
        add r11, rdi
        mov [rsp+24], r11
        mov r11, [rsp+32]
        add r11, 1
        mov [rsp+32], r11
        jmp .L0
    .L1:
        mov rax, [rsp+24]
        ret
```

`a` and `b` got `rcx` and `rdx`; `c` lives at `[rsp+48]`; the loop temporaries alternate between `rdi` and `rsi`.
And the loop counter `i` (`[rsp+32]`) and the accumulator `acc` (`[rsp+24]`) are *in memory*, loaded and stored on
every iteration. That's a poor choice, and §13 asks you to find out why the heuristic made it.

---

## Pass 2 · Systems level — *What LLVM's back end decides*

### 4. Under the hood

**LLVM's back end** [LIB] runs the same three jobs at production strength:

- **Instruction selection** matches patterns in the IR (as a *SelectionDAG*, or with the newer *GlobalISel* framework
  on some targets) and picks instructions: `x * 3 + 1` becomes `lea rax, [rdi + 2*rdi]; inc rax` (Chapter 17.1), and a
  64-bit division gets a 32-bit fast path (Chapter 17.7).
- **Register allocation** uses LLVM's *greedy* allocator by default (since LLVM 3.0, 2011): it assigns the most
  expensive live ranges first, *splits* ranges so a value can be in a register in hot code and on the stack elsewhere,
  and chooses what to spill by a *spill weight* that counts uses and scales them by loop depth. Cranelift, rustc's
  alternative back end for fast debug builds, uses the `regalloc2` allocator [LIB].
- **Emission** produces an object file: machine code, a symbol for each function (v0-mangled names, Chapter 7.1), and
  *relocations* for every reference whose final address the linker decides.

Listing `ch08-02-rustc-regs.rs` shows the calling convention and allocation decisions on three small functions (release
assembly, real output). First, a function with seven integer arguments:

```rust,ignore
#[inline(never)]
pub fn seven(a: i64, b: i64, c: i64, d: i64, e: i64, f: i64, g: i64) -> i64 {
    a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g
}
```

```text
playground::seven:
	mov	r10, qword ptr [rsp + 8]
	lea	rax, [rdi + 2*rsi]
	lea	rdx, [rdx + 2*rdx]
	add	rdx, rax
	lea	rax, [rdx + 4*rcx]
	lea	rcx, [r8 + 4*r8]
	add	rcx, rax
	lea	rax, [r9 + 2*r9]
	lea	rax, [rcx + 2*rax]
	lea	rax, [rax + 8*r10]
	sub	rax, r10
	ret
```

The first six arguments arrive in `rdi, rsi, rdx, rcx, r8, r9` [LANG: the System V ABI]; the seventh is on the stack
at `[rsp + 8]` (`[rsp]` holds the return address). Instruction selection turned every multiplication into `lea`
address arithmetic, and `7 * g` into `8g - g` (`lea rax, [rax + 8*r10]` then `sub rax, r10`). No value spills:
pressure never exceeds the registers available.

Second, values that must survive a call:

```rust,ignore
/// `p`, `q`, `x`, `y` are needed after the call: they must survive it.
#[inline(never)]
pub fn across_call(x: i64, y: i64) -> i64 {
    let p = x * 3;
    let q = y * 5;
    let r = opaque(p + q);
    r + p + q + x + y
}
```

```text
playground::across_call:
	push	r15
	push	r14
	push	rbx
	mov	rbx, rsi
	mov	r14, rdi
	lea	rax, [rdi + 2*rdi]
	lea	r15, [rsi + 4*rsi]
	add	r15, rax
	mov	rdi, r15
	call	qword ptr [rip + playground::opaque@GOTPCREL]
	add	r14, rbx
	add	r14, r15
	add	rax, r14
	pop	rbx
	pop	r14
	pop	r15
	ret
```

The System V convention divides registers into *caller-saved* (a callee may overwrite them: `rax, rcx, rdx, rsi, rdi,
r8–r11`) and *callee-saved* (a callee must restore them: `rbx, rbp, r12–r15`). `x`, `y`, and `p + q` are needed after
the call, so the allocator put them in callee-saved registers (`r14`, `rbx`, `r15`), and the function saves and restores
those three with `push`/`pop`: that's the price of keeping values alive across a call. Two optimizer decisions are also
visible. `p + q` is computed once and serves both as the argument and as part of the result (GVN). And the final sum is
*reassociated* to `r + ((x + y) + (p + q))`, which is legal because integer addition in release mode wraps and wrapping
addition is associative. The call goes through the GOT (`@GOTPCREL`), the position-independent form that the linker may
relax into a direct call (Part XIX).

Third, `opaque` is `black_box`:

```text
playground::opaque:
	mov	qword ptr [rsp - 8], rdi
	lea	rax, [rsp - 8]
	#APP
	#NO_APP
	mov	rax, qword ptr [rsp - 8]
	ret
```

The value is stored *below* the stack pointer, at `[rsp - 8]`, without adjusting `rsp`. That's the System V **red
zone**: a leaf function may use 128 bytes below `rsp` as scratch space, because nothing (not even a signal handler,
which the kernel places further down) will overwrite it. The empty inline-assembly block between `#APP` and
`#NO_APP` is the barrier: it claims to read the memory, so the value must really be stored there.

**Linking** [OS]. The object file isn't a program yet. A call to `opaque` in another object file, or into the standard
library, is a relocation: "put the address of symbol X here". The linker (`cc` driving `ld`, or `lld`) combines object
files, resolves every symbol, applies relocations, and writes the executable. Part XIX follows that process in detail;
from a compiler's point of view, codegen's contract with the linker is "one object file per codegen unit, with symbols
and relocations", which is what lets LLVM compile codegen units on separate threads (Chapter 7.3).

### 5. Memory

A spill is a stack slot, and spill slots make up much of a function's **stack frame**. Listing 17.8-1 at K = 4 used
seven slots (`[rsp+0]` to `[rsp+48]`, 56 bytes), plus the callee-saved registers a real function would push. That's
the mechanism behind Chapter 17.3's measurement: a recursive-descent parser used 1,968 bytes of stack per nesting level
in debug and 224 in release. A debug build spills *every* local (it keeps each variable in its own stack slot so a
debugger can find it: the `alloca`s of Chapter 17.6), so its frames are large; a release build keeps most values in
registers.

Three details of frame layout come from the ABI [LANG: System V]:

- The stack pointer must be 16-byte aligned at a `call` (which is why functions sometimes `push` a register they don't
  need: `divide` in Chapter 17.7 does `push rax`).
- The red zone (128 bytes below `rsp`) lets leaf functions skip adjusting `rsp` at all.
- Callee-saved registers are saved in the prologue and restored in the epilogue, so a function that needs many values
  across calls pays a fixed cost per call.

### 6. CPU / OS

**What a spill costs.** A register operand costs nothing extra. A memory operand that hits the L1 cache costs a few
cycles of latency (on the order of 4–5 cycles load-to-use on current x86 cores: order of magnitude, not measured here),
and store-to-load forwarding makes a reload of a just-spilled value cheaper than a cache access, but not free. Listing
17.8-1's loop at K = 4 executes 138 memory operands where K = 12 executes 15: about 9× more memory traffic for the same
work. Out-of-order execution hides much of that latency when there's independent work, and very little when the
spilled value is on the loop's critical path, as `i` and `acc` are here (each iteration needs the previous iteration's
value).

**Register renaming doesn't remove spills.** Modern x86 cores have far more *physical* registers than the 16
architectural ones (on the order of 200), and rename architectural registers to physical ones to remove false
dependencies [CPU]. That lets reused registers (`rdi` and `rsi` alternating in the K = 4 loop) run in parallel. It
can't help a value that the *compiler* put in memory: the load and store are real instructions.

**Compile time.** Register allocation is one of the more expensive back-end phases. Linear scan is linear in the
number of intervals (after sorting); graph coloring needs the interference graph, which can be quadratic in the number
of values; LLVM's greedy allocator sits in between. That's one reason debug builds are faster to compile, and one
reason JITs, which compile at run time, favor linear scan (§8).

---

## Pass 3 · Architect level — *Should your language generate machine code at all?*

### 7. Trade-offs

**Register allocators.**

| Allocator | Code quality | Compile time | Used by |
|---|---|---|---|
| Local (per basic block) | poor across blocks | fastest | simple JITs, baseline compilers |
| Linear scan (listing 17.8-1) | good; weak spill choices without weights | linear | HotSpot C1, GraalVM, many JITs |
| Linear scan with interval splitting | very good | near-linear | C1 (Wimmer and Mössenböck, 2005) |
| Graph coloring (Chaitin–Briggs) | very good | higher, quadratic worst case | HotSpot C2, classic AOT compilers |
| Greedy / backtracking with splitting | excellent | moderate | LLVM (greedy), Cranelift (`regalloc2`) |

**Evaluation strategies for a language you own.** Chapter 17.1's table becomes concrete here:

| Strategy | Per evaluation (Chapter 10.1, one run) | What you must build | Operational risk |
|---|---|---|---|
| Tree-walking interpreter | 11.45 ns/record | an evaluator | lowest |
| Closure compilation | 6.28 ns/record | a compile-to-closures pass | low |
| Bytecode VM | between the two above (predicted, not measured) | a compiler + a VM loop | low |
| JIT to machine code (e.g., Cranelift) | close to hand-written (predicted) | a code generator, executable memory | executable memory in a service; a new class of bugs |
| Ahead-of-time code (hand-written or generated Rust) | 0.89 ns/record | a build and deploy per rule change | rules change without deploys only if you add dynamic loading |

> **Why not generate machine code for rules?** Because the win is a few nanoseconds per evaluation and the cost is
> permanent. A JIT needs writable-then-executable memory in a production service (which hardening policies such as
> W^X forbid or complicate), a code generator whose bugs are silent wrong answers, and a security review for "user
> input becomes machine code". Chapter 10.1 measured closure compilation at 6.28 ns per record: for 2 million
> evaluations per second, about 1.3% of one core. Generate machine code when profiling says evaluation dominates
> *and* you can afford a compiler team.

### 8. Java comparison

HotSpot's two JIT compilers made opposite register-allocation choices, for the reasons in §6 [LIB]. **C1**, the fast
tier, uses linear scan with interval splitting (Wimmer and Mössenböck, "Optimized Interval Splitting in a Linear Scan
Register Allocator", 2005), because it compiles many methods quickly. **C2**, the optimizing tier, uses graph coloring,
because it compiles only hot methods and can afford the time. GraalVM's compiler uses linear scan by default. All of
them allocate at *run time*, inside the JVM process, so allocation time is paid on the application's CPUs during
warm-up.

Compiled Java methods also use their **own calling convention** internally, not the platform's C ABI; calls between
Java and native code go through adapter stubs (the JNI and FFM transitions of Part XVI) that move arguments into the C
convention [LIB]. rustc uses the platform C ABI for `extern "C"` functions and an unspecified Rust ABI for everything
else (Chapter 2.4), which is why Rust functions called from Java must be `extern "C"`.

> **Analogy limit.** "LLVM's register allocator is C2's" is wrong on budget, not on algorithms. C2 must finish in
> milliseconds per method while the application waits for the compiled code, and it can recompile later with better
> profiles. LLVM can spend seconds on a large function at build time and never revisits it. The same algorithm is
> tuned differently: a JIT accepts worse spills to start sooner; an AOT compiler accepts slower builds to produce
> better code once.

### 9. Production scenario

**Sieve's evaluation back end.** Sieve evaluated three back ends against the rule set and the fraud team's latency
budget (p99 under 5 ms for the whole score, Chapter 1.2):

- A **tree-walking interpreter** over the checked AST, kept as the *reference implementation*: simple enough to be
  obviously correct, and the oracle for everything else.
- **Closure compilation** of the rule IR (Chapter 10.1's technique), the production back end. Each rule compiles to
  nested closures once, when its service loads it. Intermediate values live in a small per-evaluation array of
  **slots**, reused between values whose lifetimes don't overlap, so an evaluation allocates nothing. Assigning slots is
  register allocation with K unbounded, using listing 17.8-1's live intervals.
- **A Cranelift JIT** prototype, rejected: faster than closures on arithmetic-heavy rules in the prototype's own
  benchmark, and irrelevant in practice, because evaluation time is dominated by feature fetches, not arithmetic. It
  would have required executable memory in the fraud service and a new security review.

The reference interpreter earns its keep in **differential testing**, the same idea as listing 17.8-1's emulator. Every
rule compiled by the closure compiler is run next to the reference interpreter on a replay of recorded evaluations in
CI, and on a 1% sample of live evaluations in shadow mode. Any difference fails the build or pages the team.

### 10. Failure scenario

**The slot that was reused too early.** Sieve v0.8 (September 2026) added list quantifiers, the language's first
loops: `any(c in high_risk_countries: c == country && amount > EUR 500.00)`. The closure compiler's slot allocator
computed each value's lifetime from its definition to its last *textual* use, a shortcut that was correct as long as
rules had no loops. Inside a quantifier, a value computed before the loop and used at the top of each iteration is
live across the back edge, all the way to the end of the loop body. The shortcut ended its lifetime at its first use,
and the next value computed in the loop body was given the same slot.

On the first iteration everything was correct. From the second on, the loop read an overwritten value. Listing
`ch08-03-intervals-bug.rs` reproduces the mechanism on listing 17.8-1's program: it's the same allocator with one
change, intervals computed from uses and definitions only (live-out sets ignored). The intervals shrink (real output):

```text
live intervals: v1:[0,11] v2:[1,12] v3:[2,13] v4:[3,14] v5:[4,15] v6:[5,16] v7:[6,17] v8:[7,22] v9:[8,19] v10:[11,12] v11:[12,13] v12:[13,14] v13:[14,15] v14:[15,16] v15:[16,17] v16:[17,18]
```

and the differential check fails at the first K it tries (real output):

```text
assertion `left == right` failed: K=2 args=[5, 3, 4]
  left: -15318
 right: -190
```

`v1` (`a`) now ends at 11, its last textual use, instead of 20; its register is handed to a loop temporary, and the
second iteration multiplies by the temporary instead of `a`.

In Sieve, shadow mode caught it before any decision was affected: in the first hour, 3 of the 11 rules that used
quantifiers disagreed with the reference interpreter on about 0.4% of evaluations (the ones whose lists had more than
one element). The fix was to compute slot lifetimes from liveness, as listing 17.8-1 does, rather than from text
positions. The lesson generalizes beyond compilers: **lifetimes must come from the control-flow graph, not from the
order in which the code is written**, which is also, not by coincidence, the idea behind Rust's non-lexical lifetimes
(Chapter 4.2).

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVII).*

1. What are the three main jobs of a code generator, and what does each one decide?
2. What is a live interval, and why does a value used at the top of a loop stay live until the loop's back edge?
3. Explain linear scan register allocation, including the spill heuristic in listing 17.8-1. What does it ignore?
4. Compare linear scan and graph coloring on code quality and compile time. Why does HotSpot use one for C1 and the
   other for C2?
5. In System V x86-64, where do the first six integer arguments go, and the seventh? What are caller-saved and
   callee-saved registers, and how do they explain `across_call`'s `push`/`pop`?
6. What is the red zone, and why can `opaque` store below `rsp`?
7. Why is a debug build's stack frame larger than a release build's for the same function? Relate it to Chapter
   17.3's 1,968 versus 224 bytes per nesting level.
8. How can you test a code generator whose output you can't execute? What is the oracle in listing 17.8-1?
9. Why doesn't register renaming in the CPU make spills free?
10. Your team proposes JIT-compiling rules to machine code. Give the measurements you'd ask for and the operational
    objections you'd raise.

### 12. Exercises

- **Beginner.** In listing 17.8-1's K = 4 output, map every virtual register to its location (register or slot) using
  the generated code. Check your map against the linear-scan rules.
- **Intermediate.** Add a `mul`-by-constant instruction selection rule to listing 17.8-1: `v = a * 3` becomes
  `lea d, [a + 2*a]`, and `v = a * 8` becomes a shift. Extend the emulator to execute them, and keep the differential
  check passing.
- **Advanced.** Implement interval *splitting* in listing 17.8-1: instead of spilling a whole interval, keep it in a
  register before the loop and in a slot inside it (or the reverse). Measure memory operands executed at K = 4.
- **Systems.** Emit listing 17.8-2 in debug mode. Count the stack slots and memory operands in `across_call` and compare
  them with release. Which values does the debug build keep in memory, and why?
- **Architecture.** Write the decision record for Sieve's back end (§9): options, measurements, the reason the JIT was
  rejected, and the conditions under which you'd revisit the decision.

### 13. Debugging exercise

Listing 17.8-1's K = 4 code keeps the loop counter `i` (`[rsp+32]`) and the accumulator `acc` (`[rsp+24]`) in memory,
while `a` and `b`, used once per iteration, have registers. The generated loop executes 138 memory operands for
`n = 10`.

1. Replay the linear scan for K = 4 by hand using the printed intervals. At which interval does the first spill happen,
   and why does the heuristic spill `v5` (`e`) *itself* rather than an active interval? Why are `v8` (`acc`) and `v9`
   (`i`) spilled?
2. What property of `i` and `acc` does the heuristic ignore? Count each value's uses per loop iteration.
3. Propose a spill weight, as production allocators use, and predict which values it would keep in registers at K = 4.
   How would you verify that the change is both correct and better?

### 14. Design exercise

**Design the back end for a pricing-rules engine** at Meridian's marketplace: about 3,000 pricing rules, each a small
arithmetic expression over 5–40 features (fees, discounts, taxes, currency rounding), evaluated about 400,000 times
per second at checkout, with a p99 budget of 1 ms for the whole pricing call. Rules change several times a day. Decide:

- Interpreter, closure compilation, bytecode, or code generation, with the measurements that would justify each step up.
- How intermediate values are stored (slots, a stack, registers of a VM) and how their lifetimes are computed.
- The oracle and the differential tests, including exact money rounding (Chapter 2.3) as a property to check.
- How a bad rule, or a bug in the back end itself, is rolled back without a deploy.
