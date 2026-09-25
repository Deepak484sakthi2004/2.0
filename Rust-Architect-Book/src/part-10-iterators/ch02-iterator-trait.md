# Chapter 10.2 — The Iterator Trait and Laziness

> **Where this sits:** Part X · Closures, Iterators, and Zero-Cost Abstractions · chapter 2 of 4
> **Prerequisites:** Chapter 10.1 (closures), Chapter 2.5 (`for` desugaring), Chapter 4.1 (iterator invalidation),
> Chapter 4.5 and 6.2 (why lending needs more than `Iterator`).
> **After this chapter you can:** predict when iterator code runs and how much of the input it reads; choose between
> `iter()`, `iter_mut()`, and `into_iter()` by ownership; implement a correct custom iterator with an honest
> `size_hint`; explain why a lending iterator can't be a std `Iterator` and build one with a GAT; and find a class of
> bugs where the same pipeline behaves differently depending on the iterator it's fed.

---

## Pass 1 · User level — *One method, seventy consequences*

### 1. Problem

Java gives you two iteration models. `java.util.Iterator` is a pull protocol with two calls, `hasNext()` and `next()`,
plus an optional `remove()`. Streams are a declarative pipeline whose internals you rarely see. Rust has **one** trait
that does both jobs:

```rust,ignore
// core::iter::Iterator, abridged
pub trait Iterator {
    type Item;
    fn next(&mut self) -> Option<Self::Item>;
    // ...plus about 75 provided methods (map, filter, fold, collect, zip, ...) built on next()
}
```

