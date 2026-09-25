# Chapter 11.2 — Send and Sync

> **Where this sits:** Part XI · Concurrency · chapter 2 of 7
> **Prerequisites:** Chapter 1.3 (`Send`/`Sync` first seen), Chapter 3.3 (aliasing XOR mutation), Chapter 4.1
> (`UnsafeCell`), Chapter 5.3 (`PhantomData`), Chapter 6.4 (`dyn Trait + Send + Sync`), Chapter 11.1.
> **After this chapter you can:** state `Send` and `Sync` precisely and derive either one for any type from its
> fields; read an E0277 thread-safety error down to the field that caused it; explain what these traits cost at run
> time (nothing) and what they let the compiler avoid (atomics you don't need); and decide when an `unsafe impl` is
> justified and what it obliges you to prove.

---

## Pass 1 · User level — *Thread safety as a property of types*

### 1. Problem

In Java, whether a class is safe to share between threads is a matter of documentation: a Javadoc sentence, a
`@ThreadSafe` annotation from *Java Concurrency in Practice* that nothing checks, or reading the source. Get it wrong
and the program compiles, runs, and corrupts data under load.

Rust makes thread safety a property of **types**, which the compiler checks at every point where a value crosses a
thread boundary. Chapter 1.3 showed the verdicts: `Rc` can't be sent, a counter needs an atomic or a `Mutex`. This
chapter explains the mechanism behind those verdicts, and the SPEC's chain of reasoning:

```text
Ownership            one owner; moving a value transfers it completely
      ↓
Borrowing            any number of &T  XOR  one &mut T   (aliasing XOR mutation, Chapter 3.3)
      ↓
Send / Sync          which of those may cross a THREAD boundary
      ↓
Thread safety        no data races in safe code: a compile-time guarantee
```

### 2. Mental model

Two marker traits, both **auto traits** [LANG]: the compiler implements them for your types automatically, based on
their fields.

- **`T: Send`**: a `T` may be **moved** to another thread. Ownership crosses, and the old thread keeps nothing.
- **`T: Sync`**: a `&T` may be **shared** with other threads. By definition, `T: Sync` exactly when `&T: Send`.

Aliasing XOR mutation answers why these are the right two questions:

```text
                 what crosses          what other threads can do meanwhile     so the requirement is
 move T          the value itself      nothing (they don't have it)            T: Send
 send &mut T     exclusive access      nothing (exclusive means exclusive)     T: Send
 share &T        shared access         also hold &T and READ, concurrently      T: Sync
```

Shared references are the danger zone, because "shared" normally means "immutable", and **interior mutability**
(Chapter 4.1) breaks that. A `&Cell<u64>` can write. So can a `&RefCell<T>` and an `&Rc<T>` (it bumps a count). None of
them synchronize, so none of them are `Sync`. Types that mutate through `&` *with* synchronization (`Mutex`, `RwLock`,
atomics) are.

**Auto traits compose structurally** [LANG]: a struct, enum, tuple, or closure is `Send` if all of its fields are
`Send`, and likewise for `Sync`. The exceptions are types that explicitly opt out (raw pointers, `Rc`, `Cell`,
`MutexGuard`) or explicitly opt in with `unsafe impl` (`Arc`, `Mutex`). One non-`Send` field makes the whole type
non-`Send`, however deep it's buried.

### 3. Rust code

Rather than memorize a table, ask the compiler. This listing reads each answer out of the type checker with a trick:
an inherent associated constant that exists only when the bound holds takes priority over a trait constant with the
same name (listing `ch02-01-auto-trait-table.rs`; the full source is in the listing):

```rust,ignore
struct Probe<T: ?Sized>(PhantomData<T>);

trait NotSend {
    const SEND: bool = false;
}
impl<T: ?Sized> NotSend for Probe<T> {}
impl<T: ?Sized + Send> Probe<T> {
    const SEND: bool = true;
}
```

The verified output (rustc 1.98.1):

```text
type                           Send?  Sync?
u64                            Send   Sync
String                         Send   Sync
Vec<u8>                        Send   Sync
&'static str                   Send   Sync
Box<dyn Fn()>                  -      -
Box<dyn Fn() + Send>           Send   -
Rc<u64>                        -      -
Arc<u64>                       Send   Sync
Cell<u64>                      Send   -
RefCell<u64>                   Send   -
OnceCell<u64>                  Send   -
&'static Cell<u64>             -      -
&'static mut Cell<u64>         Send   -
Arc<Cell<u64>>                 -      -
Mutex<Cell<u64>>               Send   Sync
RwLock<Cell<u64>>              Send   -
Mutex<Rc<u64>>                 -      -
MutexGuard<'static, u64>       -      Sync
OnceLock<u64>                  Send   Sync
AtomicU64                      Send   Sync
Sender<u64>                    Send   Sync
Receiver<u64>                  Send   -
*const u8                      -      -
PhantomData<*const ()>         -      -
```

Every row follows from the rules, and a few are worth deriving:

- **`Cell<u64>` is `Send` but not `Sync`.** Moving a `Cell` to another thread is fine, because the old thread no
  longer has it. Sharing it is not, because two threads could `set` at once.
- **`&Cell<u64>` is not `Send`**, because `&T: Send` requires `T: Sync`. **`&mut Cell<u64>` is `Send`**, because an
  exclusive reference is as good as ownership for the duration of the loan.
- **`Arc<Cell<u64>>` is neither.** `Arc<T>` is `Send` and `Sync` only if `T: Send + Sync` [LIB], because an `Arc`
  gives every clone a shared `&T`. Arc makes *ownership* shareable. It doesn't make the *contents* thread-safe.
- **`Mutex<Cell<u64>>` is both, but `RwLock<Cell<u64>>` is only `Send`.** A `Mutex` hands out one `&mut T` at a time,
  so it needs only `T: Send`. An `RwLock` lets many readers hold `&T` **at the same time**, so it needs `T: Sync` too.
- **`MutexGuard` is not `Send`.** Unlocking must happen on the locking thread (some platforms' mutexes require it
  [OS]), so the guard can't move. It is `Sync`, because sharing `&MutexGuard<T>` only gives out `&T`.
- **`Box<dyn Fn()>` is neither.** A trait object has *only* the auto traits you name. The compiler no longer knows the
  concrete type, so it can't infer them. That's why real APIs say `Box<dyn Fn() + Send + Sync>` (Chapter 6.4).
- **`Receiver` is `Send` but not `Sync`.** std's channel is single-consumer (Chapter 11.5). `Sender` became `Sync` in
  Rust 1.72 [VERSION].

The errors read the same chain in the other direction. `Arc` doesn't make a `RefCell` shareable (listing
`ch02-02-arc-refcell.rs`, the message trimmed):

