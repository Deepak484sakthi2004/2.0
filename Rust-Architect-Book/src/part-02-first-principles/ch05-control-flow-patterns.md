# Chapter 2.5 — Control Flow and Pattern Matching

> **Where this sits:** Part II · Rust From First Principles · chapter 5 of 7
> **Prerequisites:** Chapters 2.3–2.4.
> **After this chapter you can:** use Rust's pattern language fluently (ranges, bindings, slices, guards, let-else,
> if-let chains); explain what exhaustiveness checking proves and how rustc checks it; predict whether a `match` becomes
> a lookup table, a jump table, or a comparison chain; and decide when a wildcard arm is a time bomb.

---

## Pass 1 · User level — *Branching on the shape of data*

### 1. Problem

A large class of production bugs comes down to "forgot a case": the unhandled enum value, the status code nobody
expected, the null that slipped through, the new event type an old consumer ignores. In Java, a classic `switch`
statement over an enum compiles happily with cases missing. Only switch *expressions* (Java 14) and pattern-matching
switches over sealed types (Java 21) are checked for exhaustiveness.

Rust makes **every `match` exhaustive**. The compiler requires a proof that every possible value of the scrutinee is
handled, and rejects the program otherwise. Combined with enums (Chapter 2.6), that turns "forgot a case" from a
production incident into a compile error. You pay for it by occasionally being forced to handle a case you "know" can't
happen, and by having to think about wildcards.

### 2. Mental model

```text
 match value {             ← the SCRUTINEE: a value (or a place) of some type T
     PATTERN => expr,      ← arms are tried top to bottom; the FIRST matching arm wins
     PATTERN if guard => ...
     _ => ...              ← wildcard: matches anything, binds nothing
 }

 The compiler proves two things:
   EXHAUSTIVE   every possible value of T matches some arm          (otherwise: error E0004)
   REACHABLE    every arm can match something the earlier arms didn't  (otherwise: an "unreachable pattern" warning)
```

**Refutable vs irrefutable.** A pattern that can fail to match (`Some(x)`, `0..=9`) is *refutable*. One that always
matches (`x`, `(a, b)` against a tuple) is *irrefutable*. `let` and function parameters need irrefutable patterns.
`match`, `if let`, `while let`, and `let ... else` accept refutable ones.

The pattern vocabulary:

| Pattern | Matches | Example |
|---|---|---|
| Literal | An exact value | `429` |
| Range | An inclusive range | `500..=599` |
| Or | Any of several patterns | `200 \| 204` |
| Wildcard | Anything, without binding | `_` |
| Binding | Anything, binding it to a name | `code` |
| `@` binding | A sub-pattern, binding the matched value | `x @ 1..=9` |
| Tuple / struct / enum | Destructures | `(x, 0)`, `Point { x, .. }`, `Some(v)` |
| Slice | Arrays and slices by length and shape | `[first, .., last]`, `[a, b, rest @ ..]` |
| Guard | An arm-level boolean condition | `(x, y) if x == y` |

The control-flow forms built on patterns:

| Form | Use |
|---|---|
| `match` | Full case analysis; exhaustive |
| `if let PAT = e { .. } else { .. }` | One interesting case plus a fallback |
| `let PAT = e else { diverge };` | **let-else** (Rust 1.65): bind on success, otherwise return, break, or panic |
| `if let PAT = e && cond { .. }` | **if-let chains** ([VERSION] edition 2024, Rust 1.88+) |
| `while let PAT = e { .. }` | Loop while a pattern keeps matching (e.g., popping a stack) |
| `matches!(e, PAT)` | A pattern test as a `bool` |

### 3. Rust code

The whole vocabulary in one program (verified):

