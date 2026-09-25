# Chapter 1.3 — Rust's Bet

> **Where this sits:** Part I · Why Rust Exists · chapter 3 of 4
> **Prerequisites:** Chapters 1.1 and 1.2.
> **After this chapter you can:** name each mechanism Rust uses to close each bug class; explain where the guarantees
> come from (language, compiler, or library signature); and state precisely what safe Rust guarantees and what it
> **doesn't**.

---

## Pass 1 · User level — *What are the rules, and what do they buy?*

### 1. Problem

Chapter 1.2 left one corner of the design space mostly empty: **enforced safety, full control, and no runtime**. To
fill it, a compiler has to know four things at compile time that C++ compilers don't know and Java runtimes only work
out at run time:

1. For every value: **who owns it**, meaning who frees it, exactly once.
2. For every reference: **what it points into, and for how long** that stays valid.
3. For every access: **whether anyone else could be writing** at the same moment.
4. For every type: **whether it's safe to hand to another thread**.

Rust makes you state (or lets the compiler infer) enough information to answer all four, and then it checks them. This
chapter is a map of those mechanisms. Parts III, IV, and XI build each one in depth.

### 2. Mental model

```text
 MECHANISM                             WHAT IT ESTABLISHES                        BUG CLASSES CLOSED
 ────────────────────────────────────  ─────────────────────────────────────────  ──────────────────────────────
 Ownership: one owner per value        exactly one party frees each value         double free; most "forgot to free"
 Destructive moves                     a moved-from name is statically dead       use-after-move; double free
 Borrowing: aliasing XOR mutation      no writer while readers exist               iterator invalidation; data races
 Lifetimes                             no reference outlives what it points to     dangling pointers; use-after-free
 Send / Sync (auto traits)             only thread-safe types cross threads        data races across threads
 Definite initialization               no read before the first write              uninitialized reads
 Bounds checks (at run time)           every index is checked                      out-of-bounds, which becomes a panic
 ────────────────────────────────────
 unsafe, encapsulated                  lets experts write what the checker can't prove, behind a safe API
 Zero-cost abstractions                none of the above adds runtime machinery beyond what you'd hand-write
```

The contract fits in one sentence:

> **In safe Rust, the compiler proves, one function at a time and using only signatures, that every access is to live,
> initialized, correctly typed memory, and that no two threads can mutate the same memory without synchronization.**
> Checks it can't do statically stay in at run time: bounds checks, overflow checks in debug builds, `RefCell` borrow
> flags, and locks.

### 3. Rust code

**Moves are destructive.** After ownership moves, the old name is dead, and the compiler enforces it:

```rust,compile_fail
fn main() {
    let a = String::from("hello");
    let b = a;
    println!("{a} {b}");
}
```

```text
error[E0382]: borrow of moved value: `a`
 --> src/main.rs:5:16
  |
3 |     let a = String::from("hello");
  |         - move occurs because `a` has type `String`, which does not implement the `Copy` trait
4 |     let b = a;
  |             - value moved here
5 |     println!("{a} {b}");
  |                ^ value borrowed here after move
  |
help: consider cloning the value if the performance cost is acceptable
  |
4 |     let b = a.clone();
  |              ++++++++
```

The compiler suggests `clone()` and attaches a condition: *if the performance cost is acceptable*. Rust makes copying
heap data an explicit, visible act, so it doesn't happen by accident.

**Destruction is deterministic.** The value is dropped when its owner goes out of scope, in reverse order of
declaration, and never for a moved-from name:

```rust
struct Connection {
    id: u32,
}

impl Drop for Connection {
    fn drop(&mut self) {
        println!("closing connection {}", self.id);
    }
}

fn main() {
    let a = Connection { id: 1 };
    {
        let _b = Connection { id: 2 };
        println!("inner scope ends");
    }
    let c = a; // ownership moves; `a` will NOT be dropped
    let _d = Connection { id: 3 };
    println!("main ends; `c` holds connection {}", c.id);
}
```

```text
inner scope ends
closing connection 2
main ends; `c` holds connection 1
closing connection 3
closing connection 1
```

Connection 1 is closed **once**, by its final owner `c`, and after `_d`, because `c` was declared first. There's no GC
and no `finally`. The destructor is part of the type, and the compiler inserts the call on every exit path.

**Data races don't compile.** Two threads incrementing one counter:

```rust,compile_fail
use std::thread;

fn main() {
    let mut counter: u64 = 0;
    thread::scope(|s| {
        s.spawn(|| {
            for _ in 0..1_000_000 {
                counter += 1;
            }
        });
        s.spawn(|| {
            for _ in 0..1_000_000 {
                counter += 1;
            }
        });
    });
    println!("counter = {counter}");
}
```

```text
error[E0499]: cannot borrow `counter` as mutable more than once at a time
   --> src/main.rs:12:17
    |
  7 |           s.spawn(|| {
    |                   -- first mutable borrow occurs here
  ...
 12 |           s.spawn(|| {
    |                   ^^ second mutable borrow occurs here
  ...
note: requirement that the value outlives `'1` introduced here
   --> .../library/std/src/thread/scoped.rs:203:35
    |
