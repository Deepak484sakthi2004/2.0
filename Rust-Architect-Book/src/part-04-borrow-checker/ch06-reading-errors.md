# Chapter 4.6 — Reading Borrow-Checker Errors as Ownership Proofs

> **Where this sits:** Part IV · The Borrow Checker · chapter 6 of 6
> **Prerequisites:** Chapters 4.1–4.5.
> **After this chapter you can:** translate any borrow-checker error into a sentence about ownership, using the A–B–C
> method; map every common error code to the rule it enforces; choose among five fix strategies and justify the choice;
> and explain, with a Miri-verified example, why silencing the checker with `unsafe` is a bug, not a fix.

---

## Pass 1 · User level — *Errors are proofs*

### 1. Problem

Chapter 1.1 made a promise: learn to read every borrow-checker error as an **ownership proof**, a short argument that
your program, as written, *could* do something unsafe. Engineers who read errors as obstacles reach for the first thing
that silences them: `.clone()`, `Rc<RefCell<_>>`, `unsafe`. Engineers who read them as proofs reach for the fix that
removes the *reason*. This chapter is the method, the vocabulary, and the practice.

### 2. Mental model

**The A–B–C triangle.** Almost every borrow error cites three program points:

```text
   A  a LOAN is created (or a value is MOVED)            "immutable borrow occurs here" / "value moved here"
   B  a CONFLICTING ACTION happens                        "mutable borrow occurs here" / "assigned to here" / "dropped here"
   C  the loan is USED LATER, which is why it's live at B  "immutable borrow later used here" / "borrow later used here"

   The proof:  "Between A and C the loan must stay valid; B would invalidate it; therefore, as written, C could
                observe something B broke."
```

Read every error by finding A, B, and C, then say the sentence out loud. The fix is always one of: **move C before B,
move B before A (or after C), make A and B touch different places, or change who owns what.**

**The translation table.** Error codes, what they mean in ownership terms, and where in this book they appeared:

| Code | Compiler says | Ownership translation | Seen in |
|---|---|---|---|
| E0382 | use of moved value | The value's owner moved; this name no longer owns anything | 1.3, 3.1, 3.2, 4.2 |
| E0499 | cannot borrow as mutable more than once | Two exclusive loans on overlapping places | 1.3, 4.1 |
| E0502 | cannot borrow as mutable because also borrowed as immutable | A shared loan is live across a mutation | 1.1, 3.3, 4.1, 4.2 |
| E0503 | cannot use because it was mutably borrowed | Reading a place while an exclusive loan on it is live | 4.1 table |
| E0505 | cannot move out because it is borrowed | Moving would relocate or free what a loan points to | 3.3 |
| E0506 | cannot assign because it is borrowed | Writing would change what a shared loan promised was frozen | 3.3, review |
| E0507 | cannot move out of index / borrowed content | Moving out would leave a hole another owner will drop | 3.2 |
| E0515 | cannot return reference to local variable | The referent dies when the function returns | 1.2 |
| E0597 | does not live long enough | The owner dies (B) while a loan on it is still used (C) | 1.2, 4.3, 4.4, 4.5 |
| E0716 | temporary value dropped while borrowed | Same as E0597, where the owner is an unnamed temporary | 4.6 (§3) |
| E0373 | closure may outlive the current function | A borrowed capture can't satisfy a `'static` requirement | 4.3 |
| E0521 | borrowed data escapes outside of closure | A lent (higher-ranked) reference would be retained | 4.5 |
| E0384 | cannot assign twice to immutable variable | The binding isn't `mut` | 4.6 (§3) |
| E0596 | cannot borrow as mutable, not declared as mutable | `&mut` needs a `mut` binding (or a `&mut` path) | 2.6 |
| E0106 | missing lifetime specifier | The signature doesn't say which input an output borrows from | 1.4, 4.3 |
| (none) | lifetime may not live long enough | A relationship between two lifetimes is required but not declared | 4.5 |

**Five fix strategies**, in rough order of preference:

```text
 1. SHORTEN     end the loan before the conflict: reorder, scope, use the value earlier, copy out a small Copy value
 2. SPLIT       make the loans touch different places: disjoint fields, split_at_mut, get_disjoint_mut, retain
 3. RE-API      use or design an API that expresses the intent: entry(), retain(), mem::take/replace, returning owned data
 4. RE-OWN      change ownership: move instead of borrow, Arc for shared lifetimes, owned copies at boundaries
 5. RESTRUCTURE change the data model: indices/arenas instead of references (3.6), messages instead of shared state (XI)

 NOT a strategy:  .clone() "until it compiles", Rc<RefCell<_>> everywhere, unsafe to extend a borrow (§10)