```rust
fn describe_status(code: u16) -> &'static str {
    match code {
        200 | 204 => "ok",
        300..=399 => "redirect",
        429 => "rate limited",
        400..=499 => "client error",
        500..=599 => "server error",
        _ => "unknown",
    }
}

fn describe_point(p: (i32, i32)) -> String {
    match p {
        (0, 0) => "origin".to_string(),
        (x, 0) | (0, x) => format!("on an axis at {x}"),
        (x, y) if x == y => format!("on the diagonal at {x}"),
        (x @ 1..=9, y @ 1..=9) => format!("small positive ({x}, {y})"),
        (x, y) => format!("elsewhere ({x}, {y})"),
    }
}

fn summarize(latencies: &[u32]) -> String {
    match latencies {
        [] => "no samples".to_string(),
        [only] => format!("one sample: {only}"),
        [first, .., last] => format!("{} samples, first {first}, last {last}", latencies.len()),
    }
}

fn parse_kv(line: &str) -> Option<(&str, u32)> {
    // let-else: bind on success, or diverge.
    let Some((key, value)) = line.split_once('=') else {
        return None;
    };
    let Ok(value) = value.trim().parse::<u32>() else {
        return None;
    };
    Some((key.trim(), value))
}

fn main() {
    for code in [200, 302, 429, 404, 503, 700] {
        println!("{code} -> {}", describe_status(code));
    }
    for p in [(0, 0), (5, 0), (3, 3), (2, 7), (-4, 12)] {
        println!("{p:?} -> {}", describe_point(p));
    }
    let cases: [&[u32]; 3] = [&[], &[42], &[10, 20, 30]];
    for s in cases {
        println!("{}", summarize(s));
    }
    for line in ["timeout = 30", "retries=x", "no equals sign"] {
        println!("{line:?} -> {:?}", parse_kv(line));
    }

    // if-let chains (edition 2024): a pattern match and a condition in one `if`.
    let config = parse_kv("max_conns = 512");
    if let Some((key, n)) = config
        && n > 256
    {
        println!("{key} is high: {n}");
    }

    // matches!: a pattern test as a bool.
    let is_retryable = |code: u16| matches!(code, 429 | 502..=504);
    println!("503 retryable: {}, 404 retryable: {}", is_retryable(503), is_retryable(404));
}
```

```text
200 -> ok
302 -> redirect
429 -> rate limited
404 -> client error
503 -> server error
700 -> unknown
(0, 0) -> origin
(5, 0) -> on an axis at 5
(3, 3) -> on the diagonal at 3
(2, 7) -> small positive (2, 7)
(-4, 12) -> elsewhere (-4, 12)
no samples
one sample: 42
3 samples, first 10, last 30
"timeout = 30" -> Some(("timeout", 30))
"retries=x" -> None
"no equals sign" -> None
max_conns is high: 512
503 retryable: true, 404 retryable: false
```

Arm **order matters**: `429` comes before `400..=499`, so rate limiting wins. If you swapped them, rustc would warn that
the `429` arm is unreachable.

**Slice patterns make parsers declarative.** Here's Chapter 2.4's header parser rewritten, where the length check, the
magic check, and field extraction are a single `match` (verified; same output as before):

```rust,ignore
fn parse_header(buf: &[u8]) -> Result<(Header, &[u8]), String> {
    match buf {
        [0xCA, 0xFE, version, flags, l0, l1, l2, l3, body @ ..] => {
            let body_len = u32::from_be_bytes([*l0, *l1, *l2, *l3]);
            Ok((Header { version: *version, flags: *flags, body_len }, body))
        }
        [a, b, _, _, _, _, _, _, ..] => Err(format!("bad magic {a:#04x} {b:#04x}")),
        short => Err(format!("need 8 header bytes, got {}", short.len())),
    }
}
```

There's no index arithmetic and no way to read past the end: a slice pattern with eight element positions only matches
slices with at least eight elements. And the match is exhaustive. The final `short` arm catches every slice shorter than
eight bytes, and the compiler checked that nothing falls through.

Exhaustiveness in action. Leave out one value of a `u8`:

```rust,compile_fail
fn bucket(n: u8) -> &'static str {
    match n {
        0..=9 => "one digit",
        10..=99 => "two digits",
        100..=254 => "three digits",
    }
}
```

```text
error[E0004]: non-exhaustive patterns: `u8::MAX` not covered
 --> src/main.rs:3:11
  |
3 |     match n {
  |           ^ pattern `u8::MAX` not covered
  |
  = note: the matched value is of type `u8`
help: ensure that all possible cases are being handled by adding a match arm with a wildcard pattern or an explicit pattern as shown
  |
6 ~         100..=254 => "three digits",
7 ~         u8::MAX => todo!(),
```

The compiler didn't just say "not exhaustive." It named the **exact missing value**, 255, by reasoning about integer
ranges.

---

## Pass 2 · Systems level — *How rustc proves and compiles a match*

### 4. Under the hood

**Exhaustiveness checking.** [RUSTC] rustc's pattern analysis (the `rustc_pattern_analysis` crate) runs on THIR, the
fully typed tree between HIR and MIR (Part XVIII). It implements a **usefulness** algorithm descended from Luc
Maranget's *Warnings for pattern matching* (2007). A pattern is *useful* if some value matches it that no earlier arm
matches:

- An arm that isn't useful is **unreachable**, which is a warning.
- A match is **exhaustive** if a wildcard `_` appended at the end would *not* be useful. If it would be useful, the
  values it would catch are exactly the ones you missed, and rustc prints a **witness**, as with `u8::MAX` above.

For integers, the algorithm splits the type's value space into ranges (0–9, 10–99, 100–254, 255) and checks each piece.
For enums it checks each variant. For nested patterns (tuples of enums, slices of structs) it recurses. The algorithm
is exponential in the worst case, and fast for real code.

**Guards weaken the proof.** [LANG] The checker doesn't reason about arbitrary boolean conditions, so an arm with a
guard counts as matching *nothing* for exhaustiveness purposes. `(x, y) if x == y` needs a later arm that covers the
general `(x, y)` case, and `describe_point` has one.

**From match to machine code.** A `match` lowers to MIR `switchInt` terminators, then to LLVM's `switch` instruction,
and LLVM's backend chooses a strategy based on the cases. Here are three functions (listing
`ch05-03-match-codegen.rs`), compiled in release by rustc 1.98.1:

```rust,ignore
pub fn weight(class: u8) -> u32 {           // dense cases, each arm a CONSTANT
    match class { 0 => 10, 1 => 25, 2 => 40, 3 => 55, 4 => 90, 5 => 120, 6 => 200, 7 => 350, _ => 0 }
}
pub fn sparse(code: u16) -> u32 {           // three widely spaced values
    match code { 200 => 1, 404 => 2, 503 => 3, _ => 0 }
}
pub fn apply(op: Op, a: i64, b: i64) -> i64 {   // six enum variants, each arm DIFFERENT CODE
    match op { Op::Add => ..., Op::Sub => ..., Op::Mul => ..., Op::Div => ..., Op::Neg => ..., Op::Abs => ... }
}
```

**Dense constants → a lookup table.** No branch per case, just a bounds check and a load:

```text
playground::weight:
	xor	eax, eax                      ; result = 0 (the `_` arm)
	cmp	dil, 7
	ja	.LBB2_2                       ; class > 7 → return 0
	movzx	eax, dil
	lea	rcx, [rip + .Lswitch.table.playground::weight]
	mov	eax, dword ptr [rcx + 4*rax]  ; result = table[class]
.LBB2_2:
	ret
```

**Dense code → a jump table.** One indirect jump to the right arm:

```text
playground::apply:
	movzx	eax, dil                      ; the enum's discriminant (0..=5)
	lea	rcx, [rip + .LJTI0_0]
	movsxd	rax, dword ptr [rcx + 4*rax]  ; load the offset of that arm's code
	add	rax, rcx
	jmp	rax                           ; ...and jump there
.LBB0_1:                                      ; Op::Add
	add	rdx, rsi
	mov	rax, rdx
	ret
	...
.LJTI0_0:                                     ; the jump table: one entry per variant
	.long	.LBB0_1-.LJTI0_0
	.long	.LBB0_2-.LJTI0_0
	...
```

There's no bounds check before the `jmp`. The compiler knows an `Op` discriminant can only be 0–5, because every other
bit pattern is an *invalid value* (Chapter 2.3). The type's validity invariant pays off directly in the machine code.
(Look at the full listing's `Op::Div` arm too: LLVM checks whether both operands fit in 32 bits and, if they do, uses
the much faster 32-bit `div` instead of 64-bit `idiv`, plus the `i64::MIN / -1` guard that `wrapping_div` requires.)

**Sparse values → a comparison chain:**

```text
playground::sparse:
	movzx	ecx, di
	cmp	ecx, 503
	je	.LBB1_5
	cmp	ecx, 404
	je	.LBB1_4
	xor	eax, eax
	cmp	ecx, 200
	jne	.LBB1_6
	mov	eax, 1
	ret
	...
```

Note that the machine code tests 503 *first*. [LANG] "First matching arm wins" is a statement about **semantics**, not
about evaluation order in the binary. When arms don't overlap, the compiler can test them in any order.

**`for` is sugar.** [LANG] `for pat in expr { body }` is defined in terms of `IntoIterator` and `loop` (simplified from
the Reference):

```rust,ignore
let mut iter = IntoIterator::into_iter(expr);
loop {
    match iter.next() {
        Some(pat) => { body }
        None => break,
    }
}
```

