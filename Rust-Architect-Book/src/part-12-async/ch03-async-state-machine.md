# Chapter 12.3 — async fn Becomes a State Machine

> **Where this sits:** Part XII · Async Rust · chapter 3 of 6
> **Prerequisites:** Chapter 12.2 (the `Future` contract). MIR from Chapter 2.3; enum layout and niches from Part V.
> **After this chapter you can:** describe every step rustc takes to turn `async fn` into a `Future`; read a coroutine's
> layout in real MIR and its dispatch in real assembly; predict a future's size and its `Send`-ness; fix the four common
> compile errors of async Rust (non-`Send` futures, recursion, `dyn` traits, the Send bound problem); and explain what
> dropping a future at an `.await` does to your invariants.

---

## Pass 1 · User level — *What you write, and what you get*

### 1. Problem

Chapter 12.2's futures were written by hand: a struct, a `poll` method, state stored in fields, the waker registered at
the right moment. That's fine for a timer. It's hopeless for business logic. A request handler that parses a request,
checks a cache, calls two services, and writes a response would be a state machine with a dozen states and every local
variable promoted to a field.

`async fn` is the compiler doing that transformation. You write:

```rust,ignore
async fn step(x: u64) -> u64 {
    let a = x + 1;
    YieldOnce(false).await;
    let b = a * 2;
    YieldOnce(false).await;
    a + b
}
```

(Excerpt of listing `ch03-01-state-machine.rs`.) And rustc produces a type that implements `Future<Output = u64>`, with one state per `.await`, holding exactly the
variables that are still needed at that point. The brief for this Part asks what happens when `async fn foo() {}` is
compiled. This chapter answers it with the compiler's own output.

### 2. Mental model

```text
 async fn step(x)                     the generated state machine (conceptually an enum)
 ─────────────────                    ──────────────────────────────────────────────────
                                      Unresumed { x }            ← calling step(x) builds THIS; no code runs
 let a = x + 1;
 YieldOnce(false).await;  ─────────►  Suspend0  { a, awaitee }   ← saved: `a` (used later), the child future
 let b = a * 2;
 YieldOnce(false).await;  ─────────►  Suspend1  { a, b, awaitee }
 a + b                                Returned                   ← poll returned Ready; polling again panics
                                      Panicked                   ← a poll panicked; polling again panics
```

- **Calling an `async fn` runs nothing.** It moves the arguments into a value in the `Unresumed` state and returns it.
- **Each `.await` is a suspension point.** If the awaited future is `Pending`, the state machine records which
  suspension it's at and returns `Pending` from its own `poll`.
- **Each state stores what's live across that point**: locals that are used after it, plus the child future being
  awaited. Nothing else. Locals that die before the `.await` cost nothing.
- **A future's size is the size of its largest state** (with overlaps, below), plus a tag. It's known at compile time.

### 3. Rust code

**Sizes, measured.** Listing `ch03-02-future-sizes.rs` measures futures with `size_of_val`, in debug and release builds
(identical results):

```text
    1 B  async fn nothing()
   16 B  async fn add_one(x: u64)
   40 B  async fn step(x: u64), two awaits
 1026 B  1 KiB array live across an await
    8 B  1 KiB array dead before the await
   32 B  Vec<u8> of 1 KiB across an await
 1028 B  two 1 KiB children awaited in sequence
 2072 B  two 1 KiB children joined
   16 B  a 1 KiB child behind Box::pin
   16 B  Pin<Box<dyn Future<Output = u8>>>
```

Read it as a set of rules:

- `async fn nothing() {}` is **1 byte**: just the state tag. No arguments, no locals, nothing to save.
- An array that's **live across an `.await`** is stored in the future (1,026 bytes). The same array, **dead before the
  `.await`**, isn't (8 bytes).
- **Sequential awaits share space.** Two 1 KiB children awaited one after the other cost 1,028 bytes, not 2,052: the first
  child is gone by the time the second exists, so they overlap.
- **Concurrent children add up.** `join!` keeps both alive at once: 2,072 bytes.
- **Boxing cuts the size** to a pointer, at the price of an allocation.

**A hand-written equivalent.** Listing `ch03-03-hand-desugar.rs` writes `step` as an explicit enum with a hand-written
`poll`, and runs both versions:

```rust,ignore
/// One variant per suspension point, holding exactly what is live there.
enum Step {
    Unresumed { x: u64 },
    Suspend0 { a: u64, awaitee: YieldOnce },
    Suspend1 { a: u64, b: u64, awaitee: YieldOnce },
    Returned,
}
```

```text
sizes: compiler 40 B, hand-written 24 B
results: compiler 63, hand-written 63
```

The same behavior, and the compiler's version is 16 bytes larger. Section 4 shows why, from the compiler's own layout.

---

## Pass 2 · Systems level — *From source to MIR to assembly*

### 4. Under the hood

**Step 1: lowering (AST → HIR)** [RUSTC]. `async fn step(x: u64) -> u64 { body }` becomes, roughly,
`fn step(x: u64) -> impl Future<Output = u64> { async move { body } }`. The arguments move into the async block, the
return type becomes an opaque type, and every `.await` expands into a polling loop. Simplified from rustc's lowering:

```rust,ignore
// `fut.await` inside an async body, as rustc lowers it (simplified; the real code uses internal lang items)
match IntoFuture::into_future(fut) {
    mut awaitee => loop {
        match unsafe { Pin::new_unchecked(&mut awaitee) }.poll(task_context) {
            Poll::Ready(result) => break result,
            Poll::Pending => {}
        }
        task_context = yield (); // suspend; on resume, receive the NEW Context (Chapter 12.2's fresh waker)
    }
}
```

Two details in that desugaring matter. `Pin::new_unchecked(&mut awaitee)` is sound because `awaitee` is a local of the
coroutine, and the coroutine itself is pinned (Chapter 12.4). And the `yield` hands back a *new* context on every
resume, which is how every child future sees the current waker.

**Step 2: a coroutine type.** The async block's type is an anonymous **coroutine**: rustc's general machinery for
functions that can suspend (the same machinery behind the unstable `gen` blocks reserved in edition 2024) [RUSTC]. Its
**auto traits** (`Send`, `Sync`, `Unpin`) are computed from the types of the values stored across suspension points,
which the compiler records as the coroutine's *witness* types. That's the mechanism behind every "future is not `Send`"
error below.

**Step 3: the state transform (MIR).** After borrow checking, a MIR pass rewrites the coroutine body into a resume
function [RUSTC]. It computes which locals are live across each suspension, turns them into saved fields, builds one
variant per suspension point, and replaces each `yield` with "set the state, return `Pending`." Listing
`ch03-01-state-machine.rs` is `step` compiled as a library; its debug MIR (`tools/emit.ps1 -Target mir`) starts like this:

```text
fn step(_1: u64) -> {async fn body of step()} {
    debug x => _1;
    let mut _0: {async fn body of step()};

    bb0: {
        _0 = {coroutine@src/lib.rs:24:34: 30:2 (#0)} { x: copy _1 };
        return;
    }
}

fn step::{closure#0}(_1: Pin<&mut {async fn body of step()}>, _2: &mut Context<'_>) -> Poll<u64> {
    coroutine layout {
        field _s0: u64;
        field _s1: YieldOnce;
        field _s2: u64;
        field _s3: YieldOnce;
        variant_fields = {
            Unresumed(0): [],
            Returned (1): [],
            Panicked (2): [],
            Suspend0 (3): [_s0, _s1],
            Suspend1 (4): [_s0, _s2, _s3],
        }
        storage_conflicts = BitMatrix(4x4) {(_s0, _s0), (_s0, _s1), (_s0, _s2), (_s0, _s3), (_s1, _s0), (_s1, _s1), (_s2, _s0), (_s2, _s2), (_s2, _s3), (_s3, _s0), (_s3, _s2), (_s3, _s3)}
    }
```

This is the answer to "what does `async fn` compile to," in the compiler's words:

- `step` itself is three lines: build the coroutine value `{ x: copy _1 }`, return it. **Calling an async fn is a
  constructor.**
- The body lives in `step::{closure#0}`, a function taking `Pin<&mut Self>` and `&mut Context`, and returning
  `Poll<u64>`: the `Future::poll` signature.
