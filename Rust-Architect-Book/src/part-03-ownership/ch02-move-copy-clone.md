# Chapter 3.2 — Move, Copy, and Clone

> **Where this sits:** Part III · Ownership · chapter 2 of 6
> **Prerequisites:** Chapter 3.1.
> **After this chapter you can:** say exactly what `let b = a;` does for any type, at the language, MIR, and machine-code
> levels; decide when a type should be `Copy`; predict (and measure) what a clone costs; and move values out of places
> that don't allow a plain move.

---

## Pass 1 · User level — *Three ways to get a second value*

### 1. Problem

`let b = a;` is the most common line in any language, and it means something different in each:

| Language | What `b = a` does for an object | Result |
|---|---|---|
| Java | Copies the **reference** | `a` and `b` are the same object: two names, one mutable thing |
| C++ | Calls the **copy constructor** (deep copy), or the move constructor for `std::move(a)` | Two objects; or a moved-from `a` that's still alive |
| Rust | **Moves** the value (a bitwise copy plus invalidating `a`), unless the type is `Copy` | One owner; `a` is statically dead |

Most "fighting the borrow checker" in the first month is really misunderstanding this one line. Get it right and
ownership becomes mechanical.

### 2. Mental model

The spec'd picture, for a `String`:

```text
let a = String::from("hello");

 Stack                               Heap
 ┌────────────────────┐
 │ a  ptr ────────────┼───────────► "hello"   (cap 5, len 5)
 │    cap = 5         │
 │    len = 5         │
 └────────────────────┘

let b = a;                           ← MOVE: copy the three words, then `a` is dead

 Stack                               Heap
 ┌────────────────────┐
 │ a  ──X (moved)     │                         the compiler will reject any use of `a`
 ├────────────────────┤                         and will NOT drop it
 │ b  ptr ────────────┼───────────► "hello"   ← the SAME buffer: nothing on the heap was touched
 │    cap = 5         │
 │    len = 5         │
 └────────────────────┘

let c = b.clone();                   ← CLONE: an explicit deep copy

 │ b  ptr ────────────┼───────────► "hello"
 │ c  ptr ────────────┼───────────► "hello"   ← a NEW buffer: one allocation + a copy of the bytes
```

**Why must `a` die?** If both `a` and `b` stayed usable, both would own the same buffer, and both would free it at scope
end: a **double free** (Chapter 1.1's temporal class). If `a` were usable but `b` freed the buffer first, `a` would be
dangling: a **use-after-free**. If both could write, you'd have unsynchronized **aliasing**, which becomes a data race
across threads. Invalidating `a` removes all three at once, with no runtime cost, because it's decided at compile time.

**Copy vs Clone:**

| | `Copy` | `Clone` |
|---|---|---|
| How | Implicit: `let b = a;` duplicates, and `a` stays usable | Explicit: `a.clone()` |
| What | Always a **bitwise** copy of the value's bytes | Whatever the type's `clone` does (usually a deep copy) |
| Cost | Copying `size_of::<T>()` bytes | Arbitrary: allocations, copying, refcount increments |
| Who can be `Copy` | Types for which a bitwise copy **is** a complete, independent duplicate | Almost anything |

`Copy` types: integers, floats, `bool`, `char`, shared references `&T`, raw pointers, function pointers, and tuples,
arrays, and structs made only of `Copy` types (if they opt in with `#[derive(Clone, Copy)]`). **Not** `Copy`: `String`,
`Vec<T>`, `Box<T>` (a bitwise copy would duplicate the ownership of the heap data), and `&mut T` (a copy would create two
exclusive references, which is a contradiction). [LANG] A type can't be both `Copy` and have a `Drop` impl (error E0184,
Chapter 3.5). If a bitwise copy were a complete duplicate, there would be nothing to clean up.

### 3. Rust code

All three, with the heap activity **measured** by a counting allocator (listing `ch02-01-move-copy-clone.rs`):

```rust,ignore
#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    x: f64,
    y: f64,
} // Copy: plain data; a bitwise duplicate IS a complete, independent copy

fn total_len(names: Vec<String>) -> usize {
    // takes ownership
    names.iter().map(|n| n.len()).sum()
} // `names` is dropped here

fn main() {
    // Copy: the original stays usable.
    let p = Point { x: 1.0, y: 2.0 };
    let q = p;
    println!("p = {p:?}, q = {q:?}");

    // Move: ownership transfers; nothing on the heap is touched.
    let names: Vec<String> = (0..1_000).map(|i| format!("name-{i}")).collect();
    let (moved, allocs, _) = measure(|| {
        let moved = names;
        moved
    });
    println!("move a Vec<String> of 1000:  {allocs} allocation(s)");

    // Clone: an explicit deep copy.
    let (cloned, allocs, _) = measure(|| moved.clone());
    println!("clone it:                    {allocs} allocation(s)");

    // Give the clone away; it dies inside the function.
    let (len, allocs, frees) = measure(|| total_len(cloned));
    println!("total_len(cloned) = {len}: {allocs} allocation(s), {frees} free(s)");
    println!("the original is untouched: {} names", moved.len());
}
```

