# Chapter 18.2 — Macro Expansion, Name Resolution, and HIR Lowering

> **Where this sits:** Part XVIII · How rustc Works · chapter 2 of 7
> **Prerequisites:** Chapter 18.1 (queries), Chapter 17.2–17.4 (lexing, parsing, name resolution in general),
> Chapter 2.6 (a real `#[derive]` expansion), Chapter 8.1 (the `?` desugaring).
> **After this chapter you can:** describe how rustc turns tokens into a fully expanded, resolved AST and then into HIR;
> read real HIR and recognize the desugaring of `for`, `?`, `while let`, `async`, `.await`, ranges, and `format_args!`;
> predict what a `macro_rules!` macro can and cannot see (hygiene); and set a team policy for macros and proc macros that
> accounts for their build-time, security, and correctness costs.

---

## Pass 1 · User level — *What the compiler rewrites before it checks anything*

### 1. Problem

By the time rustc type-checks your function, it isn't looking at your function any more. `for x in xs` has become a
`loop` with a `match` on `Iterator::next`. `?` has become a `match` with an early `return`. `.await` has become a polling
loop with a `yield`. `println!` has disappeared into calls you never wrote, with the format string already parsed.
Every macro has been run, every name has been tied to a definition, and every elided lifetime has been made explicit.

That rewriting explains several things that otherwise look like magic or bugs:

- Why a `macro_rules!` macro can't see a local variable in its caller, but *can* see (and be hijacked by) a function in
  its caller's scope.
- Why `for` loops, `?`, and `.await` work with your own types as long as they implement the right trait: the rewrite
  targets traits.
- Why a format-string typo is a compile error and costs nothing at run time.
- Why a proc macro is a supply-chain risk, and why proc-macro-heavy crates sit on the critical path of every build.

### 2. Mental model

```text
 source text
     │  rustc_lexer: characters → tokens
     ▼
 token trees             ( [ {  ...  } ] ) grouped; a macro's input is just a token tree
     │  rustc_parse
     ▼
 AST with unexpanded macro calls         vec![1, 2, 3] is still a "macro call" node
     │  ┌────────────────────────────────────────────────────────────┐
     │  │ EXPANSION ⟷ RESOLUTION, repeated until nothing is left      │
     │  │  find macro calls → resolve each macro's path → run it →    │
     │  │  parse its output → new items/imports may enable more       │
     │  │  resolution → new macro calls may appear → repeat           │
     │  └────────────────────────────────────────────────────────────┘
     ▼
 fully expanded AST       (what -Zunpretty=expanded, the Playground's "Expand macros", prints)
     │  late resolution: every path in every body → a definition
     ▼
 AST → HIR lowering       desugar for/?/while let/async/.await/ranges/format_args; make elided lifetimes explicit
     ▼
 HIR                      what type checking (18.3) works on
```

**Hygiene is the rule for which names a macro's code can see.** [LANG] Every identifier remembers which macro
expansion produced it (its *syntax context*), and name lookup takes that into account. For `macro_rules!` the rule is
**mixed-site** (the Reference, *Macros By Example § Hygiene*):

| Name kind in the macro body | Resolved at | Consequence |
|---|---|---|
| Local variables, labels | The macro's **definition** site | A macro's `let t` never collides with the caller's `t`, and the macro can't read the caller's locals unless they're passed in |
| `$crate` | The **defining crate** | `$crate::path::f` always means *that* crate's `f` |
| Everything else: functions, types, modules, other macros | The **call** site | `mask(x)` in a macro body means whatever `mask` is in scope where the macro is called |

Procedural macros are different: [LANG] tokens they create with `Span::call_site()` resolve as if written at the call
site (unhygienic), `Span::mixed_site()` (stable since 1.45) gives `macro_rules!`-style hygiene, and true definition-site
hygiene is unstable.

**Lowering is where sugar dies.** [RUSTC] `rustc_ast_lowering` turns the AST into **HIR** (high-level IR). HIR still
looks like Rust, but the control-flow sugar has been replaced by a smaller core (`loop`, `match`, `if`, `let`, calls to
**lang items**, the special std items the compiler knows by name), paths point to definitions, and elided lifetimes have
become explicit `'_`. Chapter 4.3's claim that "elision is syntax, applied while lowering to HIR" is visible in every
HIR signature below: `fn total(xs: &'_ [u32])`.