203 |         F: FnOnce() -> T + Send + 'scope,
    |                                   ^^^^^^
```

This is the same error code as two `&mut` borrows in single-threaded code. Rust has no separate "concurrency checker."
A data race is simply two mutable borrows that overlap in time, and the ordinary borrow rules reject it. (The `note`
points into the standard library. That's a clue to where the guarantee actually lives, which we'll get to in Pass 2.)

Here are three correct designs, verified in a release build:

```rust
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

const THREADS: u64 = 4;
const PER_THREAD: u64 = 1_000_000;

// Fix A: make the shared counter atomic.
fn with_atomic() -> u64 {
    let counter = AtomicU64::new(0);
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                for _ in 0..PER_THREAD {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    counter.into_inner()
}

// Fix B: put the counter behind a lock.
fn with_mutex() -> u64 {
    let counter = Mutex::new(0u64);
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                for _ in 0..PER_THREAD {
                    *counter.lock().unwrap() += 1;
                }
            });
        }
    });
    counter.into_inner().unwrap()
}

// Fix C: don't share. Each thread owns a private count; combine at the end.
fn with_no_sharing() -> u64 {
    thread::scope(|s| {
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                s.spawn(|| {
                    let mut local = 0u64;
                    for _ in 0..PER_THREAD {
                        local += 1;
                    }
                    local
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).sum()
    })
}

fn main() {
    println!("atomic:     {}", with_atomic());
    println!("mutex:      {}", with_mutex());
    println!("no sharing: {}", with_no_sharing());
}
```

```text
atomic:     4000000
mutex:      4000000
no sharing: 4000000
```

All three are correct, and they're very different architectures. The compiler accepted each of them because each one
makes the sharing *explicit*: an atomic type, a lock that owns its data, or no sharing at all. Fix C is the one an
experienced systems engineer reaches for first. **The cheapest synchronization is the synchronization you designed
away.** §6 explains why at the CPU level.

**Thread-safety is a property of types.** `Rc` (a non-atomic reference-counted pointer) can't be sent to another
thread:

```rust,compile_fail
use std::rc::Rc;
use std::thread;

fn main() {
    let shared = Rc::new(vec![1, 2, 3]);
    let clone = Rc::clone(&shared);
    let handle = thread::spawn(move || {
        println!("{:?}", clone);
    });
    handle.join().unwrap();
}
```

```text
error[E0277]: `Rc<Vec<i32>>` cannot be sent between threads safely
   --> src/main.rs:8:32
    |
    = help: within `{closure@src/main.rs:8:32: 8:39}`, the trait `Send` is not implemented for `Rc<Vec<i32>>`
note: required by a bound in `spawn`
   --> .../library/std/src/thread/functions.rs:128:8
    |
125 | pub fn spawn<F, T>(f: F) -> JoinHandle<T>
    |        ----- required by a bound in this function
...
128 |     F: Send + 'static,
    |        ^^^^ required by this bound in `spawn`
```

If two threads could hold clones of one `Rc`, both could update its plain (non-atomic) reference count at the same time
and corrupt it, so the value would be freed too early or never. The fix is `Arc`, whose count is atomic, and the type
system forces you to choose it.

**Safe doesn't mean "never fails."** Integer overflow is a good example:

```rust
use std::hint::black_box;

fn main() {
    let x: u8 = black_box(255);
    println!("wrapping_add:    {}", x.wrapping_add(1));
    println!("checked_add:     {:?}", x.checked_add(1));
    println!("saturating_add:  {}", x.saturating_add(1));
    println!("overflowing_add: {:?}", x.overflowing_add(1));
    println!("plain `+`:       {}", x + 1);
}
```

Release build:

```text
wrapping_add:    0
checked_add:     None
saturating_add:  255
overflowing_add: (0, true)
plain `+`:       0
```

Debug build: the first four lines print the same, then:

```text
thread 'main' (14) panicked at src/main.rs:11:37:
attempt to add with overflow
```

[LANG] Overflow is **never undefined behavior** in Rust. Plain arithmetic either panics or wraps (two's complement), and
the implementation must panic when debug assertions are on. [RUSTC] Today's default profiles panic in `dev` and wrap in
`release`. The `overflow-checks` profile setting controls this. The explicit methods say exactly what you mean:
`checked_*` for "tell me", `wrapping_*` for hashes and sequence numbers, `saturating_*` for counters and limits. Money
code should never rely on the default `+`.

---

## Pass 2 · Systems level — *Where do the guarantees come from, and what's left at run time?*

### 4. Under the hood

**A move is a shallow copy plus a compile-time death certificate.** [LANG] After `let b = a;`, `a` is uninitialized as
far as the language is concerned. [RUSTC] At the machine level the move copies the value's inline representation. For a
`String` that's three words: pointer, capacity, length. The heap buffer isn't touched. The compiler then (1) rejects
any later use of `a`, and (2) doesn't emit a destructor call for `a`. The optimizer usually removes the copy completely,
so `a` and `b` end up as the same registers or stack slot. When a value is moved on only *some* paths
(`if cond { consume(a) }`), rustc tracks it with a hidden boolean **drop flag** and checks it at scope end. That flag
is usually optimized away too.

**Drop order is part of the language.** [LANG] Locals are dropped in reverse order of declaration, and a struct's fields
are dropped in declaration order after the struct's own `Drop::drop` runs. The compiler generates **drop glue** for
every type that needs cleanup, and inserts it on every exit path: normal return, `?` early return, `break`, and panic
unwinding.

**Lifetimes are erased.** A lifetime is a compile-time fact about which region of the program a reference is valid for.
[LANG] Lifetimes never affect code generation. [RUSTC] They're checked on MIR and then erased before codegen, so no
lifetime information exists at run time. A `&'a T` is a plain pointer in the binary.

**Aliasing XOR mutation pays for itself in optimization.** [RUSTC] Because `&mut T` is guaranteed to be the only active
path to its target, rustc tells LLVM that the pointer is `noalias`, the same promise C's `restrict` makes but checked
instead of trusted. LLVM can then keep values in registers across stores through other pointers, reorder memory
operations, and vectorize loops that it would otherwise have to treat pessimistically. (History: mutable `noalias` was
disabled several times because it exposed LLVM miscompilation bugs that C, which rarely uses `restrict`, had never
triggered. It was re-enabled by default in 2021 once those bugs were fixed.)

**Thread safety is a library achievement built on two language features.** Look again at the two concurrency errors
above. Both `note`s point into std: `F: Send + 'static` on `thread::spawn`, and `F: Send + 'scope` on
`Scope::spawn`. The compiler has no special rules for threads. Instead:

- [LANG] `Send` and `Sync` are **auto traits**. The compiler implements them for a type automatically when all its
  fields do. `Rc<T>` opts out explicitly. Raw pointers opt out by default, so a type containing them must opt back in
  with an `unsafe impl` and take responsibility for the claim.
- [LANG] **Lifetime bounds** like `'static` and `'scope` say how long captured references must stay valid.
- [LIB] `thread::spawn` combines the two into a signature: *"give me a closure that owns its captures (`'static`) and is
  safe to move to another thread (`Send`)."*

That design is powerful. Tokio, Rayon, and crossbeam all use the same two features to state their own thread-safety
contracts, and the compiler enforces those contracts just as it enforces std's. **Rust's concurrency safety is
extensible by libraries.** Java's `synchronized`, by contrast, is baked into the language and the JVM.

**Zero-cost abstractions, verified.** Two ways to write "sum of squares of the even numbers":

```rust,ignore
pub fn sum_even_squares_loop(data: &[u64]) -> u64 {
    let mut total = 0;
    for i in 0..data.len() {
        let x = data[i];
        if x % 2 == 0 {
            total += x * x;
        }
    }
    total
}

pub fn sum_even_squares_iter(data: &[u64]) -> u64 {
    data.iter().filter(|&&x| x % 2 == 0).map(|&x| x * x).sum()
}
```

The iterator version builds a `Filter<Map<Iter<u64>>>` pipeline with two closures and a trait-based `sum`. In a
release build (rustc 1.98.1, x86-64), the hot loop of each function compiles to this:

```text
; sum_even_squares_iter                     ; sum_even_squares_loop
.LBB0_5:                                    .LBB1_8:
  mov    r9, qword ptr [rdi + 8*rcx]          mov    r9, qword ptr [rdi + 8*rcx]
  mov    r10d, r9d                            mov    r10d, r9d
  imul   r9, r9                               imul   r9, r9
  test   r10b, 1                              test   r10b, 1
  cmovne r9, r8                               cmovne r9, r8
  add    r9, rax                              add    r9, rax
  mov    rax, qword ptr [rdi + 8*rcx + 8]     mov    rax, qword ptr [rdi + 8*rcx + 8]
  mov    r10d, eax                            add    rcx, 2
  imul   rax, rax                             mov    r10d, eax
  test   r10b, 1                              imul   rax, rax
  cmovne rax, r8                              test   r10b, 1
  add    rax, r9                              cmovne rax, r8
  add    rcx, 2                               add    rax, r9
  cmp    rdx, rcx                             cmp    rdx, rcx
  jne    .LBB0_5                              jne    .LBB1_8
```

It's the same code with one instruction scheduled differently. Both loops are unrolled by two and branchless (`test` on
the low bit, then `cmovne` picks zero for odd numbers). The loop version's `data[i]` **bounds check has disappeared**,
because LLVM proved `i < data.len()` from the loop bounds. The closures, the adapter structs, and the `Iterator` trait
calls are all gone. **Monomorphization** (Part VII) generated code specialized for these exact closure types, and
**inlining** (Part X) folded it together.

