# Chapter 17.4 — ASTs, Symbol Tables, and Name Resolution

> **Where this sits:** Part XVII · Compilers · chapter 4 of 8
> **Prerequisites:** Chapter 17.3 (the Ore parser and its AST); Chapter 2.7 (modules and visibility); Chapter 5.2
> (enum layout and boxing large variants); Chapter 9.3 (hashing costs).
> **After this chapter you can:** traverse an AST with a rustc-style visitor; shrink AST nodes with measured techniques;
> intern identifiers; resolve every name in a program with scopes, items, namespaces, and shadowing; produce "did you
> mean" suggestions; and explain hygiene and why a symbol's meaning must never change under a stored program.

---

## Pass 1 · User level — *What does this name mean?*

### 1. Problem

After parsing, a program is a tree full of *strings*: `Var("total")`, `Call(Var("helper"), ...)`, a parameter typed
`"int"`. None of those strings means anything yet. **Name resolution** answers one question for every occurrence of a
name: *which declaration does it refer to?* The answer depends on rules every language chooses differently:

- **Scopes.** A `let` inside a block is invisible outside it. A parameter is visible in its whole function body.
- **Order.** Can you call a function defined further down the file? (Rust: yes. C: only with a prior declaration.) Can a
  `let` refer to itself in its own initializer? (Rust: no, and §13 shows why that matters.)
- **Shadowing.** Does a second `let x` replace the first, or is it an error? (Rust: it shadows. Java: an error for
  locals.)
- **Namespaces.** Can `int` be a type *and* a variable at the same time? (Rust: yes, types and values live in different
  namespaces.)
- **Macros.** Does a variable a macro introduces capture the caller's variable of the same name? (Rust `macro_rules!`:
  no, which is called *hygiene*.)

Around resolution sit three engineering questions: how to walk the tree without writing the traversal a dozen times
(**visitors**), how to keep names cheap to store and compare (**interning**), and where to record the answers
(**side tables**). This chapter answers all of them with Ore, then with rustc.

### 2. Mental model

A resolver walks the tree with a **stack of scopes**, and records each answer in a table keyed by the name's position:

```text
fn main() -> int {             module scope (pass 1): { main: fn#0, helper: fn#1, bad: fn#3 }
    let x = 1;                   block scope: { x: local#0 }
    let x = x + 1;               initializer resolved FIRST (x -> local#0), then x: local#1 shadows it
    let int = 5;                 { x: local#1, int: local#2 }   (a VALUE named int)
    helper(x, int)               helper -> fn#1 (module scope), x -> local#1, int -> local#2
}

side table:  byte offset of each use  ->  Local(id) | Fn(id)
```

Five rules produce that picture:

1. **Items first.** Pass 1 collects every function into the module scope, so a call may precede the definition.
2. **Innermost wins.** Lookup searches scopes from the innermost outward, then the module scope.
3. **Initializer before declaration.** `let x = x + 1` resolves the right-hand side while only the *old* `x` exists,
   then declares the new one.
4. **Namespaces are separate tables.** Looking up a type never finds a variable, and vice versa.
5. **Answers go in a side table**, not into the AST. The tree stays what the parser produced; resolution results,
   types (Chapter 17.5), and later facts are tables keyed by node identity.

### 3. Rust code

**Walking the tree: a visitor.** Every later analysis needs to traverse the AST, and writing the full recursive match
in each one is how traversal bugs multiply. Listing `ch04-01-ast-visitor.rs` uses rustc's pattern: a `Visitor` trait
whose default methods delegate to free `walk_*` functions that visit the children.

```rust,ignore
pub trait Visitor<'ast>: Sized {
    fn visit_fn(&mut self, f: &'ast FnDecl) { walk_fn(self, f) }
    fn visit_block(&mut self, b: &'ast Block) { walk_block(self, b) }
    fn visit_expr(&mut self, e: &'ast Expr) { walk_expr(self, e) }
}
```

An analysis overrides only what it cares about. The call-graph analysis overrides one method:

```rust,ignore
impl<'ast> Visitor<'ast> for CallGraph<'ast> {
    fn visit_expr(&mut self, e: &'ast Expr) {
        if let ExprKind::Call(callee, _) = &e.kind {
            if let ExprKind::Var(name) = &callee.kind {
                self.calls.push(name);
            }
        }
        walk_expr(self, e); // keep going into children (forgetting this is the classic visitor bug)
    }
}
```

