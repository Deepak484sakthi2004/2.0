# Chapter 4.5 — Higher-Ranked Trait Bounds

> **Where this sits:** Part IV · The Borrow Checker · chapter 5 of 6
> **Prerequisites:** Chapters 4.3–4.4. (Traits and closures get full treatment in Parts VI and X. Here we need only
> "a closure's type implements `Fn`/`FnMut`, with a signature.")
> **After this chapter you can:** tell apart "a lifetime the caller picks" from "every lifetime"; write callback APIs
> with `for<'a>` bounds; explain why closures that return references need help; and see how a higher-ranked bound
> guarantees at compile time that a callback can't keep the data it was lent.

---

## Pass 1 · User level — *"For some lifetime" vs "for every lifetime"*

### 1. Problem

A common API shape in systems code: *"I'll hand you each item, you look at it, and then I'll reuse or free it."* A log
reader passes each line to a callback. A parser passes each field. A connection loop passes each frame from a reused
buffer. The data the callback sees is owned by the **callee** and lives only for one call. How do you type the callback
so that (a) it accepts a reference with that short, callee-chosen lifetime, and (b) it can't smuggle the reference out
and keep it?

Chapter 4.3's tool, a lifetime parameter on the function, doesn't work. A lifetime parameter is chosen **by the
caller**, and it must outlive the whole call. The callee needs to say something stronger: *the callback must work for
**any** lifetime I choose, including ones that end inside my function.* That's a **higher-ranked trait bound** (HRTB):
`for<'a> Fn(&'a str)`.

### 2. Mental model

```text
 fn visit<'x, F: Fn(&'x str)>(f: F)          "there is ONE lifetime 'x (the caller picks it), and f accepts &'x str"
                                              → 'x must outlive the whole call → callee-local data can't satisfy it

 fn visit<F: for<'x> Fn(&'x str)>(f: F)       "for EVERY lifetime 'x, f accepts &'x str"
                                              → the callee may pass references to its own short-lived data
                                              → f can't assume anything about how long 'x lasts, so it can't store it
```

Three facts:

1. **You've been using HRTBs all along.** Lifetime elision in `Fn` bounds produces them: `F: Fn(&str)` *means*
   `F: for<'a> Fn(&'a str)`. So do trait objects: `Box<dyn Fn(&str)>` is `Box<dyn for<'a> Fn(&'a str)>`. Function
   pointer types too: Chapter 4.4's error printed `for<'a> fn(&'a _)`.
2. **You write `for<'a>` explicitly** when elision doesn't produce what you need: multiple references whose lifetimes
   must be related, a returned reference tied to an argument, or bounds on non-`Fn` traits.
3. **The callback can't retain the reference.** Storing it anywhere that outlives the call would require the arbitrary
   `'x` to outlive that place, which "for every `'x`" can't guarantee. The compiler reports it as E0521.

### 3. Rust code

**The right bound** (verified):

```rust
/// Calls `f` with a line that THIS function creates and owns. `f` must work for any lifetime,
/// including one that ends inside this function: a higher-ranked bound.
fn for_each_line<F>(raw: &[u8], mut f: F) -> usize
where
    F: for<'line> FnMut(&'line str),
{
    let mut count = 0;
    for chunk in raw.split(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(chunk).into_owned(); // owned by this iteration only
        f(&line);
        count += 1;
    } // `line` dropped here, every iteration
    count
}

fn main() {
    let raw = b"GET /health 200\nPOST /orders 201\nGET /orders 500";
    let mut errors = 0;
    let lines = for_each_line(raw, |line| {
        if line.ends_with("500") {
            errors += 1;
        }
    });
    println!("lines={lines} errors={errors}");
}
```

```text
lines=3 errors=1
```

(The `for<'line>` is spelled out for clarity. `F: FnMut(&str)` means the same thing.)

**The wrong bound**, with a caller-chosen lifetime (verified):

```rust,compile_fail
fn for_each_line<'line, F>(raw: &[u8], mut f: F) -> usize
where
    F: FnMut(&'line str),
{
    let mut count = 0;
    for chunk in raw.split(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(chunk).into_owned();
        f(&line);
        count += 1;
    }
    count
}
```

```text
error[E0597]: `line` does not live long enough
   |
 4 | fn for_each_line<'line, F>(raw: &[u8], mut f: F) -> usize
   |                  ----- lifetime `'line` defined here
...
10 |         let line = String::from_utf8_lossy(chunk).into_owned();
   |             ---- binding `line` declared here
11 |         f(&line);
   |         --^^^^^-
   |         | |
   |         | borrowed value does not live long enough
   |         argument requires that `line` is borrowed for `'line`
12 |         count += 1;
13 |     }
   |     - `line` dropped here while still borrowed