> **What actually happens?** When I first compiled these two functions as a library to read the assembly, the *loop*
> version was missing from the output entirely. That wasn't a bug. [RUSTC] Since late 2023 (Rust 1.75), rustc treats
> small leaf functions as inlinable across crates and, like `#[inline]` functions, generates their code only in crates
> that call them. Marking both `#[inline(never)]` forced standalone copies, and that's what the comparison above shows.
> Lesson: *when you inspect generated code, make sure you're looking at what you think you're looking at.* Part XVIII
> explains codegen units and when rustc instantiates code.

What "zero-cost" does **not** mean:

- **Not zero compile-time cost.** Monomorphization and inlining make the compiler do the work. That's a large part of
  why Rust compiles slowly (Chapter 1.4).
- **Not zero cost in debug builds.** Without optimization, the iterator version really does call `next()`, `filter`, and
  closures through several layers. Debug Rust can be many times slower than release Rust, far more than the typical
  C debug/release gap.
- **Not "free."** Stroustrup's definition, which Rust adopts: *what you don't use, you don't pay for, and what you do
  use, you couldn't hand-code any better.* The abstraction costs nothing **relative to the hand-written equivalent**.
  The hand-written equivalent still costs what it costs.

### 5. Memory

A move, at the level of bytes (Part III expands this into a full model):