```rust,compile_fail
use std::cell::RefCell;
use std::sync::Arc;
use std::thread;

fn main() {
    let seen = Arc::new(RefCell::new(Vec::<u32>::new()));
    let for_worker = Arc::clone(&seen);
    let h = thread::spawn(move || {
        for_worker.borrow_mut().push(1); // an unsynchronized borrow flag, touched from two threads
    });
    seen.borrow_mut().push(2);
    h.join().unwrap();
}
```

```text
error[E0277]: `RefCell<Vec<u32>>` cannot be shared between threads safely
    = help: the trait `Sync` is not implemented for `RefCell<Vec<u32>>`
    = note: if you want to do aliasing and mutation between multiple threads, use `std::sync::RwLock` instead
    = note: required for `Arc<RefCell<Vec<u32>>>` to implement `Send`
note: required because it's used within this closure
note: required by a bound in `spawn`
128 |     F: Send + 'static,
    |        ^^^^ required by this bound in `spawn`
```

Read the notes bottom up: `spawn` needs the closure to be `Send`, the closure contains the `Arc`, `Arc<X>: Send` needs
`X: Sync`, and `RefCell` isn't. The suggestion to use `RwLock` isn't the compiler's own reasoning. It's a note std
attaches to the `Sync` trait for `Cell` and `RefCell` types, through its on-unimplemented diagnostic attributes [LIB].