(Excerpts of listing 17.4-1.) The `walk_expr` call at the end is the part people forget: without it, the visitor stops
at the first call and misses `fib(n - 1)` nested inside `fib(n - 1) + fib(n - 2)`. Running the call graph and a node
census over three functions (real output):

```text
fib     calls ["fib", "fib"] 17 exprs, 3 blocks, 0 loops  (recursive)
sum_to  calls []             17 exprs, 2 blocks, 1 loops
main    calls ["sum_to", "fib"] 7 exprs, 1 blocks, 0 loops
```

**Resolving names.** Listing `ch04-03-resolver.rs` resolves Ore in two passes. Pass 1 collects items and reports
duplicates:

```rust,ignore
    pub fn collect_items(&mut self, items: &'a [FnDecl], src: &str) {
        for (i, f) in items.iter().enumerate() {
            let name_span = Span { lo: f.span.lo + 3, hi: f.span.lo + 3 + f.name.len() as u32 };
            if let Some(&(_, prev)) = self.fns.get(f.name.as_str()) {
                self.error(name_span, format!("the name `{}` is defined multiple times", f.name),
                           Some(format!("previous definition of `{}` is on line {}", f.name, line_col(src, prev.lo).0)));
            } else {
                self.fns.insert(&f.name, (i as u32, name_span));
            }
        }
    }
```

Pass 2 walks each body with the scope stack. Two short pieces carry most of the semantics, the lookup order and the
`let` order:

```rust,ignore
    fn lookup(&self, name: &str) -> Option<Res> {
        for scope in self.scopes.iter().rev() {
            if let Some(&id) = scope.get(name) {
                return Some(Res::Local(id));
            }
        }
        self.fns.get(name).map(|&(id, _)| Res::Fn(id))
    }
```

```rust,ignore
                Stmt::Let { name, mutable, init, span, .. } => {
                    self.expr(init); // the initializer is resolved BEFORE the new name exists
                    let decl = Span { lo: span.lo, hi: span.lo + 3 }; // the `let` keyword
                    self.declare(name, *mutable, decl);
                }
```

(Excerpts of listing 17.4-3.) The test program has a shadowed `x`, a local *named* `int`, a call that precedes its
callee's definition, a typo, a duplicate function, an unknown type, and an assignment to an immutable parameter. Real
output:

```text
resolutions in `main` (value namespace):
  3:13  x       -> local #0 `x` declared at 2:5
  5:5   helper  -> fn #1 `helper`
  5:12  x       -> local #1 `x` declared at 3:5
  5:15  int     -> local #2 `int` declared at 4:5
type annotations resolved: 7 (int/bool live in the type namespace)

4 errors:
  10:46: error: cannot find value `totl` in this scope
         help: a local variable or fn with a similar name exists: `total`
  13:4: error: the name `helper` is defined multiple times
         help: previous definition of `helper` is on line 8
  15:19: error: cannot find type `intt` in this scope
         help: a type with a similar name exists: `int`
  16:5: error: cannot assign twice to immutable variable `n`
         help: consider making this binding mutable
```

Read the first block line by line. The `x` on line 3 (inside `let x = x + 1`) resolves to local #0, the *first* `x`,
because the initializer was resolved before the new `x` existed. The `x` on line 5 resolves to local #1. `int` is an
ordinary local in the value namespace, while the seven type annotations (`-> int`, `a: int`, ...) resolved in the type
namespace, and neither disturbed the other. `helper` is called on line 5 and defined on line 8, which works because of
pass 1.

The suggestions come from edit distance: the resolver computes the Levenshtein distance between the unknown name and
every visible name, and suggests the closest one within a threshold that scales with the name's length (`len / 3`,
minimum 1).

**rustc does the same.** Listing `ch04-05-rust-e0425.rs`:

```rust,compile_fail
fn main() {
    let total = 10;
    let doubled = totl * 2;
    println!("{doubled}");
}
```

```text
error[E0425]: cannot find value `totl` in this scope
 --> src/main.rs:5:19
  |
5 |     let doubled = totl * 2;
  |                   ^^^^
  |
help: a local variable with a similar name exists
  |
5 |     let doubled = total * 2;
  |                      +
```

And listing `ch04-06-rust-namespaces-hygiene.rs` observes Rust's resolution rules directly: items visible before their
definition, a value and a type both named `i32`, a hygienic macro, and shadowing (real output):