So `for x in vec` calls `Vec::into_iter`, which **consumes** the vector and yields owned elements. `for x in &vec`
calls `<&Vec<T>>::into_iter`, which yields `&T` and leaves the vector intact. Part III explains why that difference is
an ownership difference, and Part X shows how the loop disappears into straight-line code.

### 5. Memory

**Binding modes: does a pattern move, copy, or borrow?** Matching destructures a *place*, and what each binding gets
depends on the scrutinee:

```text
 let opt: Option<String> = Some("hi".to_string());

 match opt        { Some(s) => ... }   // s: String   (MOVES the String out; opt is partially moved)
 match &opt       { Some(s) => ... }   // s: &String  (default binding mode: borrows through the reference)
 match opt        { Some(ref s) => ... } // s: &String (explicit `ref`: borrow, don't move)
 match opt.as_ref() { Some(s) => ... } // s: &String  (Option<&String>: the idiomatic spelling)
```

The second line is **match ergonomics** (RFC 2005, stable since 2018). When you match a reference against a non-reference
pattern, the compiler automatically dereferences and switches the bindings to borrow. It's why `match self { ... }` in
methods taking `&self` "just works" (Chapter 2.6). [VERSION] Edition 2024 tightened the rules for combining explicit
`ref`, `mut`, and `&` with these default binding modes, reserving some combinations for future changes. `cargo fix
--edition` rewrites affected code.

Matching itself doesn't copy the scrutinee. A `match` on an enum reads its **discriminant** (one byte, in the jump
table above) and then works on the payload in place. The payload is only copied or moved if a *binding* takes it by
value.

### 6. CPU / OS

- **Lookup tables** are branch-free. The cost is one cache line for the table, which is cheap when the match is hot.
- **Jump tables** cost one *indirect* branch. Modern CPUs predict indirect branches from recent history (the branch
  target buffer). That works well when the same arm keeps repeating and badly when the input is random, where a
  misprediction costs roughly 15–20 cycles on current x86 cores (order of magnitude; measure on yours).
- **Comparison chains** cost one conditional branch per test. Frequent cases tested early are cheap, and LLVM doesn't
  know which cases are frequent unless you give it profile data (PGO, Part XX).
- **Cold paths.** Mark error-handling functions `#[cold]` so LLVM moves them out of the hot path and optimizes callers
  for the common case. The `panic_const_add_overflow` call from Chapter 2.2 is exactly such a cold path.
- The source order of arms is **not** a performance knob. If a match is truly hot, measure it (Part XX), and consider
  restructuring the *data* (a table indexed by the value) rather than reordering arms.

---

## Pass 3 · Architect level — *Exhaustiveness as a design tool*

### 7. Trade-offs

**Ways to dispatch on a value:**

| | `match` | Lookup table (data) | Trait objects (Part VI) |
|---|---|---|---|
| Adding a **new case** (variant) | Every `match` must be updated, and the **compiler finds them all** | Add a row | Add a type; existing code untouched |
| Adding a **new operation** | Add one function with one `match` | Add a column | Every implementing type must change |
| Exhaustiveness | Checked | Not checked (a missing row is a run-time lookup miss) | N/A |
| Speed | Lookup or jump table, often branch-free | One load | An indirect call through a vtable |
| Best for | **Closed** sets you own (states, events, protocol messages) | Configuration-driven mappings | **Open** sets extended by others (plugins) |

That trade-off, easy to add cases *or* easy to add operations but not both, is the classic **expression problem**.
Rust's enums put you on the "closed set, add operations freely" side, with the compiler watching the cases.

**Wildcards: `_` is a decision.** A wildcard arm says "every other value, *including values that don't exist yet*,
behaves like this." For a `u16` status code that's often right, since new numbers appear and "unknown" is a legitimate
answer. For an **enum you own**, a wildcard switches off the compiler's help for future variants. The rules Meridian
adopted:

1. On enums you own, list the variants. Use `_` only when the behavior for future variants is *genuinely* identical and
   *safe*.
2. If you do need a catch-all, make it **fail closed** (reject, error, or alert), never **fail open** (ignore, treat as
   zero, succeed).
3. Clippy's restriction lint `wildcard_enum_match_arm` can forbid wildcards on enums in critical crates.

**`#[non_exhaustive]` is the other side of the same trade-off.** A library can mark a public enum `#[non_exhaustive]`,
which *forces* downstream code to include a `_` arm. The library can then add variants without a breaking change, but
downstream code loses exhaustiveness checking against those new variants. Use it for enums that will genuinely grow (an
error kind, a protocol message set). Avoid it for enums whose completeness users depend on.