A guard can't move to another thread (listing `ch02-03-guard-not-send.rs`):

```rust,compile_fail
use std::sync::Mutex;
use std::thread;

fn main() {
    let balance = Mutex::new(100i64);
    let guard = balance.lock().unwrap();
    thread::scope(|s| {
        s.spawn(move || {
            let mut g = guard; // unlock would happen on this other thread
            *g -= 10;
        });
    });
}
```

```text
error[E0277]: `std::sync::MutexGuard<'_, i64>` cannot be sent between threads safely
```

---

## Pass 2 · Systems level — *Auto traits in the compiler, atomics in the CPU*

### 4. Under the hood

**The declarations** [LANG] [LIB]. In `core::marker`, the two traits are declared `unsafe auto trait Send {}` and
`unsafe auto trait Sync {}`. `auto` makes the compiler implement them structurally. `unsafe` means that writing an impl
by hand is a promise the compiler can't check. std opts types out with negative impls, an unstable feature std uses
internally (`impl<T: ?Sized> !Send for *const T {}`, and likewise for `Rc`, `Cell`'s `Sync`, and `MutexGuard`'s `Send`),
and opts others in with `unsafe impl` (`unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}`). On stable, your own code
opts *out* by containing a field that isn't `Send` or `Sync`. The idiom is `PhantomData<*const ()>`, the last row of the
table, and Chapter 5.3's steering table. You opt *in* with `unsafe impl`.

**Where the check happens** [RUSTC]. Nothing checks "thread safety" in general. What gets checked is **trait bounds**
at call sites: `spawn<F, T>(f: F) where F: FnOnce() -> T + Send + 'static, T: Send + 'static`, and the same bounds on
`Scope::spawn`, `rayon::join`, and `tokio::spawn`. The trait solver proves `F: Send` by looking through the closure's
**captured variables** (a closure is a struct whose fields are its captures) and then through each field's type. That's
why the error notes read "required because it's used within this closure" and "required because it appears within the
type `Job`": each note is one step of that proof.

**Captures are places, not variables** [VERSION]. Since edition 2021, a closure captures the *places* it uses
(`job.id`), not whole variables (`job`). That changes which types the solver checks. Chapter 5.3 showed it for a
phantom-typed struct, and here it is for a thread (listing `ch02-07-disjoint-capture.rs`, checked on both editions):

```rust
use std::rc::Rc;
use std::thread;

struct Job {
    id: u64,
    trace: Rc<String>, // a per-thread trace buffer: not Send
}

fn main() {
    let job = Job { id: 21, trace: Rc::new(String::from("req-7")) };
    let h = thread::spawn(move || job.id * 2); // 2021+: captures only `job.id`
    println!("result {} (trace {} stays on main)", h.join().unwrap(), job.trace);
}
```

```text
edition 2024:  result 42 (trace req-7 stays on main)
edition 2018:  error[E0277]: `Rc<String>` cannot be sent between threads safely
               note: required because it appears within the type `Job`
```

Under edition 2018 the closure captured all of `job`, including the `Rc`. Under 2021 and later it captures a `u64`. The
2021 behavior is more permissive and still sound, because only a `u64` crosses. But it can surprise you in the other
direction, as the next section's listing shows.

**An `unsafe impl` is checked only where it's used.** Here's a counter struct that someone declared `Sync` "because
it's only a metric" (listing `ch02-04-unsound-sync.rs`):

```rust
use std::cell::Cell;
use std::thread;

struct Stats {
    hits: Cell<u64>,
}

// WRONG: Cell has no synchronization. This impl claims `&Stats` may be shared across threads anyway.
// SAFETY: (none: this is the bug the listing demonstrates)
unsafe impl Sync for Stats {}