```

`clone()` is sometimes the *right* answer (strategy 4, when an independent copy is what the design needs). It's the
wrong answer when it's used to avoid deciding who owns what.

### 3. Rust code

Two more errors for the gallery (both verified).

**E0716: borrowing from a temporary.**

```rust,compile_fail
fn main() {
    // OK: `let x = &temporary;` gets TEMPORARY LIFETIME EXTENSION: the String lives as long as `tenant`.
    let tenant: &str = &format!("tenant-{}", 42);
    // E0716: here the temporary String is the receiver of a method call. It is NOT extended,
    // so it is dropped at the end of this statement, and `trimmed` would point into freed memory.
    let trimmed: &str = String::from("  acme  ").trim();
    println!("{tenant} {trimmed}");
}
```

```text
error[E0716]: temporary value dropped while borrowed
 --> src/main.rs:7:25
  |
7 |     let trimmed: &str = String::from("  acme  ").trim();
  |                         ^^^^^^^^^^^^^^^^^^^^^^^^       - temporary value is freed at the end of this statement
  |                         |
  |                         creates a temporary value which is freed while still in use
8 |     println!("{tenant} {trimmed}");
  |                         ------- borrow later used here
  |
help: consider using a `let` binding to create a longer lived value
  |
7 ~     let binding = String::from("  acme  ");
8 ~     let trimmed: &str = binding.trim();
```

A–B–C: **A**, `.trim()` borrows the temporary `String`. **B**, the temporary is freed at the `;`. **C**, `trimmed` is used
in the `println!`. The line above it compiles because of a specific rule [LANG]: in `let x = &expr;`, the temporary is
**extended** to live as long as `x`. That rule doesn't reach through method calls. The fix gives the owner a name
(strategy 4, re-own), which is what the compiler's help suggests.

**E0384: assigning to an immutable binding.**

```text
error[E0384]: cannot assign twice to immutable variable `retries`
 --> src/main.rs:5:9
  |
3 |     let retries = 3;
  |         ------- first assignment to `retries`