```text
let a = String::from("hello");            let b = a;

 stack                   heap              stack                   heap
 ┌──────────────┐                          ┌──────────────┐
 │ a.ptr  ──────┼──►┌─────────────┐        │ a  (dead: compile-time only; bytes may linger)  │
 │ a.cap  = 5   │   │ h e l l o   │        ├──────────────┤
 │ a.len  = 5   │   └─────────────┘        │ b.ptr  ──────┼──►┌─────────────┐
 └──────────────┘                          │ b.cap  = 5   │   │ h e l l o   │   same buffer,
                                           │ b.len  = 5   │   └─────────────┘   never copied
                                           └──────────────┘
 one owner, one future free               still one owner, still one future free (by b)
```

C++'s `std::move` looks similar, but the moved-from `std::string` is left in a "valid but unspecified" state. Its
destructor **still runs**, and the compiler lets you keep using it. Rust's move is **destructive**: the moved-from name
has no destructor call and can't be used. That difference is what lets Rust guarantee exactly one free without any
runtime bookkeeping.

The drop timeline of the `Connection` example:

```text
 time ──────────────────────────────────────────────────────────────────────────────────────►
 a = Conn(1) ─┐
              │   { _b = Conn(2) ── "inner scope ends" ── drop(_b) }
              └─ move ──► c ─────────────────────────────────────────────────────── drop(c) ► "closing 1"
                          _d = Conn(3) ──────────────────────── "main ends" ── drop(_d) ► "closing 3"
                                                                  (reverse declaration order: _d, then c)
```

### 6. CPU / OS

**The checks that remain at run time**, and what they cost:

| Check | Where | Cost at the machine level | Can the compiler remove it? |
|---|---|---|---|
| Bounds check on `v[i]` | Indexing slices, `Vec`, arrays | A compare and a well-predicted branch; can block vectorization | Often, as in §4. Iterators avoid it by construction. |
| Overflow check | `+ - *` in builds with `overflow-checks` (default in `dev`) | A flag check after the arithmetic | N/A. Off by default in `release`. |
| `RefCell` borrow flag | Interior mutability, single-threaded | A counter increment/decrement per borrow | Rarely |
| `Rc` / `Arc` counts | Shared ownership | Plain increment (`Rc`); atomic read-modify-write (`Arc`) | Sometimes elided |
| Locks | `Mutex`, `RwLock` | Uncontended: an atomic RMW. Contended: a futex syscall and a context switch. | No |

**Why Fix C beats Fix A beats Fix B, mechanically.** The ranking is *predicted from mechanism*. Measure it yourself in
the exercises before you trust it:

- **Fix A (atomic):** [CPU] On x86-64, `fetch_add` compiles to a `lock`-prefixed read-modify-write (`lock xadd`, or
  `lock add`/`lock inc` when the old value is unused). Each one needs exclusive ownership of the counter's
  **cache line**. With four cores hammering one line, it bounces between their caches (coherence traffic), and
  every increment waits for the line to arrive.
- **Fix B (mutex):** Each increment is a lock acquire and release (at least two atomic RMWs) on a contended line, plus
  futex syscalls and sleeps when the lock is taken. It's the same bouncing with more work per bounce.
- **Fix C (no sharing):** Each thread's `local` lives in a **register**. There's no shared line and no coherence
  traffic. [RUSTC] In a release build LLVM will typically go further and replace the loop with `local = 1_000_000` outright
  (check it in the assembly, as in the Advanced exercise). The
  only communication is one value per thread at `join`.

**Why `Ordering::Relaxed` is enough in Fix A.** Every atomic read-modify-write works on the latest value of that
variable, whatever the ordering argument, so no increment can be lost. The final read happens after `thread::scope`
returns, and joining a thread establishes a **happens-before** edge, so all increments are visible. Relaxed would **not**
be enough if the counter were used to *publish* other data ("when count reaches N, read the buffer"). That needs
Acquire/Release. Part XIV covers this carefully. Memory ordering deserves more than a paragraph.

