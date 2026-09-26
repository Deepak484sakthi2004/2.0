# Chapter 17.3 — Parsing: Recursive Descent and Pratt Parsing

> **Where this sits:** Part XVII · Compilers · chapter 3 of 8
> **Prerequisites:** Chapter 17.2 (tokens and spans); Chapter 2.4 (stack frames); the Part IX interlude (stack depth,
> guard pages, and why a stack overflow aborts a Rust process).
> **After this chapter you can:** write a recursive-descent parser and a Pratt parser for the same expression grammar
> and prove they agree; express precedence and associativity as data; recover from syntax errors without cascades;
> choose where AST nodes live using measured allocation counts; and put a depth limit where input controls recursion.

---

## Pass 1 · User level — *From a flat list to a tree*

### 1. Problem

The lexer produced a flat sequence: `Ident(a) Plus Int(1) Lt Ident(b) Star Int(2) AndAnd Ident(c)`. The parser's job is
to recover the *tree* the author meant:

```text
            &&
          /    \
         <      c
       /   \
      +     *
     / \   / \
    a   1 b   2
```

Four questions decide that tree, and every one of them has caused a production incident somewhere:

- **Precedence.** Does `*` bind tighter than `+`, and `&&` tighter than `||`? (The Sieve incident in §10 is a
  precedence bug.)
- **Associativity.** Is `1 - 2 - 3` equal to `(1 - 2) - 3` or `1 - (2 - 3)`? Is `a = b = c` legal, and which way does
  it group? Is `a < b < c` meaningful at all?
- **Errors.** When the input is wrong, where does the parser say so, and can it keep going to find the next mistake?
- **Depth.** Every parser of nested structure recurses somewhere. If the *input* decides how deep, the input decides
  whether the process survives (the Part IX interlude's crash loop).

This chapter parses Ore twice, first by recursive descent and then by Pratt parsing, checks that both produce the same
trees, and then measures the two costs every parser pays: memory for the tree and stack for the recursion.

### 2. Mental model

A grammar says what well-formed programs look like. Ore's expression grammar, one line per precedence level from
loosest to tightest:

```text
expr    := assign
assign  := or ("=" assign)?                 right-associative
or      := and ("||" and)*                  left-associative
and     := cmp ("&&" cmp)*
cmp     := add (cmp_op add)?                non-associative: a < b < c is an error
add     := mul (("+" | "-") mul)*
mul     := unary (("*" | "/" | "%") unary)*
unary   := ("-" | "!") unary | postfix
postfix := primary ("(" args ")")*          calls: f(1)(2)
primary := INT | "true" | "false" | IDENT | "(" expr ")" | block | if | while | return
```

**Recursive descent** turns each line into a function. `add` calls `mul` to get an operand, and loops while it sees `+`
or `-`. Precedence falls out of the call structure: `*` binds tighter because `mul` runs *inside* `add`. The call stack
at any moment mirrors the path from the root of the tree to the node being built.

**Pratt parsing** (Vaughan Pratt, 1973, "Top Down Operator Precedence") replaces the ladder of functions with one loop
and a table. Each infix operator has two *binding powers*, a left one and a right one:

```text
          ||      &&      <       +       *
   l,r:  3,4     5,6     7,8    9,10   11,12        higher binds tighter

   a + b * c:  after `a`, the loop sees `+` (l=9 >= min 0), parses the right side with min 10;
               inside, after `b`, it sees `*` (l=11 >= 10): so `*` grabs `b` first.
```

Associativity is the *asymmetry* of the pair. For left-associative `+` the right power is higher (9, 10), so in
`1 - 2 - 3` the second `-` (left power 9) is too weak to continue inside the right operand (minimum 10), and the tree
leans left. For right-associative `=` the pair is reversed (2, 1). Non-associativity isn't expressible as powers
alone, so it's an explicit check. Both techniques are *top-down*: they decide what they're building before building
it. That makes them easy to write by hand and easy to give good error messages, which is why production compilers
use them.

### 3. Rust code

**Recursive descent.** Listing `ch03-01-recursive-descent.rs` is the Ore parser: one function per grammar rule and one
per precedence level, with a span on every node. The left-associative levels share one helper that *is* the grammar's
`X (op X)*` pattern:

```rust,ignore
    /// One left-associative precedence level: operand (op operand)*.
    fn left_assoc(&mut self, ops: &[(Tok, BinOp)], operand: fn(&mut Self) -> PResult<Expr>) -> PResult<Expr> {
        let mut lhs = operand(self)?;
        while let Some(&(_, op)) = ops.iter().find(|(t, _)| t == self.peek()) {
            self.bump();
            let rhs = operand(self)?;
            let span = lhs.span.to(rhs.span);
            lhs = Expr { kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span };
        }
        Ok(lhs)
    }

    fn or(&mut self) -> PResult<Expr> {
        self.left_assoc(&[(Tok::OrOr, BinOp::Or)], Self::and)
    }
    fn and(&mut self) -> PResult<Expr> {
        self.left_assoc(&[(Tok::AndAnd, BinOp::And)], Self::cmp)
    }
```

(Excerpt of listing 17.3-1.) The loop builds the tree leftward, which *is* left associativity: each new operator takes
the tree so far as its left child. Comparisons are handled differently: `cmp` parses at most one comparison and then
refuses a second one, so `a < b < c` is an error rather than a silent `(a < b) < c`. Parsing three functions prints
their ASTs as S-expressions (real output):

```text
(fn fib (n:int) -> int (block (if (< n 2) (block n) (block (+ (call fib (- n 1)) (call fib (- n 2)))))))
   span 1..78
(fn sum_to (n:int) -> int (block (let mut i 0) (let mut total 0) (while (< i n) (block (= i (+ i 1)) (= total (+ total i)))) total))
   span 80..230
(fn main () -> int (block (+ (call sum_to 10) (call fib 10))))
   span 232..277
fn f() { a < b < c }       error at 15..16: comparison operators cannot be chained; use `&&`, found Lt
fn g() { 1 + }             error at 13..14: expected an expression, found RBrace
fn h() { 3 = x; }          error at 9..10: invalid left-hand side of assignment
fn k() { let x = 1 x }     error at 19..20: expected `;`, found Ident("x")
```

The listing's tests fix precedence (`1 + 2 * 3` is `(+ 1 (* 2 3))`, `a || b && c` is `(|| a (&& b c))`, `!a == b`
is `(== (! a) b)`) and associativity (`1 - 2 - 3` is `(- (- 1 2) 3)`, `x = y = 3` is `(= x (= y 3))`,
`f(1)(2)` is `(call (call f 1) 2)`). All three tests pass.

