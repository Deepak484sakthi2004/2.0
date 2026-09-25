# Chapter 4.3 — Lifetime Annotations and Elision

> **Where this sits:** Part IV · The Borrow Checker · chapter 3 of 6
> **Prerequisites:** Chapters 4.1–4.2.
> **After this chapter you can:** read and write lifetime annotations as *relationships* between loans; apply the
> three elision rules and spot when they tie an output to the wrong input; design structs that borrow (and know when
> not to); and use `'static` correctly in both its meanings.

---

## Pass 1 · User level — *Naming relationships between loans*

### 1. Problem

Inside a function body, the checker infers every region (Chapter 4.2). **At a function boundary it can't**, because the
checker reads only signatures (Chapter 1.1). If a function returns a reference, the signature must say *which input
it borrows from*, so that callers can be checked without reading the body. The same goes for a struct that holds a
reference: its type must say that it borrows, so every holder knows it can't outlive the data.

Earlier Parts used annotated signatures without explaining them: `longest<'a>` (3.3), `Record<'a>` in `logstat`,
`Tx<'db>` in the transaction guard (3.5), and `redact<'a>(line: &'a str, ...) -> Cow<'a, str>` in Project Level 2.
This chapter explains all of them.

### 2. Mental model

**A lifetime parameter is a name for a region the *caller* chooses.** An annotation never makes anything live longer or
shorter. It **states a relationship** the compiler then enforces on both sides:

```text
 fn longest<'a>(a: &'a str, b: &'a str) -> &'a str

 reads as:  "for some region 'a that the caller picks, if a and b are both borrowed for at least 'a,
             then the result is valid for 'a"
 in effect: the result may borrow from EITHER input, so it's valid only while BOTH loans are
```

The vocabulary:

| Syntax | Meaning |
|---|---|
| `&'a T` | A reference valid for (at least) region `'a` |
| `'a: 'b` | "`'a` outlives `'b`": every point in `'b` is also in `'a` |
| `T: 'a` | Every reference inside `T` outlives `'a` (`T` holds no borrow shorter than `'a`) |
| `'static` (on a reference) | `&'static T`: the referent lives until the program ends (literals, leaked data, statics) |
| `T: 'static` (as a bound) | `T` contains no non-`'static` borrows. **Owned data like `String` satisfies it.** It does *not* mean "lives forever." |
| `'_` | "Infer this lifetime" (elided, but marked explicitly for readability) |

**Elision: the three rules** [LANG]. Most signatures need no annotations, because the compiler fills them in:

1. Each elided lifetime **in the inputs** becomes a distinct lifetime parameter.
2. If there's **exactly one** input lifetime, it's assigned to every elided output lifetime.
3. If one of the inputs is **`&self` or `&mut self`**, *self's* lifetime is assigned to every elided output lifetime.