```

`'line` is a parameter of `for_each_line`, so the *caller* chooses it, and it must outlive the whole call (a universal
region, Chapter 4.3). A `String` created and dropped inside one loop iteration can't possibly live that long. Moving the
lifetime *into* the bound with `for<'line>` flips who chooses: now the callee picks it, per call.

**A callback that tries to keep what it was lent** (verified):

```rust,compile_fail
fn main() {
    let raw = b"GET /health 200\nGET /orders 500";
    let mut failures: Vec<&str> = Vec::new();
    for_each_line(raw, |line| {
        if line.ends_with("500") {
            failures.push(line); // try to KEEP a line after the callback returns
        }
    });
    println!("{failures:?}");
}
```

```text
error[E0521]: borrowed data escapes outside of closure
   |
14 |     let mut failures: Vec<&str> = Vec::new();
   |         ------------ `failures` declared here, outside of the closure body
15 |     for_each_line(raw, |line| {
   |                         ---- `line` is a reference that is only valid in the closure body
16 |         if line.ends_with("500") {
17 |             failures.push(line); // try to KEEP a line after the callback returns
   |             ^^^^^^^^^^^^^^^^^^^ `line` escapes the closure body here
   |
   = note: requirement occurs because of a mutable reference to `Vec<&str>`
   = note: mutable references are invariant over their type parameter
```

Read the notes. The closure captures `&mut failures`, whose type involves `Vec<&'? str>` for one fixed lifetime.
**Invariance** (Chapter 4.4) prevents that lifetime from adapting, and the higher-ranked `'line` can't be named outside
the closure. So the only lifetime that could make `push` valid doesn't exist. The fix is to keep an **owned** copy
(`failures.push(line.to_owned())`), which is exactly the right cost: pay for a copy of the few lines you keep, nothing for
the rest.

**Closures that return references.** Closures don't get function-style lifetime elision (verified):

```rust,compile_fail
fn main() {
    // A closure that returns (part of) its argument. Closures don't get fn-style lifetime elision,
    // so the compiler infers two unrelated lifetimes for the parameter and the return value.
    let first_word = |s: &str| -> &str { s.split(' ').next().unwrap_or("") };
    println!("{}", first_word("GET /health"));
}
```

```text
error: lifetime may not live long enough
 --> src/main.rs:5:42
  |
5 |     let first_word = |s: &str| -> &str { s.split(' ').next().unwrap_or("") };
  |                          -        -      ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ returning this value requires that `'1` must outlive `'2`
  |                          |        |
  |                          |        let's call the lifetime of this reference `'2`
  |                          let's call the lifetime of this reference `'1`
```

Two fixes (verified: prints `GET` and `/health`):

```rust
/// Fix 1: a function item gets normal elision (one input lifetime -> the output).
fn first_word(s: &str) -> &str {
    s.split(' ').next().unwrap_or("")
}

/// Fix 2: let an expected higher-ranked signature drive the closure's inference.
fn apply<F>(f: F, input: &str) -> &str
where
    F: for<'a> Fn(&'a str) -> &'a str,
{
    f(input)
}

fn main() {
    println!("{}", first_word("GET /health"));
    println!("{}", apply(|s| s.split(' ').nth(1).unwrap_or(""), "GET /health"));
}
```

When a closure is passed directly to a function whose bound says `for<'a> Fn(&'a str) -> &'a str`, the compiler infers
the closure's signature *from that bound*, relationship included. A closure defined standalone has no such guidance.
[VERSION] Stable Rust has no syntax for writing `for<'a>` directly on a closure (it exists as an unstable feature), so the
usual answers are a function item or a context that supplies the signature.

---

## Pass 2 · Systems level — *How the compiler checks "for every lifetime"*

### 4. Under the hood