### 8. Java comparison

| Java | Rust |
|---|---|
| Classic `switch` statement on an enum: **not** checked for missing constants | Every `match` is exhaustive |
| `switch` expression (Java 14+): must be exhaustive | Same idea, and more general patterns |
| Sealed interfaces (17) + record patterns (21) + pattern `switch` (21): exhaustive over a sealed hierarchy | `enum` with data + `match` |
| `case null` handling; NPE if the scrutinee is null and there's no `case null` | No null to handle; absence is `Option` and must be matched |
| No slice or range patterns; guards via `when` | Slices, ranges, `@` bindings, guards via `if` |

There's a subtle difference in how the guarantee **survives separate compilation**. Java links at run time. If a
library adds an enum constant and your switch expression was compiled against the old version, javac's generated
default branch throws at run time (`MatchException` since Java 21; `IncompatibleClassChangeError` before). Exhaustiveness
is checked at compile time and *re-checked by an exception* at run time. Rust links statically. A new variant in a
dependency means your code gets **recompiled** against it, so the exhaustiveness proof is redone. The only way a new
variant "sneaks in" is through `#[non_exhaustive]`, which made you write the `_` arm up front.

> **Analogy limit.** "A Rust `enum` + `match` is a Java sealed interface + pattern `switch`" holds for modeling closed
> sets with data. It breaks on cost and representation. Java's sealed hierarchy is a set of heap objects, with dispatch
> through type checks. A Rust enum is an inline tagged union, and dispatch is a jump table on a one-byte discriminant
> (§4). Chapter 2.6 shows the memory side.

### 9. Production scenario

**Meridian's payment state machine.** Payments move through states in response to events. Modeling the transitions as
a `match` on a `(state, event)` tuple puts the whole state machine on one screen and under the exhaustiveness checker
(verified):

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Pending,
    Authorized,
    Captured,
    Refunded,
    Failed,
}

#[derive(Debug, Clone, Copy)]
enum Event {
    Authorize,
    Capture,
    Refund,
    Fail,
}

fn next(state: State, event: Event) -> Result<State, String> {
    use Event::*;
    use State::*;
    match (state, event) {
        (Pending, Authorize) => Ok(Authorized),
        (Pending | Authorized, Fail) => Ok(Failed),
        (Authorized, Capture) => Ok(Captured),
        (Captured, Refund) => Ok(Refunded),
        (s @ (Refunded | Failed), e) => Err(format!("{s:?} is terminal; rejected {e:?}")),
        (s, e) => Err(format!("invalid transition {s:?} + {e:?}")),
    }
}

fn main() {
    let mut state = State::Pending;
    for event in [Event::Authorize, Event::Capture, Event::Refund, Event::Capture] {
        match next(state, event) {
            Ok(s) => {
                println!("{state:?} --{event:?}--> {s:?}");
                state = s;
            }
            Err(e) => println!("rejected: {e}"),
        }
    }
    println!("{:?}", next(State::Pending, Event::Refund));
    println!("{:?}", next(State::Authorized, Event::Fail));
}
```

```text
Pending --Authorize--> Authorized
Authorized --Capture--> Captured
Captured --Refund--> Refunded
rejected: Refunded is terminal; rejected Capture
Err("invalid transition Pending + Refund")
Ok(Failed)
```

Note the last arm, `(s, e) => Err(...)`. It *is* a catch-all, but it **fails closed**: any combination not explicitly
allowed is rejected. When someone adds a `PartiallyRefunded` state, nothing silently succeeds. The new state's
transitions are rejected until someone writes them, and tests catch that immediately. That's rule 2 from §7 applied to a
state machine. Part V will push further and use the *type-state pattern* to make invalid transitions fail at compile time
rather than at run time.

### 10. Failure scenario

**The chargeback that didn't count.** Meridian's ledger computes merchant balances from events. The function was
written when only charges and refunds existed:

```rust
#[derive(Debug, Clone, Copy)]
enum LedgerEvent {
    Charge(i64),
    Refund(i64),
    Chargeback(i64), // added six months after `balance_delta` was written
}

fn balance_delta(event: LedgerEvent) -> i64 {
    match event {
        LedgerEvent::Charge(cents) => cents,
        LedgerEvent::Refund(cents) => -cents,
        _ => 0, // "other events don't affect the balance": true on the day it was written
    }
}