If none of the rules determines an output lifetime, you must write it (E0106). And rule 3 can be *wrong for your
intent*: it assumes the output borrows from `self`, which may not be what you mean (§3's tokenizer).

### 3. Rust code

**When elision can't decide** (verified):

```rust,compile_fail
fn longest(a: &str, b: &str) -> &str {
    if a.len() >= b.len() { a } else { b }
}
```

```text
error[E0106]: missing lifetime specifier
 --> src/main.rs:2:33
  |
2 | fn longest(a: &str, b: &str) -> &str {
  |               ----     ----     ^ expected named lifetime parameter
  |
  = help: this function's return type contains a borrowed value, but the signature does not say whether it is borrowed from `a` or `b`
help: consider introducing a named lifetime parameter
  |
2 | fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
  |           ++++     ++          ++          ++
```

Two inputs, so rule 2 doesn't apply, and there's no `self`. The help text is the whole idea in one sentence: *the
signature does not say whether it is borrowed from `a` or `b`.* With `<'a>` on both, the answer is "possibly either," so
callers must keep **both** alive (verified):

```rust,compile_fail
fn main() {
    let service = String::from("gateway");
    let winner;
    {
        let region = String::from("eu-west-1");
        winner = longest(&service, &region); // 'a must cover BOTH inputs' loans
    } // `region` dropped here
    println!("{winner}");
}
```

```text
error[E0597]: `region` does not live long enough
   |
10 |         let region = String::from("eu-west-1");
   |             ------ binding `region` declared here
11 |         winner = longest(&service, &region); // 'a must cover BOTH inputs' loans
   |                                    ^^^^^^^ borrowed value does not live long enough
12 |     } // `region` dropped here
   |     - `region` dropped here while still borrowed
13 |     println!("{winner}");
   |                ------ borrow later used here
```

Notice what the checker did *not* do: look into `longest` and see that "gateway" is longer, so the result comes from
`service`. The signature says "either," and callers are checked against the signature.

**Where the output borrows from matters: a tokenizer.** A zero-copy tokenizer returns slices of its input. Written with
an explicit `'a`, tokens borrow from the **input**, not from the tokenizer (verified):

```rust
/// A zero-copy tokenizer. Tokens borrow from the INPUT ('a), not from the tokenizer.
struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(input: &'a str) -> Self {
        Tokenizer { input, pos: 0 }
    }

    /// `&mut self` is borrowed only for the call; the returned token lives as long as the input.
    fn next_token(&mut self) -> Option<&'a str> {
        let rest = &self.input[self.pos..];
        let start = rest.len() - rest.trim_start().len();
        let rest = &rest[start..];
        if rest.is_empty() {
            return None;
        }
        let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        self.pos += start + len;
        Some(&rest[..len])
    }
}

fn main() {
    let line = String::from("GET /api/orders HTTP/1.1");
    let mut tokens = Tokenizer::new(&line);
    let method = tokens.next_token(); // holds a token...
    let path = tokens.next_token(); // ...while calling next_token again: fine
    let version = tokens.next_token();
    println!("{method:?} {path:?} {version:?} {:?}", tokens.next_token());
}
```

```text
Some("GET") Some("/api/orders") Some("HTTP/1.1") None
```

Now write the same method with the output lifetime **elided**, `fn next_token(&mut self) -> Option<&str>`. Rule 3 ties
the token to `&mut self`, so each token keeps the tokenizer mutably borrowed, and a second call conflicts (verified):

```text
error[E0499]: cannot borrow `tokens` as mutable more than once at a time
   |
29 |     let method = tokens.next_token();
   |                  ------ first mutable borrow occurs here
30 |     let path = tokens.next_token();
   |                ^^^^^^ second mutable borrow occurs here
31 |     println!("{method:?} {path:?}");
   |                ------ first borrow later used here
```

Same body, and one annotation's difference in the signature. With elision the API says "a token is valid only until you
use the tokenizer again," which is a *lending* API and much less useful. With `'a` it says "a token is valid as long as
the input is." **Elision picks a sensible default, and for methods that return data from a borrowed input rather than
from `self`, the default is wrong.**

**`'static` as a bound: threads** (verified):

```rust,compile_fail
use std::thread;

fn main() {
    let tenant = String::from("acme");
    let handle = thread::spawn(|| {
        println!("warming cache for {tenant}");
    });
    handle.join().unwrap();
}
```

```text
error[E0373]: closure may outlive the current function, but it borrows `tenant`, which is owned by the current function
 --> src/main.rs:6:32
  |
6 |     let handle = thread::spawn(|| {
  |                                ^^ may outlive borrowed value `tenant`
7 |         println!("warming cache for {tenant}");
  |                                      ------ `tenant` is borrowed here
  |
note: function requires argument type to outlive `'static`
help: to force the closure to take ownership of `tenant` (and any other referenced variables), use the `move` keyword
```

`thread::spawn` requires `F: 'static` (Chapter 1.3), because the thread may outlive `main`'s frame. The closure borrows
`tenant`, a local, so it isn't `'static`. `move` fixes it by making the closure *own* the `String`, and an owned `String`
satisfies `'static`: it contains no borrows at all. **`T: 'static` means "owns everything it needs," not "lives
forever."**

**The earlier signatures, decoded:**

| Signature (from) | Elision? | What it says |
|---|---|---|
| `fn parse_header(buf: &[u8]) -> Result<(Header, &[u8]), String>` (2.4) | Rule 2: one input lifetime | The body slice borrows from `buf` |
| `struct Record<'a> { path: &'a str, .. }` (logstat) | Structs never elide | A record can't outlive the line it was parsed from |
| `fn find(&self, id: u64) -> Option<&User>` (1.1) | Rule 3 | The user borrows from the registry, so the registry is frozen while you hold it (hence E0502 on `add`) |
| `struct Tx<'db> { db: &'db mut Db, .. }` (3.5) | Structs never elide | A transaction holds exclusive access to the database for its whole life |
| `fn redact<'a>(line: &'a str, stats: &mut Stats) -> Cow<'a, str>` (L2) | Two input lifetimes, no `self`: **must annotate** | The output borrows from `line`, never from `stats` |
| `fn longest<'a>(a: &'a str, b: &'a str) -> &'a str` (3.3) | Two inputs: must annotate | The output borrows from either, so it's valid while both are |