```text
p = Point { x: 1.0, y: 2.0 }, q = Point { x: 1.0, y: 2.0 }
move a Vec<String> of 1000:  0 allocation(s)
clone it:                    1001 allocation(s)
total_len(cloned) = 7890: 0 allocation(s), 1001 free(s)
the original is untouched: 1000 names
```

(The same numbers in debug and release builds.) A move of a thousand strings costs **zero** allocations: 24 bytes of
`Vec` header change hands. A clone costs **1,001**: one for the new vector's buffer, and one per string. Passing the
clone *by value* into `total_len` costs nothing going in, and 1,001 frees when the function's scope ends, because the
function became the owner.

**You can't move out of a place that would be left with a hole:**

```rust,compile_fail
fn main() {
    let names = vec![String::from("ada"), String::from("grace")];
    let first = names[0];
    println!("{first} {}", names.len());
}
```

```text
error[E0507]: cannot move out of index of `Vec<String>`
 --> src/main.rs:4:17
  |
4 |     let first = names[0];
  |                 ^^^^^^^^ move occurs because value has type `String`, which does not implement the `Copy` trait
  |
help: consider borrowing here
  |
4 |     let first = &names[0];
  |                 +
help: consider cloning the value if the performance cost is acceptable
  |
4 |     let first = names[0].clone();
  |                         ++++++++
```

If this were allowed, `names` would still own an element whose bytes had been moved out, and dropping `names` would free
that `String` a second time. The idiomatic ways to move out of a place leave something valid behind (verified):

```rust
use std::mem;

fn main() {
    let mut names = vec![String::from("ada"), String::from("grace"), String::from("linus")];

    let borrowed: &String = &names[0]; // borrow: nothing moves
    println!("borrowed {borrowed}");

    let taken = mem::take(&mut names[1]); // move out, leave String::default() behind
    let replaced = mem::replace(&mut names[2], String::from("ken")); // move out, put a value back
    let removed = names.swap_remove(0); // move out of the Vec itself (O(1); last element fills the gap)
    println!("taken={taken} replaced={replaced} removed={removed} left={names:?}");

    let mut slot: Option<String> = Some(String::from("session-42"));
    let owned = slot.take(); // Option::take: move out, leave None
    println!("owned={owned:?} slot={slot:?}");
}
```

```text
borrowed ada
taken=grace replaced=linus removed=ada left=["ken", ""]
owned=Some("session-42") slot=None
```

`mem::take`, `mem::replace`, `mem::swap`, `Option::take`, `Vec::swap_remove`/`remove`/`pop`: each **moves out while
keeping the container fully initialized**. They're the everyday tools for "I need to own something that currently lives
inside a structure I only borrowed." `String::new()`, which `take` leaves behind, doesn't allocate.

---

## Pass 2 · Systems level — *What a move compiles to*

### 4. Under the hood

**At the machine level, a move *is* a copy.** [LANG] A move copies the value's bytes, `size_of::<T>()` of them, and
makes the source unusable. The "unusable" part exists only in the compiler. Here's MIR for a function that builds a
tuple from a `u64` and a `String` (listing `ch02-06-move-codegen.rs`, debug build, `--emit=mir`):

```rust,ignore
pub fn pass_along(n: u64, s: String) -> (u64, String) {
    (n, s)
}
```

```text
fn pass_along(_1: u64, _2: String) -> (u64, String) {
    debug n => _1;
    debug s => _2;
    let mut _0: (u64, std::string::String);
    bb0: {
        _0 = (copy _1, copy _2);
        return;
    }
}
```

The `String` operand is spelled `copy _2`, not `move _2`. That isn't a bug. [RUSTC] In the MIR the **borrow checker**
analyzes, that operand is `move _2`, and the move is what makes any later use of `s` an error and cancels `s`'s drop
obligation. MIR operands are `copy` or `move`, and for a non-`Copy` type the borrow checker only accepts `move`. The MIR
printed by `--emit=mir` is *after* borrow checking, drop elaboration, and some optimization. Once those phases have used
the move information, a move and a copy are **the same machine operation**, and an optimization pass is free to
respell it. The ownership fact did its work at compile time and then got erased, just like lifetimes (Chapter 1.3).

