# Part XVII Review — Sieve's First Compiler & Interview Mode

> Consolidate Part XVII, then use it: review the first pull request for Sieve, Meridian's rule language. The compiler
> is 130 lines, runs its demo, and has at least nine defects spread across every stage this Part built. Then answer
> senior-level questions without notes. Answers are in **Appendix A, Part XVII**.

---

## Part XVII on one page

```text
 LEXER        bytes -> (kind, span); maximal munch; errors are tokens; spans not text (0 vs 516,192 allocations)
              a lexer is a DFA (388 bytes of tables); the lexer decides what text may LOOK like (confusables, bidi)
     │
 PARSER       recursive descent (one fn per level) or Pratt (binding powers: l<r left-assoc, l>r right-assoc)
              comparisons non-associative; synchronize after errors (4 real errors vs 7 with cascades)
              nodes: Box 1,500,001 allocs vs arena 21; stack: 1,968 B/level debug, 224 release -> DEPTH LIMIT
     │
 RESOLVER     items first, then bodies; innermost scope wins; initializer before declaration; namespaces
              side tables keyed by node id; interning (200,017 -> 2,039 allocs); hygiene = (symbol, context)
              a stored program must keep its names' MEANING: store resolved ids + table version
     │
 TYPES        bidirectional: infer bottom-up, check pushes expectations down; {error} stops cascades
              HM: variables + unification (union-find) + occurs check + generalization (let-polymorphism)
              Rust: infer inside bodies, never signatures (E0121); closures not generalized
     │
 IR           basic blocks + terminators; dominators, frontiers, back edges; SSA with phis (Cytron et al.)
              dataflow: forward/backward x must/may; must starts at TOP, may at BOTTOM
              MIR = CFG over places (not SSA) for borrowck; LLVM IR = SSA (mem2reg/SROA from allocas)
     │
 OPTIMIZER    fold, propagate, GVN over the dominator tree, DCE, merge blocks (19 -> 4 instructions)
              LEGALITY: never remove, add, or move a run-time error; may_trap guards DCE, GVN, LICM
              LLVM: closed forms (SCEV), versioned LICM, vectorization; UB and data-race freedom are licenses
     │
 BACK END     instruction selection (two-address, lea); liveness -> intervals -> linear scan / coloring
              spills = stack slots (K=4: 138 memory operands vs 15 at K=12); System V: args, callee-saved, red zone
              when you can't run the output, EMULATE it and compare with an interpreter
```

## Ten ideas to carry forward

1. **Each mistake is reported by the earliest stage that has the information.** Design a language by deciding which
   stage owns each error class.
2. **Spans are the product.** Every stage keeps them, because every error message is only as good as its location.
3. **Errors are values in every stage**: error tokens, error nodes, the `{error}` type. One mistake, one message.
4. **Precedence and associativity are data**, and a table change is a semantic change: test every pair of operators.
5. **If input controls recursion depth, enforce a limit** (or grow the stack, or use an explicit stack). A stack
   overflow in Rust takes the whole process down.
6. **Resolve names once and store the answers.** A stored program re-resolved against a changed symbol table has
   silently changed meaning.
7. **Infer inside, annotate at the boundaries.** Signatures are contracts, and inference across them makes errors
   drift and compilation non-local.
8. **Lower to a CFG before asking control-flow questions**, and choose must or may deliberately: they answer different
   questions.
9. **An optimizer may remove work, never errors.** In a safe language, run-time errors are observable behavior.
10. **Every translation needs an oracle.** Two parsers that must agree, an interpreter next to the optimized IR, an
    emulator next to the generated code, shadow mode next to production.

---

## Capstone: review Sieve's first compiler

A teammate submits `sieve-compiler v0.1` (listing `review-01-sieve-pr.rs`; verified to compile and run). Sieve rules
look like `amount > 1000 && country == "DE"`. The public entry point is `check(rule, record)`. The demo harness in the
listing's `main` isn't part of the review.

