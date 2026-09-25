# Chapter 12.4 — Pin and Self-Referential Futures

> **Where this sits:** Part XII · Async Rust · chapter 4 of 6
> **Prerequisites:** Chapter 12.3 (a future stores its live locals). Moves as bitwise copies (Chapter 3.2), raw
> pointers and `unsafe` (Chapter 1.3; Part XV goes deep), `noalias` (Chapters 3.3 and 4.1).
> **After this chapter you can:** explain why a future that borrows its own locals can't be moved; state exactly what
> `Pin` promises, who makes the promise and who relies on it; choose between `Pin::new`, `pin!`, and `Box::pin`; write
> (or reject in review) a pin projection; and explain what Pin's *drop guarantee* makes possible: waiter lists with no
> allocation per waiter.

---

## Pass 1 · User level — *Why a future can't be moved*

### 1. Problem

Chapter 12.3 established that a future stores every local that's alive across an `.await`. Now take an ordinary
`async fn`:

```rust,ignore
async fn checksum() -> usize {
    let data = [1u8, 2, 3, 4];
    let view: &[u8] = &data;   // a borrow of another local
    YieldOnce(false).await;    // both are live across this suspension...
    view.iter().map(|&b| b as usize).sum()
}
```

(Excerpt of listing `ch04-05-pin-projection.rs`.) Both `data` and `view` survive the `.await`, so both are fields of the
state machine, and `view` points at `data`. **The future contains a pointer into itself.** Writing code like this, with
borrows across `.await`, is what makes async Rust read like synchronous Rust. Take it away and every piece of state that
crosses an `.await` has to be owned, cloned, or put behind an `Arc`, which is what Rust's futures 0.1 era (2016–2018)
looked like.

The trouble is what a move is in Rust: a bitwise copy of the value to a new address (Chapter 3.2), with nothing fixed up.
Move a future that holds a self-reference, and `view` still points at the *old* `data`, in memory that now belongs to
someone else. A garbage collector would update the pointer as it moves the object. Rust has no GC and no move
constructors, so it needs a different answer: once a future may contain self-references, **it must not move again**.

That rule can't be enforced by the borrow checker directly. The borrow checker reasons about lifetimes of references,
not about "this value, from now on, stays at this address." `Pin` is that rule, expressed as a type.

### 2. Mental model

```text
 before the first poll                    after one poll                          after a MOVE (memcpy to a new address)
 ┌───────────────────────┐                ┌───────────────────────┐               ┌───────────────────────┐
 │ state: Unresumed      │                │ state: Suspend0       │  0x1000       │ state: Suspend0       │  0x2000
 │ (no locals yet)       │   poll ───►    │ data: [1, 2, 3, 4] ◄─┐│               │ data: [1, 2, 3, 4]    │
 │                       │                │ view: 0x1008 ────────┘│               │ view: 0x1008 ─────────┼──► old address:
 └───────────────────────┘                └───────────────────────┘               └───────────────────────┘    freed or reused
   safe to move: no self-references         must not move from here on               use-after-free on the next poll
```

`Pin<P>` wraps a pointer `P` (`&mut T`, `Box<T>`, ...) and carries a promise about the value it points to:

> **From the moment a value is pinned until its destructor runs, it will not be moved, and its memory will not be
> reused.** Unless its type is `Unpin`, in which case the promise means nothing.

Two parties are involved, and it helps to keep them apart:

| Party | Role | Examples |
|---|---|---|
| Whoever **creates** the `Pin` | Makes the promise | `Box::pin(fut)`, `pin!(fut)`, `unsafe { Pin::new_unchecked(&mut x) }` |
| The pinned **type's own code** | Relies on it | an `async fn`'s generated `poll`, which may create self-references; an intrusive list node |

`Unpin` is the opt-out: an auto trait meaning "this type doesn't care whether it moves." Almost everything is `Unpin`
(`u64`, `String`, `Vec<T>`, `Box<T>` for any `T`). A type is `!Unpin` if it contains `PhantomPinned` or a `!Unpin` field,
and **every future generated from `async` is `!Unpin`**.

### 3. Rust code

**The bug, without `Pin`.** Listing `ch04-01-selfref-dangling.rs` builds a hand-written self-referential parser: a
16-byte buffer and a `cursor` pointer into it. `Parser::new` sets the cursor, then returns the parser, which moves it:

```rust,ignore
fn new(src: &[u8]) -> Parser {
    let mut p = Parser { buf: [0; 16], cursor: std::ptr::null() };
    p.buf[..src.len()].copy_from_slice(src);
    p.cursor = p.buf.as_ptr(); // valid... until `p` moves
    p // returning moves it: `cursor` now points into this dead stack frame
}
```

Native run: `cursor points into buf: false`. Under Miri, the first dereference of the cursor:

```text
error: Undefined Behavior: memory access failed: alloc216 has been freed, so this pointer is dangling
  --> src/main.rs:23:18
   |
23 |         unsafe { *self.cursor }
   |                  ^^^^^^^^^^^^ Undefined Behavior occurred here
help: alloc216 was allocated here:
  --> src/main.rs:14:13
14 |         let mut p = Parser { buf: [0; 16], cursor: std::ptr::null() };
help: alloc216 was deallocated here:
  --> src/main.rs:18:5
18 |     }
```

Miri even names the moment the memory died: the closing brace of `new`, when the stack frame that held the original
`p` went away.

**The same struct, pinned.** Listing `ch04-02-pinned-selfref.rs` adds `PhantomPinned` (making it `!Unpin`), constructs
it *inside* a `Pin<Box<_>>`, sets the cursor only after pinning, and gives every method a pinned receiver:

```rust,ignore
fn new(src: &[u8]) -> Pin<Box<Parser>> {
    let mut p = Box::pin(Parser { buf: [0; 16], len: src.len(), cursor: std::ptr::null(), _pin: PhantomPinned });
    // SAFETY: we only write fields; nothing is moved out of the pinned Parser.
    let this = unsafe { p.as_mut().get_unchecked_mut() };
    this.buf[..src.len()].copy_from_slice(src);
    this.cursor = this.buf.as_ptr(); // set AFTER pinning: the address is now final
    p
}

fn peek(self: Pin<&Self>) -> Option<u8> { /* ... */ }
fn advance(self: Pin<&mut Self>) { /* ... */ }
```

`main` pushes the `Pin<Box<Parser>>` into a `Vec` (moving the box, a pointer, not the parser) and parses:

```text
cursor points into buf: true
parsed "GET /"
```

Miri reports nothing under Stacked Borrows or Tree Borrows (two `miri-ok` checks).

**What the type system now refuses.** Safe code can't get a `&mut Parser` out of a `Pin<&mut Parser>`, so it can't
`mem::swap`, `mem::replace`, or assign over it (listing `ch04-03-unpin-error.rs`):

```text
error[E0277]: `PhantomPinned` cannot be unpinned
   --> src/main.rs:15:42
    |
 15 |     let slot: &mut Parser = Pin::get_mut(a.as_mut()); // would allow `*slot = b`, a move
    |                             ------------ ^^^^^^^^^^ within `Parser`, the trait `Unpin` is not implemented for `PhantomPinned`
    = note: consider using the `pin!` macro
            consider using `Box::pin` if you need to access the pinned value outside of the current scope
note: required by a bound in `Pin::<&'a mut T>::get_mut`
```

**Futures are `!Unpin`, always.** An API that takes `F: Future + Unpin` rejects an `async` block that borrows a local
(listing `ch04-04-async-not-unpin.rs`). It also rejects one that borrows nothing and never awaits (listing
`ch04-09-trivial-async-not-unpin.rs`):

```rust,compile_fail
//! Every future from an `async` block or `async fn` is !Unpin, even one that holds no
//! references and never awaits [RUSTC]: the compiler doesn't analyze whether the state
//! machine actually borrows from itself.
fn is_unpin<T: Unpin>(_: &T) {}

fn main() {
    let fut = async { 1 + 1 }; // no borrows, no awaits
    is_unpin(&fut);
}
```

```text
error[E0277]: `{async block@src/main.rs:8:15: 8:20}` cannot be unpinned
 --> src/main.rs:9:14
  |
9 |     is_unpin(&fut);
  |     -------- ^^^^ the trait `Unpin` is not implemented for `{async block@src/main.rs:8:15: 8:20}`
  = note: consider using the `pin!` macro
          consider using `Box::pin` if you need to access the pinned value outside of the current scope