And the machine code (release, rustc 1.98.1):

```text
playground::pass_along:                    ; rdi = hidden pointer to the result (sret), rsi = n, rdx = &s
	mov	rax, rdi
	mov	qword ptr [rdi], rsi               ; result.0 = n
	movups	xmm0, xmmword ptr [rdx]            ; copy the String's first 16 bytes
	movups	xmmword ptr [rdi + 8], xmm0
	mov	rcx, qword ptr [rdx + 16]          ; ...and its last 8
	mov	qword ptr [rdi + 24], rcx
	ret

playground::wrap:                          ; Wrapper { s, n: 1 }
	mov	rax, rdi
	mov	rcx, qword ptr [rsi + 16]
	mov	qword ptr [rdi + 16], rcx          ; copy the String's 24 bytes...
	movups	xmm0, xmmword ptr [rsi]
	movups	xmmword ptr [rdi], xmm0
	mov	qword ptr [rdi + 24], 1            ; ...and set n
	ret
```

**Moving a `String` costs exactly 24 bytes of copying**: one 16-byte SSE move plus one 8-byte move. No allocator call,
no touching of the characters. [RUSTC] Also note that the `String` argument arrived **by pointer** (`rdx`/`rsi`): values
too big for the argument registers are passed as a pointer to a caller-made temporary. At the machine level, "moving
into a function" often means "passing a pointer to the bytes, which the caller then treats as moved-from." Inlining
usually removes even that.

**Clone is just a trait method.** `x.clone()` calls `<T as Clone>::clone(&x)`. For `String` that's an allocation plus a
`memcpy` of `len` bytes. For `Vec<String>` it's one allocation for the buffer plus one `String::clone` per element,
which is the 1,001 measured above. For `Rc<T>` it's a refcount increment and **no** deep copy (Chapter 3.6). `Clone`
promises "an independent-enough duplicate," and each type decides what that costs. `#[derive(Clone)]` produces the
field-by-field version you saw expanded in Chapter 2.6.

**Moves in loops.** A value moved in one loop iteration is gone in the next, so the compiler rejects:

```rust,compile_fail
fn consume(s: String) -> usize {
    s.len()
}

fn main() {
    let payload = String::from("payload");
    let mut total = 0;
    for _ in 0..3 {
        total += consume(payload);
    }
    println!("{total}");
}
```

```text
error[E0382]: use of moved value: `payload`
  --> src/main.rs:10:26
   |
 7 |     let payload = String::from("payload");
   |         ------- move occurs because `payload` has type `String`, which does not implement the `Copy` trait
 8 |     let mut total = 0;
 9 |     for _ in 0..3 {
   |     ------------- inside of this loop
10 |         total += consume(payload);
   |                          ^^^^^^^ value moved here, in previous iteration of loop
   |
note: consider changing this parameter type in function `consume` to borrow instead if owning the value isn't necessary
  --> src/main.rs:2:15
   |
 2 | fn consume(s: String) -> usize {
   |    -------    ^^^^^^ this parameter takes ownership of the value
```

Read the note carefully. The compiler's *first* suggestion is to fix the API (`consume` doesn't need to own a string to
measure it), and cloning is only the fallback. That ordering is the right instinct to learn.

### 5. Memory

What each operation touches:

```text
                     stack bytes copied        heap: allocations    heap: bytes copied     source usable after?
 Copy (u64)          8                         0                    0                      yes
 Copy (Point)        16                        0                    0                      yes
 move (String)       24                        0                    0                      no (compile-time)
 move (Vec<String>)  24                        0                    0                      no
 move ([u8; 4096])   4096 (!)                  0                    0                      no
 move (Box<[u8;4096]>) 8                       0                    0                      no
 clone (String)      24                        1                    len                    yes
 clone (Vec<String>) 24                        1 + n                sum of lengths + 24n   yes
 clone (Rc<T>)       8                         0                    0 (count += 1)         yes
```

Two rows deserve attention. **A move's cost is `size_of::<T>()`, not the size of what it owns.** Moving a `Vec` of a
billion elements copies 24 bytes. But moving a `[u8; 4096]` array (or a struct containing one, or the 4 KiB enum
variant from Chapter 2.6) copies 4 KiB every time. That's why values that move often and are large *inline* belong
behind a `Box` (8 bytes to move). And `Rc::clone` is how you "copy" something expensive cheaply, by sharing it, which is
Chapter 3.6's subject.