**Stack overflow.** [OS] [RUSTC] On the major platforms each thread's stack has a **guard page** below it, and rustc
emits **stack probes** so that even a function with a large frame touches the guard page instead of jumping over it into
other memory. Unbounded recursion therefore ends in a clean abort with
`thread '...' has overflowed its stack`, not silent corruption. That behavior is platform-specific. It isn't a [LANG]
promise, and `no_std` targets may have no guard page at all.

**Panics.** [RUNTIME] By default a panic **unwinds**: it walks back up the stack, running destructors, which is why the
pool guard in §9 returns its connection even on panic. Unwinding needs landing pads and unwind tables (larger binaries,
some optimizations inhibited). With `panic = "abort"` in the profile, a panic kills the process immediately. That gives
smaller, slightly faster code and no destructors on the way out. Part VIII covers the trade-off.

---

## Pass 3 · Architect level — *What exactly is guaranteed, and what isn't?*

### 7. Trade-offs

The most important table in Part I. An architect who oversells Rust's guarantees will be embarrassed in production.

| Safe Rust **guarantees** there is none of... | Safe Rust does **not** prevent... |
|---|---|
| Use-after-free, double free, dangling references | **Memory leaks.** `mem::forget`, `Box::leak`, and `Rc` cycles are all safe (shown below). |
| Data races | **Race conditions.** Check-then-act across two lock acquisitions (shown below), and races with other processes, the database, or the filesystem. |
| Null dereference (references are never null) | **Deadlocks** (shown below) and livelocks |
| Reads of uninitialized memory | **Panics.** Out-of-bounds index, `unwrap()` on `None`, overflow in debug, a double `RefCell` borrow. Safe, but your request (or process) still dies. |
| Out-of-bounds access (it becomes a panic) | **Integer overflow** wrapping in release builds (defined behavior, possibly wrong) |
| Invalid values (a `bool` that's 3, a `&str` that isn't UTF-8, a bad enum discriminant) | **Stack overflow** and **out-of-memory**. Both abort (platform-dependent), neither is UB. |
| Type confusion (no unchecked casts; downcasting via `Any` is checked) | **Logic errors**: wrong business rules, wrong units, wrong time zones |
|  | **Bugs in `unsafe` code**: yours, your dependencies', the C libraries you link |
|  | **Distributed-system failures**: partitions, clock skew, duplicate delivery |

And the fine print that makes the left column conditional:

```text
             your safe code
                   │  "cannot cause UB"... provided that everything below is correct:
                   ▼
 ┌────────────────────────────────────────────────────────────────────┐
 │ unsafe code inside std          (Vec, HashMap, Arc, Mutex, ...)     │
 │ unsafe code in your dependencies                                    │   THE TRUSTED
 │ C/C++ libraries you link        (libc, OpenSSL, a database driver)  │   COMPUTING BASE
 │ rustc and LLVM                  (codegen must be correct)           │
 │ the OS and the hardware                                             │
 └────────────────────────────────────────────────────────────────────┘
```

Rust's promise is **soundness relative to that base**. The base has had bugs. The compiler itself has known soundness
holes, tracked under the `I-unsound` label. The most famous, rust-lang/rust#25860, has been open since 2015. It needs
deliberately contrived code to trigger and has never been reported in real code, but it's there. The architectural
consequence is to keep the trusted base **small and audited**. That's why `unsafe` is a keyword you can search for,
why tools like Miri (an interpreter that detects UB in `unsafe` code; Part XV) exist, and why supply-chain tools
(`cargo audit`, `cargo vet`) matter.

> **Why is leaking "safe"?** A true story that shaped the language. In early 2015, just before 1.0, it was found that a
> std API for scoped threads relied on a guard's destructor running to guarantee that borrowed data outlived the
> threads. Leaking the guard with an `Rc` cycle skipped the destructor and produced a use-after-free *in safe code*. The
> Rust team concluded that **a safe API must never rely on a destructor running for memory safety**, because destructors
> can always be skipped by leaking. They removed that API and made `mem::forget` a safe function (RFC 1066).
> Scoped threads came back in Rust 1.63 as `thread::scope`, redesigned so the *scope function* joins every thread
> before returning, whether or not any destructor runs. That's the `thread::scope` used throughout this chapter.
> The lesson for your own designs: **destructors are for resource management, not for soundness.**

> **Why not `clone()` everything?** It compiles, and it's often the right call for small values or at cold boundaries.
> In hot paths it turns ownership questions into allocation and copy costs. Worse, it hides the design question: *who
> should own this?* Treat a `clone()` on a large value in a hot path as a design smell to investigate, not a fix.