The `redact` row is worth a second look. Without `'a`, the elided signature `fn redact(line: &str, stats: &mut Stats)
-> Cow<'_, str>` fails (E0106), because there are two input lifetimes. Writing `'a` on `line` only states the truth: the
output never borrows the stats.

---

## Pass 2 · Systems level — *What an annotation compiles to*

### 4. Under the hood

**Lifetime parameters are erased generics.** [RUSTC] Type parameters are monomorphized: one copy of code per concrete
type (Part VII). Lifetime parameters aren't: `longest` is compiled **once**, however many different regions callers use.
They exist only during checking.

**Inside the function, a lifetime parameter is a universal region.** When checking the body of `longest<'a>`, the
compiler doesn't know which region `'a` will be, so the body must be correct **for every possible `'a`**. That's why you
can't return a reference to a local from a function with `-> &'a str`: no local outlives an arbitrary caller-chosen
region (E0515, Chapter 1.2).

**At each call site, the lifetime is inferred.** The caller's region for `'a` is chosen by the same constraint solver as
Chapter 4.2: the smallest region satisfying "both argument loans cover `'a`" and "the result's uses lie within `'a`."
E0597 above is that solver failing: `region`'s loan can't cover the point where `winner` is used.

**Elision is syntax.** [RUSTC] The rules are applied while lowering the AST to HIR (Part XVIII). By the time the checker
runs, an elided signature is indistinguishable from the hand-annotated one. That's why the elided tokenizer and the
annotated one behave so differently: after elision they're *different signatures*.

**`T: 'a` is often implied.** A type like `&'a T` is only well-formed if `T: 'a` (the referent outlives the reference),
so the compiler adds that bound implicitly ("implied bounds"). The same implied-bounds logic, interacting with variance,
is where the famous soundness hole #25860 lives (Chapter 4.4).

### 5. Memory

The tokenizer's structure in memory. Tokens point into the input buffer, not into the tokenizer:

```text
 stack                                          heap (owned by `line`)
 ┌──────────────────────────────┐               ┌──────────────────────────────────────────┐
 │ line: String  ptr ───────────┼─────────────► │ G E T   / a p i / o r d e r s   H T T P …│
 ├──────────────────────────────┤               └──────────────────────────────────────────┘
 │ tokens: Tokenizer<'a>        │                  ▲        ▲                     ▲
 │   input: &'a str ────────────┼──────────────────┘        │                     │
 │   pos: usize                 │                           │                     │
 ├──────────────────────────────┤                           │                     │
 │ method: Option<&'a str> ─────┼── "GET" ──────────────────┘ (start of buffer)   │
 │ path:   Option<&'a str> ─────┼── "/api/orders" ────────────┘                   │
 │ version:Option<&'a str> ─────┼── "HTTP/1.1" ───────────────────────────────────┘
 └──────────────────────────────┘
   all tokens depend on `line` ('a), none on `tokens`: mutating the tokenizer can't invalidate them
```

With the elided signature, the diagram would be identical *at run time*. The difference is purely which loan the
compiler considers each token to extend. That's the chapter in one sentence: **lifetimes describe relationships the
machine never sees, and they decide which programs you're allowed to write.**