fn main() {
    let stats = Stats { hits: Cell::new(0) };
    let shared = &stats; // capture the reference itself: a `move` closure stops at the deref, so the
                         // bound checked is `Stats: Sync` (the unsafe impl), not `Cell<u64>: Sync`
    thread::scope(|s| {
        for _ in 0..2 {
            s.spawn(move || {
                for _ in 0..100 {
                    shared.hits.set(shared.hits.get() + 1); // read-modify-write, unsynchronized
                }
            });
        }
    });
    println!("hits = {}", stats.hits.get());
}
```

It compiles and runs. Miri runs it on an interpreter that tracks every memory access per thread, and reports the bug
(verified):

```text
error: Undefined Behavior: Data race detected between (1) non-atomic write on thread `unnamed-1` and
       (2) non-atomic read on thread `unnamed-2` at alloc214
   --> .../core/src/cell.rs:558:18
558 |         unsafe { *self.value.get() }
    |                  ^^^^^^^^^^^^^^^^^ (2) just happened here
help: and (1) occurred earlier here
  --> src/main.rs:23:21
23 |                     shared.hits.set(shared.hits.get() + 1); // read-modify-write, unsynchronized
```

A data race is **undefined behavior** in Rust [LANG], the same category as a use-after-free. It isn't just "a wrong
count". The optimizer may assume a `Cell` read by one thread isn't changed by another, and act on that. (The first
draft of this listing wrote `stats.hits` inside a non-`move` closure. It was **rejected**: disjoint capture captured
`&stats.hits`, a `&Cell<u64>`, so the solver checked `Cell<u64>: Sync` and never consulted the wrong impl on `Stats`.
The compiler was right. It just checked a different place than the author expected.)

The correct version needs no `unsafe` at all. `AtomicU64` is `Sync` on its own, and Miri reports no UB (listing
`ch02-05-sound-atomic.rs`, `miri-ok`, prints `hits = 200`).

**Library contracts in the type system.** Crates encode their threading rules the same way. `rusqlite::Connection` is
`Send` (move it to another thread) but not `Sync` (never use it from two at once), because it caches prepared statements
in a `RefCell`. The compiler finds both fields (listing `ch02-06-connection-not-sync.rs`, trimmed):

```text
error[E0277]: `RefCell<rusqlite::inner_connection::InnerConnection>` cannot be shared between threads safely
    = help: within `Connection`, the trait `Sync` is not implemented for `RefCell<...InnerConnection>`
error[E0277]: `RefCell<hashlink::lru_cache::LruCache<Arc<str>, rusqlite::raw_statement::RawStatement>>` cannot be
              shared between threads safely
note: required because it appears within the type `rusqlite::cache::StatementCache`
```

In Java, the equivalent fact about a JDBC `Connection` is a line in the documentation. Here it's a compile error at the
exact call that would have violated it.

### 5. Memory

`Send` and `Sync` have **no run-time representation** [LANG]. They're zero-sized facts the compiler proves and then
erases, like lifetimes. What they change is **which types you're allowed to use where**, and those types differ in
memory and in cost:

| Single-threaded (not `Send`/`Sync`) | Thread-safe counterpart | Layout difference |
|---|---|---|
| `Rc<T>`: `RcBox { strong: Cell<usize>, weak: Cell<usize>, value }` | `Arc<T>`: `ArcInner { strong: AtomicUsize, weak: AtomicUsize, data }` | Same shape; plain vs atomic counts |
| `Cell<u64>` (8 bytes) | `AtomicU64` (8 bytes) | Same size; plain vs atomic access |
| `RefCell<T>`: borrow flag + `T` | `RwLock<T>` / `Mutex<T>`: lock word(s), poison flag, `T` | Chapter 11.4 measures them |

Same bytes, different instructions. The type system's job is to let single-threaded code use the cheap column
**safely**. It can do that because the cheap types can't escape to another thread.

### 6. CPU / OS

**What `Arc` pays that `Rc` doesn't** [CPU]. Release assembly for cloning each (listing `ch02-08-rc-arc-asm.rs`, via
`tools/emit.ps1`):

```text
playground::clone_rc:
	mov	rax, qword ptr [rdi]          ; the RcBox pointer
	inc	qword ptr [rax]               ; strong += 1: a plain read-modify-write
	je	.LBB0_1                       ; wrapped to 0? abort (ud2)
	ret