> **Why not `unsafe` when the borrow checker is in the way?** Because `unsafe` doesn't turn off the rules. It makes
> *you* responsible for upholding them, without help. A borrow-checker error is a signal that the design has an
> aliasing or lifetime conflict. Silencing the signal doesn't remove the conflict. Legitimate `unsafe` is rare, small,
> documented with a **safety invariant**, and wrapped in a safe API (Part XV).

### 8. Java comparison

**`Drop` vs `try-with-resources` vs finalizers.**

| | Java | Rust |
|---|---|---|
| Memory | GC, eventually | Owner's scope end, deterministic |
| Other resources (files, sockets, locks, pooled connections) | `try-with-resources` / `finally`, **only if you remember to write them** | `Drop`, **automatically, on every exit path** |
| "Destructor" | `finalize()` (deprecated for removal since JDK 18, JEP 421); `Cleaner` runs at a GC-determined time | `Drop::drop`, at a statically known point |
| Failure mode | Leak on the path where someone forgot the `try` | Leak only if you explicitly `forget`, `exit`, abort, or create a cycle |

Rust **unifies memory and resource management**. In Java they're two separate systems: the GC for memory, and your
discipline for everything else. In Rust, closing a file, releasing a lock, and freeing memory are all the same
mechanism.

**`synchronized` vs `Mutex<T>`. The lock protects code in one, and owns data in the other.**

```java
class Inventory {
    private final Map<String, Integer> stock = new HashMap<>();

    synchronized void reserve(String sku) { stock.merge(sku, -1, Integer::sum); }

    int available(String sku) { return stock.getOrDefault(sku, 0); }   // forgot `synchronized`: a data race
}
```

That compiles, passes code review often enough, and passes tests. The lock and the data are **associated only by
convention**. In Rust the lock *contains* the data:

```rust
use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;

struct Inventory {
    stock: Mutex<HashMap<String, i64>>, // the lock OWNS the data it protects
}

impl Inventory {
    fn reserve(&self, sku: &str) {
        let mut stock = self.stock.lock().unwrap();
        *stock.entry(sku.to_string()).or_insert(0) -= 1;
    }

    fn available(&self, sku: &str) -> i64 {
        // There is no path to the map that skips the lock.
        self.stock.lock().unwrap().get(sku).copied().unwrap_or(0)
    }
}

fn main() {
    let inventory = Inventory {
        stock: Mutex::new(HashMap::from([("sku-1".to_string(), 1_000)])),
    };
    thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..100 {
                    inventory.reserve("sku-1");
                }
            });
        }
    });
    println!("available: {}", inventory.available("sku-1"));
}
```

```text
available: 600
```

The Java bug above **can't be expressed** here. The only way to reach the `HashMap` is through `lock()`, and the guard
releases the lock when it's dropped.

**`final` vs Rust immutability.** Java's `final` is *shallow*: a `final List<X>` can't be reassigned, but its contents
can change freely. Rust's immutability is *inherited*. Through a non-`mut` binding or a `&T` you can't mutate the value
**or anything it owns**. The one deliberate exception is **interior mutability** (`Cell`, `RefCell`, `Mutex`, atomics),
which is opt-in, visible in the type, and still checked (Part XI).

> **Analogy limit.** "`Drop` is like `try-with-resources`" holds for *when cleanup runs*. It fails for *who has to
> remember*. In Java the **caller** must remember the `try`. In Rust the **type** remembers, and every caller gets it
> for free. And "`Mutex<T>` is like `synchronized`" holds for mutual exclusion. It fails for association: Java can't
> stop you touching guarded data without the lock, and Rust makes it impossible.

### 9. Production scenario

**Meridian's connection-pool exhaustion.** A Java payments service ran out of database connections at 03:10 one
morning. Every request thread was blocked in `pool.getConnection()`. The cause was a code path added two weeks earlier:
an early `return` inside a retry branch that skipped the `connection.close()` in a hand-written `finally` block, which
someone had later refactored in a way that bypassed it. Each failed retry leaked one connection. With 50 connections in
the pool and roughly 20 retry failures an hour, the pool drained overnight.

In Rust, a pooled connection is a **guard** whose `Drop` returns it to the pool. Every exit path returns it, including
`?` and panics:

```rust
use std::cell::RefCell;
use std::panic::{self, AssertUnwindSafe};

struct Pool {
    idle: RefCell<Vec<u32>>,
}

// A checked-out connection. It borrows the pool, so it cannot outlive it.
struct PooledConn<'p> {
    id: u32,
    pool: &'p Pool,
}

impl Pool {
    fn get(&self) -> Option<PooledConn<'_>> {
        let id = self.idle.borrow_mut().pop()?;
        Some(PooledConn { id, pool: self })
    }
}

impl Drop for PooledConn<'_> {
    fn drop(&mut self) {
        self.pool.idle.borrow_mut().push(self.id); // runs on EVERY exit path
    }
}

fn handle(pool: &Pool, fail: bool) -> Result<(), String> {
    let conn = pool.get().ok_or("pool exhausted")?;
    if fail {
        return Err(format!("query on conn {} failed", conn.id)); // early return: still returned
    }
    Ok(())
}

fn main() {
    let pool = Pool { idle: RefCell::new(vec![1, 2]) };

    let failures = (0..10).filter(|i| handle(&pool, i % 3 == 0).is_err()).count();
    println!("requests: 10, failed: {failures}");

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let _conn = pool.get().unwrap();
        panic!("bug in handler"); // unwinding runs _conn's destructor
    }));
    println!("handler panicked: {}", result.is_err());
    println!("idle connections: {}", pool.idle.borrow().len());
}
```