### 6. CPU / OS

- **Copies of 24 bytes are essentially free**: a couple of instructions, usually in cache, often eliminated entirely
  when the optimizer builds the value directly in its final location.
- **Big inline moves are real memory traffic.** A 4 KiB `memcpy` is dozens to hundreds of cycles, and in a hot loop or
  a channel it shows up in profiles. Clippy's `large_types_passed_by_value` and `large_enum_variant` lints look for
  this.
- **Clones are allocator traffic plus cache misses.** Every allocation can contend on allocator state, and the new
  buffers are cold memory. Clone-heavy hot paths often show `malloc`/`free` and `memcpy` near the top of a flame graph.
- **`Arc::clone` is an atomic increment.** Cheap when uncontended; a shared cache line ping-ponging between cores when
  many threads clone the same `Arc` at a high rate (Chapter 1.3's counter example, again). Part XI covers it.

---

## Pass 3 · Architect level — *Move, borrow, clone, or share?*

### 7. Trade-offs

| You need... | Use | Cost | Watch out for |
|---|---|---|---|
| To hand the value to someone else for good | **Move** | `size_of::<T>()` bytes | Large inline types (box them) |
| Temporary access | **Borrow** (`&T` / `&mut T`, 3.3) | A pointer | Lifetimes tie the borrower to the owner |
| An independent copy you'll mutate separately | **Clone** | Allocation + copying | Hot paths; deep structures |
| Many readers of the same big value, with independent lifetimes | **`Rc`/`Arc`** (3.6) | A count per handle | Interior mutability, cycles, atomic contention |
| Small plain data passed around freely | **`Copy`** | Bytes | Only if a bitwise copy is *semantically* a copy |

**When should a type be `Copy`?** When it's small (a couple of machine words) and its identity doesn't matter, only its
value: coordinates, IDs, money amounts, timestamps, flags. **Don't** derive `Copy` just because you can. It's a
public-API commitment (removing it later is a breaking change), and implicit copies of a large struct happen silently
wherever it's used by value.

> **Why not `clone()` everything?** Because "clone to make the borrow checker happy" usually converts a *design*
> question (who should own this?) into a *run-time cost* (allocation and copying) that nobody sees until a profile
> shows it. Clone freely at cold boundaries: startup, configuration, error paths, and small values. In hot paths, treat
> a clone of a large value as a question to answer, not a fix.

### 8. Java comparison

| Java | Rust |
|---|---|
| `b = a` copies a reference; both names see every mutation | `b = a` moves; exactly one name owns the value |
| Aliasing is the default, so defensive copies are common (`new ArrayList<>(input)`, `List.copyOf`) | Aliasing of mutable data is impossible by default, so defensive copies are rarely needed |
| `Object.clone()` + `Cloneable`: shallow by default, famously awkward (*Effective Java*, Item 13) | `Clone`: an ordinary trait, derived field by field, deep by convention |
| Primitives are copied, objects are shared | `Copy` types are copied, everything else moves |
| Records are immutable, so sharing is safe | Immutability plus ownership: sharing is explicit (`&T`, `Rc`, `Arc`) |

The deeper difference is *why Java code copies*. A Java method that stores a list it received has to copy it, because
the caller still holds a reference and may mutate it later. That's a **defensive copy**. A Rust function that takes
`Vec<T>` by value *owns* it, and the caller provably can't touch it again, so no copy is needed. A function that takes
`&[T]` can't store it without cloning, and the signature tells the caller that too. Rust turns a convention ("copy on
the way in") into a type-level fact.

> **Analogy limit.** "A Rust move is like passing a Java reference and promising never to use the original" captures
> the zero-copy part. It misses the enforcement, which is the point, and it misses the machine-level truth that the
> *bytes* of the value (not a reference to it) are copied. For small values those bytes are the whole value, and no heap
> reference exists at all.

### 9. Production scenario

**Meridian's ingestion pipeline: ownership handoff instead of defensive copies.** The Java version of the event
pipeline (decode → validate → enrich → publish) copied each 64 KB message buffer twice: once when the validator stored
a snapshot for auditing, and once when the enricher, afraid the decoder would reuse its buffer, took a defensive copy.
In the Rust version each stage *takes ownership* of the message and passes it on:

```rust,ignore
fn validate(msg: Message) -> Result<Message, Rejected> { ... }   // owns it, returns it (or not)
fn enrich(msg: Message, geo: &GeoDb) -> Enriched { ... }         // consumes it, produces a new owner
fn publish(ev: Enriched, out: &mut Producer) { ... }            // consumes it
```

Every handoff is a move of a small header struct: the buffer's `Vec<u8>` is 24 bytes to move, however large the
payload. The auditor that needs a copy of *some* messages clones exactly those, explicitly. The allocation profile
dropped to one buffer per message plus the audited clones, and the reason is visible in the signatures, not in comments
saying "don't mutate this after passing it."

### 10. Failure scenario

**The per-request config clone.** A Meridian service wrapped its routing configuration in a struct and "fixed" a
borrow-checker error by cloning it into every request handler. It compiled, it passed review ("it's just a clone"), and
it shipped. Listing `ch02-05-config-clone.rs` measures what each design costs per request for a 1,000-route table:

```text
allocations per request: clone 2001, Arc 0, borrow 0
```

Cloning a `HashMap<String, String>` of 1,000 entries is 2,001 allocations: the table, plus a key and a value per entry.
At 20,000 requests per second that's 40 million allocations per second, and as many frees, for data that never changes.
The profile showed the allocator and `memcpy` dominating CPU, and the service needed several times more cores than its
work justified.

The fixes, in order of preference: **borrow** (`&Config`) if the handler's lifetime is contained in the owner's; **share**
(`Arc<Config>`) if handlers outlive the scope that owns the config, for example because they're spawned onto another
thread or task; clone **only** for data that's small or that the handler genuinely needs to mutate independently. The
review rule Meridian added: **a `clone()` of a collection inside a request path needs a comment saying why.**

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part III).*