playground::clone_arc:
	mov	rax, qword ptr [rdi]          ; the ArcInner pointer
	lock inc	qword ptr [rax]       ; strong += 1: an ATOMIC read-modify-write
	jle	.LBB1_2                       ; went past isize::MAX? abort (ud2)
	ret
```

The only difference is one prefix, `lock`. It makes the increment indivisible **across cores**. The core must own the
counter's cache line exclusively (the MESI "Modified/Exclusive" state from Chapter 3.3) for the duration. Uncontended,
that costs a few nanoseconds. Contended, it costs a trip for the cache line between cores on every operation. Measured
on the Playground (listing `ch02-09-arc-contention.rs`, release, one run):

```text
clone + drop, 1 thread,  Rc:                       1.1 ns
clone + drop, 1 thread,  Arc:                      3.5 ns
clone + drop, 4 threads, one shared Arc:        57.2 ns (wall time per op per thread)
clone + drop, 4 threads, one Arc per thread:     3.6 ns (wall time per op per thread)
```

Three lessons:

1. **`Rc` over `Arc` is a 3× saving on the refcount alone** when data really is thread-confined, and `Send`/`Sync` let
   you take it without risk.
2. **A shared `Arc` cloned on a hot path across threads is a contention point**, 16× slower here, even though nothing
   in the code looks "shared mutable". The refcount *is* shared mutable state. Chapter 11.4's `ArcSwap` and 11.7's
   measurements come back to this: hand out `&T`, or clone once per request, not once per operation.
3. **Per-thread `Arc`s cost the same as a single-threaded one.** Contention, not atomicity, is the expensive part.

---

## Pass 3 · Architect level — *Thread safety as API*

### 7. Trade-offs

**Auto traits are part of your public API, implicitly** [LANG]. If `pub struct Session` gains an `Rc` field, it stops
being `Send`, and every downstream `thread::spawn`, `tokio::spawn`, or `Arc<Session>` that relied on it breaks. The
change is **semver-major** even though no signature changed. `cargo-semver-checks` detects lost auto traits (Part XXII).
Design rule: **decide a public type's `Send`/`Sync` status on purpose**, and test it with a compile-time assertion such
as `const _: () = { fn assert_send<T: Send>() {} fn check() { assert_send::<Session>(); } };`.

**When `unsafe impl Send/Sync` is justified**, and what it obliges you to prove:

| Situation | Typical impl | What you must prove |
|---|---|---|
| Wrapper around a C handle that the C library documents as thread-safe | `unsafe impl Send + Sync` | The library's thread-safety contract, for every method you expose |
| C handle usable from any thread, but not concurrently | `unsafe impl Send` only | No `&self` method touches the handle without exclusive access |
| Your own lock-free structure built on raw pointers | `unsafe impl<T: Send> Send`, `unsafe impl<T: Send> Sync` | Every shared access is synchronized (atomics with the right orderings, Part XIV). Run it under Miri or loom (Part XV) |
| A type containing `Cell`/`RefCell` that "is only used from one thread" | **Don't.** | You can't: the impl lets *every* user share it. Use atomics, a lock, or `thread_local!` |

**Bounds to put on your own APIs:**

- Anything that spawns or stores work for other threads: `F: FnOnce() + Send + 'static` (or `'scope`).
- Errors that cross threads or `.await` points: `Box<dyn Error + Send + Sync>` (Chapter 8.2's reason).
- Trait objects shared across worker threads: `Arc<dyn Trait + Send + Sync>` (Chapter 6.4's middleware).
- Don't add `Send`/`Sync` bounds to purely single-threaded code. They take options away from your callers for nothing.

### 8. Java comparison

Java has no `Send` or `Sync`. Its closest concepts are the vocabulary of *Java Concurrency in Practice* (Goetz et al.,
2006), enforced by discipline:

| JCIP concept | Java mechanism | Rust counterpart | Checked by |
|---|---|---|---|
| Thread confinement | Convention; `ThreadLocal` | `!Send` types, `thread_local!` | Compiler |
| Safe publication | `final` fields, `volatile`, locks, concurrent collections | Moving ownership (`Send`) + join/channel happens-before | Compiler + std |
| Immutable objects | `final` fields, no setters, careful construction | `&T` where `T` has no interior mutability (`Sync` for free) | Compiler |
| Thread-safe class | `synchronized`, `java.util.concurrent` | `Sync` types: `Mutex`, `RwLock`, atomics, channels | Compiler + `unsafe impl` authors |
| `@ThreadSafe` / `@NotThreadSafe` | Annotations and Javadoc | `Sync` / `!Sync` | Compiler |

JIT compilers do infer some of this: HotSpot's escape analysis can remove locking on objects it proves never escape
their thread ("lock elision"). But that's an optimization based on what one run happens to do. It can't reject a program.

> **Analogy limit.** It's tempting to read `Sync` as "the class is annotated `@ThreadSafe`". The difference is where
> the claim lives and who checks it. A Java annotation is attached to a class, and nothing stops a caller from sharing
> an unannotated class anyway. A Rust `Sync` bound is attached to the **operation** that shares (`spawn`, `Arc::new` +
> send, `rayon::join`). Sharing a non-`Sync` value isn't discouraged; it doesn't compile. And the property is computed
> from fields, so a thread-unsafe field makes the whole type non-`Sync` without anyone writing anything.

### 9. Production scenario

**Meridian's gateway middleware and a plugin's cache.** The gateway shares its middleware chain across all worker
threads as `Arc<Vec<Box<dyn Middleware + Send + Sync>>>` (Chapter 6.4). A team writing a geo-blocking middleware added a
small lookup cache: a `RefCell<HashMap<IpPrefix, Country>>` field, the way they would have with a Java `HashMap` field.
Registration failed to compile. The E0277 notes pointed through `GeoBlock` to its `RefCell`, and the diagnostic suggested
`RwLock`.

The team considered three designs, each shaped by the traits:

- **`RwLock<HashMap<..>>`**: correct, but every request would write-lock on a cache miss, and the gateway serves 400K
  req/s at peak (Chapter 1.4).
- **`thread_local!` caches**, one per worker: no synchronization, `Cell`/`RefCell` allowed because the data never leaves
  its thread. That costs duplicate memory per worker, and the cache is warm only per thread.
- **A prebuilt, immutable prefix table** refreshed every few minutes and published with `ArcSwap` (Chapter 11.4):
  lock-free reads, no per-request writes.

They chose the third. The data was a slowly changing GeoIP database, not a cache that needed to learn per request. The
compile error didn't just prevent a race. It forced the question "who writes this, and how often?" before a single
request was served. The Java version of the same plugin, reviewed the year before, had shipped with an unsynchronized
`HashMap`, which is how the Chapter 1.2 "shared-map failure" table got its Java row.

### 10. Failure scenario

**"It's only a metric."** In the market-data fan-out's Rust port (the service from Chapter 1.1), a developer needed
per-feed counters shared by the decoder threads and the stats reporter. `AtomicU64` "seemed heavy", so they wrote
`struct FeedStats { msgs: Cell<u64>, gaps: Cell<u64> }` plus `unsafe impl Sync for FeedStats {}` with a comment:
"counters only; small errors acceptable".

Three things went wrong, in order:

1. **The counts were wrong, and not just a little.** Two decoder threads doing unsynchronized `get` + `set` lose
   increments. Under load the gap counter under-reported sequence gaps by about half, and gap alerts were tuned on it.
2. **It was undefined behavior, not an approximation.** A data race lets the compiler assume no other thread writes, so
   release builds could keep a counter in a register across a loop and publish a stale value. "Small errors" was never a
   property anyone could promise.
3. **Nothing flagged it** until the team adopted Chapter 4.6's "Miri in CI" rule for crates with `unsafe`. The Miri run
   failed with exactly the `Data race detected` report shown in §4.

The fix was the boring one: `AtomicU64` with `Relaxed` increments (a few nanoseconds uncontended, per §6), and a lint
rule in review: **every `unsafe impl Send` or `Sync` needs a `// SAFETY:` comment that names the synchronization
mechanism.** "Only a metric" names none, so it can't be written.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XI).*