```text
requests: 10, failed: 4
handler panicked: true
idle connections: 2
```

A pool of two connections served ten requests (four of them failing early) and a panicking handler, and ended with both
connections idle. Real pools (`deadpool`, `bb8`, `sqlx`'s pool) work the same way. The honest caveats: `Drop` does
**not** run on `std::process::exit`, on `panic = "abort"`, or for a guard you deliberately `mem::forget`. And in async
code, a cancelled future is dropped mid-flight, which runs these destructors at points you might not expect (Part
XIII).

### 10. Failure scenario

Three programs that **compile cleanly** and are still wrong. Rust's guarantees stop exactly here, and so should your
confidence.

**(a) A race condition with no data race.** Every access is synchronized, and the program is still wrong:

```rust
use std::sync::{Barrier, Mutex};
use std::thread;
use std::time::Duration;

struct Account {
    balance: Mutex<i64>,
}

impl Account {
    // BUG: a race condition, not a data race. Every access is synchronized,
    // but the check and the act happen under two separate lock acquisitions.
    fn withdraw_racy(&self, amount: i64) -> bool {
        let current = *self.balance.lock().unwrap(); // lock, read, unlock
        if current >= amount {
            thread::sleep(Duration::from_millis(1)); // "real work" between check and act
            *self.balance.lock().unwrap() -= amount; // lock again: the world may have changed
            true
        } else {
            false
        }
    }

    // Correct: check and act under one guard.
    fn withdraw(&self, amount: i64) -> bool {
        let mut balance = self.balance.lock().unwrap();
        if *balance >= amount {
            *balance -= amount;
            true
        } else {
            false
        }
    }
}

// Eight threads each try to withdraw 100 from an account holding 100.
fn final_balance(withdraw: fn(&Account, i64) -> bool) -> i64 {
    let account = Account { balance: Mutex::new(100) };
    let start = Barrier::new(8);
    thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| {
                start.wait(); // line the threads up so they really race
                withdraw(&account, 100)
            });
        }
    });
    account.balance.into_inner().unwrap()
}

fn main() {
    println!("racy:    final balance = {}", final_balance(Account::withdraw_racy));
    println!("correct: final balance = {}", final_balance(Account::withdraw));
}
```

```text
racy:    final balance = -700
correct: final balance = 0
```

All eight threads passed the check before any of them acted, so the account paid out 800 from a balance of 100. This is
a **time-of-check to time-of-use (TOCTOU)** bug. Rust guarantees *memory* consistency, not *business* invariants. The
fix is architectural: make check-and-act atomic, whether under one guard, with a compare-and-swap, or with
`UPDATE ... WHERE balance >= ?` in the database.

**(b) A deadlock.** This compiles and hangs forever:

```rust,no_run
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() {
    let accounts = Arc::new((Mutex::new(100i64), Mutex::new(100i64)));

    let a = Arc::clone(&accounts);
    let t1 = thread::spawn(move || {
        let _from = a.0.lock().unwrap();
        thread::sleep(Duration::from_millis(50));
        let _to = a.1.lock().unwrap(); // waits for t2 to release account 1
    });

    let b = Arc::clone(&accounts);
    let t2 = thread::spawn(move || {
        let _from = b.1.lock().unwrap();
        thread::sleep(Duration::from_millis(50));
        let _to = b.0.lock().unwrap(); // waits for t1 to release account 0
    });

    t1.join().unwrap();
    t2.join().unwrap();
    println!("never printed");
}
```

Two transfers in opposite directions each take one lock and wait for the other's. The type system can't see lock
*order*. The standard fixes are architectural too: a global lock order (always lock the lower account ID first),
`try_lock` with back-off, or one owner per account with message passing.

**(c) A leak.** Two nodes that point at each other through `Rc` keep each other alive forever:

```rust
use std::cell::RefCell;
use std::rc::Rc;

struct Node {
    name: &'static str,
    next: RefCell<Option<Rc<Node>>>,
}

impl Drop for Node {
    fn drop(&mut self) {
        println!("dropping {}", self.name);
    }
}

fn main() {
    {
        let a = Rc::new(Node { name: "a", next: RefCell::new(None) });
        let b = Rc::new(Node { name: "b", next: RefCell::new(Some(Rc::clone(&a))) });
        *a.next.borrow_mut() = Some(Rc::clone(&b)); // a -> b -> a: a cycle
        println!("a strong count = {}", Rc::strong_count(&a));
        println!("b strong count = {}", Rc::strong_count(&b));
    }
    println!("scope ended; no \"dropping\" line above means both nodes leaked");
}
```

```text
a strong count = 2
b strong count = 2
scope ended; no "dropping" line above means both nodes leaked
```

When the scope ends, each count drops from 2 to 1 and never reaches zero. It's memory-safe, and it's a leak. The fix is
`Weak` for back-edges, or an arena with indices (Chapter 1.4).

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part I).*