```text
used before its definition: 101
value namespace vs type namespace: 14
hygiene: double_it!(x) = 20 (unhygienic textual expansion would give 4)
an item defined by a macro: answer() = 42
shadowing: v = 2
```

The hygiene line is the interesting one. The macro is:

```rust,ignore
macro_rules! double_it {
    ($e:expr) => {{
        let x = 2; // the macro's own `x`
        x * $e     // `$e` still refers to the caller's `x`
    }};
}
```

(Excerpt of listing 17.4-6.) The caller writes `double_it!(x)` with its own `x = 10`. A textual expansion (a C
preprocessor) would produce `let x = 2; x * x`, which is 4. Rust's expansion keeps the two `x`s apart because each
identifier carries the *syntax context* it came from, and resolution compares names *and* contexts: 2 × 10 = 20.

---

## Pass 2 · Systems level — *Tables, symbols, and syntax contexts*

### 4. Under the hood

**rustc's resolver** [RUSTC]. Name resolution in rustc (`rustc_resolve`) runs on the AST *interleaved with macro
expansion*, not after it: expanding a macro can define new items (`make_fn!(answer)` above defines `fn answer`), and
resolving a path can require expanding a macro first, so the two run as a fixed-point loop until nothing changes
(Chapter 18.2). The results are recorded per AST node id in side tables, and lowering to HIR (Chapter 18.2) bakes
them in: in HIR, every path already knows its `Res` (a definition, a local, a primitive type, and so on). Suggestions
use an edit-distance implementation in `rustc_span` plus many special cases (a similarly named item in another module
that could be imported, a method instead of a function, a missing `self.`), which is why rustc's "did you mean" is
better than listing 17.4-3's.

**Namespaces** [LANG]. The Reference defines separate namespaces, chiefly the *type namespace* (types, traits, modules),
the *value namespace* (functions, constants, statics, local variables), and the *macro namespace*, plus lifetimes and
loop labels. That's why `let i32 = 7_i32; let n: i32 = i32 * 2;` compiles: the annotation is looked up in the type
namespace, the multiplication's operand in the value namespace. It's also why a tuple struct `struct Meters(u64);`
occupies *both*: the type `Meters` and the constructor function `Meters`.