**Pratt.** Listing `ch03-02-pratt.rs` parses the same expressions with three small tables and one loop:

```rust,ignore
/// Infix operators: (left bp, right bp). left < right: left-associative; left > right: right-associative.
fn infix_bp(op: &str) -> Option<(u8, u8)> {
    Some(match op {
        "=" => (2, 1), // right-assoc: a = b = c  is  a = (b = c)
        "||" => (3, 4),
        "&&" => (5, 6),
        "==" | "!=" | "<" | "<=" | ">" | ">=" => (7, 8), // non-associative: checked below
        "+" | "-" => (9, 10),
        "*" | "/" | "%" => (11, 12),
        "**" => (16, 15), // right-assoc, and tighter than prefix minus: -2 ** 2 is -(2 ** 2)
        _ => return None,
    })
}
```

```rust,ignore
            if let Some((l_bp, r_bp)) = infix_bp(op) {
                if l_bp < min_bp { break; }
                self.next();
                let rhs = self.expr_bp(r_bp)?;
                lhs = S::Cons(op.to_string(), vec![lhs, rhs]);
                if is_cmp(&Tok::Op(op)) && is_cmp(self.peek()) {
                    return Err("comparison operators cannot be chained".into());
                }
                continue;
            }
```

(Excerpts of listing 17.3-2.) `**` is not part of Ore. It's added here to show that a new operator is *one table row*:
right-associative (16, 15), and tighter than prefix minus (13), so `-2 ** 2` means `-(2 ** 2)`, as in mathematics and
Python. Real output:

```text
binding powers (higher binds tighter):
  =   l=2  r=1  right
  ||  l=3  r=4  left
  &&  l=5  r=6  left
  <   l=7  r=8  left (non-assoc, checked)
  +   l=9  r=10 left
  *   l=11 r=12 left
  **  l=16 r=15 right
  prefix - !  r=13   postfix call  l=17

  1 + 2 * 3              => (+ 1 (* 2 3))
  a || b && c            => (|| a (&& b c))
  x = y = 3              => (= x (= y 3))
  -2 ** 2                => (- (** 2 2))
  2 ** 3 ** 2            => (** 2 (** 3 2))
  f(a, b + 1)(c) * -d    => (* (call (call f a (+ b 1)) c) (- d))
  a < b < c              => error: comparison operators cannot be chained
  (1 + 2                 => error: expected `)`
```

(The printout labels `<` as "left" because its powers are (7, 8); the non-associativity comes from the explicit
check, as the note says.) The listing's test `agrees_with_recursive_descent` runs listing 17.3-1's ten precedence and
associativity cases through the Pratt parser and requires identical output. Both tests pass: **two parsers, one
language**. That agreement test is worth more than either parser's own tests, because it checks that the grammar you
wrote down twice means the same thing both times.

**Rust's own choice.** Rust makes comparisons non-associative too. Listing `ch03-03-rust-chained-comparison.rs`:

```rust,compile_fail
fn main() {
    let (a, b, c) = (1, 2, 3);
    if a < b < c {
        println!("ascending");
    }
}
```

```text
error: comparison operators cannot be chained
 --> src/main.rs:6:10
  |
6 |     if a < b < c {
  |          ^   ^
  |
  = help: use `::<...>` instead of `<...>` to specify lifetime, type, or const arguments
  = help: or use `(...)` if you meant to specify fn arguments
help: split the comparison into two
  |
6 |     if a < b && b < c {
  |              ++++
```

The two `help` lines about `::<...>` reveal why rustc cares so much: `a < b > c` could also be the beginning of a
generic argument list, `a<b>`, and Rust's expression grammar requires the *turbofish* `::<>` for generic arguments in
expressions precisely so that the parser never has to guess. The parser noticed the ambiguity and offered all three
readings.

---

## Pass 2 · Systems level — *What the parser remembers, and where*

### 4. Under the hood

**Grammar classes.** Recursive descent and Pratt parsing are *LL-style*: they decide what to build from the next token
or two. That imposes two rules on the grammar. It must not be *left-recursive* (`add := add "+" mul` would call `add`
forever before consuming anything), which is why listing 17.3-1 writes `add := mul ("+" mul)*` as a loop. And every
choice must be decidable with bounded lookahead. The alternative family, *LR* parsers (yacc, bison, and Rust's
`lalrpop` crate), builds the tree bottom-up from a generated table, accepts left recursion, and handles a larger class
of grammars, at the cost of an external grammar file and error messages that are hard to make good. PEG parsers
(Rust's `pest`) and parser combinators (`nom`) sit in between.

**Grammar restrictions as design.** Language designers bend grammars to keep parsing simple. Three examples from Rust
[LANG]:

- Blocks always need braces, so there's no "dangling `else`" ambiguity (`if a if b x else y`).
- Generic arguments in expressions need the turbofish (`parse::<u32>()`), so `<` is always "less than" in expression
  position.
- A struct literal isn't allowed directly in an `if` or `while` condition: `if x == S {}` parses `S` as a path and `{}`
  as the body. You write `if x == (S {})` if you mean the literal.

**rustc's parser** [RUSTC] is hand-written recursive descent over token trees (`rustc_parse`). Binary operators are
parsed by precedence climbing over a table of associative operators, which is a Pratt loop under another name. A large
share of its code is *recovery*: when the input is almost right, the parser guesses the intended program, reports an
error with a suggestion (the `&&` fix above), and continues with an error node (`ExprKind::Err`) in the AST. For
deep recursion, rustc wraps many of its recursive passes in `ensure_sufficient_stack`, which grows the stack on demand
when it runs low (via the `stacker` crate) instead of imposing a fixed depth limit [RUSTC]. That's the third answer to
the depth problem, next to the two in §6.

**Error recovery.** When a statement is malformed, the parser must decide where to resume. Listing
`ch03-05-error-recovery.rs` compares two strategies on five lines, four of them broken. **Synchronization** records a
diagnostic, puts an `<error>` node in the tree, and skips to a token where a statement can safely restart (`;` or
`let`). **Skip one token** drops one token and tries again from there. Real output, synchronizing first:

```text
=== recovery: synchronize at `;` / `let`: 4 errors, 5 statements
    <error>
    <error>
    <error>
    <error>
    (let d (+ a c))
error: expected an expression, found `;`
 --> line 1, col 13
  |
1 | let a = 1 + ;
  |             ^

error: expected a name after `let`, found `5`
 --> line 2, col 5
  |
2 | let 5 = x * 2;
  |     ^

error: expected `)`, found `;`
 --> line 3, col 15
  |
3 | let c = (a + 3;
  |               ^

error: expected `;`, found `3`
 --> line 4, col 11
  |
4 | let e = 2 3 4 5;
  |           ^
```