### 6. CPU / OS

Zero cost: lifetimes generate no code and no data. What they buy at the machine level is **the freedom to not copy**.
`Record<'a>`, `Tokenizer<'a>`, and `redact`'s `Cow<'a, str>` exist so that parsing a line allocates nothing (measured in
Parts II and III). Without lifetimes, a safe language has two choices: copy (allocate) or GC (keep everything reachable
alive). Lifetimes are the third option: borrow, checked.

---

## Pass 3 · Architect level — *Designing with lifetimes*

### 7. Trade-offs

**Borrowing structs vs owning structs:**

| | `struct Frame<'a> { body: &'a [u8] }` | `struct Frame { body: Vec<u8> }` | `struct Frame { body: Bytes }` (refcounted) |
|---|---|---|---|
| Parsing cost | None (zero-copy) | An allocation + copy | A refcount increment |
| Can be queued, stored, sent to another thread | No, not beyond the buffer's life | Yes | Yes |
| Signature impact | `'a` on every holder ("lifetime infection") | None | None |
| Best for | Parse-and-process within one scope | Long-lived, independent data | Shared buffers across tasks (Part XXI) |

**Rules of thumb:**

- Put lifetime parameters on **short-lived "view" types** (parsers, tokenizers, iterators, guards like `Tx<'db>`). Keep
  them off long-lived domain types.
- When a method returns data from a borrowed input, **write the lifetime explicitly** instead of letting rule 3 tie it
  to `self`.
- **`'static` bounds on spawned work** (threads, tasks) are satisfied by *owning*: `move` closures, `Arc`, owned
  messages. Scoped threads (`thread::scope`, Chapter 1.3) are the tool when you genuinely want to lend stack data to
  threads.
- Never satisfy a `'static` requirement with `Box::leak` in a path that runs more than once (§10).

### 8. Java comparison

Java has no lifetimes, but high-performance Java has the *problem* lifetimes solve, and it handles it with conventions
and run-time checks. The best-known example is **Netty's pooled `ByteBuf`**: buffers are reference-counted and returned
to a pool on `release()`, and a handler that holds a buffer after releasing it (or after passing it on) reads memory the
pool has handed to someone else. Netty detects some of this at run time (`IllegalReferenceCountException`, a leak
detector that samples allocations), and the rest shows up as corrupted data.

`ByteBuffer.slice()` views, "don't retain this array" Javadoc notes, and defensive copies are the same problem again.
A Rust `&'a [u8]` view into a pooled buffer is the compile-time version. The buffer can't be released while a view
exists, and a view can't be stored anywhere that outlives the buffer.

> **Analogy limit.** Netty's reference counting is *dynamic*: counts change at run time and errors are found at run
> time, if at all. Lifetimes are *static*: no counts, no run-time cost, and errors at compile time. The Rust equivalent
> of Netty's dynamic model is `Bytes` (refcounted, run-time). Rust lets you choose static or dynamic per boundary.

### 9. Production scenario

**Meridian's frame parser API** (Chapter 2.4's design exercise, resolved). The shared library exposes both shapes:

```rust,ignore
pub struct FrameRef<'a> { pub header: Header, pub body: &'a [u8] }   // zero-copy view
pub struct Frame { pub header: Header, pub body: Bytes }             // owned, shareable

pub fn parse(buf: &[u8]) -> Result<(FrameRef<'_>, usize), ParseError>;   // rule 2: borrows from buf
impl FrameRef<'_> { pub fn to_owned(&self) -> Frame { ... } }           // the explicit boundary
```