4 |     if std::env::args().count() > 5 {
5 |         retries = 5;
  |         ^^^^^^^^^^^ cannot assign twice to immutable variable
  |
help: consider making this binding mutable
```

That's not a borrow error but a *binding* error (Chapter 2.3). It belongs in the gallery because the idiomatic fix is
often not `let mut` but an **expression**: `let retries = if verbose { 5 } else { 3 };` (Chapter 2.4). That's shorter,
immutable, and has no second assignment to reason about.

**A worked example: four errors, four proofs.** The Part IV review gives you a program with four different borrow
errors to triage. Here's how to approach the first one, from Chapter 4.1:

```text
error[E0499]: cannot borrow `balances[_]` as mutable more than once at a time
3 |     let a = &mut balances[from];      ← A: exclusive loan on balances[_]
4 |     let b = &mut balances[to];        ← B: a second exclusive loan on balances[_]
5 |     *a -= amount;                     ← C: the first loan is used after B
```

The proof: *"`a` and `b` are both exclusive loans on `balances[_]`; the checker can't prove `from != to`; if they were
equal, two `&mut` would alias one `i64`."* The strategy: **SPLIT**, with a run-time proof of disjointness
(`get_disjoint_mut`, which turns `from == to` into an explicit error). And notice that the proof found a real bug.
`transfer(acct, acct, ...)` *is* a case the original code didn't handle.

---

## Pass 2 · Systems level — *How rustc builds the proof*

### 4. Under the hood

**Where A, B, and C come from.** [RUSTC] Borrow checking on MIR (Chapter 4.2) finds a *conflict*: an access at B
against a loan in scope at B. To report it usefully, the diagnostics code then:

- names the loan's **creation point** (A) from the MIR location of the borrow;
- finds a **use that keeps the loan live** at B (C) by walking the liveness and outlives constraints backwards. When
  several exist, it picks a "best blame" constraint, usually the closest use, which is why messages say "later used
  *here*" about one specific point;
- **names anonymous regions** so they can be discussed: `let's call the lifetime of this reference '1`, `'2`;
- explains **why a region must be long**, citing the constraint's origin: "argument requires that `line` is borrowed for
  `'line`", "returning this value requires...", "type annotation requires that `local` is borrowed for `'static`"
  (Chapters 4.4 and 4.5).

These messages come from the same constraint graph the solver used, so they're **derivations**, not guesses. That's why
the method works: the compiler has literally printed the three premises of its proof.

**Why several errors at once?** Borrow checking runs per function, and rustc continues after a borrowck failure in one
function to check the others. The Part IV review's program reports four errors from three functions in one compile.
(Chapter 2.7 showed the opposite case: a type error in an earlier *phase* hides errors from later phases.)

### 5. Memory

**What happens if you bypass the proof.** A developer "fixes" Chapter 1.1's E0502 by laundering the reference through a
raw pointer (verified):

```rust
// "Silencing" E0502 with a raw-pointer round trip. It compiles, it runs, it may even print 1.
// It is Undefined Behavior: push() reallocates (capacity was 3), and `first` dangles.
fn main() {
    let mut v = vec![1, 2, 3];
    let first: &i32 = unsafe { &*(&v[0] as *const i32) }; // the borrow checker can't see through this
    v.push(4);
    println!("first = {first}");
}
```

Compiled and run normally, on the Playground:

```text
first = 1
```

It printed the "right" answer. Now the same program under **Miri**, Rust's interpreter for detecting undefined behavior
(run on the Playground's nightly toolchain; Part XV covers it in depth):

```text
error: Undefined Behavior: constructing invalid value of type &i32: encountered a dangling reference (use-after-free)
    --> .../library/core/src/fmt/mod.rs:2872:71
     |
2872 |             fn fmt(&self, f: &mut Formatter<'_>) -> Result { $tr::fmt(&**self, f) }
     |                                                                       ^^^^^^^ Undefined Behavior occurred here
     |
     = help: this indicates a bug in the program: it performed an invalid operation, and caused Undefined Behavior
     = note: stack backtrace:
             0: <&i32 as std::fmt::Display>::fmt
             ...
             9: main
```

The memory picture is Chapter 1.1's, exactly: `push` moved the elements to a new buffer and freed the old one, and
`first` points into the freed block. The borrow checker's E0502 was a correct proof of this bug. The `unsafe` block
didn't fix the bug. It deleted the proof. The safe version (read the `Copy` value first, then push) prints the same line
and runs clean under Miri (verified).

### 6. CPU / OS

**Why the UB program printed `1`.** [OS] [LIB] Freed memory usually stays mapped. The allocator keeps it in a free
list, so reading it doesn't fault (Chapter 1.1). The old buffer's first four bytes still happened to hold `1`, or held
it long enough. Different allocation patterns, a different allocator, a different optimization level, or a busy
production heap could print garbage or corrupt something else. **"It printed the right answer" is not evidence of
correctness when UB is involved.** That's the argument for running tests under Miri in CI for any crate that contains
`unsafe`.

---

## Pass 3 · Architect level — *Triage as an engineering practice*

### 7. Trade-offs

Choosing a fix strategy for a borrow error:

| Strategy | Typical cost | Risk | When it's right |
|---|---|---|---|
| **Shorten** | None | None | The overlap was accidental (ordering) |
| **Split** | None, or one run-time check | Low | The data really is disjoint |
| **Re-API** | Often *negative* (fewer lookups) | Low | A standard API expresses the intent (`entry`, `retain`, `take`) |
| **Re-own** | An allocation or a refcount | Low; performance only | Lifetimes are genuinely independent, or data crosses a thread or task |
| **Restructure** | Design time | Medium (a larger change) | The error reflects a real design conflict: graphs, shared mutable state |
| `clone()` to silence | Allocations, maybe hidden semantic divergence | Medium | Only when an independent copy is the actual requirement |
| `Rc<RefCell<_>>` to silence | Run-time checks, panics, cycles | High | Rarely; see Chapter 3.6 |
| `unsafe` to silence | None visible | **Undefined behavior** | Never for this purpose |

### 8. Java comparison

Many borrow errors correspond to a Java **run-time** failure mode. The mapping is a useful way to convince Java
colleagues that the errors are worth reading:

| Rust compile-time error | Java run-time counterpart |
|---|---|
| E0382 use after move | `IllegalStateException: stream has already been operated upon or closed`; use after `close()` |
| E0502 mutate while iterating | `ConcurrentModificationException` (best-effort), or a silent skip |
| E0499 two mutable aliases | A data race (with threads) or aliasing corruption (without) |
| E0597 / E0716 dangling | None in Java: the GC keeps the object alive (but see stale-view bugs, 3.3) |
| E0521 lent data escapes | Retaining a pooled buffer (Netty use-after-release, 4.5) |
| E0506 assign while borrowed | A "read-only view" observing a mutation (3.3's report bug) |

> **Analogy limit.** The mapping isn't one-to-one. Several Rust errors have **no** Java counterpart because the GC makes
> the underlying bug impossible (dangling). And several Java failures have no Rust compile-time counterpart, because they
> aren't aliasing or lifetime problems (logic errors, deadlocks, Chapter 1.3).

### 9. Production scenario

**Meridian's borrow-error triage playbook.** When the platform team started onboarding Java engineers into Rust
services, code reviews kept finding the same anti-pattern: `.clone()` and `Rc<RefCell<_>>` added until the build was
green. The team wrote a one-page playbook, which became part of their Rust onboarding:

1. **Find A, B, and C** in the error and write the one-sentence proof in the PR description.
2. **Name the strategy** (shorten, split, re-API, re-own, restructure) before writing the fix.
3. **A `clone()` added to fix a borrow error needs a comment** saying why an independent copy is correct, not just that
   it compiles.
4. **No `unsafe` to fix a borrow error.** If you believe the code is sound and the checker is too conservative (problem
   case #3), find the safe restructuring or ask for review from the owners of `unsafe` code.
5. **Crates with `unsafe` run their tests under Miri in CI.**

Within a quarter, clone counts in reviewed PRs dropped, and the proofs in PR descriptions turned out to catch real bugs.
The `transfer(acct, acct)` case in §3 was one of them.

### 10. Failure scenario

**The `unsafe` that shipped.** A service team hit problem case #3 (Chapter 4.2) in a hot cache path, decided the checker
was "wrong," and extended the borrow with a raw-pointer round trip, the §5 technique. The first version happened to be
sound, because the returned reference really was never followed by an insert. Six months later, someone added an
eviction step between the lookup and the return, to keep the cache bounded. Eviction could reallocate the map's storage.
The borrow checker would have rejected that change with E0502, and it didn't, because the `unsafe` block had switched it
off for that reference.

In production it was intermittent memory corruption: wrong cache values served to about 1 in 10⁶ requests, in bursts
correlated with eviction. It was found weeks later by running the service's tests under Miri, which reported a dangling
reference on the first eviction. The lessons: **`unsafe` doesn't just assert something about the current code. It
disables the checker for all future edits to that code.** The safe entry-API version would have taken ten minutes, and
the checker would have caught the eviction change at compile time.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part IV).*

1. Describe the A–B–C method. Apply it to an E0502 you've seen in this book.
2. Translate E0505, E0506, E0597, and E0716 into ownership sentences.
3. Why does `let x = &format!(...)` compile, while `let x = String::from(..).trim()` fails with E0716?
4. Name the five fix strategies and give an example of each from Parts III–IV.
5. When is `clone()` the right fix for a borrow error, and when is it a smell?
6. How does rustc find the "later used here" point, and why is its explanation a derivation rather than a guess?
7. Why did the UB program print the correct value? Why is that not evidence of correctness?
8. Why is using `unsafe` to silence a borrow error worse than it looks, even when the current code is sound?

### 12. Exercises

- **Beginner.** Take five errors from the translation table and write, for each, a minimal program that triggers it and
  the one-sentence proof.
- **Intermediate.** Take the review program in the Part IV review (four errors). For each, write A, B, C, the proof,
  and the strategy you'd choose, before looking at the fixed version.
- **Advanced.** Write a program where `clone()` *is* the correct fix for a borrow error (an independent copy that must
  diverge). Then write one where `clone()` compiles but introduces a *semantic* bug (two copies that should have stayed
  in sync).
- **Systems.** Run the `unsafe` listing under Miri with `-Zmiri-tree-borrows` (Miri's newer aliasing model; on the
  Playground, Miri's aliasing-model setting). Does the report change? What does each model check?
- **Architecture.** Write your team's version of the triage playbook. What would you add for async code (Part XIII)
  and for FFI boundaries (Part XVI)?

### 13. Debugging exercise

A teammate's PR "fixes" E0515 (*cannot return reference to local variable*) like this:

```rust,ignore
fn cache_key(user: &str) -> &'static str {
    let key = format!("user:{}", user.to_lowercase());
    Box::leak(key.into_boxed_str())
}
```

1. Write the original error's A–B–C (before the fix, when the function returned `&key`).
2. What does the "fix" do at run time, per call? (Chapter 4.3's measurement.)
3. Propose the correct fix. What should `cache_key` return, and why? Which strategy is it?

### 14. Design exercise

**A borrow-error budget for a codebase.** Meridian's Rust codebase has grown to 200K lines across 30 crates. Leadership
asks whether ownership is being handled well. Design metrics you could compute from the code and from CI:
`unsafe` blocks per crate, `clone()` calls in hot modules, `Rc<RefCell<_>>` and `Arc<Mutex<_>>` counts, Miri coverage,
and borrow-error fix strategies recorded in PR descriptions. Say which of these indicate real problems and which are
noise, how you'd set targets without incentivizing bad fixes (such as replacing clones with `unsafe`), and what you'd
review manually instead of measuring.