Then skipping one token:

```text
=== recovery: skip one token: 7 errors, 9 statements
    <error>
    <error>
    <error>
    (expr (* x 2))    <- phantom statement
    <error>
    <error>
    <error>
    <error>
    (let d (+ a c))
    expected an expression, found `;`
    expected a name after `let`, found `5`
    expected an expression, found `=`
    expected `)`, found `;`
    expected `;`, found `3`
    expected `;`, found `5`
    expected an expression, found `;`
```

Synchronization reports exactly the four real mistakes, one per broken line, each with the right caret. Skipping one
token reports seven, three of them *cascades* (an error caused only by the previous recovery), and invents a phantom
statement `x * 2` out of the remains of line 2. Users learn to read only the first error of a compiler that cascades;
good recovery is what makes the second and third errors worth reading. The error nodes matter too: later stages see
`<error>` and stay quiet about it, which is the same idea as the `{error}` type in Chapter 17.5.

### 5. Memory

A parser allocates one node per construct, so where nodes live is the parser's biggest memory decision. Listing
`ch03-06-ast-arena.rs` parses 100,000 small expressions with one generic parser and three node stores (a `Builder`
trait supplies the node constructors), with the counting allocator installed:

- **`Box`**: the textbook enum, every child in its own heap allocation.
- **Arena**: all nodes in one `Vec<Node>`, children referred to by `u32` index.
- **Bump**: nodes in a `bumpalo` arena, children as `&'bump` references.

Real output (release):

```text
node sizes: BoxExpr 24 B (a Box adds no header), arena Node 16 B, BumpExpr 24 B
Box:   allocs   1500001  frees   1500001  sum 7351786
arena: allocs        21  frees        21  sum 7351786  (1500000 nodes)
bump:  allocs        18  frees        18  sum 7351786  (67107264 bytes in chunks)
```

All three compute the same sum over the same 1.5 million nodes. The `Box` tree costs 1,500,001 allocations and the same
number of frees (one per node, plus the vector of roots). The index arena costs 21: the node vector doubling its way
up, plus the roots. The bump arena costs 18 chunk allocations, and frees everything at once when the arena is dropped.
Two layout details show up in the sizes. The arena's node is smaller (16 bytes) because a `u32` index is half a
pointer. And the bump arena reserved 67 MB of chunks for about 36 MB of nodes (1.5 million × 24 bytes), because it grows
by doubling chunk sizes: arenas trade some slack for the allocations they save.

This is the representation choice rustc makes at scale: most compiler data structures (HIR nodes, types, MIR bodies)
live in arenas that last for the whole compilation session, and are referred to with lifetimes such as `'tcx` or by
index [RUSTC]. Nothing is freed node by node; the arenas are dropped when the session ends. Chapter 17.4 goes further,
shrinking the node itself.

### 6. CPU / OS

**Time.** The same listing times parse, evaluate, and drop for each store (release, best of 5, one Playground run:
noisy):

```text
parse+eval+drop (release, best of 5, one run): Box 68.1 ms, arena 42.8 ms, bump 30.5 ms
```