1. Rust moves are destructive and C++ moves aren't. What does each design cost, and what does each make possible?
2. Rust's thread-safety guarantees are mostly enforced through **library signatures**. Explain how auto traits and
   lifetime bounds make that possible. What exactly would break if `Rc<T>` were `Send`?
3. Why is leaking memory considered *safe* in Rust? What design rule came out of the 2015 scoped-thread episode, and how
   does it affect APIs you'd write?
4. Name three bug classes that safe Rust doesn't prevent, each with a concrete example from backend systems, and the
   architectural mechanism you'd use against each.
5. The atomic counter used `Ordering::Relaxed`. Why is that sufficient there? Describe a change to the program that
   would make it insufficient.
6. What does "zero-cost abstraction" promise? What doesn't it promise? Be precise about compile time, debug builds, and
   binary size.
7. What does `&mut T` tell LLVM that a `T*` in C doesn't, and why does that help optimization?
8. "Safety is a property of an API boundary, not of a line of code." Explain using `Vec<T>`, which is implemented with
   `unsafe` internally.

### 12. Exercises

- **Beginner.** Before running it, predict the output order of a program with a struct `Outer { a: Noisy, b: Noisy }`
  (where `Noisy` prints on drop), two local `Noisy` values, and one of them moved into a function that returns
  immediately. Then run it and explain every line.
- **Intermediate.** Extend the three race fixes to 8 threads and wrap each in `std::time::Instant` timing. Build in
  **release** mode. Write down your predicted ranking and rough ratios *before* running. Explain the results in terms of
  cache-line ownership. (The Playground works for this, but its CPU count and noise level are unknown. Say how that
  limits your conclusions.)
- **Advanced.** On the Playground (ASM output) or Compiler Explorer, compile both `sum_even_squares_*` functions with
  `#[inline(never)]` in release mode and confirm they match. Then change the element type from `u64` to `u32` and
  compare again. Did vectorization change? Form a hypothesis about why before looking for the answer.
- **Systems.** Write a function that recurses without bound. Run it in Rust (note the message and exit status), in Java
  (`StackOverflowError`, which you can catch; should you?), and in Go (goroutine stacks grow up to a large limit, then
  `fatal error: stack overflow`). Compare the three failure models and what each implies for a server process.
- **Architecture.** For each item in the "does not prevent" column of §7, name the mechanism you'd rely on instead:
  lock hierarchies, idempotency keys, database constraints, `panic = "abort"` plus supervisor restarts, checked
  arithmetic policy, `Weak`/arenas, `cargo vet`/Miri for dependencies. Write one sentence of justification for each.

### 13. Debugging exercise

This program is meant to time a slow query. It prints (verified):

```text
slow_query took 190ns
```

but the query takes 200 ms.

```rust
use std::thread;
use std::time::{Duration, Instant};

struct Timer {
    label: &'static str,
    start: Instant,
}

impl Timer {
    fn start(label: &'static str) -> Self {
        Timer { label, start: Instant::now() }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        println!("{} took {:?}", self.label, self.start.elapsed());
    }
}

fn slow_query() {
    thread::sleep(Duration::from_millis(200));
}

fn main() {
    let _ = Timer::start("slow_query");
    slow_query();
}
```

1. Explain precisely **when** the `Timer` is dropped, and why. What's the difference between the patterns `_` and
   `_timer`?
2. Fix it with a one-token change.
3. The same mistake with a lock guard, `let _ = AUDIT_LOG.lock().unwrap();`, is **rejected** by rustc 1.98:
   `error: non-binding let on a synchronization lock` from the deny-by-default lint `let_underscore_lock`. Why can a
   lint catch the mutex case but not the `Timer` case? What does that tell you about the difference between the type
   system's guarantees and lints?

### 14. Design exercise

**Meridian's session cache.** A Rust service holds 2 million user sessions in memory. Requirements: 100,000 lookups/s,
5,000 updates/s, sessions expire after 30 minutes idle, 16 cores, p99 lookup < 100 µs. Request handlers sometimes need
a session for the whole request (up to 50 ms).

Decide the **ownership model**:

- Who owns a `Session`?
- What does a handler hold while it works: a clone, an `Arc<Session>`, a lock guard, or only an ID?
- What happens when a session expires while a handler is using it?

Compare at least three designs: one `RwLock<HashMap<..>>`; a sharded `Vec<Mutex<HashMap<..>>>`; immutable `Arc<Session>`
values that updates replace (copy-on-write); and a single owner thread reached by messages. Cover memory, contention,
tail latency, and failure modes. Pick one, and say what measurement would make you switch.