Everything else follows from that one required method. To use it well, you need to know when work actually happens
(laziness), who owns the items (the three `IntoIterator` forms), what the trait promises and what it doesn't
(`None` isn't necessarily final, and `size_hint` is only a hint), and what it can't express at all (items that borrow
from the iterator itself).

### 2. Mental model

**An iterator is a small state machine that produces one item per `next()` call, and it does nothing until asked.**

```text
 consumer            adapters (lazy)                            source
 ─────────           ─────────────────────────────────          ──────────
 collect() ──next()──▶ Map ──next()──▶ Filter ──next()──▶ slice::Iter
     ▲                  │               │  (loops until       │ returns &a[i], i += 1
     │                  │               │   predicate true)   │
     └──── Some(item) ◀─┴──── Some ◀────┴───── Some(&x) ◀─────┘
 one item travels the whole pipeline before the next item starts
```

- **Adapters** (`map`, `filter`, `take`, `zip`, `chain`, `enumerate`, ...) take an iterator and return a new iterator
  that wraps it. They run *no* code when called.
- **Consumers** (`collect`, `sum`, `fold`, `for_each`, `count`, `any`, `find`, a `for` loop, ...) drive the pipeline
  by pulling items.
- **Short-circuiting consumers** (`any`, `all`, `find`, `position`, `take_while`, ...) stop pulling when they know the
  answer, so the rest of the input is never read.

**The contract, precisely** [LIB] (these are documented properties of the trait):

| Property | Guaranteed? | What it means |
|---|---|---|
| `next()` returns `None` when exhausted | Yes | ...but it **may** return `Some` again later, unless the iterator is `FusedIterator` |
| `size_hint() -> (lower, Option<upper>)` | Only as a hint | Safe code may use it to pre-allocate; **unsafe code must not trust it** |
| `ExactSizeIterator::len()` | Must match `size_hint` | A logic contract; a wrong `len()` is a bug, not UB |
| `DoubleEndedIterator::next_back()` | Yes, if implemented | Consume from both ends: enables `rev()`, `rposition()` |
| `TrustedLen` | Internal, `unsafe`, unstable | std's own promise that a length is exact, used for specialization (§4 of Chapter 10.3) |

**Ownership of items comes from which method created the iterator:**

| Call | Item type | The collection afterwards |
|---|---|---|
| `v.iter()` / `for x in &v` | `&T` | unchanged, borrowed while iterating |
| `v.iter_mut()` / `for x in &mut v` | `&mut T` | exclusively borrowed while iterating |
| `v.into_iter()` / `for x in v` | `T` | **consumed**: moved into the iterator, freed when the iterator is dropped |

### 3. Rust code

**Laziness, observed** (listing `ch02-01-laziness.rs`):

```rust
// Iterators are lazy and pull-based: nothing runs until a consumer asks for the next item,
// and then each item travels the whole pipeline before the next one starts.
fn main() {
    let amounts = [120_u64, 5, 980, 42, 3_000];

    let pipeline = amounts
        .iter()
        .inspect(|a| println!("  source yields {a}"))
        .filter(|&&a| {
            let keep = a >= 100;
            println!("    filter({a}) -> {keep}");
            keep
        })
        .map(|&a| {
            println!("      map({a}) -> {}", a * 2);
            a * 2
        });
    println!("pipeline built; nothing has run yet");

    let first_two: Vec<u64> = pipeline.take(2).collect();
    println!("first_two = {first_two:?}  (3,000 was never read)");

    // Short-circuiting consumers stop pulling as soon as the answer is known:
    let big = amounts.iter().inspect(|a| println!("  any() looked at {a}")).any(|&a| a > 500);
    println!("any > 500: {big}");
}
```

```text
pipeline built; nothing has run yet
  source yields 120
    filter(120) -> true
      map(120) -> 240
  source yields 5
    filter(5) -> false
  source yields 980
    filter(980) -> true
      map(980) -> 1960
first_two = [240, 1960]  (3,000 was never read)
  any() looked at 120
  any() looked at 5
  any() looked at 980
any > 500: true
```

Read the output as a timeline. Building the pipeline printed nothing. Each item went through `filter` and `map` before
the next item was read, **vertically**, not stage by stage. `take(2)` stopped the pull after two results, so `42` and
`3,000` were never touched. `any` stopped at `980`.

Because adapters do nothing until consumed, discarding one is almost always a bug, and the compiler says so. With
`#![deny(unused_must_use)]` (listing `ch02-02-lazy-lint.rs`):

```text
error: unused `Map` that must be used
 --> src/main.rs:6:5
  |
6 |     ids.iter().map(|id| println!("processing {id}"));
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = note: iterators are lazy and do nothing unless consumed
```

A second lint (`map_unit_fn`, on by default) adds *"you might have meant to use `Iterator::for_each`"*. Both are
warnings by default. Chapter 8.1 argued for `-D warnings` in CI, and this is one more reason.

**The three `for` forms** (listing `ch02-03-into-iter-forms.rs`):

```rust
// The three IntoIterator impls on Vec<T>, and what each `for` loop gets.
use std::any::type_name_of_val;

fn main() {
    let mut routes = vec!["/pay".to_string(), "/refund".to_string()];

    for r in &routes {
        // IntoIterator for &Vec<T>: items are &T, the Vec is only borrowed
        println!("for r in &routes      -> {}", type_name_of_val(&r));
    }
    for r in &mut routes {
        // IntoIterator for &mut Vec<T>: items are &mut T, edit in place
        r.push_str("/v2");
    }
    println!("after &mut: {routes:?}");
    for r in routes {
        // IntoIterator for Vec<T>: items are T (owned), the Vec is consumed
        println!("for r in routes       -> {}", type_name_of_val(&r));
    }
    // `routes` is gone here: its buffer was freed when the loop's IntoIter was dropped.

    // Arrays: by-value IntoIterator since edition 2021 (listing ch02-05 shows edition 2018).
    let codes = [200_u16, 404, 503];
    let first = codes.into_iter().next().unwrap();
    println!("[u16; 3].into_iter()  -> {}", type_name_of_val(&first));
}
```

```text
for r in &routes      -> &alloc::string::String
for r in &routes      -> &alloc::string::String
after &mut: ["/pay/v2", "/refund/v2"]
for r in routes       -> alloc::string::String
for r in routes       -> alloc::string::String
[u16; 3].into_iter()  -> u16
```

Use the collection after `for x in v`, and the error explains the desugaring (listing `ch02-04-vec-moved-by-for.rs`):

```text
error[E0382]: borrow of moved value: `batch`
 --> src/main.rs:7:37
  |
4 |     for evt in batch {
  |                ----- `batch` moved due to this implicit call to `.into_iter()`
...
7 |     println!("published {} events", batch.len()); // the loop consumed `batch`
  |                                     ^^^^^ value borrowed here after move
  |
help: consider iterating over a slice of the `Vec<String>`'s content to avoid moving into the `for` loop
  |
4 |     for evt in &batch {
```

Which form should you write? **Borrow (`&v`) by default. Consume (`v`) when the loop is the collection's last user
and you want to move the items out**, for example to hand each `String` to another owner without cloning it. The
consuming form is the only one that can move items out without a clone. It's also the form Java doesn't have.

[VERSION] Arrays got by-value `IntoIterator` in Rust 1.53, but `array.into_iter()` in *method-call* syntax kept its
old meaning (iterate by reference) in editions 2015/2018 for compatibility. The same source file prints different
types by edition (listing `ch02-05-array-into-iter-2018.rs`: `item type: &u16` under 2018, with an `array_into_iter`
warning, and `item type: u16` under 2021).

**A custom iterator: zero-copy frames** (listing `ch02-06-frames-iterator.rs`). This is Meridian's length-prefixed frame
format from Chapter 2.4, simplified to magic `0xCAFE`, a type byte, a big-endian `u32` body length, and the body. The
iterator yields borrowed frames out of a buffer:

```rust,ignore
struct Frames<'a> {
    buf: &'a [u8],
    failed: bool,
}

const HEADER: usize = 7;

impl<'a> Iterator for Frames<'a> {
    type Item = Result<Frame<'a>, String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed || self.buf.is_empty() {
            return None;
        }
        if self.buf.len() < HEADER || self.buf[..2] != [0xCA, 0xFE] {
            self.failed = true; // a framing error can't be resynchronized: stop for good after reporting it
            return Some(Err(format!("bad header, {} bytes left", self.buf.len())));
        }
        let len = u32::from_be_bytes(self.buf[3..7].try_into().unwrap()) as usize;
        let Some(body) = self.buf.get(HEADER..HEADER + len) else {
            self.failed = true;
            return Some(Err(format!("truncated body: want {len}, have {}", self.buf.len() - HEADER)));
        };
        let frame = Frame { kind: self.buf[2], body };
        self.buf = &self.buf[HEADER + len..];
        Some(Ok(frame))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.failed || self.buf.is_empty() {
            (0, Some(0))
        } else {
            // at least one item (a frame or an error); at most one per 7-byte header
            (1, Some(self.buf.len() / HEADER))
        }
    }
}

// We return None forever after the first None, so we can promise it (FusedIterator is a marker trait):
impl FusedIterator for Frames<'_> {}
```

```text
size_hint before: (1, Some(7))
kind 1 body "MRDN,12550"
kind 2 body "HB"
kind 1 body "ACME,990"
error: truncated body: want 50, have 1
quote frames: 2
size_of::<Frames>() = 24 B
```

Four design decisions are visible here. **The item type borrows from the buffer** (`Frame<'a>` holds `&'a [u8]`), not
from the iterator. That's what makes it a legal std `Iterator` (see §4). **Errors are items**
(`Result<Frame, String>`), so a consumer can stop at the first error (`collect::<Result<Vec<_>, _>>()`, Chapter 10.4)
or skip errors (`filter_map(Result::ok)`). **After an error, it stops for good**, and `FusedIterator` promises that to
adapters such as `fuse()`, which can then skip their own bookkeeping. **`size_hint` is honest but loose**: at most one
item per 7 header bytes.

Adapters are free. The listing's last line uses `frames(&wire).filter_map(Result::ok).filter(|f| f.kind ==
1).map(|f| f.body).collect()` without writing any more iterator code.

**What `size_hint` looks like through adapters** (listing `ch02-07-size-hints.rs`):

```text
v.iter()                     (100, Some(100))
v.iter().map(..)             (100, Some(100))
v.iter().filter(..)          (0, Some(100))
v.iter().take(10)            (10, Some(10))
v.iter().skip(95)            (5, Some(5))
v.iter().zip(0..30)          (30, Some(30))
v.iter().chain(v.iter())     (200, Some(200))
w.iter().flatten()           (0, None)
(0..).take(5)                (5, Some(5))
(0..)                        (18446744073709551615, None)
```

`filter` keeps the upper bound and drops the lower to 0 (it might reject everything). `flatten` knows nothing
(`(0, None)`), because inner collections could be any length. `(0..)` is endless: the lower bound saturates at
`usize::MAX` and there's no upper bound. `collect` uses the lower bound to pre-allocate (Chapter 9.1 measured it: an
exact hint gives one allocation, `filter` grows the result by doubling).

**`None` is not necessarily final** (listing `ch02-08-fuse.rs`). An iterator over a growing source, like `tail -f` on a
log, can legitimately say "nothing now" and later "here's another":

```text
unfused: [Some("line A"), None, None, Some("line B")]
fused:   [Some("line A"), None, None, None]
```

`fuse()` makes `None` final. Code that polls again after `None` (a loop resumed through `by_ref()`, a retry around a
`Peekable`, your own `while let` after a pause) sees whatever the source does. std's own adapters are careful here:
`Chain`, for instance, drops each half the first time it returns `None` and never asks it again [LIB]. A custom
iterator that can resume should document it. One that can't should `impl FusedIterator`.

**Iterators without a struct** (listing `ch02-09-from-fn-successors.rs`): `iter::successors` builds each item from the
previous one (Chapter 8.4's backoff schedule), and `iter::from_fn` keeps its state in a closure:

```rust
// Iterators without a struct: from_fn (state in a closure) and successors (each item from the previous one).
use std::iter;