**Placeholders.** [RUSTC] To check that a closure satisfies `for<'a> Fn(&'a str)`, the trait solver replaces `'a` with a
fresh **placeholder** region: an opaque lifetime about which nothing is known except what the bound itself states. If
the closure's body type-checks against that placeholder, it works for every `'a`. If it needs some property of `'a`
(outliving `failures`'s element lifetime, say), the check fails. That's E0521, or "lifetime may not live long enough."

**Closure signature inference.** A closure's parameter and return types are inferred. When the expected type is known
(the closure is passed straight to a function with an `Fn` bound), the compiler uses that bound, including its
higher-ranked lifetimes, as the closure's signature. Without an expected type, it infers each elided reference lifetime
separately, which is why the standalone `first_word` closure got two unrelated regions `'1` and `'2`.

**Where HRTBs show up in real code:**

- **Every callback that receives references**: `Fn(&T)`, `FnMut(&str)`, `Box<dyn Fn(&Request) -> Response>`.
- **`serde`**: `for<'de> Deserialize<'de>` means "deserializable from input of any lifetime," and serde names that bound
  `DeserializeOwned` ("owns everything it deserializes; borrows nothing from the input"). Part XXII uses this to decide
  when zero-copy deserialization is possible.
- **Iterator and parser combinators** whose closures take borrowed items.

[VERSION] On stable Rust, `for<...>` binders quantify over **lifetimes only**. Binders over types (`for<T>`) are an
unstable experiment.

### 5. Memory

Nothing new at run time. The closure's environment is a struct of its captures (Chapter 2.4), and calls through `F` are
direct and inlinable (monomorphized, Part VII), or indirect through a vtable for `dyn Fn` (Part VI). What HRTB changes
is *which programs type-check*, and the one it rules out, a callback keeping a pointer into a buffer the caller is about
to reuse, is a use-after-free.

### 6. CPU / OS

HRTB-based visitor APIs are how zero-copy streaming stays zero-copy. The producer reuses one buffer (Project Levels 1
and 2 both do), each consumer borrows it briefly, and the compiler guarantees no consumer holds on past the reuse. The
machine-level payoff is the one Part III measured: no allocation per item, and a working set that stays in cache.

---

## Pass 3 · Architect level — *Designing lending APIs*

### 7. Trade-offs

Three API shapes for "give each item to the consumer":

| Shape | Signature | Allocation per item | Can the consumer keep items? | Control flow |
|---|---|---|---|---|
| **Visitor with HRTB** | `fn for_each_line(f: impl FnMut(&str))` | None | No (compile error); must copy | The producer drives |
| **Owned items** | `fn lines() -> impl Iterator<Item = String>` | One per item | Yes | The consumer drives (`break`, `take`, adapters) |
| **Borrowed items from a stable source** | `fn lines<'a>(buf: &'a str) -> impl Iterator<Item = &'a str>` | None | Yes, while `buf` lives | The consumer drives |

The standard `Iterator` trait can't express a fourth, common wish: an iterator that yields references into its **own
reused buffer** (a "lending iterator"), because `Iterator::Item` can't borrow from the `&mut self` of each `next()` call.
Generic associated types (stable since Rust 1.65) make such traits expressible, but std's `Iterator` isn't one of them.
In practice the visitor with an HRTB bound is the zero-allocation choice for "reuse the buffer," and borrowed-from-source
iterators are the choice when the whole input is in memory.

### 8. Java comparison

Java lambdas receive references freely and can keep them freely. The GC keeps the objects alive, so retention is
*memory-safe*, but it can still be **wrong** when the object is logically reused. The textbook example is again
**Netty**: a `ChannelInboundHandler` receives a pooled `ByteBuf`, and if it keeps a reference after the buffer is released
back to the pool, later reads see another message's bytes. Netty's rules ("release it or pass it on, never both, never
keep it without `retain()`") are enforced by convention, run-time reference counts, and a sampling leak detector.

A Rust visitor with a higher-ranked bound turns "you may not keep this" into a type error (E0521). The consumer that
genuinely needs to keep data has to copy it (`to_owned()`) or the API has to hand out refcounted owners (`Bytes`), and
either choice is visible in the code.

> **Analogy limit.** "HRTB is like Java's `Consumer<T>`" holds for the shape: a callback invoked per item. It fails on
> the key property: `Consumer<T>` can retain `T` forever, and the GC makes that safe but possibly *stale*. `for<'a>
> FnMut(&'a T)` can't retain at all. It's closer to a **promise enforced by the compiler** that Netty's documentation
> asks handlers to keep.

### 9. Production scenario

**Meridian's log visitor library.** Three tools (`logstat`, `redact`, and a new anomaly detector) all read huge logs
line by line. The shared library exposes:

```rust,ignore
pub fn for_each_line<R: BufRead, F>(input: R, f: F) -> io::Result<u64>
where
    F: for<'line> FnMut(LineNo, &'line str) -> ControlFlow<()>,
```

One reusable buffer per reader, no allocation per line, and an early-exit signal through `ControlFlow`. The anomaly
detector's first version tried to keep "suspicious" lines in a `Vec<&str>` for a report, and hit E0521 (§3). The fix,
`suspicious.push(line.to_owned())`, copies only the few lines kept, typically dozens out of millions. The compiler
turned a buffer-reuse corruption bug (what Netty would call use-after-release) into a one-line decision about ownership.

### 10. Failure scenario

**The Java handler that kept a buffer.** A Meridian Java service decoded messages in a Netty handler and, for a
"recent messages" debug endpoint, stored the incoming `ByteBuf` in a ring buffer, without calling `retain()`. Under load,
the pooled buffers were recycled for new messages, and the debug endpoint began showing *other customers'* messages,
labelled with the wrong IDs. That's a data exposure found by a customer, not by a test. The Netty leak detector, which
samples allocations, didn't flag it, because this was use-after-release rather than a leak.

The same design in Rust is the §3 E0521 listing: the ring buffer is `failures`, and the compiler refuses. The designs
that compile are exactly the correct ones: copy the bytes (`to_owned()`), or make the buffer shared and refcounted
(`Bytes`), so that storing it keeps it alive instead of letting the pool reuse it.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part IV).*

1. What's the difference between `fn f<'a, F: Fn(&'a str)>(..)` and `fn f<F: for<'a> Fn(&'a str)>(..)`? Who chooses the
   lifetime in each?
2. Why can't a callback with a higher-ranked bound keep the reference it receives? What error do you get, and what do its
   notes about invariance mean?
3. Where do HRTBs appear implicitly in everyday Rust? Give three examples.
4. Why does a standalone closure `|s: &str| -> &str { .. }` fail, and what are the two standard fixes?
5. What is `for<'de> Deserialize<'de>`, and how does it relate to `DeserializeOwned`?
6. How does the compiler check a higher-ranked bound (placeholder regions)?
7. Why can't std's `Iterator` express a lending iterator? What do people use instead?
8. Compare a Rust HRTB visitor with a Netty handler in terms of when buffer-retention bugs are found.

### 12. Exercises

- **Beginner.** Write `fn for_each_field(line: &str, f: impl FnMut(&str))` that splits on commas and calls `f` for each
  field. Use it to count empty fields.
- **Intermediate.** Change `for_each_line` so the callback returns `ControlFlow<()>` to stop early. Use it to find the
  first line containing "ERROR" without reading the rest.
- **Advanced.** Write a function taking `F: for<'a> Fn(&'a str) -> &'a str` (a "projection") and apply it to every line,
  collecting the results *counts* per distinct projection into a `HashMap<String, usize>`. Where must you allocate, and
  why only there?
- **Systems.** Measure allocations with the counting allocator (Part III) for (a) `for_each_line` with a callback that
  only counts, and (b) an iterator yielding `String` per line, over 100,000 lines.
- **Architecture.** Design a Rust callback API for a message consumer that receives frames from a reused network buffer.
  Include how a consumer that needs to keep a message does so, and compare with the Netty contract.

### 13. Debugging exercise

The E0521 listing in §3.

1. Explain the error without using the word "lifetime": what would happen at run time if the compiler allowed it?
2. Fix it by copying. Then fix it differently by changing the *producer* to keep all lines alive (read the whole input
   into one `String`, then yield `&str` slices of it). What does each fix cost in memory and allocations?
3. The notes mention invariance. If `failures` were a `Vec<String>`, would invariance matter at all? Why not?

### 14. Design exercise

**Meridian's middleware chain.** The gateway runs each request through a chain of middleware: auth, rate limiting,
logging, header rewriting. Each middleware needs read access to the request (headers and path borrowed from the receive
buffer), some need to add annotations for later middleware, and the logging middleware wants to keep some request data
for an asynchronous log shipper.

Design the middleware trait (Part VI covers traits in depth; sketch the method signature). Decide what each middleware
receives (`&Request<'buf>`, `&mut Context`), how annotations are stored (owned? interned? indices?), and exactly where
data crosses from borrowed to owned for the log shipper. Where do higher-ranked lifetimes appear in your design, and what
do they prevent?