- The layout names four saved locals: `_s0` is `a`, `_s1` and `_s3` are the two `YieldOnce` awaitees, `_s2` is `b`. The
  five variants are exactly the enum from §2, including `Panicked`.
- `storage_conflicts` lists which saved locals are alive at the same time. `_s1` (the first awaitee) and `_s2` (`b`)
  never conflict, so they may share bytes. This is how sequential awaits overlap in the sizes table.

The body dispatches on the state, and each suspension is a store and a return:

```text
    bb0: {
        _34 = move _2 as std::ptr::NonNull<std::task::Context<'_>> (Transmute);
        _33 = std::future::ResumeTy(move _34);
        _32 = copy (_1.0: &mut {async fn body of step()});
        _31 = discriminant((*_32));
        switchInt(move _31) -> [0: bb22, 1: bb21, 2: bb20, 3: bb18, 4: bb19, otherwise: bb6];
    }
    ...
    bb7: {
        _0 = Poll::<u64>::Pending;
        discriminant((*_32)) = 3;
        return;
    }
    ...
    bb16: {
        _30 = move (_29.0: u64);
        _0 = Poll::<u64>::Ready(move _30);
        discriminant((*_32)) = 1;
        return;
    }

    bb17 (cleanup): {
        discriminant((*_32)) = 2;
        resume;
    }
    ...
    bb21: {
        assert(const false, "`async fn` resumed after completion") -> [success: bb21, unwind continue];
    }
```

`bb7` is the first `.await` returning `Pending`: set the state to 3 (`Suspend0`) and return. `bb16` is completion: state
1 (`Returned`). `bb17` is the unwind path: if anything panics during a poll, the state becomes 2 (`Panicked`) before
the unwind continues. And `bb21` is what polling a `Returned` future does. Listing `ch03-15-resumed-after-completion.rs`
triggers it:

```text
first poll:  Ready(42)
thread 'main' (13) panicked at src/main.rs:7:26:
`async fn` resumed after completion
```

**Step 4: layout, and the 40 bytes.** The release assembly of the same resume function
(`tools/emit.ps1 -Target asm -Mode release`; the listing forces it to be emitted through a `Box<dyn Future>`) shows both
the dispatch and the field offsets:

```text
playground::step::{closure#0}:
	push	rbx
	mov	rbx, rdi
	movzx	eax, byte ptr [rdi + 16]          ; load the state tag (offset 16)
	lea	rcx, [rip + .LJTI1_0]
	movsxd	rax, dword ptr [rcx + 4*rax]
	add	rax, rcx
	jmp	rax                               ; jump table: one entry per state

.LBB1_1:                                      ; Unresumed
	mov	rax, qword ptr [rbx]              ; x      (offset 0)
	inc	rax
	mov	qword ptr [rbx + 8], rax          ; a = x + 1   (offset 8)
	mov	byte ptr [rbx + 24], 0            ; awaitee = YieldOnce(false)   (offset 24)
	jmp	.LBB1_5

.LBB1_4:                                      ; Suspend0: re-poll the first awaitee (inlined)
	cmp	byte ptr [rbx + 24], 0
	je	.LBB1_5
	mov	rax, qword ptr [rbx + 8]
	add	rax, rax
	mov	qword ptr [rbx + 32], rax         ; b = a * 2   (offset 32)
	mov	byte ptr [rbx + 24], 0            ; second awaitee reuses offset 24
	jmp	.LBB1_12

.LBB1_5:                                      ; YieldOnce's first poll: wake, then Pending
	mov	byte ptr [rbx + 24], 1
	mov	rax, qword ptr [rsi]              ; cx -> &Waker
	mov	rcx, qword ptr [rax]              ; waker vtable
	mov	rdi, qword ptr [rax + 8]          ; waker data
	call	qword ptr [rcx + 16]              ; vtable slot 2: wake_by_ref
	mov	byte ptr [rbx + 16], 3            ; state = Suspend0
	mov	eax, 1                            ; Poll::Pending
	pop	rbx
	ret
	...
.LJTI1_0:
	.long	.LBB1_1-.LJTI1_0                  ; 0 Unresumed
	.long	.LBB1_2-.LJTI1_0                  ; 1 Returned  -> "resumed after completion" panic
	.long	.LBB1_3-.LJTI1_0                  ; 2 Panicked  -> "resumed after panicking" panic
	.long	.LBB1_4-.LJTI1_0                  ; 3 Suspend0
	.long	.LBB1_11-.LJTI1_0                 ; 4 Suspend1
```