```rust,ignore
#[derive(Debug, Clone, PartialEq)]
enum Tok { Ident(String), Int(i64), Str(String), Op(&'static str) }

const OPS: [&str; 10] = ["==", "!=", "<=", ">=", "&&", "||", "<", ">", "*", "+"];

fn lex(src: &str) -> Vec<Tok> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() { i += 1; }
            let s: String = chars[start..i].iter().collect();
            out.push(Tok::Int(s.parse().unwrap()));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') { i += 1; }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else if c == '"' {
            let start = i + 1;
            i += 1;
            while chars[i] != '"' { i += 1; }
            out.push(Tok::Str(chars[start..i].iter().collect()));
            i += 1;
        } else {
            let rest: String = chars[i..].iter().take(2).collect();
            let op = OPS.iter().find(|op| rest.starts_with(**op)).expect("unknown operator");
            out.push(Tok::Op(*op));
            i += op.len();
        }
    }
    out
}

#[derive(Debug, Clone)]
enum Expr { Int(i64), Str(String), Feature(String), Bin(&'static str, Box<Expr>, Box<Expr>) }

fn prec(op: &str) -> u8 {
    match op {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" | "<" | "<=" | ">" | ">=" => 3,
        "+" => 4,
        "*" => 5,
        _ => 0,
    }
}

fn parse(toks: &[Tok], at: &mut usize, min_prec: u8) -> Result<Expr, String> {
    let mut lhs = match toks.get(*at) {
        Some(Tok::Int(n)) => Expr::Int(*n),
        Some(Tok::Str(s)) => Expr::Str(s.clone()),
        Some(Tok::Ident(f)) => Expr::Feature(f.clone()),
        _ => return Err("parse error".into()),
    };
    *at += 1;
    while let Some(Tok::Op(op)) = toks.get(*at) {
        let p = prec(op);
        if p < min_prec { break; }
        *at += 1;
        let rhs = parse(toks, at, p + 1)?;
        lhs = Expr::Bin(*op, Box::new(lhs), Box::new(rhs));
    }
    Ok(lhs)
}

/// Constant folding, so rules like `amount > 1000 * 1000` cost nothing at run time.
fn fold(e: Expr) -> Expr {
    match e {
        Expr::Bin(op, a, b) => match (op, fold(*a), fold(*b)) {
            ("*", Expr::Int(x), Expr::Int(y)) => Expr::Int(x * y),
            ("+", Expr::Int(x), Expr::Int(y)) => Expr::Int(x + y),
            (op, a, b) => Expr::Bin(op, Box::new(a), Box::new(b)),
        },
        other => other,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum V { Int(i64), Str(String), Bool(bool) }

struct Record<'a> {
    fields: HashMap<&'static str, V>,
    fetch_graph_score: &'a dyn Fn() -> i64, // remote call to the graph service
}

fn feature(r: &Record, name: &str) -> V {
    if name == "graph_score" {
        return V::Int((r.fetch_graph_score)());
    }
    r.fields.get(name).cloned().unwrap_or(V::Int(0))
}

fn eval(e: &Expr, r: &Record) -> V {
    match e {
        Expr::Int(n) => V::Int(*n),
        Expr::Str(s) => V::Str(s.clone()),
        Expr::Feature(f) => feature(r, f),
        Expr::Bin(op, a, b) => {
            let (x, y) = (eval(a, r), eval(b, r));
            let as_int = |v: &V| match v { V::Int(n) => Some(*n), V::Bool(b) => Some(*b as i64), V::Str(_) => None };
            V::Bool(match *op {
                "&&" => x == V::Bool(true) && y == V::Bool(true),
                "||" => x == V::Bool(true) || y == V::Bool(true),
                "==" => x == y,
                "!=" => x != y,
                cmp => match (as_int(&x), as_int(&y)) {
                    (Some(p), Some(q)) => match cmp { "<" => p < q, "<=" => p <= q, ">" => p > q, _ => p >= q },
                    _ => false,
                },
            })
        }
    }
}

/// The entry point: evaluate a rule against a record.
fn check(rule: &str, r: &Record) -> Result<bool, String> {
    let toks = lex(rule);
    let e = fold(parse(&toks, &mut 0, 0)?);
    Ok(eval(&e, r) == V::Bool(true))
}
```

(Excerpt of listing R-1: the compiler, without the `use` line and the harness.) The author's demo harness runs seven
rules against sample records, catching panics so it can report them, and then one rule over 10,000 records with a
counting stub for the remote `graph_score` feature (which always returns 50). Real output:

```text
[1] country == "DЕ"                                            -> Ok(false)
[2] amount > 99999999999999999999                              -> COMPILER PANICKED: called `Result::unwrap()` on an `Err` value: ParseIntError { kind: PosOverflow }
[3] 1 < amount < 1000                                          -> Ok(true)
[4] velocty_1h > 20                                            -> Ok(false)
[5] amount > "1000"                                            -> Ok(false)
[6] amount > 1000 * 1000 * 1000 * 1000 * 1000 * 1000 * 1000    -> COMPILER PANICKED: attempt to multiply with overflow
[7] amount >                                                   -> Err("parse error")
[8] amount > 100 && graph_score > 80: 0 matches, graph_score fetched 10000 times for 10,000 records
```

(Rule [1] contains a Cyrillic `Е`, U+0415, as in Chapter 17.2's incident; rule [3] ran against an amount of 5,000; rule
[4] against a record whose `velocity_1h` is 50.)

**Your review.**

1. Find **at least nine** defects. For each, name the stage that should have caught it, the chapter that covers the
   technique, and what happens in production (who notices, and when).
