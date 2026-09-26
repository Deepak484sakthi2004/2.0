# Part XVII — Compilers

> **Part question:** *What does a compiler actually do between your source file and the machine, stage by stage, and
> what would it take for you to build one?*

Until now this book has used the compiler as an instrument: we asked rustc for MIR, LLVM IR, and assembly, and read the
answers. Part XVII turns you from a user of compilers into someone who can build one. It covers the general theory,
the techniques that every production compiler uses in some form, and working, tested implementations of each of them in
Rust.

The Part has one running example, a tiny language called **Ore**. It has functions, `let` and `let mut`, `if`/`else` as
expressions, `while`, `return`, integer and boolean values, and calls. Each chapter adds one stage to Ore's compiler:
a lexer, two parsers, a name resolver, a type checker and a type inferencer, a CFG builder, SSA construction, an
optimizer, and a register allocator. Every stage is a verified listing, and every stage's output is checked by running
the program: an interpreter for each IR, and an emulator for the generated assembly. Part XXVI grows Ore (the name is a
nod to Ferrite: this is the raw material) into a Rust-like language with ownership and a borrow checker.

The other running thread is Meridian's **Sieve**, a small typed rule language that the risk platform introduces in 2026
to replace JSON rules (the Part IX interlude's crash loop) and ad hoc rule engines (Chapter 6.5). Each chapter's production
and failure scenarios are a stage of Sieve's compiler, and the Part review is a code review of Sieve's first compiler.

Part XVIII then opens rustc itself. This Part stays general, and uses rustc, LLVM, javac, and HotSpot as reference
implementations, with every internal detail tagged [RUSTC] or [LIB] and hedged, because internals change.

## Chapter map

```text
17.1 The Compiler Pipeline End to End   a whole compiler in one file; which stage catches which mistake; one Rust
                                         function seen as HIR, MIR, LLVM IR, and assembly
      │
17.2 Lexing                              bytes -> tokens with spans; maximal munch; a lexer as a DFA; zero-copy tokens
                                         (0 vs 516,192 allocations); Unicode confusables and rustc's defenses
      │
17.3 Parsing                             recursive descent and Pratt parsing; precedence and associativity as data;
                                         error recovery; arenas vs Box (1,500,001 vs 21 allocations); stack depth limits
      │
17.4 ASTs, Symbol Tables, Resolution     visitors; node layout (72 -> 12 bytes); interning; scopes, namespaces,
                                         shadowing, hygiene; "did you mean" suggestions
      │
17.5 Type Checking and Inference         bidirectional checking with an {error} type; Hindley-Milner unification with
                                         union-find; let-polymorphism; where Rust draws its inference boundaries
      │
17.6 IRs, CFGs, and SSA                  lowering to basic blocks; dominators and dominance frontiers; SSA construction;
                                         dataflow (definite assignment, liveness); rustc's MIR (not SSA) and LLVM IR (SSA)
      │
17.7 Optimization                        folding, GVN, DCE, CFG simplification with legality checks; LICM and traps;
                                         what LLVM does with the same ideas (a loop that disappears)
      │
17.8 Code Generation and Register        instruction selection; liveness intervals; linear scan with spilling; an
     Allocation                          emulator as the test oracle; calling conventions in rustc's output; linking
      │
Part XVII Review                         review Sieve's first compiler (a PR with nine defects); interview mode
```

## What you'll be able to do after Part XVII

- Explain what each stage of a compiler knows, decides, and forgets, and predict which stage reports a given mistake.
- Write a lexer, a recursive-descent or Pratt parser, a resolver, and a type checker for a small language, with spans,
  error recovery, and depth limits: the parts every DSL at work needs.
- Implement unification-based type inference and explain where Rust, Java, and ML draw their inference boundaries.
- Lower a language to a CFG, build SSA form, and run dataflow analyses to a fixpoint.
- Implement and justify optimizations, including the legality condition that separates a correct optimizer from one that
  introduces crashes.
- Read a register allocator's output, and rustc's, in terms of liveness, spills, and calling conventions.
- Decide, as an architect, between an interpreter, closure compilation, a bytecode VM, a JIT, and ahead-of-time
  compilation for a rule engine or DSL.

## Listings

`listings/part-17/`: 43 files and 50 checks, all verified on rustc 1.98.1 (edition 2024) with `tools/verify.ps1`,
including 8 intended compile errors (rustc's own lexer, parser, resolver, and checker diagnostics), one intended stack
overflow, and three debugging-exercise variants that misbehave on purpose (`ch06-06`, `ch07-04`, `ch08-03`: each is an earlier
listing with one line changed). Several larger listings are assembled from the same verified pieces: the Ore lexer and
parser (listing 17.3-1) appear unchanged in the resolver, checker, inference, CFG, SSA, and optimizer listings, so each
file is self-contained. Compiler artifacts (HIR, MIR, LLVM IR, assembly) come from `tools/emit.ps1`.