(Comments added; labels as emitted; the Suspend1 and panic blocks are trimmed.) Reading the offsets off the assembly:

```text
 offset  0      8      16    17..23   24         25..31   32      40
        ┌──────┬──────┬─────┬────────┬──────────┬────────┬───────┐
        │  x   │  a   │ tag │ (pad)  │ awaitee  │ (pad)  │   b   │   = 40 bytes
        └──────┴──────┴─────┴────────┴──────────┴────────┴───────┘
         prefix: upvar, promoted local, tag       per-state fields (both awaitees share offset 24)
```

[RUSTC] rustc lays a coroutine out as a **prefix** shared by all states (the captured arguments, the state tag, and any
saved local that's live in more than one state, like `a`) followed by per-state fields that may overlap. The argument
`x` stays in the prefix for the future's whole life, even though no state after `Unresumed` needs it, and the tag sits
in the middle of the prefix. A hand-written enum is laid out variant by variant, so the compiler packs `Suspend1 { a, b,
awaitee }` plus its tag into 24 bytes. Neither layout is guaranteed, and coroutine layout optimization is ongoing
compiler work; the lesson is *what* gets stored, not the exact byte count.

The same assembly answers the CPU question. A `.await` that returns `Pending` is a store of the state tag and a `ret`;
resuming is one indirect jump through a table. `YieldOnce::poll` was inlined into the state machine, and the only call
left is the waker's `wake_by_ref` through its vtable (Chapter 12.5 builds such a vtable by hand).

**Auto traits: why a future is or isn't `Send`.** A multi-threaded executor moves tasks between threads, so it requires
`Future + Send`. The coroutine is `Send` only if everything it stores across an `.await` is `Send`. Three verified
failures, each moving a future to another thread with `std::thread::spawn` (what a work-stealing runtime does):

```text
error: future cannot be sent between threads safely           (ch03-04-not-send-rc.rs)
   = help: within `{closure@src/main.rs:16:24: 16:31}`, the trait `std::marker::Send` is not implemented for `Rc<usize>`
note: future is not `Send` as this value is used across an await
  8 |     let counter = Rc::new(1usize);
  9 |     flush().await; // `counter` is live across this await: it's stored in the future

error: future cannot be sent between threads safely           (ch03-05-not-send-guard.rs)
   = help: ... the trait `std::marker::Send` is not implemented for `std::sync::MutexGuard<'_, u64>`
note: future is not `Send` as this value is used across an await
 11 |     let mut hits = HITS.lock().unwrap();
 13 |     flush().await; // the guard is still alive here

error: future cannot be sent between threads safely           (ch03-06-not-send-error.rs)
   = help: the trait `std::marker::Send` is not implemented for `dyn std::error::Error`
note: future is not `Send` as this value is used across an await
  9 |     let parsed: Result<u32, Box<dyn Error>> = s.parse::<u32>().map_err(|e| e.into());
 10 |     audit().await; // `parsed` (maybe an Err(Box<dyn Error>)) is live across the await
```

The third is the one Chapter 8.2 promised: `Box<dyn Error>` without `+ Send + Sync` is fine in synchronous code and
poisons any future that holds it across an `.await`. The fixes (listing `ch03-07-send-fixes.rs`) are to use `Arc`
instead of `Rc`, end the guard's scope before the `.await`, and use `Box<dyn Error + Send + Sync>`:

```text
handler -> 1
record  -> 1
parse   -> Ok(42)
parse   -> Err("invalid digit found in string")
```

The `MutexGuard` case is more than a type error. A `std::sync::Mutex` held across an `.await` stays locked for as long as
the task is suspended, however long the awaited operation takes, and any other task that locks it blocks its whole
executor thread. On a single-threaded executor, that other task can be the only thing that would let the first one
finish: a deadlock (the debugging exercise). The compile error is the compiler
catching a design bug. Part XIII covers `tokio::sync::Mutex`, which is designed to be held across `.await`, and why you
usually still shouldn't.

**Recursion.** A recursive `async fn` would contain its own future type inside itself: an infinitely sized type.

```text
error[E0733]: recursion in an async fn requires boxing              (ch03-08-recursion.rs)
 --> src/main.rs:3:1
  |
3 | async fn depth(n: u32) -> u32 {
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
4 |     if n == 0 { 0 } else { 1 + depth(n - 1).await }
  |                                ------------------ recursive call here
  |
  = note: a recursive `async fn` call must introduce indirection such as `Box::pin` to avoid an infinitely sized future
```

`Box::pin(depth(n - 1)).await` breaks the cycle (listing `ch03-09-recursion-boxed.rs`, with the counting allocator):

```text
future size: 16 bytes
depth(  1) =   1:   1 allocations,    16 bytes
depth( 10) =  10:  10 allocations,   160 bytes
depth(100) = 100: 100 allocations,  1600 bytes
```

One 16-byte allocation per level, on the heap instead of the stack. That's the "async doesn't remove the stack" point
from the Part IX interlude: recursion depth now costs heap memory and allocator calls, and an unbounded depth still needs
a limit.

**`async fn` in traits** (stable since 1.75) [VERSION]. Listing `ch03-10-async-trait.rs` uses three forms:

```rust,ignore
// 1. Static dispatch: each impl's `fetch` returns its own anonymous future type.
trait Fetch {
    async fn fetch(&self, id: u64) -> String;
}

// 2. The same method, desugared by hand, so that the trait can PROMISE callers a Send future.
//    Implementations may still write `async fn`; the compiler checks that their future is Send.
trait FetchSend {
    fn fetch(&self, id: u64) -> impl Future<Output = String> + Send;
}

// 3. dyn-compatible: every impl returns the same concrete type, a boxed, type-erased future.
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
trait DynFetch: Send + Sync {
    fn fetch(&self, id: u64) -> BoxFuture<'_, String>;
}
```

```text
("payments#1", "payments#2")
payments#8 (Send)
refunds#9 (Send)
payouts#9 (Send)
ledger#10 (Send)
```

Form 1 can't be used as `dyn Fetch`, which keeps an open question from Chapter 6.4:

```text
error[E0038]: the trait `Fetch` is not dyn compatible                (ch03-11-async-trait-dyn.rs)
 5 |     async fn fetch(&self, id: u64) -> String;
   = help: consider moving `fetch` to another trait
```

Every implementation's `fetch` returns a *different* anonymous type with a different size, so there's no single vtable
signature. Form 3 is the standard answer: return a boxed future (one allocation per call), which is what the
`async-trait` crate generates for you [LIB]. And form 1 has a second limit, the **Send bound problem**: generic code
can't require that the anonymous future is `Send`, because it has no name to put a bound on. Listing
`ch03-12-async-trait-send.rs` hits it, and rustc suggests form 2:

```text
error[E0277]: `impl futures::Future<Output = String>` cannot be sent between threads safely
help: `std::marker::Send` can be made part of the associated future's guarantees for all implementations of `Fetch::fetch`
  5 -     async fn fetch(&self, id: u64) -> String;
  5 +     fn fetch(&self, id: u64) -> impl std::future::Future<Output = String> + std::marker::Send;
```

[VERSION] "Return type notation" (RFC 3654), which lets a caller write `T: Fetch<fetch(..): Send>`, is unstable on
1.98. Until it lands, public traits meant for multi-threaded runtimes use form 2.

**What the returned future borrows.** The opaque future of an `async fn` captures every lifetime in its arguments
[LANG]: `fetch(&self, ..)` returns a future that holds `&self` from the moment it's created, polled or not, until it's
dropped. Listing `ch03-18-future-borrows-args.rs` creates the future and then mutates the service:

```text
error[E0502]: cannot borrow `svc.name` as mutable because it is also borrowed as immutable
16 |     let fut = svc.fetch(1); // not polled yet, but it already holds `&svc`
   |               --- immutable borrow occurs here
17 |     svc.name.push_str("-v2"); // mutation while the future's borrow is live
   |     ^^^^^^^^^^^^^^^^^^^^^^^^ mutable borrow occurs here
18 |     drop(fut);
   |          --- immutable borrow later used here
```

Form 2's `-> impl Future<Output = String> + Send` behaves the same way: return-position `impl Trait` in a trait
captures all in-scope lifetimes (it always has; edition 2024 extended the same rule to free functions, Chapter 7.3), so
switching between the two forms doesn't change what callers may do. A future that must outlive the borrow (to be spawned as `'static`, say) has to own its data: clone what
it needs into an `async move` block.

**Async closures** (stable since 1.85) [VERSION]. A closure returning an `async` block can't lend its captures to the
future it returns; an `async` closure can. Listing `ch03-13-async-closure.rs` borrows `endpoint` and `log` from the
enclosing scope inside the retried operation, bounded by the `AsyncFn` trait:

```text
Ok("ch_3")
  attempt 1 -> processor-eu-1
  attempt 2 -> processor-eu-1
  attempt 3 -> processor-eu-1
```

### 5. Memory

**A future is a value, and it can be huge.** Listing `ch03-16-big-future-overflow.rs` holds a 1 MiB render buffer across
an `.await`, and pins the future on the stack of a thread with 512 KiB of stack:

```text
future size: 1048579 bytes
thread '<unknown>' (45) has overflowed its stack
fatal runtime error: stack overflow, aborting
```

(Verified in debug *and* release builds. In release, the listing needs `std::hint::black_box` to stop the optimizer
from shrinking a buffer that's mostly zeros; real buffers don't get that luck.) The future overflowed the stack before
its first `.await`, because **constructing and pinning it on the stack needs its full size**. The fix
(`ch03-17-big-future-fixed.rs`) keeps the buffer on the heap:

```text
future size: 32 bytes
result: 7
```

Where a large future lives depends on the executor. Tokio boxes spawned tasks [LIB], so a big future costs heap, but
`block_on(fut)` and `pin!(fut)` put it on the current stack, and a future passed by value through several function calls
may be copied several times [RUSTC]. Futures nest by containment: a request handler's future *contains* the futures of
everything it's currently awaiting, recursively, so one large leaf makes every ancestor large.

**What dropping a future frees.** Dropping a suspended future runs the destructors of exactly the locals saved in its
current state [RUSTC]: the coroutine's drop glue switches on the state tag just as `poll` does. That makes cancellation precise, and it
makes it abrupt, which §10 is about.

### 6. CPU / OS

The state machine is plain code: a jump table, loads and stores into the future's memory, and calls into child futures,
which are usually inlined when the types are known (as `YieldOnce::poll` was). There are no allocations per `.await`, no
syscalls, no context switch. A task that awaits ten things in sequence and finds them all ready runs straight through, in
one `poll` call, without returning to the executor.

What *does* cost CPU:

- **Size.** A large future is copied when it moves before being pinned (into a `Box`, into a task), and its memory
  touches more cache lines. A 40-byte future fits in one line; an 80 KiB one doesn't fit in L1.
- **Deep nesting.** Each `poll` of the outer future walks down to the leaf that's actually waiting: a chain of calls
  through the states of every ancestor. It's usually cheap, but a combinator that polls *all* its children on every wake
  (a naive `join_all`) turns N wakes into N² child polls. Chapter 12.5's failure scenario measures it.

---

## Pass 3 · Architect level — *Designing with state machines you don't see*

### 7. Trade-offs

| Choice | Cost | When |
|---|---|---|
| `async fn` everywhere | Invisible future sizes; `Send`-ness decided by what crosses `.await` | The default for application code |
| Hand-written `Future` | You own the waker contract (Chapter 12.2) | Leaf primitives only |
| `Box::pin` a child | One allocation; the parent shrinks to a pointer | Recursion; rarely taken large branches; very large futures |
| `dyn Future` (boxed) | Allocation per call, dynamic dispatch | `dyn`-compatible traits, plugin boundaries, heterogeneous task lists |
| Buffers on the heap (`Vec`, `BytesMut`) | One allocation, reused | Anything larger than a few hundred bytes held across `.await` |

A useful discipline, borrowed from embedded Rust: **treat future size as a budget**. A test like
`assert!(size_of_val(&handler(...)) < 4096)` catches the day someone adds a buffer to a hot path. The Part XII review's
fixed code carries one (`deliver_future_is_small_and_send`).

**Cancellation safety** is the architectural cost of "cancellation = drop." Every `.await` in an `async fn` is a point
where the function may stop forever, with its saved locals dropped and nothing after it run. A function is
**cancellation-safe** if being dropped at any `.await` leaves the system in a valid state. Tokio documents cancellation
safety per method for exactly this reason [LIB].

### 8. Java comparison

Java has no `async`/`await`, but the JVM world has three relatives:

| | Kotlin `suspend fun` | C# `async` method | Rust `async fn` |
|---|---|---|---|
| Transformation | CPS: a hidden `Continuation` parameter; a state machine object with a `label` field and spilled locals | A compiler-generated state machine `struct` | A coroutine, lowered to a state machine in MIR |
| Allocation | A continuation object per suspending call, on the heap | The struct is boxed onto the heap the first time an `await` doesn't complete synchronously | None per call: children are stored *inside* the parent; one allocation per spawned task, if the executor boxes |
| Size known at compile time | No (heap objects) | No after boxing | **Yes** (`size_of_val`) |
| Moving after first suspension | Fine: the GC moves objects and fixes references | Fine: same | Forbidden: `Pin` (Chapter 12.4) |

Java's own answer, virtual threads (Chapter 12.6), avoided the transformation altogether: a blocked virtual thread's
stack frames are saved to the heap by the runtime, so ordinary blocking code becomes suspendable without the compiler
rewriting it and without function coloring.

> **Analogy limit.** "An `async fn` is like a Kotlin `suspend fun`" holds for the programming model and the CPS-shaped
> state machine. It breaks at memory: Kotlin allocates a continuation per suspending call and relies on the GC, while
> Rust nests the whole call tree's state into one value of known size, with no GC to move it, which is why Rust needs
> `Pin` and Kotlin doesn't.

### 9. Production scenario

**merchant-notify's future-size budget.** The pilot's per-connection task was a single `async fn connection_loop` that
read frames into a `[u8; 65536]` buffer and assembled outgoing batches in a `[u8; 16384]` buffer, both alive across the
`.await`s on the socket. Nobody had looked at its size until a load test at 100,000 connections showed RSS near 8 GB.
The arithmetic matched: about 80 KiB of future per connection. Most connections are idle, so almost all of it was
buffers that were never used.

The team made three changes:

1. **Buffers move to the heap and start small**: `Vec::with_capacity(512)`, grown for the rare large frame and shrunk
   back after it (a capacity policy, as in Chapter 9.1).
2. **A size test in CI**: `size_of_val(&connection_loop(...)) < 2048`, with a comment asking reviewers to justify any
   increase.
3. **Debug-build tests spawn through `Box::pin`.** A test that built the old future on a 2 MiB test thread had crashed
   with "has overflowed its stack" in debug builds only, which is listing `ch03-16` in production form.

The future shrank to a few hundred bytes plus per-connection heap buffers that only grow when a merchant is actually
receiving, and the fleet estimate for 100,000 connections dropped by more than an order of magnitude.

### 10. Failure scenario

**The payout that stayed reserved.** Meridian's marketplace payouts service (Rust) moves money from a marketplace's
escrow balance to a seller. Its first version reserved the amount, awaited a fraud check, then paid out. The API layer
wrapped every request in a timeout. When the fraud service slowed down, the timeout fired, the request future was
dropped between "reserve" and "pay out," and the reservation was never released. Listing `ch03-14-cancellation.rs`
reproduces it with a 10 ms timeout around a 50 ms fraud check:

```rust,ignore
async fn payout_v1(escrow: &RefCell<Escrow>, amount: i64) {
    {
        let mut e = escrow.borrow_mut();
        e.available -= amount;
        e.reserved += amount;
    }
    sleep_ms(50).await; // fraud check (slow today)
    let mut e = escrow.borrow_mut();
    e.reserved -= amount;
    e.paid_out += amount;
}
```

```text
v1 timed out: true; escrow Escrow { available: 700, reserved: 300, paid_out: 0 }
   reservation of 300 released by Drop (future cancelled)
v2 timed out: true; escrow Escrow { available: 1000, reserved: 0, paid_out: 0 }
```

In production, 212 sellers' payouts sat in "reserved" until a reconciliation job flagged them the next morning. The fix
(`payout_v2`) turns the reservation into a guard whose `Drop` releases it unless `commit` was called. It's the same RAII
pattern as Chapter 3.5's transaction guard, now applied to cancellation: **anything that must be undone if the future
is dropped mid-flight must be owned by a local whose destructor undoes it.** The team's review checklist gained one
line for every `async fn` that mutates shared state: *"What if this is dropped at each `.await`?"*

Two limits of the guard pattern belong in the same review. `Drop` can't be async, so a guard can only undo things that
can be undone synchronously (here, in-memory state; for a database, a rollback the driver performs when the connection
is returned). And a crash, as opposed to a cancellation, runs no destructors at all, which is why the reconciliation job
stayed.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XII).*

1. What happens, step by step, when `async fn foo() {}` is compiled? What does calling `foo()` do at run time, and why
   is its future 1 byte?
2. What determines the size of a future? Why do two children awaited in sequence cost 1,028 bytes but two joined
   children 2,072?
3. Walk through the MIR `coroutine layout` of `step`. What are `Unresumed`, `Returned`, and `Panicked` for?
4. Why is the compiler-generated `step` future 40 bytes when a hand-written enum is 24?
5. Why is a future that holds a `std::sync::MutexGuard` across an `.await` not `Send`, and why is that a *design* bug,
   not just a type error?
6. Why does recursion in an `async fn` require boxing? What does it cost per level?
7. Why can't a trait with an `async fn` be used as `dyn Trait`? What are the standard workarounds?
8. What is the Send bound problem, and how does `-> impl Future<Output = T> + Send` in the trait solve it?
9. What does "cancellation-safe" mean? Give an example of a function that isn't, and fix it.

### 12. Exercises

- **Beginner.** Predict, then measure, the size of an `async fn` that holds a `String` and a `u32` across one `.await`,
  and of one that holds `[u64; 4]` across two consecutive `.await`s. Check with `size_of_val`.
- **Intermediate.** Take a handler that awaits three services one after another and stores each response (1 KiB, 2 KiB,
  4 KiB) until the end. Measure its future. Restructure it so each response is reduced to the fields you need before the
  next `.await`, and measure again.
- **Advanced.** Write the enum-based state machine for an `async fn` with a `loop` containing one `.await` and a
  counter. How does a loop map onto states? Compare your size with the compiler's.
- **Systems.** Emit the MIR (`tools/emit.ps1 -Target mir`) of an `async fn` that holds a `String` across an `.await`, and
  find the coroutine's drop glue. How does dropping the future decide whether to drop the `String`?
- **Architecture.** Audit an async service you know for cancellation safety. List every `async fn` that mutates shared
  state across an `.await` and classify it: safe, safe with a guard, or needs a redesign (e.g., spawn the critical
  section so it can't be cancelled).

### 13. Debugging exercise

This compiles, runs correctly in every test, and deadlocks in production on a single-threaded runtime:

```rust,ignore
static CACHE: std::sync::Mutex<Option<Config>> = std::sync::Mutex::new(None);

async fn config() -> Config {
    let mut guard = CACHE.lock().unwrap();
    if guard.is_none() {
        *guard = Some(fetch_config().await); // an HTTP call
    }
    guard.clone().unwrap()
}
```

1. Why does it compile at all? (Where must it be running for the `Send` error not to fire?)
2. Describe the interleaving of two tasks that deadlocks the executor thread.
3. Fix it in two different ways, and state the trade-off: (a) no lock held across `.await`, accepting duplicate
   fetches; (b) a single-flight design in which later callers wait for the first fetch (Chapter 12.2's production
   scenario is one).

### 14. Design exercise

**A cancellation-safe transfer API.** The payouts team wants `transfer(from, to, amount).await` to be safe to cancel at
any point, including while the ledger write is in flight. Design it:

- Which steps can be undone by a synchronous `Drop` guard, and which can't?
- Would you make the critical section uncancellable by spawning it as its own task (so dropping the caller's future
  doesn't drop the work)? What does the caller then get back, and how does it learn the outcome?
- How do idempotency keys (Chapter 8.4) and a reconciliation job complete the design?

State the guarantee your API offers in one sentence, as it would appear in its doc comment.