```

The compiler doesn't analyze whether a particular state machine actually borrows from itself; it marks them all
`!Unpin` [RUSTC]. The fix is in the note: pin it.

**Three ways to pin.** Listing `ch04-07-pin-kinds.rs` uses all three and checks that a local inside a pinned `async fn`
keeps its address across polls:

```text
Pin::new(&mut Ready): 7 after 1 poll
pin!:     local at the same address after 2 polls: true
Box::pin: local at the same address after 2 polls: true
sizes: Pin<&mut F> 8 B, Pin<Box<F>> 8 B
```

| Constructor | Where the value lives | Safe? | Use it for |
|---|---|---|---|
| `Pin::new(&mut x)` | anywhere | Yes, **only if `T: Unpin`** | `Unpin` futures (`Ready`, hand-written leaves); it pins nothing |
| `pin!(fut)` | the current stack frame | Yes | polling a future inside one function (`select` loops, `block_on`) |
| `Box::pin(fut)` | the heap (one allocation) | Yes | storing futures (task lists, struct fields), returning them, recursion |
| `Pin::new_unchecked(p)` | anywhere | **No**: you promise the pinning rules | executors, projections, self-referential types |

---

## Pass 2 · Systems level — *What Pin is, and what it isn't*

### 4. Under the hood

**`Pin` is an API restriction, not a runtime mechanism.** `Pin<P>` is a struct with one field, the pointer. It adds no
data (8 bytes for both `Pin<&mut F>` and `Pin<Box<F>>`, above) and no code. What it changes is which methods safe code
can call [LIB: `core::pin`]:

```text
 Pin<&mut T>
   ├── as_ref(), Deref          → &T                  always
   ├── get_mut(), DerefMut      → &mut T              only if T: Unpin
   ├── set(value)               → drops the old T in place, writes a new one   (allowed: not a move of the old value)
   ├── get_unchecked_mut()      → &mut T              unsafe: you promise not to move out of it
   └── map_unchecked_mut(f)     → Pin<&mut Field>     unsafe: you promise the field is structurally pinned
```

Every way to move a value out through a reference (`mem::swap`, `mem::replace`, `*r = new`, `Option::take`) needs a
`&mut T`. Withholding `&mut T` from safe code is the whole mechanism.

**Why `Future::poll` takes `self: Pin<&mut Self>`.** Before the first `poll`, a generated future is in `Unresumed` and
holds no self-references: moving it is fine, which is why you can build futures, return them from functions, and put them
in `Vec`s. The first `poll` may create self-references. So the trait makes every caller of `poll` prove that the future
is pinned, and from then on it stays put. That's also why Chapter 12.3's desugaring of `.await` could write
`Pin::new_unchecked(&mut awaitee)`: the awaitee is a field of the parent coroutine, the parent is pinned (its own `poll`
received `Pin<&mut Self>`), and the coroutine never moves its awaitee field. **Pinning is transitive through fields
that are *structurally* pinned.**

**The guarantee, precisely** [LANG: the `core::pin` module documentation]. Pinning has two halves:

1. **No moves.** A pinned value stays at its address until it's dropped.
2. **The drop guarantee.** Its memory is not invalidated or repurposed (freed, reused, overwritten) until its `drop`
   runs. If `drop` never runs (the value is leaked), the memory must stay valid *forever*.

The second half is easy to miss and does real work. `pin!(fut)` takes `fut` by value and hands you only a
`Pin<&mut F>` to a hidden local, so you can't `mem::forget` the future itself; the frame's end will drop it.
`mem::forget(Box::pin(fut))` is allowed, and it keeps the promise by leaking: the box's memory is never freed. What you
may not do is pin a value and then free or reuse its memory without running its destructor.

**What the drop guarantee buys: waiters with no allocation.** Listing `ch04-10-intrusive-waiters.rs` builds a small
`Notify`. Each waiting future contains a list node, and on its first poll it links **its own address** into the
notifier's list. There's no `Vec<Waker>` and no `Box` per waiter; the node is a field of the future:

```rust,ignore
fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
    let this = self.into_ref().get_ref(); // shared access is enough: all state is in Cells
    if this.node.notified.get() {
        return Poll::Ready(());
    }
    this.node.waker.set(Some(cx.waker().clone())); // refresh on every poll (Chapter 12.2)
    if !this.node.linked.get() {
        this.node.linked.set(true);
        this.node.next.set(this.notify.head.get());
        this.notify.head.set(&this.node); // our address escapes: fine, we're pinned
    }
    Poll::Pending
}
```

The future's `Drop` unlinks the node. Three waiters are polled, one is dropped (cancelled) before the notification, and
`notify_all` walks the list:

```text
a, b, c polled: Pending Pending Pending
linked waiters: 3
after dropping c: 2
notify_all woke 2
a, b polled again: Ready(()) Ready(())
size_of::<Notified>() = 40 bytes (the node is a field of the future: no Box, no Vec)
```

It passes Miri under both aliasing models. It's sound for exactly two reasons, the two halves of the guarantee: a
pinned `Notified` never moves, so the address in the list stays right; and its memory can't be reused before its `Drop`
runs, and `Drop` unlinks it. Listing `ch04-11-intrusive-no-unlink.rs` is the same code with the `Drop` impl deleted. It
still compiles. When `c`'s scope ends, its stack slot dies with its node still linked, and the next walk of the list
follows the stale pointer:

```text
a, b, c polled: Pending Pending Pending
linked waiters: 3
error: Undefined Behavior: constructing invalid value of type &Node: encountered a dangling reference (use-after-free)
  --> src/main.rs:41:28
   |