fn main() {
    // Exponential backoff schedule (Chapter 8.4): 100, 200, 400, ... capped at 2,000 ms, 6 attempts.
    let delays: Vec<u64> = iter::successors(Some(100_u64), |&d| Some((d * 2).min(2_000))).take(6).collect();
    println!("backoff ms: {delays:?}");

    // A tokenizer whose state (`rest`) is captured by a closure.
    let line = "SET  user:42   active";
    let mut rest = line;
    let tokens = iter::from_fn(move || {
        rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let end = rest.find(' ').unwrap_or(rest.len());
        let (tok, tail) = rest.split_at(end);
        rest = tail;
        Some(tok)
    });
    println!("tokens: {:?}", tokens.collect::<Vec<_>>());

    // repeat_with + take: generate IDs lazily.
    let mut next_id = 1000;
    let ids: Vec<u32> = iter::repeat_with(|| {
        next_id += 1;
        next_id
    })
    .take(3)
    .collect();
    println!("ids: {ids:?}");
}
```

```text
backoff ms: [100, 200, 400, 800, 1600, 2000]
tokens: ["SET", "user:42", "active"]
ids: [1001, 1002, 1003]
```

`from_fn`'s closure is `FnMut` (it mutates `rest`), and `move` makes it own `rest`, a `&str` into `line`, so the
iterator can outlive the scope that created it as long as `line` does. That's Chapter 10.1 in a single line.

---

## Pass 2 · Systems level — *What the trait can't say, and what it costs*

### 4. Under the hood

**`for` is sugar over `IntoIterator` and `next()`** (Chapter 2.5 showed the full desugaring):

```rust,ignore
// for x in expr { body }   becomes, roughly [LANG]:
match IntoIterator::into_iter(expr) {
    mut iter => loop {
        match iter.next() {
            None => break,
            Some(x) => { body }
        }
    },
}
```

The loop holds `iter` mutably for its whole duration, which is why you can't call `iter.next()` inside a `for` loop
over it (this chapter's debugging exercise), and why a loop over `&v` keeps `v` borrowed until the loop ends
(Chapter 4.1's iterator invalidation).

**Why a lending iterator can't be a std `Iterator`.** Look at `next`'s signature again:
`fn next(&mut self) -> Option<Self::Item>`. `Item` is one type, fixed by the impl, and it can't mention the lifetime
of the `&mut self` borrow of each call. So an item can borrow from *something else* (a slice the iterator points into,
like `Frames<'a>`), but never from the iterator's own storage. Try to write a line reader that reuses its buffer
(listing `ch02-10-lending-std-fails.rs`):