**Hygiene** [LANG]. `macro_rules!` macros have *mixed-site* hygiene: local variables, labels, and `$crate` introduced by
the macro resolve where the macro was *defined*, while other names (items, like `fn answer`) resolve where it was
*invoked*. Procedural macros default to *call-site* hygiene, where everything resolves as if the caller had written
it. The mechanism is the syntax context attached to every span (Chapter 17.2's 8-byte `Span` includes it): a name is
the pair (symbol, context), not just the symbol.

**Visitors in rustc** [RUSTC]. rustc has visitor traits for each IR (AST, HIR, THIR, MIR), all with the same design as
listing 17.4-1: `visit_*` methods with default bodies that call `walk_*`. Analyses such as lints override a handful of
methods. The classic bug is the same too: forgetting to call `walk_*` in an override silently skips a subtree.

### 5. Memory

**Node layout.** Listing 17.4-1 measures four shapes for the same expression enum (plus Ore's own `Expr`):

```text
bytes per expression node (+ 8 for a span, if stored inline):
  1. String names, If inline       72
  2. box the If variant            32
  3. + interned names, boxed Call  24
  4. arena indices                 12
  this listing's Ore Expr          64  (ExprKind 56 + Span 8)
```

An enum is as large as its largest variant (Chapter 5.2), so one fat variant taxes every node:

1. **72 bytes.** `If` holds a condition and two `Block`s inline. Every `Int(5)` pays for them.
2. **32 bytes.** Boxing the rare, large `If` variant brings the enum down to the size of `Call(Box<Expr>, Vec<Expr>)`.
3. **24 bytes.** Interning names (a 4-byte symbol instead of a 24-byte `String`) and boxing the call's argument list
   leave `Binary(u8, Box, Box)` as the largest variant.
4. **12 bytes.** In an arena (Chapter 17.3), children are `u32` indices, and large literals move to a side table.

Going from 72 to 12 bytes is a 6× reduction in the memory for the tree, and a 6× increase in how many nodes fit in a
cache line. rustc applies all four techniques, and enforces them: it has static assertions on the sizes of its hottest
types (the AST and HIR expression kinds among them), so a change that grows a node fails the build [RUSTC].

**Interning.** Listing `ch04-02-interner.rs` interns every identifier of a synthetic program with 1,000 distinct local
names used over and over. The interner maps each distinct string to a `Symbol(u32)` and stores the text once:

```rust,ignore
    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&sym) = self.map.get(s) {
            return sym; // hit: no allocation
        }
        let sym = Symbol(self.strings.len() as u32);
        let rc: Rc<str> = Rc::from(s);
        self.strings.push(rc.clone());
        self.map.insert(rc, sym);
        sym
    }
```

(Excerpt of listing 17.4-2.) Real output (release; allocations exact):

```text
200000 identifier occurrences, 2001 distinct
  Vec<String>:  200017 allocations, 24 B per handle
  Vec<Symbol>:    2039 allocations, 4 B per handle
  round trip: Symbol(0) -> "let"
```

Owned strings cost one allocation per *occurrence* (200,000, plus vector growth). Interning costs one per *distinct*
name (2,001) plus the table's growth, 98× fewer, and each handle is 4 bytes instead of 24. rustc's `Symbol` is exactly
this: a `u32` index into a session-wide interner, with keywords and common names pre-interned at fixed indices so that
comparing against `kw::SelfLower` is an integer comparison [RUSTC].

**Side tables.** Listing 17.4-3's resolver records answers in `HashMap<u32, Res>` keyed by a use's byte offset. rustc
keys its tables by node identity (`NodeId` on the AST, `HirId` on HIR) and prefers dense vectors indexed by those ids
(`IndexVec`) over hash maps where ids are contiguous [RUSTC]. Side tables keep the AST immutable and shareable, let
each analysis own its results, and make "which nodes did this analysis touch?" answerable, which incremental
compilation needs (Chapter 18.1).

### 6. CPU / OS

The interner listing also times two things later passes do constantly, looking names up in a symbol table and
comparing names (release, best of 5, one Playground run: noisy):

```text
lookups (release, best of 5, one run): String keys 4.04 ms, Symbol keys 2.36 ms
equality scans:                          String 0.16 ms, Symbol 0.04 ms
```

A `HashMap<Symbol, _>` lookup hashes 4 bytes; a `HashMap<String, _>` lookup hashes the whole string (with the default
SipHash, Chapter 9.3) and then compares it byte by byte. Equality on symbols is a single integer comparison, on
strings a length check plus a `memcmp`. In a compiler these operations run once per name *per pass*, so the constant
factor compounds: resolution, type checking, and lints all compare names.

Scope lookup has its own cost model. A stack of hash maps, as in listing 17.4-3, costs one probe per enclosing scope,
and deeply nested code probes many empty scopes. Alternatives trade build cost for lookup cost: one map from symbol to
a *stack of bindings* (push on declaration, pop on scope exit) makes every lookup a single probe. After resolution,
none of this matters: the resolver has replaced every name with an id, which later stages index directly. That is the
whole point of resolving names once.

---

## Pass 3 · Architect level — *Names are contracts*

### 7. Trade-offs

| Decision | Option A | Option B | What decides it |
|---|---|---|---|
| Where answers live | side tables keyed by node id | fields on AST nodes (`resolved: Option<DefId>`) | several analyses, incremental reuse, and immutability favor side tables |
| How many passes | collect items, then resolve bodies | one pass, declarations before uses | forward references and mutual recursion need two |
| Resolution vs type checking | separate passes (listing 17.4-3) | fused (listing 17.5-1) | a separate pass reports all name errors before any type errors; fused is less code |
| Name representation | `Symbol(u32)` + interner | `String` / `Rc<str>` | interning pays off as soon as names are compared or hashed more than once |
| Shadowing | allowed (Rust) | forbidden (Java locals) | readability of transformations (`let x = x.trim()`) versus accidental reuse |
| Hygiene | mixed-site (`macro_rules!`) | none (C macros) | whether macro authors can reason locally |

> **Why not store the answer in the AST node?** It looks simpler: add `res: Option<Res>` to `Var`. But then the AST
> must be mutable after parsing, every consumer must handle the "not resolved yet" state, and the tree can no longer
> be shared between threads or cached between compilations without copying. Side tables give each stage its own
> immutable output. rustc's HIR goes one step further: it's a *new* tree in which resolution is part of the structure,
> so later stages can't observe an unresolved path at all.

### 8. Java comparison

`javac` resolves names in phases that mirror listing 17.4-3's two passes [LIB]. The *Enter* and *MemberEnter* phases
build symbol tables for every class and member first, which is why a Java method may call another defined later in
the file, or in a class compiled in the same batch. Then *Attr* ("attribute") resolves the names in method bodies and
type-checks them in the same pass, the fused design of Chapter 17.5.

Java's scoping rules differ from Rust's in two places a Java engineer notices immediately:

- **Locals can't shadow locals.** `int x = 1; { int x = 2; }` is an error ("variable x is already defined"). Rust's
  `let x = x + 1` idiom has no Java equivalent. A local *can* shadow a field, which is why Java code writes `this.x = x`.
- **Namespaces exist, with "obscuring" rules.** The JLS separates package, type, and expression names, and a simple
  name that could be both a variable and a type is resolved by context rules (§6.4.2, obscuring). Rust keeps the
  namespaces fully separate, so the question never arises.

Java has no macros and therefore no hygiene. Annotation processors generate *new source files*, which are then
compiled like hand-written code: generated code that accidentally uses a name from the surrounding package simply
resolves to it.

> **Analogy limit.** In Rust, resolution is finished at compile time: every path in the binary refers to a definition
> the compiler saw. In Java, *linking* is deferred to run time: a class compiled against version 1 of a library can
> load against version 2 and fail with `NoSuchMethodError` when a call executes. And Java inlines compile-time
> constants (`static final int LIMIT = 5`) into the *calling* class, so changing `LIMIT` without recompiling callers
> leaves them using the old value. The same "a name's meaning changed under a compiled user" failure is §10's subject.

### 9. Production scenario

**Sieve's feature catalog is its symbol table.** Every name a Sieve rule can use lives in the feature catalog, and the
catalog was designed with this chapter's vocabulary:

- **Features are items in the module scope.** Each has a name, a type, an owning service, a cost class (local field,
  or remote call like `graph_score`), and a stable numeric id. Rules can refer to any feature regardless of order,
  and rule-local `let` bindings shadow nothing in the catalog (a `let` with a catalog name is an error, a deliberate
  deviation from Rust's shadowing, because analysts found it confusing).
- **Names are interned to ids at compile time.** A compiled rule stores feature *ids* and the catalog version it was
  compiled against, never names. The evaluator indexes a dense feature vector by id (Chapter 9.5's name-to-index
  pattern, which removed 400 allocations per score in the fraud library).
- **Two namespaces.** Features and functions (`count_distinct`, `ago`) are separate namespaces, so a new feature can't
  break a rule by colliding with a function name.
- **"Did you mean" in the editor.** Unknown names get edit-distance suggestions restricted to features the rule's
  owning team may use.
- **Reverse index.** Because resolution is a side table, "which rules use feature X?" is a query, and the catalog uses
  it to block deleting a feature that stored rules still reference.

### 10. Failure scenario

**The feature that changed meaning.** Before feature ids existed, compiled Sieve rules stored feature *names* and
re-resolved them against the current catalog when an evaluation service loaded them. In June 2026 the fraud team
redefined `velocity_1h` from "payments on this card in the last hour" to "payments at this merchant in the last hour"
(the card-level feature was renamed `card_velocity_1h`). It was a one-line catalog change, reviewed as a data change.

Thirty-seven stored rules still said `velocity_1h > 20`. Their meaning changed without any rule being edited: a
merchant-level count is routinely far above 20 for a busy merchant, so those rules started firing on most payments at
large merchants. The manual-review queue went from about 1,200 cases a day to about 19,000 within five hours, before
on-call pinned the evaluation services to the previous catalog version.

The root cause was a resolution-time error: the rules' names were resolved against a *different* symbol table from the
one they were written against. The fixes are the textbook ones:

1. **Resolve once, store ids.** Compiled rules store feature ids plus the catalog version; loading never re-resolves.
2. **A symbol's meaning is immutable.** An id is never reused; changing what a feature means requires a new feature
   (new id, new name), and the old one is deprecated with the reverse index listing every rule that must move.
3. **Catalog changes run the compiler.** A catalog PR recompiles every stored rule against the proposed catalog and
   shows the diff in resolved ids and types, the same way a library's CI runs its dependents (Chapter 22.7).

The Java analogy limit in §8 is the same failure in another costume: a constant inlined into a client, or a method
linked at run time against a different class than the one compiled against. Rust avoids it for code by resolving
everything at compile time and recompiling dependents; a data-driven language has to rebuild that guarantee
deliberately.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVII).*

1. What is name resolution? List five language rules that change its result, with an example of each.
2. Why does a resolver collect items in a separate pass before resolving bodies? What does a single-pass resolver
   forbid?
3. In `let x = x + 1;`, which `x` does the right-hand side mean, and what resolver ordering produces that answer?
4. Why keep resolution results in side tables rather than AST fields? What does rustc's HIR add to that idea?
5. Explain Rust's namespaces with the `let i32 = 7_i32; let n: i32 = i32 * 2;` example, and the tuple-struct case.
6. What is hygiene? Explain why `double_it!(x)` gives 20, and what mixed-site hygiene means for items versus locals.
7. How do you shrink an AST node from 72 to 12 bytes? Which technique gives the biggest win for the least effort?
8. What does interning buy, in allocations, memory, and time? Quote this chapter's numbers.
9. How does "did you mean" work, and why does the threshold scale with the name's length?
10. A data-driven rule language stores rules by name. What can go wrong when the name table changes, and what
    guarantees does storing resolved ids restore?

### 12. Exercises

- **Beginner.** Add a lint pass to listing 17.4-1 as a visitor: report every `let` binding that is never used
  (resolution gives you the uses). Make sure it recurses into nested blocks.
- **Intermediate.** Replace listing 17.4-3's stack of `HashMap`s with one `HashMap<String, Vec<u32>>` of binding stacks
  plus a per-scope list of names to pop. Show it resolves the test program identically.
- **Advanced.** Intern all names in listing 17.4-3 (use listing 17.4-2's interner) so the resolver compares `Symbol`s.
  What changes in the "did you mean" code, which needs the text?
- **Systems.** Measure listing 17.4-2's lookup benchmark with `foldhash` or `FxHash` instead of SipHash for both key
  types (both crates are on the Playground; one run each, noisy). How much of the String/Symbol gap was hashing, and how
  much was comparison?
- **Architecture.** Sieve wants reusable rule snippets (`include "new_merchant_checks"`). Design their name
  resolution so that a snippet's local names can't capture or be captured by the including rule's names. Relate your
  design to macro hygiene.

### 13. Debugging exercise

Listing `ch04-04-resolver-bug.rs` contains two resolvers for a sequence of `let` statements that differ in one ordering
decision (`declare_before_init`). Both resolve and evaluate this program, whose `y` should be 4:

```text
let x = 1;  let x = x + 1;  let y = x + x;
```

```rust,ignore
fn resolve(prog: &[Let], declare_before_init: bool) -> Vec<(usize, R)> {
    let mut scope: Vec<(&str, usize)> = Vec::new();
    let mut out = Vec::new();
    for (slot, l) in prog.iter().enumerate() {
        if declare_before_init {
            scope.push((l.name, slot));
            out.push((slot, resolve_expr(&l.init, &scope)));
        } else {
            out.push((slot, resolve_expr(&l.init, &scope)));
            scope.push((l.name, slot));
        }
    }
    out
}
```

(Excerpt of listing 17.4-4.) One mode prints `y = 4`, the other `y = 2`.

1. Which mode is wrong? Work out the resolved form of the second `let` in both modes (which slot does its `x` read?).
2. Why doesn't the wrong mode crash? What value does it read, and why? (Look at how `main` initializes `slots`.) What
   would Rust do with the equivalent program?
3. Declaring before resolving the initializer is *correct* for one construct. Which one, and why?

### 14. Design exercise

**Design Sieve's feature catalog as a symbol table.** It holds about 900 features across 14 owning services, changes a
few times a day, and is read by the rule compiler and three evaluation services. Decide:

- The identity of a feature (id, name, version) and which of them compiled rules store.
- The namespaces (features, functions, lists, snippets) and what happens when a name exists in two of them.
- Renames, deprecations, type changes, and deletions: which are allowed in place, which need a new id, and what the
  catalog's CI checks before accepting a change.
- How "did you mean" suggestions respect team ownership and deprecated features.
- How an evaluation service handles a rule compiled against a catalog version it doesn't have yet.