41 |             cur = unsafe { &*cur }.next.get();
   |                            ^^^^^ Undefined Behavior occurred here
```

This is the pattern behind production primitives: `tokio::sync::Notify`, its semaphore, and its timer entries keep waiters
in intrusive linked lists whose nodes live inside pinned futures [LIB]. Waiting on a Tokio mutex or semaphore doesn't
allocate per waiter. It's also why these futures are `!Unpin`.

**Pin projection and structural pinning.** A combinator such as a timeout holds an inner future. Its `poll` receives
`Pin<&mut Self>` and must call the inner future's `poll`, which needs `Pin<&mut Inner>`. Getting from one to the other is a
**pin projection**. The type's author decides, per field, whether pinning is *structural*: if `Self` is pinned, is this
field pinned too? For a structurally pinned field the `core::pin` documentation lists the obligations [LANG]:

1. The type may be `Unpin` only if all its structurally pinned fields are `Unpin`.
2. Its `Drop` impl must not move the field out (`drop` gets `&mut self`; treat it as pinned).
3. It must uphold the drop guarantee for the field: no freeing or reusing its memory without dropping it.
4. It must not offer any API that moves the field out while `Self` is pinned (an `Option::take` on it, say).
5. No `#[repr(packed)]` (the compiler may move packed fields to align them).

Listing `ch04-05-pin-projection.rs` writes the projection by hand for `WithBudget<F>`, a combinator that fails its inner
future after N polls:

```rust,ignore
struct WithBudget<F> {
    inner: F,        // structurally pinned: if WithBudget is pinned, so is `inner`
    polls_left: u32, // not pinned: plain data, freely mutable
}

impl<F: Future> Future for WithBudget<F> {
    type Output = Result<F::Output, BudgetExceeded>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: we never move `inner` out of `self`: no mem::swap/replace on it, no
        // `impl Unpin for WithBudget<F>` without `F: Unpin` (the auto trait gets this right),
        // no Drop impl that moves it, no #[repr(packed)]. `polls_left` is not structurally pinned.
        let this = unsafe { self.get_unchecked_mut() };
        if this.polls_left == 0 {
            return Poll::Ready(Err(BudgetExceeded));
        }
        this.polls_left -= 1;
        // SAFETY: `this.inner` is pinned because `self` is, and it stays where it is until dropped.
        let inner = unsafe { Pin::new_unchecked(&mut this.inner) };
        inner.poll(cx).map(Ok)
    }
}
```

Driving it with the self-referential `checksum` future from §1:

```text
budget 5: Ok(10)
budget 1: Err(BudgetExceeded)
```

Miri is clean. The SAFETY comment is long because the argument is global: it depends on what *other* code does with
`WithBudget`, not just on these lines.

**One safe line that breaks it.** Listing `ch04-06-wrong-unpin.rs` is the same combinator plus one line:

```rust,ignore
impl<F> Unpin for WithBudget<F> {} // BUG: a safe impl that contradicts the projection below
```

`Unpin` is a safe trait, so this compiles without `unsafe`. But it tells the world that `WithBudget<F>` may be moved
even after it's been polled, which is exactly what the projection's SAFETY argument ruled out. `main` polls it once in a
`Box` (the inner `checksum` future now points into the box), moves it out of the box, and polls again:

```text
first poll: Pending (the inner future now points into the box)
error: Undefined Behavior: constructing invalid value of type &[u8]: encountered a dangling reference (use-after-free)
  --> src/main.rs:27:5
   |
27 |     view.iter().map(|&b| b as usize).sum()
   |     ^^^^ Undefined Behavior occurred here
   = note: stack backtrace:
           0: checksum::{closure#0}
           1: <WithBudget<{async fn body of checksum()}> as std::future::Future>::poll
           2: main
```

The UB surfaces inside `checksum`, two layers away from the bug. This is the situation Part XV calls **unsafe code that
depends on safe code in the same module**: the `unsafe` block was correct when written, and a later safe edit
invalidated it. Nothing in the compiler connects the two.

**The safe alternative.** `pin-project-lite` (used by Tokio itself [LIB]) generates the projection and the correct,
*conditional* `Unpin` impl from a declaration. Listing `ch04-08-pin-project-lite.rs` has no `unsafe` at all:

```rust,ignore
pin_project_lite::pin_project! {
    struct WithBudget<F> {
        #[pin]
        inner: F,
        polls_left: u32,
    }
}

impl<F: Future> Future for WithBudget<F> {
    type Output = Result<F::Output, BudgetExceeded>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project(); // this.inner: Pin<&mut F>, this.polls_left: &mut u32
        if *this.polls_left == 0 {
            return Poll::Ready(Err(BudgetExceeded));
        }
        *this.polls_left -= 1;
        this.inner.poll(cx).map(Ok)
    }
}
```

```text
budget 5: Ok(10)
budget 1: Err(BudgetExceeded)
WithBudget<YieldOnce> is Unpin: true
```

The generated impl makes `WithBudget<F>: Unpin` exactly when `F: Unpin`, which is what the hand-written version got from
the auto trait and what `ch04-06` broke. And the `ch04-06` mistake can't be repeated through the macro: a manual
`impl<F> Unpin for WithBudget<F> {}` next to it conflicts with the generated one (listing
`ch04-13-ppl-no-manual-unpin.rs`):

```text
error[E0119]: conflicting implementations of trait `Unpin` for type `WithBudget<_>`
  = note: this error originates in the macro `$crate::__pin_project_make_unpin_impl` which comes from the expansion of
          the macro `pin_project_lite::pin_project`
```

### 5. Memory

`Pin` has no runtime representation; the question is only *where* the pinned value lives, because that's where it must
stay:

```text
 pin!(fut)               Box::pin(fut)                 inside a pinned parent
 ─────────               ─────────────                 ──────────────────────
 current stack frame     one heap allocation           a field of the parent's state machine
 ┌──────────┐            ┌──────────┐    ┌─────────┐   ┌───────────────────────────┐
 │ fut      │            │ Pin<Box> │───►│ fut     │   │ parent (pinned)           │
 │ (pinned) │            │ (8 B,    │    │(pinned) │   │   ├── awaitee (pinned)    │
 └──────────┘            │ movable) │    └─────────┘   │   └── locals              │
 lives until the         └──────────┘                  └───────────────────────────┘
 function returns        the handle moves freely;      pinned for free, as long as the parent is
                         the future never does
```

- **`pin!`** costs nothing but stack space, and the future can't outlive the frame. It's the choice inside a function
  that polls a future to completion: `block_on`, a `select` loop.
- **`Box::pin`** costs one allocation and gives a handle that can be stored, returned, or moved between threads. It's
  what executors do with spawned tasks [LIB], what `dyn Future` needs, and what breaks async recursion (Chapter 12.3).
- **Structural pinning** costs nothing: a child future stored inline in its parent is pinned because the parent is.
  That's how a whole call tree of `async fn`s becomes one pinned value with one allocation.

The intrusive waiter in `ch04-10` is 40 bytes (a waker slot, three `Cell`s, and a `&Notify`), stored inside the task
that waits. The alternative design, a `Mutex<Vec<Waker>>` in the notifier, needs heap memory proportional to the peak
number of waiters, allocated at the moment a burst arrives, and removal of a cancelled waiter is a search through the
vector. The intrusive list moves that memory into the futures, which already exist.

### 6. CPU / OS

**Pin compiles to nothing.** `Pin::new_unchecked`, `get_unchecked_mut`, and `as_mut` are identity functions on a
pointer. There's no check at run time, and the `ch04-07` polls ran exactly the code they would have run without `Pin`.