```rust,compile_fail
// Attempt: a line reader that yields slices of its OWN reused buffer through std's Iterator.
use std::io::BufRead;

struct Lines<R> {
    src: R,
    buf: Vec<u8>,
}

impl<'a, R: BufRead> Iterator for Lines<R> {
    type Item = &'a [u8]; // borrows from self.buf... but which self? `'a` is tied to nothing
    fn next(&mut self) -> Option<&'a [u8]> {
        self.buf.clear();
        match self.src.read_until(b'\n', &mut self.buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(&self.buf),
        }
    }
}

fn main() {}
```

```text
error[E0207]: the lifetime parameter `'a` is not constrained by the impl trait, self type, or predicates
  --> src/main.rs:10:6
   |
10 | impl<'a, R: BufRead> Iterator for Lines<R> {
   |      ^^ unconstrained lifetime parameter

error: lifetime may not live long enough
  --> src/main.rs:16:22
   |
12 |     fn next(&mut self) -> Option<&'a [u8]> {
   |             - let's call the lifetime of this reference `'1`
...
16 |             Ok(_) => Some(&self.buf),
   |                      ^^^^^^^^^^^^^^^ method was supposed to return data with lifetime `'a` but it is returning data with lifetime `'1`
```

The second error is the real one: the data comes from `'1` (this call's borrow of `self`), but the trait's shape
demands a lifetime chosen once for the whole impl. The compiler's suggestion to add `'a` to `Lines` would change the
design into "borrow from an outside buffer," which is `Frames`.

And the trait is *right* to refuse. If `next` could hand out `&self.buf`, then `let a = it.next(); let b = it.next();`
would give you `a` pointing into a buffer that the second call just overwrote. `collect()` would produce a `Vec` of
slices that all alias one buffer. Every adapter that holds more than one item at a time (`collect`, `zip` with itself,
`windows`, `max_by_key` that remembers the best so far) would be unsound.

**The GAT version** (Chapter 6.2 introduced generic associated types, stable since Rust 1.65). Make the item type
generic over the borrow of each call (listing `ch02-11-lending-gat.rs`, abridged; the counting allocator is the Part III
instrument):

```rust,ignore
trait LendingIterator {
    type Item<'a>
    where
        Self: 'a;
    fn next(&mut self) -> Option<Self::Item<'_>>;
}

impl<R: BufRead> LendingIterator for Lines<R> {
    type Item<'a>
        = &'a [u8]
    where
        Self: 'a;

    fn next(&mut self) -> Option<&[u8]> {
        self.buf.clear(); // keeps the capacity: the same buffer serves every line
        match self.src.read_until(b'\n', &mut self.buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(&self.buf),
        }
    }
}
```

```text
lines=10000 errors=200 allocations while reading=0
```

Zero allocations for 10,000 lines, in debug and release. std's `BufRead::lines()` returns a new `String` per line
[LIB], which is the price of fitting the std `Iterator` shape. The GAT version pays in a different currency: **you can
hold only one item at a time** (listing `ch02-12-lending-hold-two.rs`):

```text
error[E0499]: cannot borrow `lines` as mutable more than once at a time
  --> src/main.rs:35:18
   |
34 |     let first = lines.next();
   |                 ----- first mutable borrow occurs here
35 |     let second = lines.next(); // the buffer `first` points into is about to be overwritten
   |                  ^^^^^ second mutable borrow occurs here
36 |     println!("{first:?} {second:?}");
   |                ----- first borrow later used here
```

The borrow checker enforces the exact property that made the std version unsound. But there's no `collect`, no `map`
adapter, no `for` loop (only `while let`), and no std ecosystem. Your trait is yours alone, and adapters must be
written by hand or taken from a crate. In practice, lending iterators are used at hot boundaries (parsers, readers) and
are converted to owned items (`to_vec()`) at the point where the consumer needs to keep data. That's the same boundary
Chapter 4.5 drew with the HRTB visitor, reached from the other side.

**`gen` blocks** [VERSION]. Writing `next()` by hand means turning your control flow inside out into an explicit state
machine (`Frames` stores `buf` and `failed` to remember where it was). Generators let the compiler do that
transformation. Edition 2024 reserved the `gen` keyword (listing `ch02-14-gen-reserved.rs`: `let gen = 7;` is
*"expected identifier, found reserved keyword `gen`"* under 2024 and fine under 2021), and nightly Rust has `gen`
blocks behind a feature gate (listing `ch02-13-gen-blocks.rs`, verified on nightly; stable rejects it with E0554):

```rust,ignore
#![feature(gen_blocks)]

fn backoff(start: u64, cap: u64) -> impl Iterator<Item = u64> {
    gen move {
        let mut d = start;
        loop {
            yield d; // the block is compiled to a state machine that implements Iterator
            d = (d * 2).min(cap);
        }
    }
}
```

```text
backoff ms: [100, 200, 400, 800, 1600, 2000]
```

The compiler turns the block into a state machine with one state per `yield`, the same coroutine transformation that
turns `async fn` into a future (Chapter 12.3). As of Rust 1.98, it isn't stable, so production code writes `next()` by
hand or uses `from_fn`.

### 5. Memory

Iterators are **small values, usually on the stack**:

```text
 slice::Iter<'_, T>        2 pointers (ptr, end)                                     16 bytes
 Frames<'a>                &[u8] (ptr, len) + bool                                   24 bytes (measured; 7 are padding)
 vec::IntoIter<T>          owns the Vec's buffer: buffer pointer + capacity + current + end
                           dropping it drops the remaining items and frees the buffer
 from_fn(closure)          exactly the closure's captures (Chapter 10.1)
 Map<Filter<Iter<T>, P>, F>  the nested structs of Chapter 10.3: 16 + |P| + |F| bytes
```

**Laziness is a memory property.** A pipeline holds *one item in flight*, not a stage's worth of items. Chapter 10.3
counts it: a fused filter-map-sum over 100,000 transactions allocates nothing, and the same logic with a `collect()`
after each stage makes 17 allocations and 2.67 MB of intermediate data. Streaming a 10 GB file through a pipeline needs
the pipeline's few bytes plus the reader's buffer, not 10 GB.

`into_iter()` is the one form that moves memory ownership. The `Vec`'s buffer now belongs to the `IntoIter`, and
Chapter 10.3 shows that `collect()` can hand that same buffer to the output `Vec` (in-place collect).

### 6. CPU / OS

- **Vertical processing keeps data hot.** Each item travels the whole pipeline while it's in registers or L1. A staged
  design that materializes each step (a `Vec` per stage, the eager "list of lists" style) streams every intermediate
  through the memory hierarchy and back. For inputs larger than the last-level cache, that's the difference between
  one pass over memory and several (Chapter 9.5's rules: bytes touched, and dependent loads).
- **`next()` is a function call only in debug builds.** In release builds the pipeline is inlined into one loop
  (Chapter 10.3). The per-item cost of an adapter is then the cost of its logic: a compare for `filter`, a counter for
  `take`.
- **I/O iterators amortize syscalls with buffers.** `BufReader::lines()` makes one `read` syscall per buffer (8 KiB by
  default [LIB]), not per line. Its per-line cost is the `String` allocation, which the lending reader avoided.
- **Short-circuiting saves I/O, not just CPU.** An `any()` or `find()` over a lazy reader stops *reading the file*
  once it has an answer.

---

## Pass 3 · Architect level — *Iterators as API surface*

### 7. Trade-offs

What should a function that produces a sequence return?

| Return type | Caller can... | Cost | When |
|---|---|---|---|
| `Vec<T>` | index, iterate many times, keep | allocates everything up front; latency to first item = total | small results, random access, several passes |
| `impl Iterator<Item = T> + '_` | iterate once lazily, chain adapters | no allocation beyond what items need; the type is unnameable | the default for internal APIs |
| named type (`pub struct Frames<'a>`) | store it in a struct, name it in signatures | you maintain a public type | public libraries (std returns `Iter`, `Chars`, `Lines`, ...) |
| `Box<dyn Iterator<Item = T> + '_>` | hold iterators of different concrete types in one variable | one allocation plus an indirect `next()` per item | heterogeneous sources chosen at run time (Chapter 10.3 measures it) |
| visitor `for_each_x(f: impl FnMut(&X))` | receive borrowed items, can't keep them | none; the producer drives | zero-copy callbacks (Chapter 4.5) |
| lending iterator (GAT) | pull borrowed items one at a time | none; no std adapters | hot readers and parsers that reuse a buffer |

Two failure-prone decisions deserve a rule each:

- **Put errors in the item type, not beside the iterator.** `Iterator<Item = Result<T, E>>` lets consumers decide
  between stop-at-first-error (`collect::<Result<..>>`), skip, and partition. An iterator that logs and silently
  skips bad records has decided for every consumer.
- **Document whether your iterator is fused and whether `size_hint` is exact.** Consumers pre-allocate from the lower
  bound. An inflated lower bound over-allocates, and code downstream may reasonably assume `None` is final.

### 8. Java comparison

| Java | Rust | Difference that matters |
|---|---|---|
| `Iterator<T>`: `hasNext()` + `next()` | `next() -> Option<T>` | One call, no "`hasNext()` has side effects" or "called `next()` without `hasNext()`" bugs |
| `Iterator.remove()`, fail-fast `ConcurrentModificationException` | No remove; mutation during iteration is a compile error (E0502/E0499) | Detection at run time, best effort, vs impossibility at compile time (Chapter 4.1) |
| `Iterable<T>` | `IntoIterator` with three impls per collection (`&C`, `&mut C`, `C`) | Java items are always shared references; Rust items are borrowed, mutably borrowed, or owned |
| `for (T x : list)` | `for x in &list` | Java has no consuming loop; Rust's `for x in list` moves items out without copying |
| `Spliterator` (splitting, characteristics like `SIZED`, `ORDERED`) | `size_hint`, `ExactSizeIterator`, `DoubleEndedIterator`; splitting is Rayon's `IndexedParallelIterator` (Chapter 10.4) | Similar metadata, different owners |
| Generators: none (virtual threads can emulate) | `from_fn`, hand-written `next()`, nightly `gen` | |

> **Analogy limit.** "A Rust `Iterator` is a Java `Iterator` with a single method" holds for the protocol. It fails on
> what iterators *are*. A Java iterator is a heap object reached through an interface, and every `next()` is a virtual
> call the JIT may devirtualize. A Rust iterator is a plain struct, usually on the stack, statically dispatched, and
> tied by a lifetime to the collection it borrows. That lifetime is why Rust needs no `ConcurrentModificationException`.

### 9. Production scenario

**Meridian's market-data replay tool.** The market-data team (Chapter 2.4's frame format) records every frame from the
exchange feeds into capture files and replays them to reproduce incidents. The replay tool is built on the `Frames`
iterator from §3:

- The capture is read into one buffer per file segment, and `Frames<'a>` yields frames that borrow from it. Replaying
  a 2 GB segment allocates the segment buffer and nothing per frame. The pipeline
  `frames(&seg).filter_map(Result::ok).filter(|f| subscribed(f.kind)).map(decode_quote)` runs vertically, so a quote
  goes from bytes to the order-book update while its bytes are still in L1.
- **Truncated trailing frames are normal** (the recorder can be stopped mid-frame), so the error is an item, not a
  panic. The replay tool counts and reports it, while the incident-analysis tool stops at it (`collect::<Result<..>>`).
- `FusedIterator` matters because the tool chains segment iterators (`seg1.chain(seg2)`), and an iterator that yielded
  frames again after reporting corruption would replay garbage.
- `size_hint`'s upper bound (one frame per 7 bytes) is deliberately loose. The tool doesn't pre-allocate from it:
  `collect` uses the lower bound, which is 1.

### 10. Failure scenario

**The batcher that lost one event in four.** Meridian's ingestion pipeline (Chapter 3.2) publishes events to the broker
in batches, because the broker's API takes at most N events per request. The publisher's batching helper took events
from a shared iterator with `by_ref().zip(0..size)`. It passed its unit test, which fed events from a `Vec`. In
production, events arrive through a channel. Listing `ch02-15-zip-loses-item.rs` reproduces both, with batches of 3:

```rust,ignore
fn batches_zip(events: &mut impl Iterator<Item = u32>, size: usize) -> Vec<Vec<u32>> {
    let mut out = Vec::new();
    loop {
        let batch: Vec<u32> = events.by_ref().zip(0..size).map(|(e, _)| e).collect();
        if batch.is_empty() {
            return out;
        }
        out.push(batch);
    }
}
```

```text
zip,  Vec source       [[1, 2, 3], [4, 5, 6], [7, 8, 9], [10]]  published 10/10
zip,  channel source   [[1, 2, 3], [5, 6, 7], [9, 10]]  published 8/10
take, channel source   [[1, 2, 3], [4, 5, 6], [7, 8, 9], [10]]  published 10/10
```

Events 4 and 8 vanished in production, and the test couldn't see it. Here's why. `Zip::next` pulls from the *first*
iterator, then from the second. When the counter `0..size` is exhausted, the event already pulled from the first
iterator is dropped. [LIB] `zip`'s documentation allows this: after one side runs out, each further attempt may advance
the first iterator at most once more. Whether it *does* depends on the path. Listing `ch02-16-zip-probe.rs` isolates it:

```text
(a) Vec source,    for loop: [[1, 2, 3], [5, 6, 7], [9, 10], []]
(b) Vec source,    collect:  [[1, 2, 3], [4, 5, 6], [7, 8, 9], [10]]
(c) filter source, collect:  [[1, 2, 3], [5, 6, 7], [9, 10], []]
```