1. Define `Send` and `Sync`. Why is `T: Sync` equivalent to `&T: Send`?
2. Using aliasing XOR mutation, explain why sending `&mut T` needs only `T: Send` but sharing `&T` needs `T: Sync`.
3. Why is `Cell<u64>` `Send` but not `Sync`? Why is `&mut Cell<u64>` `Send` but `&Cell<u64>` not?
4. Why does `Arc<T>` require `T: Send + Sync` to be `Send`? What does that say about `Arc<RefCell<T>>`?
5. Why is `Mutex<T>` `Sync` when `T: Send`, while `RwLock<T>` needs `T: Send + Sync`?
6. Why is `MutexGuard` not `Send`?
7. What is an auto trait? How does the compiler decide whether a closure passed to `thread::spawn` is `Send`?
8. How did edition 2021's disjoint capture change which types are checked for `Send`? Give an example in each direction.
9. What does an `unsafe impl Sync` promise? What does Miri report when the promise is false?
10. What do `Send`/`Sync` cost at run time? What do they let the compiler avoid, as seen in the `Rc`/`Arc` assembly?
11. Why can adding a private field be a semver-breaking change?

### 12. Exercises

- **Beginner.** Predict `Send`/`Sync` for: `(String, Rc<u8>)`, `Vec<Arc<Mutex<u64>>>`, `Option<&RefCell<u8>>`,
  `Box<dyn Error + Send>`, `fn(u64) -> u64`, `std::sync::mpsc::Sender<Rc<u8>>`. Then add them to
  `ch02-01-auto-trait-table.rs` and check.