1. What exactly does `let b = a;` do when `a` is a `String`? Answer at the language, MIR, and machine-code levels.
2. Why must the moved-from variable become unusable? Name the three bug classes this prevents.
3. What's the difference between `Copy` and `Clone`? Why can't a type be both `Copy` and `Drop`?
4. Why isn't `&mut T` `Copy`, when `&T` is?
5. What does moving a `Vec` of a billion elements cost? What does moving a `[u8; 4096]` cost? Why the difference?
6. Why is `let first = names[0];` rejected for a `Vec<String>`? Give three idiomatic alternatives and their costs.
7. When should a type derive `Copy`, and when should it deliberately not?
8. Why do Java codebases take defensive copies, and why do Rust codebases rarely need to?

### 12. Exercises

- **Beginner.** Predict, then check, which of these compile: moving a `Point` (`Copy`) twice; moving a `String` twice;
  moving a tuple `(u32, String)` and then using its `.0` field; moving `(u32, u32)` and using `.0`.
- **Intermediate.** Write `fn dedupe(names: Vec<String>) -> Vec<String>` that removes duplicates **without cloning any
  string** (hint: take ownership and move strings into the result). Measure its allocations with the counting
  allocator.
- **Advanced.** Partial moves: `let order = Order { ... }; let customer = order.customer;`. Which fields of `order` can
  you still use? Can you call a `&self` method on `order`? What does the compiler generate to drop the remaining fields?
  (Look at the MIR.)
- **Systems.** Write a function that moves a `[u8; 65536]` through three function calls, and another that moves a
  `Box<[u8; 65536]>`. Compare the release assembly: where are the `memcpy`s, and did the optimizer remove any?
- **Architecture.** Audit a Rust codebase you have access to (or a popular open-source crate) for `.clone()` calls on
  collections in hot paths. Classify each one: necessary, replaceable by a borrow, or replaceable by `Arc`.

### 13. Debugging exercise

The loop listing above fails with E0382: "value moved here, in previous iteration of loop."

1. Explain the error as an ownership argument. Who owns `payload` after the first iteration?
2. Give **three** fixes: change `consume`'s signature, clone per iteration, or restructure so the loop borrows. Measure
   the allocations of each with the counting allocator.
3. When is "clone per iteration" genuinely the right answer? Give a concrete example where `consume` must own its input.

### 14. Design exercise

**Message ownership in Meridian's fan-out.** The market-data fan-out (Chapters 1.1 and 2.6) sends each incoming 4 KB
frame to up to 500 subscribers, each with its own send queue. Compare:

- **clone per subscriber** (`Vec<u8>` cloned 500 times);
- **`Arc<[u8]>` per frame**, cloned (a refcount bump) into each queue;
- **a shared ring buffer** with per-subscriber read positions (indices, no per-subscriber copies);
- **`bytes::Bytes`** (a refcounted, sliceable buffer, Part XXII).

Analyze allocations per frame, memory when one subscriber is slow and falls 10,000 frames behind, the cost of the last
subscriber freeing a frame, and contention on the refcount. Pick one, and say what measurement would change your mind.