The `Box` version is about 2.2× slower than the bump arena. Allocation and freeing are part of it, and locality is the
rest: arena nodes are allocated consecutively, so a tree walk touches memory in roughly the order it was written, while
`Box` nodes land wherever the allocator put them (Chapter 9.5's pointer-chasing argument).

**Stack.** A recursive-descent parser recurses once per nesting level *per precedence level*: each `(` in the input
passes through `expr → assign → or → and → cmp → add → mul → unary → postfix → primary`, ten frames, before it can
recurse again. Listing `ch03-07-deep-nesting-overflow.rs` feeds a ten-level parser 50,000 nested parentheses on a thread
with the default 2 MiB stack (real output):

```text
parsing 50000 nested parentheses on a thread with the default 2 MiB stack...

thread '<unknown>' (44) has overflowed its stack
fatal runtime error: stack overflow, aborting
```

The process aborts: in Rust a stack overflow hits the guard page, and the runtime's handler prints the message and
aborts the whole process, every thread with it (the Part IX interlude). Listing `ch03-08-depth-limit.rs` measures the
stack cost per nesting level (the address of a local at depth 0 and at depth 200) and adds the fix, a depth counter:

```text
debug build, 200 nested levels: value Ok(3) / Ok(3)
  stack per nesting level: recursive descent 1968 B, Pratt 753 B
  => a 2 MiB stack overflows near depth 1065 (RD) vs 2785 (Pratt)
  depth    250: Ok(3)
  depth    257: Err("expression nested more than 256 levels deep at byte 256")
  depth  50000: Err("expression nested more than 256 levels deep at byte 256")
```

```text
release build, 200 nested levels: value Ok(3) / Ok(3)
  stack per nesting level: recursive descent 224 B, Pratt 144 B
  => a 2 MiB stack overflows near depth 9362 (RD) vs 14563 (Pratt)
```

Three facts follow. **Pratt parsing uses less stack per nesting level** (two functions per level instead of ten), 2.6×
less in debug and 1.6× less in release. **Release builds use far less stack** than debug builds (224 bytes against
1,968), because inlining merges frames and locals live in registers; a parser tested only in release can still
overflow in a debug build, and the reverse is never safe to assume. And **only the limit makes the result independent of
the build**: with `MAX_DEPTH = 256`, depth 257 and depth 50,000 both return the same clean error, in either profile,
on any stack size. The three production answers are, in order of robustness: a depth limit (Sieve's choice), growing
the stack on demand (rustc's `stacker`), or an explicit stack instead of recursion (the Part IX interlude's evaluator).

---

## Pass 3 · Architect level — *The grammar is a contract*

### 7. Trade-offs

| Technique | Grammar lives in | Error messages | Left recursion | Typical users |
|---|---|---|---|---|
| Hand-written recursive descent | code, one fn per rule | excellent: fully controlled | must be rewritten as loops | rustc, javac, Go, Clang, V8 |
| Pratt / precedence climbing | a binding-power table + one loop | excellent | not needed for operators | expression parts of most of the above |
| LR / LALR generators (`lalrpop`, bison) | a grammar file | hard to make good | fine | older compilers, SQL engines |
| PEG (`pest`) | a grammar file | medium | not allowed | config languages, DSLs |
| Parser combinators (`nom`) | code, composed functions | medium | not allowed | binary and text formats |
| ANTLR (ALL(*)) | a grammar file | medium, customizable | direct left recursion supported | the Java DSL ecosystem |

A useful rule: **use a generator when the grammar will change more often than the error messages matter**, and write
the parser by hand when users will read its errors every day. A rule DSL like Sieve is the second kind: analysts see
its error messages constantly, and its grammar changes a few times a year.

> **Why not a parser generator?** Generators produce correct parsers from a grammar you can read, which is a real
> benefit. What they make hard is everything around correctness: an error message that says "expected `)` to close the
> `(` at column 9", recovery that doesn't cascade, and a depth limit. Those are ordinary code in a hand-written parser,
> and they're most of the work.

### 8. Java comparison

`javac`'s parser (`JavacParser`) is hand-written recursive descent, like rustc's, and it parses binary expressions with
an operator-precedence loop over an explicit operator stack rather than one method per level [LIB]. Java also has the
generics-versus-less-than ambiguity, and resolves it the other way: instead of requiring a turbofish, javac looks ahead
to decide whether `a < b > c` in certain positions is a generic type, and the language restricts where explicit type
arguments may appear (`Collections.<String>emptyList()` requires the qualifier).

On depth, the two languages differ in failure mode. A deeply nested expression makes a recursive Java parser throw
`StackOverflowError` (default thread stack 1 MiB on HotSpot/Linux x64, Part IX interlude), which the parser's caller can
catch and turn into "input too complex". In Rust the same overflow aborts the process, which is why a Rust parser for
untrusted input needs one of §6's three answers *before* it ships.

> **Analogy limit.** "Catching `StackOverflowError` is the Java equivalent of a depth limit" is only partly true. The
> JVM does unwind cleanly from a stack overflow in the common case, but the error can hit in the middle of any method,
> including inside a `finally` block or a class initializer, leaving state half-updated. A depth limit fails at one
> known place with one known error. Treat `StackOverflowError` as a crash that happens to be catchable, not as control
> flow.

### 9. Production scenario

**Sieve's parser.** Sieve's parser is a Pratt parser whose binding-power table is a reviewed file, not code scattered
across functions. The team's reasons map directly onto this chapter:

- **Precedence as data.** Adding an operator (Sieve v0.4 added `in`, for set membership) is one row. The row is reviewed
  like configuration, with a table of "how does this parse?" examples in the PR description.
- **Two parsers during the migration.** Sieve replaced a recursive-descent prototype. During the migration both
  parsers ran on every rule, and a test required identical trees for every rule in the rule store (about 4,100 rules
  at the time), exactly like listing 17.3-2's `agrees_with_recursive_descent`.
- **A depth limit of 64**, the same limit the onboarding service adopted after the Part IX interlude's crash loop,
  enforced when a rule is saved. No legitimate rule has come within a factor of four of it.
- **The editor shows the parse.** Hovering an operator in the rule editor shows the rule fully parenthesized. Analysts
  catch precedence surprises themselves, before review.
- **Shadow mode before cutover.** Every compiler change ships behind shadow evaluation: the new compiler's rules run
  next to the old ones on live traffic, only the old decisions are used, and every differing decision is logged. §10
  is why this is non-negotiable.

### 10. Failure scenario

**The row that merged `&&` and `||`.** In Sieve v0.3 a refactor "simplified" the binding-power table by merging the
logical operators into one row. Listing `ch03-04-precedence-bug.rs` reproduces the change:

```rust,ignore
fn buggy_bp(op: &str) -> Option<(u8, u8)> {
    Some(match op {
        "||" | "&&" => (1, 2), // the bug: && no longer binds tighter than ||
        "<" | ">" => (5, 6),
        _ => return None,
    })
}
```

(Excerpt of listing 17.3-4.) With both operators at the same power, the parse of a manual-review rule changed shape.
The rule's intent is "review if the country is high-risk, *or* the merchant is new *and* high-volume". Real output:

```text
rule:    high_risk_country || volume_30d > 50000 && age_days < 30
correct: (|| high_risk_country (&& (> volume_30d 50000) (< age_days 30)))
buggy:   (&& (|| high_risk_country (> volume_30d 50000)) (< age_days 30))

old merchant, high-risk country   correct=1 buggy=0
new merchant, high volume         correct=1 buggy=1
new merchant, high-risk country   correct=1 buggy=1
old merchant, high volume         correct=0 buggy=0
```

The buggy parse requires *every* flagged merchant to be under 30 days old, so an established merchant in a high-risk
country is no longer reviewed. Three of the four hand-picked examples still agree, and the unit tests agreed too:
every test rule used only one kind of logical operator, so none exercised the relative precedence of `&&` and `||`.
Shadow mode caught it. On the first day, the new compiler's decisions differed from the old one's on a large fraction
of evaluations of this rule. The listing's simulation of 10,000 synthetic merchants shows the scale:

```text
shadow mode, 10,000 merchants: correct flags 906, buggy flags 100, decisions differ 806
```

The buggy parser flags 100 merchants where the correct one flags 906. Deployed without shadow mode, this would have
silently removed about 89% of this rule's reviews. The fix was one line; the lasting changes were three: a test that
parses every rule in the store with both the old and new table and diffs the trees, a precedence table test covering
every *pair* of operators (`a OP1 b OP2 c` for all pairs, compared with a fully parenthesized oracle), and the editor's
parenthesized hover.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVII).*

1. How does recursive descent encode precedence and associativity? Why must left recursion be removed, and how?
2. Explain Pratt parsing's binding powers. How do (9, 10) and (2, 1) produce left and right associativity?
3. Why are comparison operators non-associative in Rust (and Ore)? What does the turbofish have to do with it?
4. Name three grammar restrictions Rust imposes to keep parsing simple, and the ambiguity each one removes.
5. Compare synchronization with skip-one-token recovery. What is a cascade, and why does it train users to ignore
   errors?
6. A parser allocates 1.5 million nodes. Compare `Box` trees, an index arena, and a bump arena on allocations, node
   size, drop cost, and ergonomics.
7. Why does a recursive-descent parser use more stack per nesting level than a Pratt parser? Why does a debug build use
   more than a release build?
8. List three ways to make a parser safe against deeply nested input, and say which one fails most predictably.
9. When would you choose a parser generator over a hand-written parser? When would you not?
10. How would you roll out a new parser for a language with thousands of stored programs, so that a precedence bug
    can't reach production?

### 12. Exercises

- **Beginner.** Add `%` to listing 17.3-2's table at the same level as `*`, and add unary `!` tests. Then add a
  right-associative `?:`-free "pipe" operator `|>` that binds looser than `||`. What row do you add?
- **Intermediate.** Add postfix indexing `a[i]` to listing 17.3-1 (tighter than calls? same level?). Add it to the Pratt
  parser too, and extend `agrees_with_recursive_descent` with five cases.
- **Advanced.** Add error recovery to listing 17.3-1: collect all errors in a function body instead of stopping at the
  first, using synchronization at `;` and `}`. Test it on a body with three independent mistakes and show that no
  cascades are reported.
- **Systems.** Run listing 17.3-8 with the depth counter removed and a spawned thread whose stack size you choose
  (`thread::Builder::stack_size`). Find, by bisection, the largest depth that survives in debug and in release, and
  compare it with the predicted 1,065 and 9,362.
- **Architecture.** Design the "every pair of operators" precedence test from §10 for Sieve's table: the oracle, how
  inputs are generated, and how the test stays small enough to run on every commit.

### 13. Debugging exercise

A teammate changes listing 17.3-1's `left_assoc` so that the right operand is parsed by the *same* level instead of the
operand level, "to support chains" (an unverified sketch of the change):

```rust,ignore
fn left_assoc(&mut self, ops: &[(Tok, BinOp)], operand: fn(&mut Self) -> PResult<Expr>,
              this: fn(&mut Self) -> PResult<Expr>) -> PResult<Expr> {
    let lhs = operand(self)?;
    if let Some(&(_, op)) = ops.iter().find(|(t, _)| t == self.peek()) {
        self.bump();
        let rhs = this(self)?;      // recurse into the same level
        let span = lhs.span.to(rhs.span);
        return Ok(Expr { kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span });
    }
    Ok(lhs)
}
```

1. What does `1 - 2 - 3` parse to now, and what does it evaluate to? Which of the listing's tests fail?
2. Does `1 + 2 * 3` still parse correctly? Why is precedence unaffected while associativity changed?
3. What happens to stack usage on a long chain `1 + 1 + 1 + ... + 1` (100,000 terms), before and after the change?

### 14. Design exercise

**Design a parser for Meridian's query filter language**, used by the support console to search payments:
`status == "failed" && (amount > EUR 500.00 || country in ["DE", "AT"]) && created > 7d ago`. About 400 support agents
write these by hand, many times a day, and a bad query must never take the console's backend down. Decide:

- Recursive descent, Pratt, or a generator, and why.
- The precedence table, including `in`, `ago`, and `!`, and which operators are non-associative.
- Error messages and recovery for the five mistakes you expect most often from support agents.
- Limits: depth, total nodes, literal list length. Where each is enforced and what the user sees.
- How you'd test that the new parser agrees with the current one (a regex-based filter with known bugs), and what you
  do with rules where they *should* disagree.