- **Intermediate.** Write a `ThreadAffine<T>` wrapper that is `!Send` and `!Sync` on stable Rust, with a
  compile-time test proving it (a `compile_fail` doc test, or a listing expected to fail). Which `PhantomData` did you
  use, and why not `PhantomData<T>`?
- **Advanced.** Wrap a raw pointer to a C-allocated buffer in `struct Buf { ptr: *mut u8, len: usize }`. Decide which
  of `Send`/`Sync` it can soundly implement if (a) the buffer is only read after construction, (b) it has a `&self`
  method that writes. Write the `SAFETY` comments.
- **Systems.** Extend `ch02-09-arc-contention.rs` with 1, 2, 4, and 8 threads sharing one `Arc`. Plot the time per
  operation. Where does it stop scaling, and why does it get *worse* rather than flat?
- **Architecture.** Your public crate has `pub struct Client { inner: Arc<Inner> }`. A contributor wants to add a
  `RefCell<Vec<u8>>` scratch buffer to `Inner`. Write the review comment: what breaks for users, is it semver-major, and
  what are the alternatives?

### 13. Debugging exercise

This compiles on a teammate's machine (edition 2021) and fails in CI (a crate still on edition 2018):

```rust,ignore
struct Request {
    id: u64,
    user: std::rc::Rc<str>, // interned
}

fn audit_later(req: Request) {
    std::thread::spawn(move || println!("audit {}", req.id));
    println!("handled {}", req.user);
}
```

1. Read the 2018 error's notes and name the field that makes the closure non-`Send`.
2. Why does the 2021 build compile? What exactly crosses the thread boundary there?
3. The teammate later changes the closure to `move || log(&req)`. What happens on 2021, and why?
4. Propose a design where the audit thread gets what it needs without depending on capture rules.

### 14. Design exercise

**A thread-affine resource.** Meridian's fraud library calls a native scoring engine through FFI (Part XVI). The
engine's handle must be created, used, and destroyed on the same thread. Design the Rust API:

- the handle type and its auto traits (how do you make it `!Send` on stable, and how do you test that?);
- how the rest of the service, running on many worker threads, submits scoring requests (hint: Chapter 11.5's
  owner-thread pattern);
- what happens on a panic inside the engine thread, and how callers find out;
- how you would document and enforce the rule "never add `unsafe impl Send` to this type".

Compare it with how you'd enforce the same rule in Java.
