# Appendix A — Answer Key: Part XVII

> Model answers. Write yours first. Where several answers are defensible, the key says so. Outputs quoted here come from
> the verified listings in `listings/part-17/` (rustc 1.98.1); "predicted" marks reasoning you should check by running
> the modified listing yourself.

---

## Chapter 17.1 — The Compiler Pipeline End to End

### Interview & architecture questions

**1. The stages.** Lexer (knows character classes; decides token boundaries; forgets whitespace and comments). Parser
(knows the grammar; decides nesting and precedence; forgets parentheses and punctuation). Resolver (knows scopes and
declarations; decides which declaration each name means; forgets names, replaced by ids). Type checker (knows every
binding's type; decides whether each operation is legal; forgets little, it records types in side tables). In Rust, the
borrow checker (on MIR: knows control flow and places; decides whether ownership rules hold). Lowering (knows control
flow; decides jumps and evaluation order; forgets nesting and high-level constructs). Optimizer (knows data flow and
dominance; decides cheaper equivalents; forgets dead and redundant code). Code generator (knows the ISA and ABI;
decides instructions and registers; forgets variables). Linker (knows every object file; decides addresses; forgets
symbol boundaries within the final image, except what's kept for debugging and dynamic linking).

**2. Earliest stage with the information.** Unknown character: lexer. Missing semicolon: parser. Undefined variable:
resolver. `1 + true`: type checker. Use of a moved value: the borrow checker, which runs on MIR after type checking,
because it needs both types (is the value `Copy`?) and control flow (was it moved on *this* path?). Division by a zero
from user input: no compile-time stage; it's a run-time failure (a panic in Rust).

**3. Division by zero.** Whether a divisor is zero on some execution depends on inputs, and in general no algorithm can
decide non-trivial properties of what every program does (Rice's theorem). A compiler can still: find *constant* zero
divisors (rustc's deny-by-default `unconditional_panic` lint rejects `1 / 0`); offer types that exclude zero
(`u64 / NonZeroU64` needs no check); and make the check explicit and visible (`checked_div` returns an `Option`).

**4. Front, middle, back.** The front end establishes *meaning* (lexing through type and borrow checking); the middle
end finds *cheaper equivalent programs* (IR and optimization); the back end fits the program to *the machine*
(instruction selection, register allocation, emission, linking). The front end produces almost all user-facing errors
because it's where a program can be invalid. By the time a program reaches the middle end, it's valid; an error there
is usually a compiler bug (or a link error, which is about the environment, not the program's meaning).

**5. `scale` at four levels.** HIR: macros expanded, attributes normalized (`#[attr = Inline(Never)]`), otherwise the
source. MIR (debug): names become locals (`_1` is `x`), arithmetic becomes explicit `MulWithOverflow`/`AddWithOverflow`
plus `assert` terminators, and control flow is three blocks; in release the overflow checks and extra blocks disappear.
LLVM IR: Rust types become machine types (`i64`), values are SSA (`%y`, `%_0`). Assembly: `y` doesn't exist at all;
`x * 3` became `lea rax, [rdi + 2*rdi]` and `+ 1` became `inc rax`.

**6. Side tables.** The AST stays immutable and shareable across threads and analyses; each analysis owns its output;
nothing has to represent "not computed yet" inside the tree; and results can be cached per node for incremental
compilation. rustc's `TypeckResults` is a table keyed by `HirId` for exactly these reasons.

**7. Rule engine at 2 million evaluations per second.** Use Chapter 10.1's measurements: a tree walk at 11.45 ns costs
about 23 ms of CPU per second (2.3% of a core); closure compilation at 6.28 ns costs about 13 ms (1.3%). Either is
affordable; choose closure compilation as the simplest design that avoids re-walking the AST, with the tree walker kept
as a reference oracle. A bytecode VM or a JIT would be justified only if profiling showed evaluation (not feature
fetching) dominating, for example after rules gain loops over large lists. AOT compilation is the right choice only if
rules can change on the deploy cadence, which for analyst-written rules they can't.

**8. C2 versus rustc and javac.** C2 optimizes with profiles it observes at run time: it inlines monomorphic virtual
calls, compiles never-taken branches as uncommon traps, and deoptimizes when an assumption breaks. rustc can't (without
PGO, and even then not adaptively). rustc does two things javac doesn't do at all: borrow checking, and
monomorphization of generics (javac erases them).

**9. Demand-driven compilation.** Instead of running each stage over the whole crate in order, rustc computes results
through *queries* ("the type of this function", "the optimized MIR of this body"), each memoized with a record of the
queries it read. After an edit, only queries whose inputs changed need recomputing ("red-green" marking), which is what
makes incremental builds fast (Chapter 18.1).

**10. Where checks live for a rule language.** At upload time, in a rule service that owns the compiler: resolution
against a versioned feature catalog, and type checking with no implicit conversions. Errors then reach the author, with
spans, before the rule exists. Evaluation services load a compiled, checked form and never parse, so a malformed rule
can't take them down (the Part IX interlude's crash loop), and stored rules carry the catalog version they were compiled
against (Chapter 17.4 §10).

### Debugging exercise (type checking before resolution)

1. It compiles, and then the *compiler* panics on the good program, at the first `let`. The checker is created with an
   empty resolver: `res.uses` has no entries and `slot_ty` has length 0. `Checker::stmts` handles `let mut i = 0` by
   computing the initializer's type and then evaluating `self.slot_ty[self.res.uses[pos]]`; indexing a `HashMap` with a
   missing key panics. (`Checker::expr` on a variable does the same lookup.) A compiler crash, not a diagnostic.
2. The checker *depends* on the resolver's side table: types are stored per slot, and slots come from resolution. The
   dependency graph here is lexer → parser → resolver → checker → lowering, with lowering also reading the resolver's
   table. Reordering stages means violating a data dependency, and Rust's type system didn't catch it only because the
   dependency goes through a runtime-populated `HashMap` rather than a type (an argument for passing each stage's output
   *as a value* to the next).
3. Some stages genuinely need each other. Macro expansion can define new items that name resolution must see, and
   resolving a macro's path requires name resolution; so rustc iterates expansion and resolution to a fixed point
   (Chapter 18.2). Type checking and trait solving interleave for the same reason.

### Selected exercises

- **Beginner.** `let z = 1; 7 % (z - 1)`: every stage passes (nothing tracks values), and the VM reports a division by
  zero at run time, exactly like `10 / z`. `let b = !5; b`: the type checker rejects it, because `!` needs a `Bool`
  operand and `5` is an `Int` ("operand must be Bool, found Int").
- **Intermediate.** Fold only when both operands are literals; skip `Div`/`Rem` by a literal zero, because folding would
  either crash the compiler or silently change a run-time error into something else (Chapter 17.7 §3). `2 * 3 + 4`
  becomes `Push(10)`.
- **Advanced (predicted).** The VM dispatches on a compact instruction vector with a tight loop; the tree walk recurses
  through boxed nodes and matches on node kinds, with worse locality. Expect the VM to win by a small constant factor;
  measure it rather than trusting the prediction.

---

## Chapter 17.2 — Lexing

### Interview & architecture questions

**1. Maximal munch.** At each position, take the longest token that matches. It rules out lexing `<=` as `<` `=` and
`iffy` as `if` `fy`. It makes some features awkward: C++'s `>>` closing two template argument lists needed a special
rule (C++11), and Rust's parser has to split `>>` into two `>` tokens when closing nested generics.

**2. Why separate lexing.** Tokens form a *regular* language, recognizable by a finite automaton with no stack, which is
fast and simple; nesting is context-free and needs a parser. Separating them keeps each simple, lets the parser work on
a small alphabet of token kinds, and puts character-level concerns (Unicode, escapes, literals) in one place.

**3. Error tokens.** So one bad byte doesn't hide every later mistake, and so the parser decides how to recover: it can
treat an `Error` token as an expression that already failed (reporting nothing more about it) and continue.

**4. Owned tokens.** Chapter 17.2's measurement: owned lexemes cost 233,756 allocations for 2 MB of source (versus 0 for
spans) and ran at 92.9 MB/s versus 1,264.7 MB/s, about 13.6× slower. Two ways out: keep spans and borrow the source for
the compilation's lifetime (rustc's `SourceMap`), or intern identifiers so tokens carry a 4-byte `Symbol` and no
lifetime (Chapter 17.4). Both avoid the cost; interning also avoids the lifetime.

**5. Offsets, not line and column.** Offsets are 4 bytes, trivially comparable, and exact; line and column depend on
line-ending conventions, tab width, and how columns are counted (bytes, chars, or UTF-16 units). They're computed from a
table of line starts, by binary search, only when a message is printed.

**6. Keywords.** Lex the whole identifier (maximal munch), then look the text up in a keyword table. `iffy` is one
identifier because the lexer never stops after `if`; "keywords are whole words" falls out for free.

**7. rustc's two layers.** `rustc_lexer` classifies text into `(kind, length)` pairs with no spans, interning, or
diagnostics; `rustc_parse` adds absolute spans, interns identifiers, validates and unescapes literals, reports errors,
and builds token trees. The lower layer is a pure function of the text, so other tools (rust-analyzer) can reuse it
without the compiler's session machinery.

**8. Trojan Source.** Bidirectional control characters change the *display order* of text. A reviewer sees code in the
visual order an editor renders, while the compiler reads the logical order, so a string or comment can appear to end
where it doesn't, and code can look commented out while it runs. rustc 1.56.1 added deny-by-default lints that reject
these characters in literals and comments unless written as escapes (CVE-2021-42574; Boucher and Anderson, 2021).

**9. Java's `\uXXXX` pre-pass.** Unicode escapes are translated before lexing (JLS §3.3), anywhere in the file, so an
escape can end a comment (`// \u000a`), close a string, or form an operator, and the source a reviewer reads isn't the
text the lexer tokenizes. Rust processes `\u{...}` only inside character and string literals, as part of lexing the
literal, so it can't change token boundaries.

**10. Unicode policy.** (a) A general-purpose language: allow Unicode identifiers (UAX #31), normalize to NFC, and ship
confusable and mixed-script lints (Rust's choice), because names in a program are chosen by people who speak many
languages. (b) A catalog-driven DSL: ASCII identifiers only (every legitimate name comes from an ASCII catalog), ASCII
literals by default with escapes for anything else, plus a confusable check where non-ASCII is allowed; the only
non-ASCII in a rule is then a mistake or an explicit, reviewable escape.

### Debugging exercise (one-byte arms first)

1. rustc warns that the moved two-byte arms are **unreachable patterns** (`unreachable_patterns`, warn-by-default),
   because `(b'<', _)` already matches everything `(b'<', Some(b'='))` would. In a large build that's one warning among
   many, and the code still compiles.
2. `if x <= 10 { y == 1 }` lexes as `If Ident Lt Assign Int(10) LBrace Ident Assign Assign Int(1) RBrace`. The *parser*
   reports it: after `x <` it expects an expression and finds `Assign` ("expected an expression, found Assign", at the
   `=`). The error is one stage later and one character away from the bug.
3. `maximal_munch` fails (its first assertion includes `<=`, `->`, and `==`); the other four still pass. Even without a
   `<=` case, the `->` and `==` checks fail. The test that needs no hand-picked cases is a **differential test** against
   the table-driven lexer (listing 17.2-6's 2,000 random inputs), which would disagree on the first input containing a
   two-byte operator.

### Selected exercises

- **Beginner.** The `>>` and `<<` arms must be listed before the one-byte `>` and `<` arms; their order relative to
  `>=` and `<=` doesn't matter, because the second byte differs. Test: `a<<b>>c` lexes as
  `Ident Shl Ident Shr Ident`.
- **Intermediate.** Nested block comments need a *counter* (depth), which a finite automaton can't keep: nesting is
  not regular. Lex `/*` by counting depth up on `/*` and down on `*/`; at end of input with depth > 0, report
  "unterminated block comment" at the span of the *outermost* opening `/*`, which you saved when depth went from 0 to 1.
- **Systems (predicted).** On 1-character identifiers separated by spaces, token kinds alternate unpredictably and the
  class table saves the most (fewer mispredicted comparison chains). On one 2 MB comment, a `memchr` for `\n` skips most
  of the input and dominates; the class table barely matters.

---

## Chapter 17.3 — Parsing: Recursive Descent and Pratt Parsing

### Interview & architecture questions

**1. Recursive descent.** One function per precedence level, each calling the next-tighter level for its operands:
precedence is the call structure. Left associativity is a loop that folds each new operator onto the tree so far.
Left recursion (`add := add "+" mul`) would call `add` forever before consuming input, so it's rewritten as
`add := mul ("+" mul)*`.

**2. Binding powers.** Each infix operator has (left, right) powers. The loop continues with an operator only if its
left power is at least the current minimum, and parses the right operand with the operator's right power as the new
minimum. For (9, 10), a second `-` has left power 9 < 10, so it can't continue inside the right operand; it attaches at
the outer level: left associativity. For (2, 1), a second `=` has left power 2 ≥ 1, so it continues inside: right
associativity.

**3. Non-associative comparisons.** `a < b < c` in most languages means `(a < b) < c`, comparing a boolean with a
number, which is almost never what the author meant. Rust rejects it and suggests `a < b && b < c`. The turbofish
matters because `a < b > c` could also start a generic argument list `a<b>`; requiring `::<>` for generics in
expressions means `<` in expression position is always "less than", and the parser never has to guess.

**4. Grammar restrictions.** Mandatory braces on blocks remove the dangling-`else` ambiguity. The turbofish removes the
generics-versus-comparison ambiguity. No struct literals directly in `if`/`while` conditions removes the ambiguity
between `S { .. }` as a literal and `{ .. }` as the body.

**5. Recovery.** Synchronization skips to a token where a construct can safely restart (`;`, `let`, `}`), records one
error, and inserts an error node. Skip-one-token restarts parsing at the next token, which is usually in the middle of
the broken construct, so it reports errors caused only by the recovery itself (cascades) and can invent phantom
statements. Listing 17.3-5: 4 real errors versus 7 with cascades and one phantom `x * 2`. Users learn that only the first
error is real, and stop reading the rest.

**6. Where nodes live.** Box: 1,500,001 allocations and as many frees, 24-byte nodes, scattered in memory, trivially
ergonomic (owned tree, drop frees it). Index arena: 21 allocations, 16-byte nodes (u32 children), contiguous, but
indices are unchecked by the type system and the arena must be passed around. Bump arena: 18 chunk allocations, one
free, references with a `'bump` lifetime (ergonomic once the lifetime is threaded through), and some slack (67 MB of
chunks for 36 MB of nodes). Measured times (one run): 68.1, 42.8, and 30.5 ms.

**7. Stack per level.** Each nesting level passes through every precedence function in recursive descent (ten frames
in listing 17.3-8) but only `expr_bp` and `atom` in Pratt: 1,968 versus 753 bytes in debug. Debug builds keep every
local in its own stack slot and don't inline, so frames are larger and more numerous; release builds inline and keep
values in registers: 224 versus 144 bytes.

**8. Deep input.** A depth limit (fails at one known place with one known error, independent of build and stack size);
growing the stack on demand (rustc's `stacker`: robust, but memory use is still input-controlled); an explicit stack
instead of recursion (the Part IX interlude). The depth limit fails most predictably.

**9. Generators.** Choose a generator when the grammar changes often, is large, and error messages are secondary
(internal formats, prototypes). Write by hand when users read the errors daily, recovery matters, or you need limits and
special cases, which is why rustc, javac, Go, and Clang are hand-written.

**10. Rolling out a new parser.** Run both parsers on every stored program and diff the trees (listing 17.3-2's
agreement test at scale); test every pair of operators against a fully parenthesized oracle; shadow-evaluate with the
new parser on live traffic and diff decisions before cutover; and show authors the parenthesized parse.

### Debugging exercise (`left_assoc` recursing into itself)

1. `1 - 2 - 3` now parses as `(- 1 (- 2 3))`: the right operand is parsed by the *same* level, which consumes the second
   `-`, so the tree leans right. It evaluates to `1 - (2 - 3) = 2` instead of `-4`. The `associativity` test fails
   (`1 - 2 - 3` and `a / b / c`); `precedence` still passes.
2. Yes. The right operand is still parsed by a function at the *same or tighter* level, so a tighter operator still
   grabs its operands first; only operators of the *same* level changed grouping.
3. Before: the loop builds a 100,000-term chain with constant stack (one loop, no recursion per term). After: each term
   recurses once more (through `add` and `left_assoc`), 100,000 levels deep. Even at ~100 bytes per level that's about
   10 MB of stack, far beyond a 2 MiB thread, so the process aborts (predicted from §6's per-frame costs). The change
   turned a loop into input-controlled recursion.

### Selected exercises

- **Beginner.** `%` goes in the `*` row. A pipe `|>` looser than `||` needs powers below (3, 4), for example (1, 2) for
  left associativity, which means shifting `=` down to (0, ...) or giving `|>` a new band below everything but
  assignment. The exercise's point: a new operator is a table decision that interacts with every existing row.
- **Systems (predicted).** Bisection should land near the predictions (1,065 debug, 9,362 release for a 2 MiB thread),
  with some slack for the thread's own startup frames.

---

## Chapter 17.4 — ASTs, Symbol Tables, and Name Resolution

### Interview & architecture questions

**1. Name resolution.** Mapping each occurrence of a name to its declaration. Rules that change the result: scopes
(a `let` inside a block), declaration order (calling a function defined later), shadowing (`let x = x + 1`), namespaces
(`int` as a type and a value), and hygiene (a macro's `x` versus the caller's).

**2. Two passes.** Collecting items first makes every function visible in every body, so calls may precede definitions
and functions may be mutually recursive. A single-pass resolver requires declarations before uses (C's prototypes).

**3. `let x = x + 1;`** The right-hand side means the *previous* `x`. The resolver resolves the initializer first, while
only the old binding exists, and declares the new binding afterwards.

**4. Side tables and HIR.** Keeping results out of the AST keeps the tree immutable, lets each analysis own its output,
and supports incremental caching. rustc's HIR goes further: it's a new tree built *after* resolution in which every
path carries its resolution, so later stages can't encounter an unresolved name.

**5. Namespaces.** In `let i32 = 7_i32; let n: i32 = i32 * 2;` the annotation is looked up in the type namespace and
the operand in the value namespace, so the two `i32`s never collide. A tuple struct `struct Meters(u64)` defines a type
`Meters` *and* a constructor function `Meters`, one in each namespace.

**6. Hygiene.** Identifiers introduced by a macro resolve in the macro's own syntax context. `double_it!(x)` expands to
`{ let x = 2; x * <caller's x> }`, and the two `x`s have different contexts, so the result is 2 × 10 = 20; textual
substitution would give 2 × 2 = 4. Mixed-site hygiene (`macro_rules!`) applies this to locals, labels, and `$crate`;
items the macro defines (`fn answer`) resolve at the call site, so the caller can use them.

**7. Shrinking a node.** Box the rare, large variant (72 → 32 bytes, the biggest win for the least effort); intern names
and box other large payloads (→ 24); move children to an arena with `u32` indices and large literals to a side table
(→ 12). Add static size assertions so the size doesn't creep back.

**8. Interning.** 200,017 allocations become 2,039 (one per distinct name), 24-byte handles become 4-byte ones, and
lookups and comparisons get cheaper (4.04 → 2.36 ms for the lookup benchmark, 0.16 → 0.04 ms for equality scans; one
run, noisy).

**9. "Did you mean".** Compute the edit (Levenshtein) distance from the unknown name to every visible candidate, and
suggest the closest within a threshold. The threshold scales with length because one typo in a 3-letter name changes a
third of it (anything would match), while a 12-letter name can tolerate several edits and still be clearly the same
word.

**10. Names in stored programs.** If stored rules are re-resolved against a changed table, a renamed or redefined
symbol silently changes what the rule means (Chapter 17.4 §10). Storing resolved ids plus the table version makes a
rule's meaning fixed at compile time; ids are never reused, so changing a meaning requires a new symbol, and the
reverse index lists every program that must move.

### Debugging exercise (`declare_before_init`)

1. `declare_before_init = true` is wrong. The second `let x = x + 1` declares slot 1 *before* resolving its initializer,
   so its `x` resolves to slot 1 itself: `Add(Slot(1), Num(1))`. With the correct order it resolves to slot 0:
   `Add(Slot(0), Num(1))`. Real output: the wrong mode prints `slot 1 (x) = Add(Slot(1), Num(1))` and `y = 2`; the right
   mode prints `Add(Slot(0), Num(1))` and `y = 4`.
2. It doesn't crash because `main` zero-initializes every slot, so reading slot 1 before it's written yields 0: `x`
   becomes `0 + 1 = 1` and `y = 1 + 1 = 2`. That's the "uninitialized memory happens to be zero" failure mode, silent and
   plausible. Rust resolves the initializer first, so the equivalent Rust program computes 4; and if a name did refer to
   an unassigned binding, definite-assignment analysis would reject the program (E0381, Chapter 17.6).
3. Declaring first is correct for **recursive functions** (and items in general): a function's body may call the
   function itself, so its name must be in scope while its body is resolved. Reusing that rule for `let` is exactly the
   Sieve bug described in the exercise.

### Selected exercises

- **Intermediate.** One `HashMap<String, Vec<u32>>` of binding stacks plus, per scope, the list of names it declared:
  declaring pushes onto the name's stack; leaving a scope pops each of its names. Lookup is one probe (the top of the
  stack). The test program must produce the same four resolutions and four errors.
- **Advanced.** Comparisons and hashing use `Symbol`, but suggestions need text: resolve candidate symbols back to
  strings (`interner.resolve`) only on the error path, where the cost doesn't matter.
- **Architecture.** Give each snippet expansion its own syntax context, exactly like hygiene: names the snippet declares
  are tagged with the snippet's context and can't be seen by, or capture, the including rule's names; references to
  catalog features resolve normally (items at the call site, as with mixed-site hygiene).

---

## Chapter 17.5 — Type Checking and Type Inference

### Interview & architecture questions

**1. Two modes.** Synthesis (`infer`) computes an expression's type from the expression alone, bottom-up. Checking
(`check`) takes an expected type and pushes it *into* the expression's parts: an `if`'s branches, a block's tail, a call's
arguments. A mismatch is then detected at the innermost part that disagrees (`false` inside the `else`), not at the
enclosing construct.

**2. Error types.** A type that means "already reported". It's compatible with every type and any operation involving
it yields it again, so one mistake produces one message instead of a cascade. rustc's `ErrorGuaranteed` is a token that
can only be obtained by emitting an error; the error type requires it, so the compiler's own types make it impossible to
create a silent error type without a real diagnostic having been reported.

**3. `pick`.** Assign `c: t0, a: t1, b: t2`, result `t3`. Constraints: `t0 = bool` (condition), `t1 = t2` (branches
agree), `t3 = t1` (the `if`'s type). Solution: `fn(bool, t1, t1) -> t1`. `t1` is unconstrained and the environment is
closed at top level, so generalization gives `forall a. fn(bool, a, a) -> a`.

**4. Occurs check.** Before binding a variable `t` to a type, check that the type doesn't contain `t`. `fn self_apply(x)
{ x(x) }` needs `t = fn(t) -> u`, an infinite type. Without the check the binding is cyclic, and any later traversal of
the type (substitution, printing, comparison) recurses forever.

**5. Let-polymorphism.** A `let`-bound (or top-level) definition's leftover type variables are generalized, so each use
can instantiate them differently (`pick` at `bool` and at `int`). Rust closures aren't generalized: a closure's parameter
type is fixed by its first use (listing 17.5-3, E0308). You write a generic function, `fn id<T>(x: T) -> T`, which is
explicitly polymorphic and monomorphized: no runtime cost, one compiled copy per type used (Part VII).

**6. E0121.** (1) API stability: a function's type would depend on its body, so an internal change could silently change
callers' types (a semver hazard). (2) Locality: each body can be checked independently, in parallel and incrementally;
inferred signatures force whole-program, dependency-ordered checking. (3) Error locality: mistakes stay inside the
function that contains them instead of surfacing at distant call sites.

**7. `c`.** `u64`. The literal gives `c` an `{integer}` inference variable; `let d: u64 = c;` unifies it with `u64`, and
the constraint flows *backward* to `c`'s declaration. Without that line nothing constrains it, and it falls back to
`i32` at the end of the body.

**8. Poly expressions.** A Java lambda has no type of its own; it's *checked* against a target type (a functional
interface), which is bidirectional checking mode. `var f = x -> x;` gives the lambda no target, so javac can't check it
and rejects the declaration. Rust instead infers a closure's parameter type from its later uses (synthesis plus
unification), which is why `let f = |x| x;` is accepted until something constrains `x`.

**9. Error distance.** Unification fails where two constraints finally collide, which may be long after the wrong use
that fixed a variable's type. The error then points at the second use, which is correct, while the first was wrong.
rustc's notes ("expected because the closure was earlier called with an argument of type `{integer}`") record the
*provenance* of a type and report the earlier constraint too.

**10. Money in several currencies.** A static type system can guarantee that amounts are never mixed with plain numbers,
that literals state their currency, and that no implicit conversion exists. It can't know which currency a particular
payment has at run time. The language makes the rest explicit: a conversion operation (`amount.in(EUR)`) whose rate
source and staleness policy are visible, or per-currency thresholds, so the question can't be skipped even though the
answer comes from data.

### Debugging exercise (no occurs check)

1. `x(x)` creates `t_x` for `x`, a fresh result variable `t_r`, and unifies `t_x ~ fn(t_x) -> t_r`. Without the occurs
   check, `unify` binds `t_x := fn(t_x) -> t_r`, a cycle. At the end of the function, `generalize` calls `zonk` on the
   function's type: `zonk(t_x)` finds `fn(t_x) -> t_r`, zonks its parameter `t_x`, finds `fn(t_x) -> t_r` again, and so on.
2. `find` terminates: it follows bindings only while they are *variables*, and `t_x`'s binding is a function type. `zonk`
   recurses without end, and the recursion overflows the main thread's stack; the process prints "has overflowed its
   stack" and aborts (Chapter 17.3 §6). Predicted; verify by deleting the check in a copy of listing 17.5-2.
3. A cycle can only be created when a variable is bound to a *constructed* type (`fn(...) -> ...`); binding a variable to
   another variable or to `int`/`bool` can't create one, so the check can be skipped in those cases. Alternatively,
   detect cycles lazily in `zonk` with a visited set. The check isn't expensive enough to delete; it's the only thing
   standing between a type error and a crash.

### Selected exercises

- **Beginner.** A `let` with an annotation calls `self.check(init, &t)`; `check` falls through to `infer(5) = int`, finds
  it incompatible with `bool`, and reports at `init.span`, which is the `5`.
- **Intermediate (predicted).** Still 5 errors for `main`: `compatible` already treats `{error}` as compatible with
  everything, so `z * 2` produces no new message. What changes is the *result*: `w` becomes `int` instead of `{error}`.
  The short-circuit's real job is to keep a *guessed* type from propagating: with `if w { }` later, the modified checker
  would report "expected bool, found int" about `w`, a cascade from the original `flag + 1` mistake.
- **Systems (predicted).** Without path compression, a chain of 10,000 bound variables makes each `find` from the end walk
  the whole chain (quadratic total work); with compression, the first walk flattens the chain and later ones take one or
  two steps.
- **Architecture.** Helper functions get fully annotated signatures (parameters and result), and bodies are inferred.
  A body change can't change a caller's type; a signature change re-checks every stored rule that calls the helper
  (reverse index, as for features), and the catalog-style versioning of Chapter 17.4 applies to helpers too.

---

## Chapter 17.6 — Intermediate Representations, CFGs, and SSA

### Interview & architecture questions

**1. Basic blocks.** Straight-line code with one entry and one exit (a terminator). A CFG makes control flow explicit, so
"what can run after this?" and "on which paths?" become graph questions. `&&`, `||`, `if`, `while`, `break`, `return`,
and `?` all hide jumps in an AST and force the lowering.

**2. Dominance.** A dominates B if every path from the entry to B passes through A. B's immediate dominator is its
closest strict dominator (the idoms form a tree). A's dominance frontier is the set of blocks that A doesn't strictly
dominate but that have a predecessor A dominates: where A's influence merges with other paths. An edge B → A where A
dominates B is a back edge, and A is a loop header.

**3. SSA and φ.** Every value is defined exactly once. A φ at the start of a block picks the value that corresponds to the
predecessor control arrived from. φs go at dominance frontiers because that's exactly where two different definitions
of a variable can first meet; anywhere earlier, one definition dominates and no choice is needed.

**4. Minimal versus pruned.** Minimal SSA places a φ wherever the (iterated) dominance frontier says definitions merge,
even when the variable is dead there. Pruned SSA checks liveness first and skips φs for dead variables. In listing
17.6-2, `bb5`'s `%5#1` and `%8#1` are unused φs (with `0` for "undefined on this path").

**5. Directions and meets.** Definite assignment: forward, must; a variable is assigned at a point only if it's assigned on
*every* path there, so paths meet by intersection. Liveness: backward, may; a variable is live if it *might* be used on
*some* path, so paths meet by union.

**6. Initial values.** Iteration finds a fixpoint, and the starting point decides which. A must-analysis must start at
"everything" (top) and remove facts only when some path lacks them, reaching the greatest fixpoint; starting from the
empty set, a loop's back edge contributes nothing on the first pass and facts are lost forever (the chapter's debugging
exercise). A may-analysis starts at "nothing" (bottom) and adds facts only when some path supplies them; starting from
"everything" would reach a correct but useless fixpoint (everything live).

**7. MIR isn't SSA.** MIR is built around places (locals with projections like `_6.0`), because borrow checking asks
which place a loan covers and whether a later access to that place conflicts, and drop elaboration asks whether a place
is initialized on each path. In SSA a variable dissolves into many values and memory locations disappear, so those
questions would have to be reconstructed.

**8. LLVM names.** `.sroa` means SROA (scalar replacement of aggregates, which subsumes `mem2reg`) promoted the stack
slots into SSA values. `lcssa` is loop-closed SSA, a normal form with a φ at the loop exit for each value leaving the
loop. `nuw nsw` means LLVM *proved* the increment can't wrap (`i` starts at 0 and stays below `n`); the flags weren't in
the source, since release mode has no overflow checks.

**9. `if true`.** rustc runs definite assignment on MIR paths and ignores values: the constant condition still has a
`false` edge on which `x` is unassigned, so the use is E0381 (listing 17.6-7, with the note "without evaluating whether
branch conditions can actually have the values shown"). The JLS specifies definite assignment over syntax and treats a
constant `true` specially ("when false" facts hold vacuously), so Java accepts the equivalent (not verified here).

**10. The planner.** Doing work *early* needs a backward **must** analysis (anticipated on every path from the entry),
started at "everything". A may-analysis (the §10 incident) fetches anything *some* path reads, doing work many
evaluations never need; for this rule and its traffic that roughly doubled calls to a shared service.

### Debugging exercise (bottom start for a must-analysis)

1. Round 1: `bb0`'s IN is the parameters `{n}`, and it defines `i` and `total`, so OUT(`bb0`) = `{n, i, total}`. `bb1`'s
   IN is the intersection of OUT(`bb0`) and OUT(`bb2`), and OUT(`bb2`) still holds its initial value, the empty set; so
   IN(`bb1`) = ∅, and `n` is gone.
2. On later rounds, OUT(`bb2`) contains what the loop body defines (`i`, `total`, and temporaries) but never `n`, because
   nothing in the loop defines `n`; the intersection keeps excluding it. The iteration converges (after 3 rounds) to the
   *least* fixpoint, which is too pessimistic. The correct answer is the *greatest* fixpoint, reached by starting at
   "everything" (listing 17.6-3's `vec![all; n]`).
3. The diamond is acyclic: iterating blocks in order, every predecessor's OUT is computed before a block reads it, so
   initial values are never read and the starting point doesn't matter. `sum_to`'s back edge (`bb2 → bb1`) makes `bb1`
   read OUT(`bb2`) before `bb2` has been processed.

### Selected exercises

- **Advanced (predicted).** Liveness shows `%5` and `%8` dead at `bb5`, so their φs there are skipped: 7 φs remain in
  total (4 in `classify`, 2 in `sum_to`, 1 in `fib`), and the SSA interpreter must still agree with the CFG interpreter
  on all 270 runs.
- **Systems (predicted).** Without `black_box`, LLVM's scalar evolution recognizes the sum and replaces the loop with a
  closed form, as it does for `triangle` in Chapter 17.7.
- **Architecture.** Static part: the CFG and, for each remote read, the paths that reach it. Measured part: branch
  probabilities from shadow mode. Present the estimate as a range with the traffic window it came from, and flag rules
  whose estimate depends on rare branches.

---

## Chapter 17.7 — Optimization

### Interview & architecture questions

**1. Five optimizations.** Constant folding (would the folded operation have trapped?); DCE (does the instruction have
effects or trap?); GVN/CSE (is the first computation available on every path to the second, i.e., does it dominate?);
LICM (would the hoisted operation have executed at all: guards, zero-trip loops?); algebraic simplification (is the
identity exact for this type? `x + 0.0` isn't, for floats).

**2. SSA and GVN.** In SSA each name has one definition, so "is `%3` constant?" looks at one instruction, and replacing a
name everywhere is safe. GVN walks the dominator tree because a value computed in block A is available exactly in the
blocks A dominates; a scoped table, truncated on the way back up, implements that.

**3. The unused `x / d`.** It's the program's behavior for `d == 0`: the division runs before the guard, so
`guarded(1, 0)` must fail. Deleting it changes an error into `Ok(0)`. A linter should say "this division runs before
the `d != 0` check that was meant to guard it" and "unused variable `unused`"; an optimizer must not silently "fix" it.

**4. LICM of a trapping operation.** Legal only if the operation would have executed anyway (its block runs on every
iteration and the loop runs at least once) or can't trap. LLVM's `guarded_sum` makes it legal by *versioning*: it tests
`n != 0` and `a != 0` first, then divides once (keeping the `MIN / -1` check) and multiplies by `n`.

**5. `triangle`.** Scalar evolution recognized `s` as the sum of an arithmetic progression and rewrote the loop's result
as a closed form, `(n - 1)(n - 2) / 2 + 2n - 1` (= `n(n + 1) / 2`). The division by 2 must apply to the full product,
which can exceed 64 bits; a 128-bit multiply (`mul`) and a 128-bit shift (`shld ..., 63`) compute the exact result
modulo 2^64, matching the wrapping loop for every `n`.

**6. Wrapping versus UB.** In Rust, `wrapping_add` is defined for every input, so `x + 1 < x` is true exactly for
`i64::MAX` and the compiler must preserve that (`cmp` + `sete`). In C, signed overflow is undefined, so the compiler may
assume `x + 1` never wraps and fold the comparison to `false`, deleting an overflow check (Chapter 1.1).

**7. Data-race freedom.** A data race is UB, so for a non-atomic location the optimizer may assume no other thread writes
it concurrently; if no store in the loop *in this thread* can change it, the load is loop-invariant and can be hoisted
(Chapter 14.1's `jmp` to itself). An atomic tells the optimizer the location may change at any time; an atomic load in
the loop stays in the loop (Chapter 14.1's `Relaxed` version).

**8. Slower code.** Hoisting and reuse lengthen live ranges, raising register pressure; beyond the available registers,
values spill to the stack (Chapter 17.8). Inlining grows code and instruction-cache pressure. Compilers rematerialize
cheap values instead of keeping them live, and limit hoisting when pressure is high.

**9. JIT versus AOT legality.** A JIT must be right only for the executions its guards admit, because it can
deoptimize. An AOT compiler must be right for every execution forever. C2 can inline a virtual call that has only seen
one receiver class, with a cheap class check that deoptimizes on a new class; LLVM can only emit a guarded fast path
with a permanent slow path (and only with profile data, via PGO).

**10. First passes for a rule language.** Checked constant folding, CSE of pure feature reads, dead-condition warnings
(not deletions). Each needs: a legality predicate (trapping, remote calls, time-dependence), a differential test over a
replay of recorded evaluations, and generated boundary values (zeros, extremes) for every operand that can trap.

### Debugging exercise (trapping instructions not roots)

1. DCE deleted `%0 = x / d` in `bb0`. With trapping instructions no longer roots, liveness starts only from terminators;
   nothing reads `%0`'s value, so it's unmarked and swept.
2. `left` is the unoptimized program (`run_ssa(&before, ..)`), which returns `Err("division by zero")`; `right` is the
   optimized one, `Ok(0)`. The left side is correct: Ore's semantics divide unconditionally on the first line, and the
   guard comes after.
3. The test loops `x` from -6 and, inside, `y` from -3 to 9; for `guarded` the first input with `d == 0` is `x = -6,
   y = 0`. Tests built from "normal" inputs rarely contain the zero divisor of a division that *looks* dead, which is why
   generated boundary values matter.
4. Predicted from mechanism (verify with `tools/emit.ps1`): rustc's MIR contains the zero and overflow checks as `assert`
   terminators that can panic, which are effects; LLVM may drop the unused `idiv` itself but must keep the checks and the
   panic calls. The panic is observable, so it stays.

### Selected exercises

- **Beginner.** `x - x → 0` is legal (exact under wrapping arithmetic, never traps). `x / x → 1` is illegal: for `x = 0`
  the source traps and the rewrite returns 1.
- **Intermediate.** Legal: if the dominating identical division executed, it either trapped (so control never reaches
  the second) or succeeded (so the second would produce the same value with the same operands in SSA). The 676-run
  differential test should still pass (predicted).
- **Systems (predicted).** With `black_box` on the counter, scalar evolution can't see the induction variable, and the
  loop stays; benchmarks of loops must prevent this kind of recognition, or they measure a formula (Chapter 20.2).

---

## Chapter 17.8 — Code Generation and Register Allocation

### Interview & architecture questions

**1. Three jobs.** Instruction selection (which machine instructions implement each IR operation, including patterns like
`lea` for `x * 3`), register allocation (which values live in registers and which in stack slots, and where to spill and
reload), and emission (machine code plus symbols and relocations for the linker).

**2. Live intervals.** From a value's definition to the last point where it's live. A value used at the top of a loop is
needed again on the next iteration, so it's live across the whole body until the back edge (listing 17.8-1's `v1:[0,20]`
despite a last textual use at 11).

**3. Linear scan.** Sort intervals by start; keep active intervals in registers; free registers of intervals that ended
before the new one starts; if none is free, spill whichever interval ends last (the new one, or an active one that ends
later, whose register it takes). It ignores *how often* each value is used, and whether its uses are inside loops.

**4. Linear scan versus coloring.** Linear scan is fast (linear after sorting) and produces decent code; graph coloring
builds an interference graph (potentially quadratic) and produces better allocations. HotSpot's C1 must compile many
methods quickly, so it uses linear scan (with interval splitting); C2 compiles only hot methods and can afford coloring.

**5. System V.** Integer arguments go in `rdi, rsi, rdx, rcx, r8, r9`; the seventh on the stack (`[rsp + 8]` on entry,
above the return address). Caller-saved registers may be clobbered by a callee; callee-saved ones (`rbx, rbp, r12–r15`)
must be restored. `across_call` needs `x`, `y`, and `p + q` after the call, so they live in callee-saved `r14`, `rbx`,
and `r15`, which the function saves and restores with `push`/`pop`.

**6. Red zone.** System V reserves 128 bytes below `rsp` that signal handlers and the kernel won't touch, so a leaf
function can use it as scratch without adjusting `rsp`. `opaque` calls nothing, so it stores the value at `[rsp - 8]`.

**7. Debug frames.** Debug builds keep every local in its own stack slot (an `alloca`, so a debugger can find it) and
don't inline, so every frame is bigger and there are more of them: 1,968 versus 224 bytes per nesting level in Chapter
17.3's parser.

**8. Testing unexecutable output.** Write an emulator for the generated code and an interpreter for the input code, run
both on many inputs, and compare (listing 17.8-1: every K, four inputs, all equal). The interpreter is the oracle; the
emulator makes the output observable.

**9. Renaming and spills.** Renaming maps the 16 architectural registers onto a larger physical register file to remove
false dependencies between reuses of the same register name. A spill is a real load or store instruction that the
compiler emitted; the CPU can't turn it back into a register access (store-to-load forwarding shortens it, but it's
still on the critical path when the value is).

**10. JIT for rules.** Ask for: profiles showing evaluation (not feature fetches) as a significant share of CPU; the p99
contribution of evaluation; the measured gain over closure compilation on real rules. Object to: executable memory in a
production service (W^X policies), miscompilation as silent wrong decisions, a new security surface ("input becomes
machine code"), harder debugging, and per-rule compile latency on every change.

### Debugging exercise (K = 4 spill choices)

1. `v1`–`v4` (starts 0–3) take the four registers. `v5` `[4,20]` finds none free; the active interval ending last ends
   at 20, which isn't *later* than 20, so the heuristic spills `v5` itself (slot 0), and likewise `v6` and `v7` (slots 1,
   2). `v8` (`acc`, `[7,22]`) ends later than every active interval, so it's spilled itself (slot 3), and `v9` (`i`,
   `[8,20]`) too (slot 4). Then `v10` `[11,12]` evicts the active interval ending last (`v4`, slot 5) and takes its
   register, and `v11` evicts `v3` (slot 6). Seven spills: `v3`–`v9`, matching the generated code.
2. Uses per iteration: `i` is read three times (the test, `a * i`, `i + 1`) and written once; `acc` is read and written
   once; each of `a`–`g` is read once. The heuristic compares only interval *ends*, which are identical (20) for all
   the loop-invariant values, and never considers use counts or loop depth.
3. A classic spill weight is (uses + defs), each weighted by 10^(loop depth), divided by interval length; spill the
   lowest weight. At K = 4 the loop temporaries need two registers, so the other two would go to `i` and `acc`,
   removing about six memory accesses per iteration while adding two (for `a` and `b`) (predicted: roughly 40 fewer
   memory operands at `n = 10`). Verify by implementing it: the differential check must still pass for every K, and
   "memory operands executed" at K = 4 should drop below 138.

### Selected exercises

- **Beginner.** From the K = 4 code: `v1` (`a`) → `rcx`, `v2` (`b`) → `rdx`, `v3` → `[rsp+48]`, `v4` → `[rsp+40]`,
  `v5` → `[rsp+0]`, `v6` → `[rsp+8]`, `v7` → `[rsp+16]`, `v8` (`acc`) → `[rsp+24]`, `v9` (`i`) → `[rsp+32]`, and the
  temporaries `v10`–`v16` alternate between `rdi` and `rsi`.
- **Intermediate.** `a * 3` → `lea d, [a + 2*a]` requires `a` in a register; `a * 8` → `shl d, 3`. The emulator needs
  both forms, and the differential check must pass for every K.

---

## Part XVII Review — Capstone: Sieve's first compiler

**1. The defects.**

| # | Defect | Stage (chapter) | What happens in production |
|---|---|---|---|
| 1 | Identifiers use `is_alphabetic` and string literals accept any character: `country == "DЕ"` (Cyrillic `Е`) compiles and never matches | lexer (17.2) | a rule silently never fires; found weeks later, if a "never fired" report exists |
| 2 | `s.parse().unwrap()` on integer literals | lexer (17.2) | a too-large literal panics the compiler |
| 3 | `while chars[i] != '"'` runs past the end on an unterminated string; unknown characters hit `.expect("unknown operator")` | lexer (17.2) | malformed input panics the compiler instead of producing an error |
| 4 | Comparisons are associative (`1 < amount < 1000` parses as `(1 < amount) < 1000`) | parser (17.3) | together with #7's coercion, the rule is true for *every* positive amount |
| 5 | Errors are `"parse error"` with no span or expectation, and trailing tokens are silently ignored (`amount > 1000 country` parses as `amount > 1000`) | parser (17.3) | authors can't find their mistakes; half-written rules deploy with half their meaning |
| 6 | Unknown features default to `V::Int(0)` (`velocty_1h`) | resolver missing (17.4) | typos make conditions silently false (or true, for `<`) |
| 7 | No type checker: `amount > "1000"` is silently false, booleans are coerced to integers, and a rule that isn't a condition (`amount + 1`) is silently false | types missing (17.5) | type confusion becomes wrong decisions, never errors |
| 8 | Constant folding uses unchecked `x * y` and `x + y` | optimizer (17.7) | panics in debug builds ("attempt to multiply with overflow"); in release, wraps to a garbage threshold |
| 9 | `&&` and `||` evaluate both operands before combining | evaluator / lowering (17.6) | `graph_score` fetched 10,000 times instead of 4,950: another team's service pays |

**2. Severity.** For a risk system, silent decision changes are the worst, because nothing alerts: #1, #4 with #7, #6, #7,
and #8 in release. Compiler crashes (#2, #3, #8 in debug) come next, and they're worse than they look, because of the
architectural finding below: this compiler runs *inside `check`*, on the evaluation path, so a panic happens on live
traffic for every evaluation of that rule (a crash loop if the service aborts on panic, as in the Part IX interlude).
#9 costs another team's service (Chapter 17.6 §10). #5 mostly costs debugging time, except the trailing-input case,
which silently changes meaning.

**3. The architectural finding.** `check` lexes, parses, and folds the rule on *every evaluation*: a `Vec<char>` of the
whole rule, a `String` per token, a `Box` per node, each time. From Chapters 17.2 and 17.3's numbers (tens of
nanoseconds per owned token, an allocation per node), a ten-token rule costs on the order of microseconds per
evaluation, hundreds of times the 6.28 ns closure evaluation of Chapter 10.1 (predicted from mechanism): at 2 million
evaluations per second, whole cores spent recompiling unchanged rules. And every compile error (or panic) happens at
evaluation time, on live traffic. Chapter 17.1's design puts lexing through folding in the rule service at upload
time; evaluation services load a compiled form (the redesign's `Rule::compile` once, `Rule::eval` many times).

**4. Rule [3].** The parser grouped `1 < amount < 1000` as `(1 < amount) < 1000` (#4). The evaluator computed
`1 < 5000` as `Bool(true)`, then `as_int` coerced it to `1` (#7), and `1 < 1000` is true. Either fix alone breaks the
chain: a non-associative parser rejects the rule; a type checker rejects `Bool < Int`.

**5. The messages.** The redesign's output is one good answer: [1] a non-ASCII character in a literal, pointing at the
`Е`; [2] "integer literal is too large", spanning the literal; [3] "comparison operators cannot be chained; use `&&`",
at the second `<`; [4] "unknown feature `velocty_1h`; did you mean `velocity_1h`?", spanning the name; [5] "mismatched
types: Int > Str", spanning the comparison; [6] "constant expression overflows i64", spanning the folded product;
[7] "expected a value or a feature name", at the end of the input. Each is produced by the stage that owns the error
class, and none requires evaluating the rule.

---

## Part XVII Review — Interview mode

**1. Errors by stage.** Upload time: lexical (bad characters, malformed literals, confusables), syntactic (structure,
chained comparisons, depth), resolution (unknown or deprecated features, with suggestions), types (unit and currency
mismatches, non-boolean rules), constant overflow, and statically dead conditions (warnings). Evaluation time: remote
feature failures and timeouts (with a fail-open/closed policy), run-time arithmetic errors on data (division by a
zero-valued feature), currency conversion with stale rates. Never catchable: a rule that is well-formed and wrong
(wrong threshold, wrong feature chosen); only shadow mode, "never fired" reports, and review catch those.

**2. Non-associative comparisons.** `(a < b) < c` compares a boolean with a number, which is almost always a mistake, and
Rust also needs `<` unambiguous for its generic syntax. For a DSL, make them non-associative and suggest `&&`; offer a
range form (`amount in 1..1000`) if authors need one.

**3. Rust's choices.** Shadowing: makes transformation chains readable (`let s = s.trim();`) without inventing names,
and the type can change; the counterargument is accidental reuse in long functions. No inferred signatures: stability,
locality, and parallel checking (E0121). No generalized closures: closures capture environments and have unique types;
generic functions give polymorphism with monomorphization instead. Arguing the opposite for signatures: ML shows
whole-module inference works for small programs and removes boilerplate; the cost appears at scale (API drift, error
distance, compile-time structure).

**4. `a + 1 < b * 2 && c`.** Tokens: `Ident(a) Plus Int(1) Lt Ident(b) Star Int(2) AndAnd Ident(c)`. Pratt: `+` (9, 10)
binds `a + 1`; `<` (7, 8) takes it as left operand; `*` (11, 12) binds `b * 2`; `&&` (5, 6) combines: `(&& (< (+ a 1)
(* b 2)) c)`. Types: `+` and `*` need ints, `<` gives bool, `&&` needs bools, so `c: bool`. Lowering: `%0 = a + 1;
%1 = b * 2; %2 = %0 < %1; branch %2 ? rhs : short`, with `rhs: r = c; jump join`, `short: r = 0; jump join`, and the
result `r` in `join` (in SSA, a φ).

**5. SSA, MIR, LLVM.** SSA: every value defined once, merges through φs. MIR isn't SSA because borrow checking and drop
elaboration reason about places (memory locations), not values. rustc emits LLVM IR that keeps locals in `alloca`s, and
LLVM's `mem2reg`/SROA promote them to SSA values with φs.

**6. Two analyses.** Definite assignment: forward, must, meet = intersection, start at "everything" (except the entry,
which starts with the parameters). Liveness: backward, may, meet = union, start at "nothing". Starting a must-analysis
at the empty set loses facts on loop back edges and reports spurious errors (listing 17.6-6 reports `n` possibly
unassigned).

**7. Linear scan.** Sort live intervals by start; maintain active intervals in registers; expire finished ones; when no
register is free, spill the interval ending last. A value used at the loop's top is live until the back edge because the
next iteration reads it; computing intervals from textual uses (listing 17.8-3) hands its register out too early.

**8. A compiler's memory.** Trees (one allocation per node without arenas: 1,500,001 versus 21), names (one per
occurrence without interning: 200,017 versus 2,039), and per-stage results. Arenas collapse allocations and free all at
once; interning makes names 4 bytes and comparisons integer operations; side tables keep trees immutable and results
indexable. Node layout shrinks the tree itself (72 → 12 bytes).

**9. Parser stack.** 1,968 bytes per nesting level in debug, 224 in release (ten precedence functions per level; debug
keeps locals in stack slots and doesn't inline). Safe options: a depth limit, stack growth on demand (`stacker`), or an
explicit stack.

**10. Vanishing loops.** `triangle`: scalar evolution replaced the loop with a closed form (using a 128-bit product to
stay exact). `guarded_sum`: versioned on the guard conditions, divided once, multiplied by `n`. Benchmarks of simple
loops may measure a formula instead of the loop; use `black_box` and check the assembly (Chapter 20.2).

**11. Choosing a back end.** Closure compilation (6.28 ns per evaluation in Chapter 10.1, about 1.3% of a core at 2 million
per second), with the tree walker as a reference oracle. Move to a bytecode VM or JIT only if profiles show evaluation
dominating and rules become computation-heavy; stay with closures if feature fetches dominate, as they usually do.

**12. Rolling out a compiler change.** Differential tests on every stored program (old versus new: trees, IR, results on
replayed data), boundary-value generation for trapping operations, shadow evaluation on live traffic with every
differing decision logged, a staged cutover, and a one-switch rollback to the previous compiler version.

**13. A changing symbol table.** Store resolved ids plus the table version; never reuse an id; require a new symbol for a
meaning change; recompile every stored program against a proposed table change in CI and review the diff; keep a
reverse index of uses to block deletions.

**14. Oracles.** Two parsers that must agree (grammar mistakes); an interpreter of the unoptimized IR next to the optimized
one (optimizer legality); an emulator next to generated code, compared with an interpreter of the input (instruction
selection and allocation); shadow mode against the old engine on live traffic (everything, including decisions no test
anticipated). Plus the differential lexer test (token boundaries).

**15. The hoisted division.** Detection: evaluation errors on one rule, a spike in fail-closed declines, and new
merchants complaining. Mitigation: disable the rule (or switch it to fail-open) within minutes. Root cause: a hoisting
pass that checked invariance but not whether the operation could trap. Process changes: a `may_trap` legality rule for
every pass (reviewed like a contract), boundary values in the differential test's data, and a second reviewer for
fail-closed rules who asks what happens to a brand-new merchant.