Services that process frames inline (the fan-out's hot path) use `FrameRef` with no allocation. Services that queue
frames to worker threads call `to_owned()` at exactly the point where data crosses the thread boundary, and the
compiler forces that decision (a `FrameRef` can't be sent to a `'static` task). The lifetime isn't an obstacle here. It
*marks* the architectural boundary between "borrowed from the network buffer" and "owned by the system."

### 10. Failure scenario

**Satisfying `'static` with a leak.** A metrics library required label names as `&'static str`. A developer needed
per-tenant labels, got a lifetime error, and "fixed" it with `Box::leak`, once per request. The counting allocator
measures what that does over 1,000 requests (listing `ch03-06-leak-static.rs`, verified):

```text
leaky: 3990 allocations, 2990 frees  -> 1000 blocks leaked
owned: 2990 allocations, 2990 frees  -> 0 blocks leaked
```

One leaked block per request, never freed. `Box::leak` is *safe* (Chapter 1.3: leaking is safe), so nothing warned.
The service's memory grew linearly with traffic, and it restarted every few days under an OOM kill. `Box::leak` is the
right tool for data created **once** that lives for the whole process (startup configuration, Chapter 3.1's table).
Per-request, it's a memory leak with extra steps.

The fixes, by preference: change the API to accept owned or shared data (`String`, `Arc<str>`); **intern** labels
(leak each *distinct* label at most once, via a set of previously leaked `&'static str`, so memory is bounded by the
number of tenants, not requests); or bound the set of labels (tenant tiers instead of tenant IDs, a cardinality
decision the metrics system probably needed anyway).

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part IV).*

1. What does a lifetime annotation *do*? Why is "it makes the reference live longer" wrong?
2. State the three elision rules. Give a signature where each one applies, and one where none does.
3. Why does `longest<'a>` force callers to keep *both* inputs alive, even when the longer one is obvious at run time?
4. Explain the tokenizer example: why does the elided version fail with E0499, and what does `'a` on the return type
   change?
5. What's the difference between `&'static T` and `T: 'static`? Why does `String` satisfy `'static`?
6. Why are lifetime parameters not monomorphized? What does "universal region" mean inside a function body?
7. When should a struct have a lifetime parameter, and when should it own its data instead?
8. Why is `Box::leak` safe, and when is it the right tool?

### 12. Exercises

- **Beginner.** Annotate these, or explain why elision already works: `fn first(v: &[u32]) -> &u32`;
  `fn pick(a: &str, flag: bool) -> &str`; `fn name(&self) -> &str`; `fn join(a: &str, b: &str) -> String`.
- **Intermediate.** Write `struct KeyValueIter<'a>` over `"k1=v1;k2=v2"` that yields `(&'a str, &'a str)` pairs without
  allocating, and implement `Iterator` for it.
- **Advanced.** Write `fn split_first_word<'a, 'b>(s: &'a str, sep: &'b str) -> (&'a str, &'a str)`. Why do you need two
  lifetimes here, and what goes wrong for callers if you use a single `'a` for both parameters?
- **Systems.** Show, using the Playground's LLVM IR output, that `longest` is emitted exactly once even when called with
  references of three different lifetimes.
- **Architecture.** Audit a library API you use (Rust or Java). Where does it lend data (views, slices, buffers), and how
  does it prevent misuse: types, docs, or run-time checks?

### 13. Debugging exercise

The elided tokenizer fails with E0499 (§3).

1. Apply the elision rules by hand to `fn next_token(&mut self) -> Option<&str>`. Which rule fires, and what's the
   resulting full signature?
2. Explain the error in loan terms: which loan does `method` extend, and why does the second call conflict with it?
3. Fix it with an explicit lifetime. Then describe a *different* API where tying the output to `&mut self` is exactly
   right. (Hint: a buffer that's overwritten on each call, a "lending" reader.)

### 14. Design exercise

**Zero-copy JSON handling for Meridian's gateway.** The gateway inspects a few fields of each JSON request body (tenant
ID, idempotency key) before forwarding the body unchanged to an upstream. Bodies are up to 1 MB. Some requests are also
logged asynchronously by a separate task.

Design the types: a borrowed view (`RequestView<'a>` with `&'a str` fields), owned structs, or a mix. Where does data
become owned, and why exactly there? What happens to the design when a field contains JSON escapes (`\"`), so it can't be
a plain slice of the input? (Hint: `Cow<'a, str>`, which is also how `serde` handles borrowed fields, Part XXII.)
Estimate allocations per request for your design and the alternatives.