With a `Vec` source and `collect` (b), both sides report exact lengths, and std's internal-iteration path computes how
many pairs exist up front and never over-pulls. A `for` loop (a) calls `next()` and loses items even from a `Vec`. A
source without an exact length (c), like a `filter` or a channel, loses them under `collect` too. The test exercised
the one combination that happens to be safe.

The loss showed up as a 0.2% gap in the nightly published-vs-received reconciliation (the real batch size was 500:
one event per 501 pulled). It was dismissed for a week as "broker duplicates being deduplicated," until someone
compared event IDs.

The fixes:

1. **Use `take(n)`**, which checks its count *before* pulling. That's the third line above. Or use a purpose-built
   batching adapter (itertools' `chunks`, or reading from the channel with a bounded loop).
2. **Test with a source that has no exact length.** Wrapping the test's `Vec` iterator in `.filter(|_| true)` or
   `iter::from_fn` exercises the general path. A test that only feeds `Vec`s tests std's specialization, not your
   code.
3. **Reconcile counts end to end**, and treat unexplained gaps as data loss until proven otherwise. This bug was found
   by arithmetic, not by a stack trace.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part X).*

1. What is the one required method of `Iterator`, and why is its return type `Option<Self::Item>` rather than a
   `hasNext`/`next` pair?