**It does change what the optimizer may assume.** Chapter 3.3 showed rustc marking `&mut T` parameters `noalias` for
LLVM: nobody else can observe or modify `*p` during the call. That's false for a pinned `!Unpin` value, which may be
pointed to from elsewhere (from itself, from an intrusive list) while a `&mut` to it exists. Listing
`ch04-12-noalias-pinned.rs` compiles the same function for an ordinary struct and for one containing `PhantomPinned`.
The release LLVM IR (`tools/emit.ps1 -Target llvm-ir -Mode release`; v0 symbols shortened to their demangled names):

```text
define noundef i64 @bump_plain(ptr noalias nofree noundef align 8 captures(none) dereferenceable(8) %p,
                               ptr noalias nofree noundef readonly align 8 captures(none) dereferenceable(8) %other)

define noundef i64 @bump_pinned(ptr noundef nonnull align 8 captures(none) %p,
                                ptr noalias nofree noundef readonly align 8 captures(none) dereferenceable(8) %other)
```

For `&mut Pinned`, rustc drops `noalias` *and* `dereferenceable` [RUSTC]. It's a deliberate compiler workaround: the
language has no first-class way yet to say "a `&mut` to this type may have aliases," and a proposal (`UnsafePinned`,
RFC 3467) aims to make that explicit [VERSION]. For this tiny function the machine code is identical
(`mov`/`inc`/`mov`/`add`/`ret` in both), because nothing in it depends on the missing guarantee. In larger functions,
losing `noalias` can mean extra reloads, the same effect Chapter 4.1 measured for `&Cell<i32>`. Pinned state machines
pay that price, invisibly, so that self-references are sound.

---

## Pass 3 · Architect level — *Why this design, and how to live with it*

### 7. Trade-offs

Rust had four ways to make borrowing futures possible, and `Pin` is the only one that costs nothing at run time:

| Design | How self-references stay valid | Cost | Who chose it |
|---|---|---|---|
| Forbid borrows across `.await` | They don't exist | Every piece of state is owned, cloned, or `Arc`ed; futures 0.1-era code | Rust 2016–2018 |
| Heap-allocate each suspended frame, never move it | Frames never move | An allocation per suspending call; no inline nesting | Kotlin, C# (Chapter 12.3 §8) |
| Move constructors | Code runs on every move to fix pointers | Every move of every type may run code; moves can fail | C++ (and rejected for Rust: Chapter 1.3's destructive moves) |
| **`Pin`** | The type system forbids moves after pinning | API complexity, `unsafe` projections, the `Unpin` loophole, lost `noalias` | Rust (stable in 1.33, 2019) |

The costs are real, and they fall on different people:

- **Application code** meets `Pin` rarely: `pin!(fut)` to poll a future in a loop, `Box::pin` to store or return one, and
  the E0277 "cannot be unpinned" note when an API wants `Unpin`.
- **Combinator authors** need projections. Use `pin-project-lite` (or `pin-project`); treat a hand-written
  `get_unchecked_mut` as a review flag.
- **Primitive authors** (runtimes, channels, synchronization) use the drop guarantee on purpose, for intrusive lists and
  in-place wakers. This is where `Pin` pays for itself, and where Miri in CI is mandatory.

### 8. Java comparison

In Java, objects move all the time. G1 evacuates live objects between regions and ZGC relocates them concurrently, and
the collector updates every reference to a moved object, including references an object holds to itself or to its own
fields' objects [RUNTIME: HotSpot]. A self-referential Java object costs nothing special, which is why Kotlin's
continuation objects (Chapter 12.3 §8) need no `Pin`.

Java does have "pinning," in two unrelated senses, and neither is Rust's:

| Java term | Meaning | Relation to Rust's `Pin` |
|---|---|---|
| GC pinning (JNI critical regions; G1 region pinning, JEP 423, JDK 22) | While native code holds a raw address of a Java array (`GetPrimitiveArrayCritical`), the GC must not move it; since JDK 22 G1 pins just that region instead of stalling collection [RUNTIME] | Close in spirit: "don't move this while a raw pointer to it exists." But it's temporary, runtime-enforced, and for the benefit of *native* code |
| Virtual-thread pinning (JDK 21) | A virtual thread can't unmount from its carrier thread while inside `synchronized` or a native frame, so it blocks the carrier; JEP 491 (JDK 24) removed the `synchronized` case [VERSION] | Unrelated: a scheduling limitation (Chapter 12.6) |

> **Analogy limit.** "`Pin` is like JNI's critical region" holds for the intent: someone holds a raw address, so the
> object must stay put. It fails everywhere else. JNI pinning is a runtime state the JVM enforces for a short window,
> and moving is the default. Rust's pinning is a compile-time promise that lasts until the value is dropped, enforced by
> withholding `&mut T`, and not moving is the default for everything that isn't `Unpin`. There's no runtime to consult
> and nothing is checked when the program runs.

### 9. Production scenario

**merchant-notify's merchant fan-out.** A large marketplace merchant can have hundreds of terminals and dashboards
connected at once, each served by its own connection task. When an event arrives for that merchant, every one of those
tasks must wake and send it. The pilot's first design kept a `Mutex<Vec<Waker>>` per merchant: each idle connection task
pushed its waker, and the event path drained the vector. Under load tests it had two problems. A cancelled connection
left a stale waker behind until the next event cleaned it up. And the vector's capacity grew with the largest burst of
connected terminals and stayed there, per merchant.

The team replaced it with `tokio::sync::Notify` plus a per-merchant event sequence number: each connection task awaits
`notified()`, then reads everything newer than the last sequence it sent. Notify's waiters are intrusive nodes inside
the pinned `notified()` futures, the `ch04-10` design [LIB], so an idle connection's wait costs the node inside a task
that already exists, and a cancelled connection's `Drop` unlinks its node immediately. The sequence number matters as
much as the primitive: a notification only says "look again," so a task that was busy sending when a notification
arrived still sees the new event on its next read. It's the waker contract from Chapter 12.2, at the application level.

Two rules came out of the review, both about `unsafe`:

1. **No hand-written pin projections in application crates.** Combinators use `pin-project-lite`; a
   `get_unchecked_mut` or `map_unchecked_mut` outside the runtime-primitives module fails review.
2. **The primitives module runs its tests under Miri in CI, with Stacked and Tree Borrows**, and every combinator test
   uses at least one self-borrowing `async fn` as the inner future. An inner future that is `Unpin` (a `ready(5)`, a
   hand-written leaf) can't detect a projection bug, because nothing breaks when it moves.

### 10. Failure scenario

**The `Deadline` that moved.** payments-core (Part VIII) has a small internal crate of async utilities. One of them,
`Deadline<F>`, wraps a future with a per-request deadline and was written with a hand-rolled projection, like
`ch04-05`. In February 2026 an engineer needed to poll a `Deadline` with `Pin::new` inside a manual `select` loop, hit
the E0277 "cannot be unpinned" error, and fixed it by adding `impl<F> Unpin for Deadline<F> {}`. The code reviewer saw a
one-line, `unsafe`-free change. Every test passed: the tests wrapped `ready()` futures and hand-written leaves, all of
them `Unpin`.

Weeks later, a batching change stored pending `Deadline`s in a `Vec` and removed completed ones with `swap_remove`, which
moves the last element into the vacated slot, between polls. Production futures were `async fn`s that borrowed their
own buffers. About once a day per pod, the process crashed with `SIGSEGV` inside the allocator, far from any async
code, and the core dumps showed corrupted heap metadata. Two weeks of investigation went into the allocator, the TLS
library, and a kernel upgrade.

The crash was found by running the utility crate's tests under Miri with a self-borrowing inner future, which reproduces
it deterministically: listing `ch04-06` is the reduction. The fixes:

1. Delete the `impl Unpin`; rewrite `Deadline` with `pin-project-lite`, whose `Unpin` impl is conditional.
2. Fix the call site properly: `pin!` the future, or store `Pin<Box<Deadline<F>>>` in the `Vec` (moving the box is fine).
3. Adopt the two review rules from §9, and add a `static_assertions`-style compile test that
   `Deadline<Pending-async-fn-future>` is `!Unpin` (a test that fails to compile if someone reintroduces the impl).

The lesson generalizes beyond `Pin`: **a safety argument that depends on the absence of some safe code is fragile.**
When you can, encode it so that the dangerous safe code doesn't compile (a conditional impl generated for you) rather
than relying on reviewers to remember a comment.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XII).*

1. Why can't a future that borrows one of its own locals across an `.await` be moved? Why isn't this a problem in Java
   or Kotlin?