2. Rank them by severity. Which ones *silently change a decision*? Which crash the compiler (and what does a compiler
   crash do to the rule service)? Which cost *another* team's service?
3. One finding is architectural rather than a bug: look at what `check` does on every call. Where should each stage run,
   given Chapter 17.1's Sieve design? What does the current design cost at 2 million evaluations per second?
4. Rule [3] returned `true` for an amount of 5,000. Explain *exactly* how, naming the two stages that each contributed
   one mistake.
5. For each of rules [1] through [7], write the error message the fixed compiler should produce, with the span it should
   point at.

The redesign (listing `review-02-sieve-fixed.rs`, verified; its three tests pass) compiles each rule once
(`Rule::compile`), with a lexer that keeps spans and errors, a parser with non-associative comparisons and a depth limit,
a checker against the feature schema that also folds constants with checked arithmetic, and a short-circuiting
evaluator. On the same inputs it prints (real output):

```text
[1] error: non-ASCII character U+0415 in a string literal (Sieve literals are ASCII)
      | country == "DЕ"
      |              ^
[2] error: integer literal is too large
      | amount > 99999999999999999999
      |          ^^^^^^^^^^^^^^^^^^^^
[3] error: comparison operators cannot be chained; use `&&`
      | 1 < amount < 1000
      |            ^
[4] error: unknown feature `velocty_1h`; did you mean `velocity_1h`?
      | velocty_1h > 20
      | ^^^^^^^^^^
[5] error: mismatched types: Int > Str
      | amount > "1000"
      | ^^^^^^^^^^^^^^^
[6] error: constant expression overflows i64
      | amount > 1000 * 1000 * 1000 * 1000 * 1000 * 1000 * 1000
      |          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
[7] error: expected a value or a feature name
      | amount >
      |         ^
[8] 0 matches, graph_score fetched 4950 times for 10,000 records
```

Every defect became a compile-time error with a span, except the last line, which shows the evaluation fix:
`graph_score` is fetched only when `amount > 100` is true (99 of every 200 records), less than half as often. Write
your review first, then compare it with the redesign and the model answers. The redesign is one defensible answer, not
the only one.

---

## Interview mode

*Senior-level. Answer aloud or in writing, without notes, before checking Appendix A.*

### Language design

1. You're designing a rule language for analysts. Which errors must be caught at upload time, which at evaluation time,
   and which are impossible to catch at all? Organize your answer by compiler stage.
2. Why are comparison operators non-associative in Rust? What would you choose for a DSL, and why?
3. Rust allows shadowing with `let`, forbids inferring signatures, and doesn't generalize closures. Defend each choice,
   then argue the opposite for one of them.

### Compiler internals

4. Walk `a + 1 < b * 2 && c` through a lexer, a Pratt parser (with binding powers), a type checker, and a lowering to
   basic blocks. Show the output of each stage.
5. What is SSA, why is rustc's MIR not in SSA, and how does LLVM IR end up in SSA anyway?
6. Explain definite assignment and liveness as dataflow problems (direction, meet, initial value). What goes wrong if
   you initialize a must-analysis to the empty set?
7. Explain linear scan register allocation, and why a value used only at the top of a loop is live until the back
   edge.

### Performance

8. Where does a compiler's memory go, and what do arenas, interning, and side tables each save? Quote numbers.
9. How much stack does a recursive-descent parser use per nesting level, why is debug so different from release, and
   what are the three ways to make deep input safe?
10. LLVM removed the loop from `triangle` and from `guarded_sum`. Explain both, and what they imply for benchmarking.

### Architecture

11. Tree-walking interpreter, closure compilation, bytecode VM, JIT, or AOT: choose for a rule engine at 2 million
    evaluations per second, and say what measurement would change your mind.
12. How do you roll out a new compiler (or a new optimizer pass) for a language with thousands of stored programs,
    without changing a single decision by accident?
13. A shared symbol table (a feature catalog) changes several times a day. How do you keep stored programs' meanings
    stable?

### Testing and operations

14. Name four oracles this Part used to test compiler stages, and what class of bug each catches.
15. The optimizer hoisted a division out of its guard in production. Walk through detection, mitigation, root cause,
    and the three process changes you'd make.

---

## Looking ahead: Part XVIII

Part XVII built a compiler from general techniques and kept pointing at rustc as the reference implementation, always
hedged: "[RUSTC], as of 2026". Part XVIII opens rustc itself: the query system and incremental compilation, macro
expansion interleaved with name resolution, HIR and its desugarings, type checking with the trait solver, THIR and MIR,
borrow checking on MIR, and monomorphization and code generation, ending with one `let x = foo();` traced through every
stage with real compiler output. Ore's pipeline is the map; Part XVIII is the territory. Part XXVI then returns to Ore
and grows it into a language with ownership and a borrow checker of its own.