2. What does "iterators are lazy" mean operationally? Describe the order in which a `filter().map().take(2)` pipeline
   processes items.
3. What are the three `IntoIterator` implementations for `Vec<T>`, and when would you pick each?
4. Can an iterator return `Some` after it has returned `None`? What do `fuse()` and `FusedIterator` do about it?
5. What is `size_hint` for? Why may unsafe code not trust it?
6. Why can't a std `Iterator` yield references into its own internal buffer? What breaks if it could?
7. How do GATs make a lending iterator possible, and what do you give up?
8. When would you return `impl Iterator`, a named iterator type, `Box<dyn Iterator>`, or a `Vec` from an API?
9. Why can `by_ref().zip(0..n)` lose items, and why might a test not notice?

### 12. Exercises

- **Beginner.** Write `fn evens(v: &[u32]) -> impl Iterator<Item = &u32> + '_`. Then write a version that takes a
  `Vec<u32>` and yields owned `u32`s. What changes in the signature?
- **Intermediate.** Implement `Iterator` for a `Fibonacci` struct that stops before overflowing `u64`. Give it an
  exact `size_hint` and `ExactSizeIterator`. How do you compute the exact remaining count?
- **Advanced.** Implement `DoubleEndedIterator` for `Frames`. What makes it hard? (Hint: can you find the last frame
  without scanning from the front?) Decide whether to implement it at all.
- **Systems.** Count allocations for reading 10,000 lines with `BufRead::lines()`, with `read_line` into a reused
  `String`, and with the lending reader. Then measure time for each in release on the Playground.
- **Architecture.** Ferrite's range scans (Project L10) must return keys and values in order from an LSM tree, without
  loading the range into memory. Sketch the scan API: `impl Iterator<Item = Result<(Vec<u8>, Vec<u8>), E>>`, a
  lending iterator over borrowed slices, or a visitor. What does each choice force on the storage engine's buffer
  management?

### 13. Debugging exercise

A config reader should skip the line after every `#continued` marker (listing `ch02-17-next-inside-for.rs`):

```rust,ignore
fn main() {
    let input = "a\n#continued\nskip-me\nb";
    let mut lines = input.lines();
    for line in lines.by_ref() {
        if line == "#continued" {
            lines.next(); // the for loop holds `lines` mutably borrowed for its whole duration
            continue;
        }
        println!("{line}");
    }
}
```

1. Predict the error code. Which two borrows conflict, and where does the compiler say the first one is "later used"?
2. Rewrite it so it compiles and prints `a` and `b`. Why does your version satisfy the borrow checker when the `for`
   loop didn't?
3. Rewrite it again without calling `next()` in the body at all (hint: `skip`-like logic with a flag, or
   `scan`/`filter` with state).

### 14. Design exercise

**Meridian's ledger statement export** (Chapter 9.2's design exercise: about 2M statements overnight, 50–5,000 lines
each) now needs an API between the ledger's storage layer and the PDF/CSV renderers. The storage layer reads from a
database cursor that fetches 1,000 rows per round trip. Design the interface:

- `fn statement_lines(account) -> impl Iterator<Item = Result<Line, LedgerError>>`, a lending iterator over a reused
  row buffer, or a visitor `for_each_line(account, |line| ...)`.
- Where the cursor's round trips happen, and what a renderer that stops early (a page limit) does to the cursor.
- How errors mid-statement surface (the half-rendered PDF problem).
- How you'd test it with a source that behaves like the real cursor (partial pages, a failure on page 3).