fn main() {
    let events = [
        LedgerEvent::Charge(10_000),
        LedgerEvent::Refund(2_000),
        LedgerEvent::Charge(2_000),
        LedgerEvent::Chargeback(10_000), // the customer's bank reversed the first charge
    ];
    let balance: i64 = events.iter().map(|&e| balance_delta(e)).sum();
    println!("merchant balance: {balance} cents (should be 0)");
}
```

```text
merchant balance: 10000 cents (should be 0)
```

When `Chargeback` was added, the compiler **didn't complain**. The wildcard had already promised that every future
variant contributes zero. For months, merchants were paid out for charges their customers' banks had already reversed.

There *was* one signal. rustc 1.98 compiled this with a warning:

```text
warning: field `0` is never read
 --> src/main.rs:6:16
  |
6 |     Chargeback(i64), // added six months after `balance_delta` was written
  |     ---------- ^^^
  |     |
  |     field in this variant
```

The chargeback *amount* was never used anywhere, which is precisely the bug. It's a dead-code lint, one line among
dozens of warnings in a busy build, and nobody connected it to money. The fix is to delete the wildcard and write
`LedgerEvent::Chargeback(cents) => -cents`. The prevention is the policy from §7: no wildcards on owned enums in
financial code (enforced with `clippy::wildcard_enum_match_arm`), and **warnings as errors in CI** (`-D warnings`), so a
signal like this can't sit unread.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part II).*

1. What does it mean for a `match` to be exhaustive? Roughly how does rustc check it, and why did it report `u8::MAX`
   specifically?
2. Refutable vs irrefutable patterns: where is each allowed? What problem does `let ... else` solve?
3. Why does a guard (`if cond`) weaken exhaustiveness checking?
4. How can a `match` compile to a lookup table, a jump table, or a comparison chain? What decides which one?
5. What is match ergonomics? What's the type of `s` when you match `&Option<String>` against `Some(s)`?
6. What does `#[non_exhaustive]` force on downstream code? Why would a library author use it anyway?
7. Compare Rust's exhaustiveness guarantee with Java 21's switch over sealed types, including what happens when a
   separately compiled module adds a variant.
8. How does `for x in v` desugar? What does that tell you about `for x in vec` versus `for x in &vec`?

### 12. Exercises

- **Beginner.** Write `fn retry_policy(status: u16) -> Retry` (where `Retry` is `Never`, `After(u32)`, or `Backoff`) for
  HTTP statuses, using ranges and or-patterns. Make sure unknown codes fail closed.
- **Intermediate.** Parse a command line held in a `&[&str]` using slice patterns only: `["get", key]`,
  `["set", key, value]`, `["del", keys @ ..]` (at least one key), anything else is an error.
- **Advanced.** Implement the payment state machine twice, once as the `match` above and once as a transition table
  (`&[(State, Event, State)]` searched at run time). Compare how each handles adding a new state, and what each
  guarantees at compile time.
- **Systems.** On the Playground (ASM, release), write a `match` on a `u8` with 16 arms that each call a different
  `#[inline(never)]` function. Find the jump table. Then make the cases sparse (multiples of 17) and see what LLVM does
  instead.
- **Architecture.** Write your team's policy on wildcard arms: where they're banned, where they're required
  (`#[non_exhaustive]` enums from dependencies), and how the rule is enforced in CI.

### 13. Debugging exercise

The `bucket` function in §3 is rejected with E0004: `u8::MAX` not covered.

1. Give three fixes: an explicit `255` arm, widening the last range to `100..=u8::MAX`, and a `_` arm. Which one is
   best, and why?
2. Six months later someone changes the parameter type to `u16`. For each of your three fixes, what does the compiler
   say, and which fix is still correct without anyone noticing?
3. What does question 2 tell you about writing patterns that stay correct when types change?

### 14. Design exercise

**How should Meridian encode order lifecycles?** Orders have seven states and eleven event types, and the product team
wants to change allowed transitions "without a deploy" for some regions.

Compare three designs:

- a `match` on `(State, Event)` (compile-time exhaustiveness, but changes need a deploy);
- a data-driven transition table loaded from configuration (flexible, with no compile-time checks);
- a hybrid: an exhaustive `match` defines the *maximal* set of legal transitions, and configuration can only
  *disable* transitions within it.

For each, describe what's checked at compile time, what's checked at startup, what a bad configuration does in
production, and how audits work. Recommend one. (Part V adds a fourth option, the type-state pattern.)