2. State the `Pin` guarantee, both halves. Who makes the promise and who relies on it?
3. What is `Unpin`? Why is `Box<T>` `Unpin` even when `T` isn't? Why is every `async` future `!Unpin`?
4. Why does `Future::poll` take `Pin<&mut Self>`? Why is it fine to move a future before its first poll?
5. Compare `Pin::new`, `pin!`, and `Box::pin`: requirements, where the value lives, cost, and when you'd use each.
6. What is a pin projection? List the obligations for a structurally pinned field.
7. How can a single safe line (`impl<F> Unpin for X<F> {}`) cause undefined behavior? Who is at fault: the line, or the
   `unsafe` block elsewhere?
8. What does Pin's drop guarantee make possible? Describe an intrusive waiter list and why it needs both halves of the
   guarantee.
9. What does rustc change in the generated code for `&mut T` when `T: !Unpin`, and why?

### 12. Exercises

- **Beginner.** Take `ch04-04` and make it compile three ways: `pin!`, `Box::pin`, and by changing `poll_once` to
  accept `Pin<&mut F>`. For each, say where the future lives and whether it could be returned from `main`'s caller.
- **Intermediate.** Write `Fuse<F>`: a combinator that returns `Pending` forever after its inner future completes, so
  it's safe to poll again. Store the inner future as `Option<F>`. Write it once with `pin-project-lite` and once with
  `Pin::set(None)` on a projected `Pin<&mut Option<F>>` (`Option::as_pin_mut` helps). Why is `set(None)` allowed on a
  pinned field but `take()` isn't?
- **Advanced.** Implement `Select2<A, B>` without `Unpin` bounds that returns the unfinished future along with the
  winner's output (the Chapter 12.2 exercise). What must happen to the loser's pinning when you give it back? (Hint:
  can you return `B` by value if it has been polled? Compare with `futures::future::select`, which requires `Unpin`.)
- **Systems.** Using `ch04-10` as a base, measure (with the Part III counting allocator) allocations per waiter for the
  intrusive `Notify` and for a `Mutex<Vec<Waker>>` design, with 10,000 waiters arriving in one burst and being notified
  together. Then measure the cost of cancelling one waiter in the middle of each structure.
- **Architecture.** Your team maintains a crate of async combinators used by 30 services. Write the crate's `unsafe`
  policy for `Pin`: what's allowed, what tooling runs in CI (which Miri aliasing models, which test futures), and what a
  reviewer must check in any PR that touches a projection.

### 13. Debugging exercise

A team's `Retrying<F>` combinator passes all its tests, and Miri reports nothing, until someone adds a test whose inner
future is an `async fn` holding a borrow across an `.await`:

```rust,ignore
struct Retrying<F> {
    inner: Option<F>,
    attempts: u32,
}

impl<F: Future> Future for Retrying<F> {
    type Output = Option<F::Output>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: we don't move out of self.
        let this = unsafe { self.get_unchecked_mut() };
        let Some(mut fut) = this.inner.take() else { return Poll::Ready(None) };
        // SAFETY: `fut` is pinned for the duration of this call.
        match unsafe { Pin::new_unchecked(&mut fut) }.poll(cx) {
            Poll::Ready(v) => Poll::Ready(Some(v)),
            Poll::Pending => {
                this.inner = Some(fut);
                Poll::Pending
            }
        }
    }
}
```

1. Where does this code move a pinned future, and how many times per poll?
2. Why did the old tests (and Miri) not notice? What property must a test's inner future have to catch it?
3. Rewrite it with `Option::as_pin_mut` (or `pin-project-lite`) so the inner future is polled in place and dropped in
   place with `Pin::set(None)` when it completes.

### 14. Design exercise

**A zero-allocation semaphore.** merchant-notify wants to cap concurrent sends per merchant with
`limiter.acquire(merchant).await`, with no heap allocation per waiting task. Design the waiter structure:

- Where does each waiter's node live? Why must the `Acquire` future be `!Unpin`?
- What must `Acquire`'s `Drop` do in each state: never polled, waiting in the list, and "a permit was assigned to me but
  I was dropped before I was polled again"? (That last case is the classic lost-permit bug.)
- The list is shared between tasks on several threads. What lock protects it, and in what order do you take the list
  lock and wake a waker? (Chapter 12.2's production scenario has the rule.)
- How would you test it: which tests run under Miri, and which interleavings do you need to force?

Compare your design with `tokio::sync::Semaphore` in Part XIII.