### 3. Rust code

**Five desugarings in real HIR.** Listing `ch02-01-desugar.rs` (verified to compile) contains:

```rust,ignore
pub fn total(xs: &[u32]) -> u32 {
    let mut sum = 0;
    for x in xs {
        sum += x;
    }
    sum
}

pub fn parse_pair(a: &str, b: &str) -> Result<u32, std::num::ParseIntError> {
    let x: u32 = a.parse()?;
    Ok(x + b.len() as u32)
}

pub fn drain(stack: &mut Vec<u32>) -> u32 {
    let mut n = 0;
    while let Some(top) = stack.pop() {
        n += top;
    }
    n
}

pub async fn fetch(id: u32) -> u32 {
    id + 1
}

pub async fn caller() -> u32 {
    fetch(7).await
}
```

Its HIR (`tools/emit.ps1 -Target hir`, nightly `-Zunpretty=hir`, verbatim except whitespace):

```text
fn total(xs: &'_ [u32]) -> u32 {
    let mut sum = 0;
    {
        let _t =
            match into_iter(xs) {
                mut iter =>
                    loop {
                        match next(&mut iter) {
                            None {} => break,
                            Some {  0: x } => { sum += x; }
                        }
                    },
            };
        _t
    };
    sum
}

fn parse_pair(a: &'_ str, b: &'_ str) -> Result<u32, std::num::ParseIntError> {
    let x: u32 =
        match branch(a.parse()) {
            Break {  0: residual } => #[allow(unreachable_code)]
                return from_residual(residual),
            Continue {  0: val } => #[allow(unreachable_code)]
                val,
        };
    Ok(x + b.len() as u32)
}

fn drain(stack: &'_ mut Vec<u32>) -> u32 {
    let mut n = 0;
    loop { if let Some(top) = stack.pop() { n += top; } else { break; } }
    n
}

async fn fetch(id: u32) -> /*impl Trait*/ |mut _task_context: ResumeTy|
    { let id = id; { let _t = { id + 1 }; _t } }

async fn caller() -> /*impl Trait*/ |mut _task_context: ResumeTy|
    {
        {
            let _t =
                {
                    match into_future(fetch(7)) {
                        mut __awaitee =>
                            loop {
                                match unsafe {
                                        poll(new_unchecked(&mut __awaitee),
                                            get_context(_task_context))
                                    } {
                                    Ready {  0: result } => break result,
                                    Pending {} => { }
                                }
                                _task_context = (yield ());
                            },
                    }
                };
            _t
        }
    }
```

Read it construct by construct:

| You wrote | HIR has | So the language feature is really… |
|---|---|---|
| `for x in xs { body }` | `match into_iter(xs) { mut iter => loop { match next(&mut iter) { None => break, Some(x) => body } } }` | Sugar over `IntoIterator` + `Iterator::next` (Chapter 2.5's `for` desugaring, now seen for real) |
| `expr?` | `match branch(expr) { Break(residual) => return from_residual(residual), Continue(val) => val }` | Sugar over the `Try` trait (Chapter 8.1) |
| `while let P = e { body }` | `loop { if let P = e { body } else { break; } }` | Sugar over `loop` + `if let` |
| `async fn f() -> T { body }` | A function returning an opaque type whose body is a closure-like **coroutine** taking `_task_context: ResumeTy` | A state machine the compiler builds (Chapter 12.3) |
| `fut.await` | `into_future(fut)` pinned in place, `poll`ed with the task's context in a `loop`, `yield ()` on `Pending` | A poll loop that suspends the coroutine: the `unsafe { … new_unchecked(&mut __awaitee) }` is sound because the coroutine is never moved once polled (Chapter 12.4) |

The names that look like free functions (`into_iter`, `next`, `branch`, `from_residual`, `into_future`, `poll`) are
**lang items**: [RUSTC] paths the compiler resolves directly to the std items marked `#[lang = "..."]`, so shadowing
`next` in your own module can't hijack a `for` loop. The pretty-printer just shows them by their short names.

**Ranges, let chains, and `println!`** (listing `ch02-02-hir-format.rs`, verified to compile):

```rust,ignore
pub fn report(limit: Option<u32>, n: u32) {
    let window = 0..n;
    if let Some(l) = limit
        && l < n
    {
        println!("limit {l} below {}", window.end);
    }
}
```

HIR:

```text
fn report(limit: Option<u32>, n: u32) {
    let window = Range { start: 0, end: n };
    if let Some(l) = limit && l < n {
        {
            ::std::io::_print({
                    super let args = (&window.end, &l);
                    super let args =
                        [format_argument::new_display(args.1),
                                format_argument::new_display(args.0)];
                    unsafe {
                        format_arguments::new(b"\x06limit \xc0\x07 below \xc0\x01\n\x00",
                            &args)
                    }
                });
        };
    }
}
```

- `0..n` is just a struct literal, `Range { start: 0, end: n }` [LANG]. There is no range "type magic."
- The `if let` chain (edition 2024, Rust 1.88) survives lowering as a chain of `let` conditions: HIR has `let`
  *expressions* for exactly this purpose.
- **`format_args!` is gone.** [RUSTC] The macro was expanded into an AST node, and lowering turned it into a call to a
  constructor with a pre-encoded template (`b"\x06limit \xc0\x07 below \xc0\x01\n\x00"`, the literal pieces and
  placeholders packed as bytes, an encoding that is internal and has changed several times) plus an array of
  arguments, each paired with its `Display::fmt`. The format string was parsed *at compile time*, which is why a typo
  like `{lmit}` is a compile error and why formatting costs no run-time parsing. The captured `{l}` became the *second*
  argument even though it appears first in the string: captured identifiers are appended after the explicit arguments.
- `super let` is an internal construct [RUSTC][VERSION] (unstable for users) that gives these temporaries the lifetime
  the macro needs. You can't write it on stable; you're only seeing it because HIR shows what lowering produced.

**Hygiene, demonstrated.** Listing `ch02-03-hygiene-expr.rs` (verified):

```rust
macro_rules! scaled {
    ($e:expr) => {{
        let t = 2; // the macro's own `t`
        t * $e
    }};
}

fn main() {
    let t = 10; // the caller's `t`
    let r = scaled!(t + 1);
    println!("r = {r}");
}
```

```text
r = 22
```

`2 * (10 + 1)`. The macro's `t` is 2, and the `t` inside the caller's expression is still the caller's 10. Now look at
the expanded source (`tools/emit.ps1 -Target expand`, verbatim):

```text
fn main() {
    let t = 10; // the caller's `t`
    let r = { let t = 2; t * (t + 1) };
    { ::std::io::_print(format_args!("r = {0}\n", r)); };
}
```

Paste that back in as source and it computes `2 * (2 + 1) = 6`. **The pretty-printed expansion is not equivalent source
code**: it has lost the syntax contexts that make the two `t`s different identifiers. Two more lessons are in there:
the `$e:expr` fragment stayed one unit, so the printer had to add parentheses (a C preprocessor would have produced
`t * t + 1`), and `println!` expanded into `format_args!`, which stays a macro call in this output because it's
**built in** and is lowered only later, into the HIR shown above.

**The other direction: a macro can't reach into its caller** (listing `ch02-04-hygiene-e0425.rs`, verified):

```rust,compile_fail
macro_rules! log_request {
    () => {
        println!("request {}", request_id)
    };
}

fn main() {
    let request_id = 42;
    log_request!();
}
```

```text
error[E0425]: cannot find value `request_id` in this scope
  --> src/main.rs:6:32
   |
 6 |         println!("request {}", request_id)
   |                                ^^^^^^^^^^ not found in this scope
...
12 |     log_request!();
   |     -------------- in this macro invocation
   |
help: an identifier with the same name exists, but is not accessible due to macro hygiene
  --> src/main.rs:11:9
   |
11 |     let request_id = 42;
   |         ^^^^^^^^^^
```

The compiler even says why. Pass the value in (`log_request!(request_id)`) and it works.

**Resolution records where names come from** (listing `ch02-08-glob-ambiguity.rs`, verified). Two glob imports
both bring in an `Error`:

```text
error[E0659]: `Error` is ambiguous
  --> src/main.rs:17:13
   |
17 |     let e = Error; // which one?
   |             ^^^^^ ambiguous name
   |
   = note: ambiguous because of multiple glob imports of a name in the same module
note: `Error` could refer to the unit struct imported here
  --> src/main.rs:14:5
note: `Error` could also refer to the unit struct imported here
  --> src/main.rs:13:5
```

(Notes trimmed.) [LANG] Two globs importing the same name is *not* an error by itself, only *using* the name is. That
rule lets `use a::*; use b::*;` keep compiling when `b` later adds an item that happens to share a name with one in `a`,
as long as you don't use it.

---

## Pass 2 · Systems level — *The expansion loop and the lowering pass*

### 4. Under the hood

**Lexing and parsing.** [RUSTC] `rustc_lexer` is a small, dependency-free crate that turns characters into raw tokens
(it's shared with rust-analyzer). `rustc_parse` "cooks" them (interning identifiers, validating literals), groups them
into **token trees**, and parses an AST. A macro call is kept as an unexpanded node holding its path and its input
token tree. The parser never needs to understand a macro's input, only to find its matching delimiter.

**The expansion–resolution fixpoint.** [RUSTC] (rustc-dev-guide, *Macro expansion* and *Name resolution*.)
Expansion and resolution depend on each other: to run `serde::Serialize`'s derive you must resolve the path
`serde::Serialize` in the macro namespace, but the item that path names might itself be produced by another macro, or
imported by a `use` that a macro generated. So `rustc_expand` iterates:

1. Collect the macro invocations in the current AST fragments.
2. Ask the resolver to resolve each invocation's macro path. Unresolvable ones wait for the next round, in case a later
   expansion defines them.
3. Expand the resolvable ones: match `macro_rules!` arms against the token tree and transcribe; call proc macros; run
   built-ins (`format_args!`, `cfg!`, `include!`, derives of built-in traits).
4. Parse each result as the expected fragment kind (expression, items, pattern…), give it a fresh expansion ID and
   syntax contexts, and splice it in. New `use` items update the import graph.
5. Repeat until nothing changes. Anything still unresolved is an error ("cannot find macro").

Import resolution itself is a fixpoint (globs can import from modules whose contents come from other globs), and so is
this loop. That's why the compiler reports ambiguities such as E0659 precisely: the resolver tracks *where* each binding
came from.

**Late resolution.** [RUSTC] Once expansion is done, the resolver walks every body and resolves every path (locals,
items, associated items, generic parameters, lifetimes) into a `Res` (resolution): a `DefId`, a local binding, a
primitive type, and so on. E0425 and E0433 come from here. [LANG] Names live in separate **namespaces** (types, values,
macros), which is why `struct Error;` (type and value) can conflict with `fn Error()` but not with `macro_rules! Error`.

**Lowering to HIR.** [RUSTC] `rustc_ast_lowering` walks the resolved AST and builds HIR:

- **Desugaring**, as the table in §3 shows, into lang-item calls. Each desugared node remembers that it came from a
  desugaring, so diagnostics can still say "in this `for` loop" or "the `?` operator".
- **Lifetimes.** Elided lifetimes become explicit (`'_` in the HIR output), following Chapter 4.3's elision rules.
- **Owner-based layout.** HIR is stored per **owner** (each item, impl item, trait item), each with its own local
  numbering of nodes. That's for incremental compilation (Chapter 18.1): editing one function changes only that
  owner's HIR, so only queries that read that owner become red.

> **What actually happens?** The `.await` lowering is the whole of async Rust in six lines. `into_future` lets any
> `IntoFuture` type be awaited. The future is stored in a local (`__awaitee`) *inside the coroutine's state*, then
> pinned in place with `new_unchecked`. That's sound only because the coroutine itself is pinned before it's polled,
> which is the entire reason `Pin` exists (Chapter 12.4). `poll` is called with the context taken from `_task_context`,
> and on `Pending` the coroutine executes `yield ()`, returning control to whoever polled it. When it's resumed,
> `_task_context` is overwritten with the new context. Part XII built executors on top of exactly this.

### 5. Memory

- **Expansion multiplies code before any checking happens.** [RUSTC] A `#[derive(Serialize, Deserialize)]` on a
  20-field struct produces hundreds of lines of AST that must be resolved, lowered, type-checked, and (for generic
  impls) monomorphized. A `vec![...]` literal with 10,000 elements is 10,000 expressions. The expanded size, not the
  source size, is what compile time scales with. `cargo expand` (third-party, not verified here) or the Playground's
  "Expand macros" shows it.
- **Recursion is bounded.** [LANG] Macro expansion depth is limited by `#![recursion_limit]` (default 128), the same
  knob that bounds trait solving (18.3). A recursive `macro_rules!` "counter" that hits it is a design smell, not a
  reason to raise the limit.
- **HIR, like types, lives in arenas** for the whole session [RUSTC]. Macro-heavy crates cost memory as well as time.

### 6. CPU / OS

**A proc macro is a native plugin running inside the compiler.** [RUSTC] A crate declared `proc-macro = true` is
compiled into a dynamic library *for the host* (the machine running the compiler, not your target), and rustc loads it
with the OS dynamic loader (`dlopen` on Linux) and calls its entry points with token streams. Consequences:

- **It runs arbitrary code at build time, with the builder's privileges.** It can read files, open sockets, and read
  environment variables (CI secrets included). Chapter 2.2 made the same point about `build.rs`. Treat both as code you
  execute on your build machines.
- **It sits on the critical path.** Pipelining (Chapter 2.1) lets a downstream crate start as soon as a dependency's
  *metadata* exists, but a proc macro must be fully compiled and linked before any crate using it can expand. That's
  why `syn`, `quote`, and derive crates show up early and long in `cargo build --timings` charts.
- **When cross-compiling, proc macros and build scripts are built for the host**, so a dependency they share with your
  target code (say, `serde`) is compiled twice, once per platform.
- **A panic in a proc macro is a compile error**, reported at the invocation, not a compiler crash [RUSTC]. The compiler
  catches it at the plugin boundary.

---

## Pass 3 · Architect level — *Choosing a code-generation mechanism*

### 7. Trade-offs

| Mechanism | Runs | Sees | Hygiene | Build cost | Error quality | Tooling (IDE) | Supply-chain surface |
|---|---|---|---|---|---|---|---|
| Plain functions + generics | Never (it's just code) | Types | N/A | Monomorphization (Part VII) | Best | Full | None |
| `macro_rules!` | During expansion | Tokens | Mixed-site | Low | Fair (can be poor for deep macros) | Good | None |
| Proc macro (derive/attribute/function-like) | During expansion, as native code | Tokens only, **no types** | Call-site by default | High: `syn`/`quote` compile + expansion size | Depends on the author | Good (rust-analyzer runs them) | **Runs arbitrary code at build time** |
| `build.rs` code generation | Before compiling the crate | Anything on disk | N/A (generates source) | Runs on every relevant change | Errors point into generated files | Weaker | Same as proc macros |
| Checked-in generated code | Never at build time | N/A | N/A | Lowest | Best | Full | Review the generator once |

Rules of thumb: prefer functions and generics. Use `macro_rules!` for syntax a function can't express (variadic
arguments, repetition over fields you list). Use proc macros for derives whose value is large and widely shared (serde,
thiserror), from a short allowlist. Use `build.rs` or checked-in generated code for schemas (protobuf, SQL), and prefer
checking it in when builds must be hermetic.

> **Why not reflection, like Java's?** Rust has no run-time reflection over fields and methods, by design: it would need
> run-time type metadata for every type (a cost everyone pays) and would bypass privacy and the borrow checker. Derive
> macros move the "walk the fields" work to compile time, where it's type-checked and costs nothing at run time. The
> price is build time and the fact that proc macros see only tokens, never types: a derive can't ask "does this field's
> type implement `Serialize`?" It generates code that *requires* it, and the type checker answers later (18.3).

### 8. Java comparison

| Java | Rust |
|---|---|
| Annotation processors (JSR 269) run inside javac after parsing, see a **typed element model**, and may only generate **new** source files | Proc macros see **tokens**, before any types exist, and replace or augment the annotated item in place |
| Lombok modifies javac's internal AST through unsupported APIs | Derive and attribute macros are the supported way to generate impls and rewrite items |
| javac's `Lower` phase desugars enhanced `for`, try-with-resources, string `switch`, inner classes, and enum switches | `rustc_ast_lowering` desugars `for`, `?`, `while let`, `async`/`.await`, ranges, `format_args!` (verified above) |
| Lambdas become `invokedynamic` call sites bootstrapped by `LambdaMetafactory` **at run time** | Closures become anonymous structs and trait impls **at compile time** (Chapter 10.1) |
| Jackson and Spring discover fields and annotations by **reflection at run time** | serde's derive writes the field-walking code at compile time (the expansion in §9) |

> **Analogy limit.** "A derive macro is an annotation processor" holds for *what* they're used for and fails on *what
> they can see*. An annotation processor can ask whether a field's type implements an interface. A proc macro can't:
> type information doesn't exist yet during expansion. That ordering is why Rust derives work by emitting trait bounds
> and letting the type checker prove them, and why their error messages point at the generated code.

### 9. Production scenario

**Meridian's macro policy.** After Chapter 8.2's error-type work, `thiserror` is used everywhere, and `meridian-types`
derives serde traits on every public type (Chapter 5.3). The platform team's policy, driven by §6:

1. **Proc-macro allowlist.** New proc-macro and `build.rs` dependencies need platform review: serde, thiserror, tracing,
   clap, and a few others are pre-approved. Reviewers read what the macro does at build time, not just what it
   generates.
2. **Read your expansions.** For any derive on a hot or security-sensitive type, the author pastes the relevant part of
   the expansion in the PR. Here is what `#[derive(Serialize)]` generates for a two-field payment struct (listing
   `ch02-07-serde-derive.rs`, `-Target expand`, attributes trimmed):

   ```text
   const _: () =
       {
           extern crate serde as _serde;
           ;
           #[automatically_derived]
           impl _serde::Serialize for Payment {
               fn serialize<__S>(&self, __serializer: __S)
                   -> _serde::__private229::Result<__S::Ok, __S::Error> where
                   __S: _serde::Serializer {
                   let mut __serde_state =
                       _serde::Serializer::serialize_struct(__serializer,
                               "Payment", false as usize + 1 + 1)?;
                   _serde::ser::SerializeStruct::serialize_field(&mut __serde_state,
                           "id", &self.id)?;
                   _serde::ser::SerializeStruct::serialize_field(&mut __serde_state,
                           "amount_cents", &self.amount_cents)?;
                   _serde::ser::SerializeStruct::end(__serde_state)
               }
           }
       };
   ```

   Two things reviewers learn to look for. First, the **anonymous `const _: () = { ... }` block**: because proc-macro
   output is resolved at the call site (unhygienic), serde wraps its impl in an unnamed scope and imports itself under a
   private alias (`extern crate serde as _serde`), so its helper names can't collide with yours and yours can't hijack
   its. Second, the serialized field names are string literals **generated from your field names**: renaming a field is
   a wire-format change unless it has `#[serde(rename)]`. That's the same "boundary" lesson as Chapter 5.3's
   `transparent`/`try_from`.
3. **Build-time budget.** CI's `--timings` report tracks the proc-macro crates on the critical path. When the gateway
   workspace's timings showed `syn`-based crates compiling before anything else could start, the team consolidated on
   one major version of `syn` across the workspace (`cargo tree -d` found three).

### 10. Failure scenario

**The audit macro that printed card numbers.** Meridian's payments code logs an audit line for every money movement.
To keep the format uniform, someone wrote a `macro_rules!` macro that masks the card number (listing
`ch02-05-audit-macro-bug.rs`, verified):

```rust
mod audit {
    /// Keep only the last four digits.
    pub fn mask(pan: &str) -> String {
        let last4 = &pan[pan.len() - 4..];
        format!("****{last4}")
    }
}

macro_rules! audit {
    ($event:expr, $pan:expr) => {
        println!("audit: {} card={}", $event, mask($pan)) // BUG: `mask` resolved at the call site
    };
}

mod checkout {
    use crate::audit::mask; // checkout works only because it happens to import the right `mask`
    pub fn pay(pan: &str) {
        audit!("charge", pan);
    }
}

mod refunds {
    /// A UI helper: formats the PAN for the agent's screen (which is access-controlled).
    fn mask(pan: &str) -> String {
        pan.to_string()
    }
    pub fn refund(pan: &str) {
        audit!("refund", pan);
    }
}

fn main() {
    checkout::pay("4111111111111111");
    refunds::refund("4111111111111111");
}
```

```text
audit: charge card=****1111
audit: refund card=4111111111111111
```

The macro compiled everywhere and worked in its first caller, because the author's module imported `mask`. The refunds
module had its own helper named `mask` for an access-controlled UI screen, which returns the number unmasked. Per the
hygiene table in §2, **function names in a `macro_rules!` body are resolved at the call site**, so in `refunds`, the
macro called the local helper. Six days of refund audit lines went to the log pipeline with full card numbers before a
data-loss-prevention scan flagged them, and the logs had to be purged and the exposure assessed.

The fix is one token (listing `ch02-06-audit-macro-fixed.rs`, verified):

```rust,ignore
macro_rules! audit {
    ($event:expr, $pan:expr) => {
        println!("audit: {} card={}", $event, $crate::audit::mask($pan))
    };
}
```

```text
audit: charge card=****1111
audit: refund card=****1111
```

The review rules that followed: **every path in a macro body is either a macro parameter or starts with `$crate::`**
(or `::std::`/`::core::` for std), and there's a test that invokes each exported macro from a module that deliberately
defines colliding names. (Clippy's `crate_in_macro_def` lint catches a related mistake, writing `crate::` instead of
`$crate::`, but not a bare `mask(..)`; the review rule covers both.)

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVIII).*

1. Walk through what happens between the parser and type checking. Why are expansion and name resolution interleaved
   instead of sequential?
2. What does "mixed-site hygiene" mean for `macro_rules!`? Give an example of a name a macro body can't see and one it
   resolves in the caller's scope.
3. Why is the output of `-Zunpretty=expanded` not always equivalent source code? Give the example from this chapter.
4. Desugar `for`, `?`, and `.await` from memory. Which traits does each depend on, and why does that make them
   extensible?
5. What are lang items, and why does the `for` desugaring call them instead of ordinary paths like `Iterator::next`?
6. Why is a format-string typo a compile-time error in Rust but a run-time exception in Java's `String.format`?
7. Why do proc-macro crates sit on the critical path of a build, even with pipelining?
8. What can an annotation processor see that a derive macro can't, and how do derive macros work around it?

### 12. Exercises

- **Beginner.** Predict the HIR of `if let Some(x) = opt { a } else { b }` and of `let [first, .., last] = arr else {
  return };`. Check with the Playground's "Show HIR".
- **Intermediate.** Write a `macro_rules!` macro `retry!(n, expr)` that evaluates `expr` up to `n` times until it returns
  `Ok`. Make it hygienic: it must not break if the caller has variables named `attempt` or `result`, or a function
  named `sleep`. Test it by defining all three at the call site.
- **Advanced.** Take listing `ch02-01-desugar.rs`, write a type that implements `IntoFuture` (not `Future`), and
  `.await` it. Find the `into_future` call in the HIR and explain why `IntoFuture` exists.
- **Systems.** Count the lines of `-Zunpretty=expanded` output for a struct with 20 fields and
  `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`. Estimate the ratio of generated to written code. What
  does that imply for the compile time of `meridian-types`?
- **Architecture.** Your team wants a proc macro that generates HTTP client code from annotated trait definitions.
  List what it would run at build time, what it could and couldn't check, and the alternatives (a `build.rs` generator
  from an OpenAPI file, checked-in generated code, generic runtime code). Recommend one.

### 13. Debugging exercise

A metrics macro works in the service crate but fails when moved to a shared crate:

```rust,ignore
#[macro_export]
macro_rules! timed {
    ($name:expr, $body:block) => {{
        let start = Instant::now();
        let out = $body;
        record($name, start.elapsed());
        out
    }};
}
```

Used from another crate, it reports `cannot find type Instant in this scope` and `cannot find function record in this
scope`.

1. Explain both errors with the hygiene table. Why did it work inside the defining crate?
2. Fix the macro so it works from any crate, whatever the caller has imported.
3. The first fix a colleague proposed was "tell callers to `use std::time::Instant; use metrics::record;`". What's wrong
   with that, beyond inconvenience? (Think about §10.)

### 14. Design exercise

**Compile-time SQL checking for payments-core.** A team proposes a proc macro, `sql!("SELECT ...")`, that connects to a
development database *at build time* to check queries against the schema, like SQLx's offline/online modes (Chapter
22.4). Design the build: what runs where, how CI builds without a database, what happens when the schema changes, what
the macro's build-time behavior means for security review, and how the result shows up in `--timings`. Compare against
(a) a `build.rs` that checks queries against a checked-in schema file and (b) integration tests. Decide, and name the
failure mode you're accepting.
