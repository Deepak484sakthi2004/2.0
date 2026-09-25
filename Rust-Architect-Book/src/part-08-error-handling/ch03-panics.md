# Chapter 8.3 — Panics, Unwinding, and Abort

> **Where this sits:** Part VIII · Error Handling · chapter 3 of 4
> **Prerequisites:** Chapters 8.1–8.2; Chapter 2.2 (profiles and the panic strategy per system); Chapter 3.5 (`Drop`,
> and the unwind cleanup blocks in MIR).
> **After this chapter you can:** decide between `Result` and a panic for a given failure; follow a panic from
> `panic!()` through the hook, the unwinder, and a landing pad (verified assembly); contain panics at thread, task,
> request, and FFI boundaries; explain poisoning, double panics, and the `extern "C"` abort; choose `unwind` or `abort`
> per system; state what a panic costs (measured); and design fallible cleanup, given that `Drop` can't fail.

---

## Pass 1 · User level — *When the program itself is wrong*

### 1. Problem

Chapters 8.1 and 8.2 handled failures a caller can do something about: bad input, a missing file, a declined card.
Some failures aren't like that. An index is out of bounds, an `Option` that "can't be `None`" is `None`, a ledger's
debits don't equal its credits. These are **bugs**. The caller can't fix them, and continuing could turn a wrong answer
into corrupted data.

For these, Rust **panics**. This chapter answers the questions a production engineer asks about that:

- What exactly happens between `panic!()` and the process (or thread, or task) ending?
- Which resources get cleaned up, and which don't?
- How do you stop one bad request from taking down a server that handles 7,000 others per second?
- What changes with `panic = "abort"`, across an FFI boundary, or when a destructor panics?
- And what does a panic cost compared with returning an `Err`?

### 2. Mental model

**Recoverable vs unrecoverable:**

| Situation | Mechanism | Why |
|---|---|---|
| Bad input, missing file, timeout, declined card | `Result` | Expected. The caller decides what to do. |
| A bug: broken invariant, impossible state, index out of range | **Panic** | The caller can't fix it, and continuing may corrupt state |
| The caller broke a documented precondition (`v.split_at(mid)` with `mid > len`) | Panic, documented under `# Panics` | A caller bug; checking would burden every correct caller |
| Out of memory | Abort, by default [RUNTIME] | Panicking itself needs to allocate |
| Stack overflow | Abort (Chapter 1.3) | There's no stack left to unwind on safely |
| The process must stop immediately | `process::exit` / `process::abort` | No unwinding at all |

**What happens after a panic starts:**

```text
 panic!("...") · index out of bounds · unwrap() on None · arithmetic overflow (debug)
      │
      ▼
 the PANIC HOOK runs on the panicking thread, BEFORE any unwinding
 (default: prints "thread '<name>' (<id>) panicked at <file>:<line>:<col>:" and the message)
      │
      ├── panic = "abort" ─────────────────────────────► abort(): SIGABRT; no destructors run
      │
      ▼ panic = "unwind" (the default)
 UNWIND: frame by frame, run each frame's cleanup (drop locals: guards unlock, Tx rolls back, ...)
      │
      ├── reaches catch_unwind ────────────────► Err(payload); the program continues
      ├── reaches the top of a spawned thread ─► the thread ends; join() returns Err(payload)
      ├── reaches the top of main ─────────────► the process exits with status 101
      ├── reaches the top of a Tokio task ─────► the task's JoinHandle yields a JoinError; the runtime carries on
      ├── reaches an extern "C" frame ─────────► abort: "panic in a function that cannot unwind"
      └── a destructor panics while unwinding ─► abort: "panic in a destructor during cleanup"
```

A panic is **per thread**, not per process [RUNTIME]: it ends the computation it happened in, and the boundaries above
decide how far that is.

### 3. Rust code

Five common ways to panic, each contained by `catch_unwind` (listing `ch03-01-panic-kinds.rs`, verified):

```rust
use std::any::Any;
use std::hint::black_box;
use std::panic;

fn index_out_of_bounds() -> u32 {
    let v = vec![1, 2, 3];
    v[black_box(7)]
}

fn unwrap_none() -> u32 {
    let port: Option<u32> = black_box(None);
    port.unwrap()
}

fn expect_err() -> u32 {
    black_box("80x").parse::<u32>().expect("PORT must be a number")
}

fn explicit_panic() -> u32 {
    panic!("ledger invariant violated: {} open transactions at shutdown", black_box(2))
}

fn overflow() -> u32 {
    let x: u8 = black_box(200);
    (x + 100) as u32 // debug build: overflow check panics
}

/// A panic payload is `Box<dyn Any + Send>`: usually a &'static str or a String, but not always.
fn describe(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        format!("&str   {s:?}")
    } else if let Some(s) = payload.downcast_ref::<String>() {
        format!("String {s:?}")
    } else {
        "<some other type>".to_string()
    }
}

fn main() {
    let cases: [(&str, fn() -> u32); 5] = [
        ("index", index_out_of_bounds),
        ("unwrap", unwrap_none),
        ("expect", expect_err),
        ("panic!", explicit_panic),
        ("overflow", overflow),
    ];
    for (name, f) in cases {
        match panic::catch_unwind(f) {
            Ok(v) => println!("{name:<9} returned {v}"),
            Err(payload) => println!("{name:<9} payload {}", describe(&*payload)),
        }
    }
}
```

stdout:

```text
index     payload String "index out of bounds: the len is 3 but the index is 7"
unwrap    payload &str   "called `Option::unwrap()` on a `None` value"
expect    payload String "PORT must be a number: ParseIntError { kind: InvalidDigit }"
panic!    payload String "ledger invariant violated: 2 open transactions at shutdown"
overflow  payload &str   "attempt to add with overflow"
```

stderr, printed by the default hook *before* each unwind (the hint line appears only once per process):

```text
thread 'main' (14) panicked at src/main.rs:8:6:
index out of bounds: the len is 3 but the index is 7
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread 'main' (14) panicked at src/main.rs:13:10:
called `Option::unwrap()` on a `None` value

thread 'main' (14) panicked at src/main.rs:17:37:
PORT must be a number: ParseIntError { kind: InvalidDigit }
...
```

Three details. The **location** is the caller's line, not a line inside std: `unwrap`, `expect`, and indexing are
`#[track_caller]` [LIB], so the panic reports `src/main.rs:13:10`, where `unwrap()` was called. `expect`'s message
includes the error's **`Debug`** output. And the **payload's type varies**: a message with no formatting arguments is a
`&'static str`, a formatted one is a `String`. Code that inspects payloads must handle both, plus "something else"
(`std::panic::panic_any` can throw any `Send` value).

`catch_unwind`'s signature is the contract:

```rust,ignore
pub fn catch_unwind<F: FnOnce() -> R + UnwindSafe, R>(f: F) -> Result<R, Box<dyn Any + Send + 'static>>
```

---

## Pass 2 · Systems level — *Landing pads, the unwinder, and abort*

### 4. Under the hood

**The path of a panic through std.** [RUNTIME] [LIB] The backtrace in listing `ch03-05-extern-c-abort.rs`'s output (§13)
shows the frames. `panic!` calls `core::panicking::panic_fmt`, which calls the `#[panic_handler]` that std provides
(`rust_begin_unwind`). That calls `panic_with_hook`, which increments the thread's panic count and runs the hook, and
then hands a boxed payload to the **panic runtime**: `panic_unwind` or `panic_abort`, selected by the profile's
`panic` setting (Chapter 2.2). On Linux, `panic_unwind` calls `_Unwind_RaiseException`, the same Itanium-ABI unwinder
C++ uses [OS].

**Two-phase unwinding.** [RUNTIME] The unwinder first **searches**: it walks up the stack using the unwind tables
(`.eh_frame`) to find a frame that will catch the exception. For Rust, that's `catch_unwind`'s internal `__rust_try`.
If none is found, there's nothing to unwind to. Then it **cleans up**: it walks the frames again, jumping into each
frame's **landing pad**, which drops that frame's live locals and calls `_Unwind_Resume` to continue to the next frame.

**A landing pad in real assembly.** Chapter 3.5 showed the MIR: `(cleanup)` blocks ending in `resume`. Here's what they
become (listing `ch03-08-landing-pad.rs`, release, rustc 1.98.1, trimmed):

```rust,ignore
pub struct Guard(pub u32);
impl Drop for Guard {
    #[inline(never)]
    fn drop(&mut self) { std::hint::black_box(self.0); }
}

#[inline(never)]
pub fn with_guard(x: u32) -> u32 {
    let g = Guard(x);
    let r = may_panic(x);   // may_panic(0) panics
    drop(g);
    r
}
```

```text
playground::with_guard:
	push	rbx
	sub	rsp, 16
	mov	ebx, edi
	mov	dword ptr [rsp + 8], edi          ; g lives in memory: the landing pad needs its address
	call	may_panic                         ; ── the only call that can unwind
	mov	dword ptr [rsp + 12], ebx
	lea	rdi, [rsp + 12]
	call	<Guard as Drop>::drop             ; normal path: drop(g)
	mov	eax, ebx
	add	rsp, 16
	pop	rbx
	ret
	mov	rbx, rax                          ; ── LANDING PAD (never reached by falling through)
	lea	rdi, [rsp + 8]                    ;    rax = the in-flight exception object
	call	<Guard as Drop>::drop             ;    unwinding path: drop(g)
	mov	rdi, rbx
	call	_Unwind_Resume@PLT                ;    continue unwinding into our caller
```

The normal path contains **no unwinding code**: no flag, no check, no handler registration. The landing pad sits after
the `ret`, reachable only by the unwinder, which finds it through a table (the LSDA, in `.gcc_except_table`) that maps
"a panic during the call to `may_panic`" to "jump here". That's why unwinding is called **zero-cost on the happy path**
[RUSTC]. It's not *quite* zero: `g` is kept in a stack slot (`[rsp + 8]`) so the pad can pass its address to `drop`,
and the possible unwind edge constrains some optimizations. Chapter 2.2's `panic = "abort"` removes the pads and those
constraints.

**`panic = "abort"`** [RUNTIME]: the hook still runs, so the message is still printed, and then the runtime calls
`abort()`. No destructors run. The Playground can't change the profile, but `process::abort()` shows the consequence
(listing `ch03-09-abort-skips-drop.rs`):

```rust,no_run
use std::io::{BufWriter, Write};

struct Span(&'static str);

impl Drop for Span {
    fn drop(&mut self) {
        println!("span {} closed", self.0); // never printed: abort runs no destructors
    }
}

// What `panic = "abort"` does to every panic, shown with process::abort (a profile can't be set on the Playground).
fn main() {
    let _span = Span("export");
    let mut out = BufWriter::new(std::io::stdout());
    writeln!(out, "row 1 (buffered, never flushed)").unwrap();
    println!("about to abort");
    eprintln!("aborting now");
    std::process::abort();
}
```

```text
stdout: about to abort
stderr: aborting now
```

Neither "row 1" (still in the `BufWriter`) nor "span export closed" appears. Chapter 3.5's export CLI lost rows the same
way with `process::exit`.

**Two panics at once is an abort.** If a destructor panics while the thread is already unwinding, there would be two
in-flight panics and no sensible way to continue, so the runtime aborts (listing `ch03-07-double-panic.rs`):

```rust,no_run
struct Connection {
    id: u32,
}

impl Drop for Connection {
    fn drop(&mut self) {
        println!("closing connection {}", self.id);
        // A fallible close that panics instead of reporting: fine on the normal path, fatal during unwinding.
        panic!("close failed for connection {}", self.id);
    }
}

fn main() {
    let _conn = Connection { id: 7 };
    panic!("request handler bug"); // unwinding drops _conn → its Drop panics → abort
}
```

```text
thread 'main' (13) panicked at src/main.rs:16:5:
request handler bug
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread 'main' (13) panicked at src/main.rs:10:9:
close failed for connection 7
stack backtrace:
   ... (a forced backtrace, trimmed) ...
panic in a destructor during cleanup
thread caused non-unwinding panic. aborting.
```

This is Chapter 3.5's `unwind terminate(cleanup)` edge in MIR, observed. §10 is the production version.

**`extern "C"` can't unwind.** [VERSION] Since Rust 1.81, a panic that reaches the boundary of an `extern "C"` function
aborts the process, even when a `catch_unwind` sits further up the stack (the debugging exercise, verified). The
compiler gives every `extern "C"` function an abort-on-unwind landing pad: the backtrace shows
`core::panicking::panic_cannot_unwind` called from the function itself. To let a panic cross the boundary on purpose,
declare the function `extern "C-unwind"` [VERSION] (stable since 1.71), and make sure every frame it crosses can handle
it (listing `ch03-06-ffi-boundary.rs`: `C-unwind: caught on the Rust side: true`).

**How the OS sees each ending.** Listing `ch03-11-exit-status.rs` re-runs its own binary as a child process in six modes:

```text
ok        code=Some(0)  signal=None     last stderr line:
err       code=Some(1)  signal=None     last stderr line: Error: "config missing"
panic     code=Some(101) signal=None     last stderr line: note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
exit(3)   code=Some(3)  signal=None     last stderr line:
abort     code=None     signal=Some(6)  last stderr line:
extern-c  code=None     signal=Some(6)  last stderr line: thread caused non-unwinding panic. aborting.
```

A panic that unwinds out of `main` exits with **101** [RUNTIME]. An abort is **signal 6** (`SIGABRT`) [OS], which shells
and Kubernetes report as exit code 134 (128 + 6), with a core dump if the system is configured for one. `main` returning
`Err` is exit code 1 (Chapter 8.1).

### 5. Memory

**Panicking allocates.** [RUNTIME] The payload is a `Box<dyn Any + Send>`, and a formatted message is a `String`. That's
one reason allocation failure aborts instead of panicking: there may be no memory to build the panic.

**Unwind tables cost space, not time.** `.eh_frame` and `.gcc_except_table` are part of the binary and are mapped into
memory, but they're only read while unwinding. `panic = "abort"` removes the landing pads and their tables; [RUSTC] on
Linux targets the frame tables are usually kept, because backtraces, debuggers, and profilers use them too. Chapter 2.2's
exercise measures the size difference.

**Unwinding leaves half-finished state behind.** Drops run, but the *logic* that was interrupted doesn't finish. Rust's
answer for shared state is **poisoning** (listing `ch03-04-poison.rs`):

```rust
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug)]
struct Ledger {
    debits: i64,
    credits: i64,
}

fn main() {
    std::panic::set_hook(Box::new(|info| println!("[hook] {}", info.payload_as_str().unwrap_or("?"))));
    let ledger = Arc::new(Mutex::new(Ledger { debits: 0, credits: 0 }));

    let l = Arc::clone(&ledger);
    let worker = thread::spawn(move || {
        let mut g = l.lock().unwrap();
        g.debits += 100; // first half of a two-part update...
        panic!("processor client bug"); // ...second half never happens; the guard unlocks during unwinding
    });
    let joined = worker.join(); // a panicked thread's join returns Err(payload)
    println!("join is_err: {}", joined.is_err());

    match ledger.lock() {
        Ok(g) => println!("lock ok: {g:?}"),
        Err(poisoned) => {
            println!("lock poisoned: {}", poisoned);
            let g = poisoned.into_inner(); // you may still look, knowing an invariant may be broken
            println!("state seen through the poison: {g:?} (balanced: {})", g.debits == g.credits);
        }
    }
    ledger.clear_poison();
    println!("after clear_poison, is_poisoned = {}", ledger.is_poisoned());
}
```

```text
[hook] processor client bug
join is_err: true
lock poisoned: poisoned lock: another task failed inside
state seen through the poison: Ledger { debits: 100, credits: 0 } (balanced: false)
after clear_poison, is_poisoned = false
```

When a `MutexGuard` is dropped during unwinding, the mutex records it [LIB], and every later `lock()` returns
`Err(PoisonError)`. Poisoning doesn't prevent access (`into_inner` gives you the guard). It **tells you** the invariant
may be broken, which is exactly the fact a Java `synchronized` block hides. `clear_poison` [VERSION] (stable since 1.77)
lets you mark the state repaired. [LIB] `parking_lot`'s mutexes don't poison, trading this signal for simplicity. Part XI
develops the policy question: `lock().unwrap()` propagates the panic to every later user, which is often right.

**`UnwindSafe`** [LANG] is the type-level version of the same concern. `catch_unwind` requires its closure to be
`UnwindSafe`, and `&mut T` isn't, because after a caught panic you could observe a `T` left half-updated.
`AssertUnwindSafe(...)` is you saying "I've considered that". It's a lint in the type system, not a safety guarantee:
no `unsafe` is involved, and nothing becomes undefined behavior if you get it wrong.

### 6. CPU / OS

**What a panic costs** (listing `ch03-12-panic-cost.rs`, release; the hook is silenced, so the numbers exclude printing;
one run on the Playground, noisy):

```text
depth  1: Err returned      3.0 ns/op   panic + catch_unwind   1792.2 ns/op   ratio   601x
depth 10: Err returned     15.8 ns/op   panic + catch_unwind   3201.3 ns/op   ratio   203x
```

Returning an `Err` through one frame costs about as much as a function call. A caught panic costs **microseconds**, and
grows with depth: the hook call, the payload allocation, and then, per frame, a table lookup to find the frame's unwind
information, a call to the personality routine, and a jump into the landing pad, done twice (search and cleanup). Those
mechanisms explain the shape: a large fixed cost plus a per-frame cost. The exact numbers depend on the machine and on
how many shared objects the unwinder has to search.

The conclusion isn't "panics are slow". It's that **panics are for bugs**. A server that panics on 1% of requests has a
bug problem, not a performance problem. A parser that uses panics for invalid input has both.

**Signals and processes.** [OS] `abort()` raises `SIGABRT`. Its default action terminates the process and may write a
core dump, which is useful for post-mortem debugging and costly on a 6 GB-heap pod. A panic in a **spawned thread**
ends only that thread [RUNTIME]. That's a real difference from Go, where an unrecovered panic in any goroutine kills the
whole process.

---

## Pass 3 · Architect level — *Containing bugs*

### 7. Trade-offs

**The panic strategy per system** (the Chapter 2.2 promise, now with verified behavior):

| System | Strategy | Boundary | Verified here |
|---|---|---|---|
| **Gateway** (Tokio, multi-tenant) | `unwind` | Every task: a panicking task becomes a `JoinError`, and the runtime and other tasks carry on | `ch03-03-tokio-task-panic.rs` |
| **Fraud feature library** (loaded into the JVM via FFM) | `unwind` + `catch_unwind` at every exported function | Each entry point converts panics into an error code; nothing unwinds into JVM frames | `ch03-06-ffi-boundary.rs` |
| **CLI tools** (`logstat`, export) | `abort` | None needed: a panic is a bug, the process ends, and the hook still prints the message | `ch03-09-abort-skips-drop.rs` (what abort skips) |
| Batch jobs with shared state | `unwind` + restart the job, or `abort` if state can't be trusted after a bug | The job runner | |
| `no_std` / embedded | A custom `#[panic_handler]` (log, reset) | The whole program | Part XV/XIX territory |

The Tokio behavior (verified):

```text
request 1: ok 10
request 2: task panicked (index out of bounds: the len is 0 but the index is 0) → 500; runtime still up
request 3: ok 30
request 4: ok 40
after the panic: Ok(40)
```

And the FFM library entry point (excerpt of listing `ch03-06-ffi-boundary.rs`):

```rust,ignore
#[unsafe(no_mangle)]
pub extern "C" fn meridian_score(amount_cents: i64, out: *mut i32) -> i32 {
    let result = panic::catch_unwind(AssertUnwindSafe(|| score_impl(amount_cents)));
    match result {
        Ok(Ok(v)) => {
            // SAFETY: the caller's contract (documented in the C header) is that `out` is valid for writes.
            unsafe { out.write(v) };
            OK
        }
        Ok(Err(_)) => ERR_INVALID,
        Err(_) => ERR_PANIC,
    }
}
```

```text
meridian_score(250) -> rc=0 out=50
meridian_score(0) -> rc=-1 out=0
meridian_score(-5) -> rc=-99 out=0
```

Three outcomes, three codes: success, a validation error (`Result`), and a bug (panic). The Java side maps `-99` to an
`IllegalStateException` and alerts. Part XVI builds the full FFM binding.

**When should a *library* panic?** Only for bugs in the caller or in itself, and document each under a `# Panics`
heading. Never on input that arrives from outside the program. `assert!` checks in every build; `debug_assert!` only in
debug builds, which is how Chapter 2.2's checkout discount incident happened. An `assert!` on a cheap invariant in
production code is usually worth keeping.

**`catch_unwind` is not `try`/`catch`.** It's for **boundaries**: a request, a task, a plugin, an FFI entry point. It
catches no aborts (double panics, `extern "C"`, OOM), catches nothing at all under `panic = "abort"`, gets an untyped
payload, and can't select by type. If you find yourself catching a panic to try something else, the callee should
return a `Result`.

**`Drop` can't fail, so fallible cleanup must be explicit.** Chapter 3.5 left this open with the transaction guard's
`commit`. A commit can fail in two very different ways: the database rejected it (nothing applied, the whole transaction
can be retried), or the connection died after `COMMIT` was sent (the outcome is **unknown**). Those are different
variants, and `commit` consumes the guard in both cases, so nobody can keep using a transaction in an undefined state
(excerpt of listing `ch03-10-tx-commit-result.rs`):

```rust,ignore
enum CommitError {
    /// Rolled back: retrying the whole transaction is safe.
    RolledBack { reason: &'static str },
    /// May or may not have committed: reconcile before doing anything else.
    OutcomeUnknown,
}

impl Tx<'_> {
    /// Consumes the guard whatever happens: on success, on a clean failure, and on an unknown outcome.
    fn commit(mut self) -> Result<Committed, CommitError> {
        self.done = true; // from here on, Drop must not issue its own ROLLBACK
        // ... send COMMIT; map the outcome to Ok(Committed { rows }) or one of the two errors ...
    }
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.db.events.push("ROLLBACK (guard dropped)".into()); // the backstop for every other path
        }
    }
}
```

```text
ok: committed 2 rows
retry the transaction: transaction rolled back: serialization conflict
do NOT blindly retry: commit outcome unknown: reconcile before retrying
["BEGIN", "COMMIT (2 rows)", "BEGIN", "ROLLBACK (serialization conflict)", "BEGIN", "COMMIT sent; connection lost"]
```

The same shape applies to files (`writer.flush()?; file.sync_all()?;` before the drop, Chapter 3.5), network shutdown,
and leases. The explicit method reports the failure; `Drop` is the silent backstop and **must never panic** (§10).
Chapter 8.4 returns to "outcome unknown", which is the central problem of distributed error handling.

### 8. Java comparison

| Java / Go | Rust | Notes |
|---|---|---|
| `Error` (`OutOfMemoryError`, `StackOverflowError`) | Abort | Java *can* catch these, and code that does rarely works afterwards. Rust doesn't offer the option. |
| `RuntimeException` for bugs (NPE, `IndexOutOfBoundsException`) | Panic | |
| `Thread.setDefaultUncaughtExceptionHandler` | `panic::set_hook` | Both run on the failing thread. The Rust hook runs **before** unwinding, so it sees the full stack. |
| `ExecutorService`: exception captured, rethrown as `ExecutionException` by `Future.get()` | `JoinHandle::join()` → `Err(payload)`; Tokio `JoinError::is_panic()` | |
| `synchronized` block exited by an exception | Mutex poisoning | Java releases the monitor and says nothing; the next thread sees half-updated state. Rust tells the next locker. |
| JNI: a C++ exception or `longjmp` across Java frames | `extern "C"` abort; `catch_unwind` at entry points | Both are undefined or fatal; Rust makes it a guaranteed abort instead of undefined behavior. |
| Go `panic` / `recover` | `panic!` / `catch_unwind` | An unrecovered Go panic in any goroutine kills the process; a Rust panic ends its thread. |

> **Analogy limit.** "A panic is Rust's `RuntimeException`" is true for *what* panics are used for (bugs) and false for
> *how* they behave. A library can't count on catching one, because the application may compile with
> `panic = "abort"`. There's no catch-by-type, the payload is an untyped `Box<dyn Any>`, a panic during cleanup aborts
> the process, and a panic can't cross an `extern "C"` boundary. Code that uses panics for control flow the way Java code
> uses exceptions will be slow (§6) and, under abort, fatal.

### 9. Production scenario

**Meridian's gateway panic boundary.** A handler bug should cost one request, be impossible to miss, and never take the
pod down. The Rust gateway does three things (listing `ch03-02-boundary-hook.rs`, the synchronous version of the
pattern; in the real gateway, Tokio's task boundary does the containment):

1. **A panic hook** that replaces the default text with one structured log line and increments a counter:

   ```rust,ignore
   panic::set_hook(Box::new(|info| {
       PANICS_TOTAL.fetch_add(1, Relaxed);
       let msg = info.payload_as_str().unwrap_or("<non-string payload>");
       let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
       let thread = std::thread::current();
       println!("[panic-hook] level=ERROR code=INTERNAL_PANIC thread={} at={at} msg={msg:?}", thread.name().unwrap_or("?"));
   }));
   ```

   `PanicHookInfo::payload_as_str` [VERSION] handles both the `&str` and `String` payloads from §3 (it compiled on
   1.98.1; on older toolchains, downcast by hand as `describe` does).

2. **A boundary per request** that converts a panic into a 500 and keeps serving:

   ```rust,ignore
   match panic::catch_unwind(AssertUnwindSafe(|| handle(path, stats))) {
       Ok(Ok(body)) => (200, body),
       Ok(Err((status, code))) => (status, code.to_string()),
       Err(_) => {
           stats.failed += 1;
           (500, "INTERNAL".to_string())
       }
   }
   ```

3. **An alert on `panics_total > 0`.** Every panic is a bug with a ticket, not background noise.

The verified run, with a handler that trusts an ID from the URL:

```text
/items/0  -> 200 keyboard
[panic-hook] level=ERROR code=INTERNAL_PANIC thread=main at=src/main.rs:31 msg="index out of bounds: the len is 3 but the index is 7"
/items/7  -> 500 INTERNAL
/items/x  -> 400 BAD_ID
/users/1  -> 404 NOT_FOUND
/items/2  -> 200 dock
served=3 failed=1 panics_total=1
```

The response to the client is `500 INTERNAL`: no message, no location. The details go to the log. `AssertUnwindSafe`
is a deliberate judgment: `stats` holds counters, and a half-updated counter after a panic is acceptable. If the handler
had a `&mut` to a cache or a connection, the judgment would be different, and the right design would be to drop that
state after a panic rather than reuse it.

### 10. Failure scenario

**The double panic that restarted the pod.** Two bugs, each harmless alone, combined in Meridian's gateway during its
first month in production:

1. A handler panicked while holding the upstream connection pool's `Mutex` (a `lock()` held across a call that could
   panic). The pool's mutex was **poisoned**.
2. The pooled-connection guard returned its connection in `Drop` with `self.pool.lock().unwrap().push(conn)`. With the
   mutex poisoned, `unwrap()` panicked **inside `Drop`**.

On a normal request, bug 2 was "just" a panic: that request failed. But the next time *any* request panicked for any
reason, unwinding dropped its pooled connection, the guard's `Drop` panicked during cleanup, and the process aborted:
listing `ch03-07-double-panic.rs`'s `panic in a destructor during cleanup`. Every in-flight request on the pod died with
it (roughly 150, by Little's law: about 7,300 requests/s per pod times about 20 ms each; arithmetic, not a measurement),
and Kubernetes reported exit code 134. Because the first bug recurred on every pod, the pods restarted in a rolling
wave.

The fixes, in order of importance:

- **`Drop` must never panic.** Return the connection with `if let Ok(mut pool) = self.pool.lock()`, or
  `lock().unwrap_or_else(PoisonError::into_inner)` if the pool's invariants survive a panic (a `Vec` of idle
  connections does). If returning fails, drop the connection and increment a metric. `std::thread::panicking()` lets a
  destructor know it runs during unwinding and must be extra conservative.
- **Don't hold a lock across code that can panic** (or `.await`, Part XIII). Take the connection out and release the lock
  before using it.
- **Decide the poisoning policy per lock.** For the pool: recover. For the ledger in §5: stop and alert.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part VIII).*

1. Give three failures that should be `Result`s and three that should be panics. What's the test you apply?
2. Walk through what happens, in order, from `panic!()` to a `catch_unwind` returning `Err`. Where does the hook run
   relative to unwinding, and why does that matter?
3. What is a landing pad? Describe the verified assembly of `with_guard`. Why is unwinding "zero-cost" on the happy
   path, and what small cost does it still impose?
4. List four situations in which a panic aborts the process even with `panic = "unwind"`.
5. What does `catch_unwind` not catch? Why is it not `try`/`catch`?
6. Explain mutex poisoning. What does it protect against, and what are your three options when `lock()` returns `Err`?
7. What is `UnwindSafe` for? Why isn't `&mut T` `UnwindSafe`, and is `AssertUnwindSafe` an `unsafe` operation?
8. Which panic strategy would you pick for a Tokio gateway, a library loaded into the JVM, and a CLI? Justify each.
9. Why must `Drop` never panic? How do you design cleanup that can fail?

### 12. Exercises

- **Beginner.** Write a function that indexes a slice with an untrusted index. Make one version panic, one return
  `Option`, and one return `Result<_, IndexError>`. Which would you publish in a library, and what would its `# Panics`
  section say?
- **Intermediate.** Install a panic hook that records the payload, location, and thread name into a
  `Mutex<Vec<String>>`, then run three panicking closures under `catch_unwind` and print the records. Restore the
  default hook with `take_hook` at the end.
- **Advanced.** Write a worker pool whose workers survive panics in jobs: each job runs under `catch_unwind`, and a
  panicking job's result becomes `Err(JobPanicked { message })`. What happens to a `Mutex` the job held? Test it.
- **Systems.** Change listing `ch03-12-panic-cost.rs` to measure depths 1, 5, 10, 20, and 50, and fit a line (fixed cost
  plus per-frame cost). Then emit release assembly for `with_guard` with a second guard. How many landing pads are there,
  and in what order do they drop?
- **Architecture.** For a service you know, list every place where one request's failure could affect another
  request: shared caches, pools, locks, global counters, background threads. For each, decide what a panic in the
  middle of an update should do to it.

### 13. Debugging exercise

A C library calls a Rust callback. The Rust side wraps the call in `catch_unwind` "to be safe" (listing
`ch03-05-extern-c-abort.rs`):

```rust,no_run
use std::panic;

/// Imagine this is exported to C or Java (FFM). The "C" ABI promises the caller that no unwind comes out.
extern "C" fn score(amount_cents: i64) -> i32 {
    if amount_cents < 0 {
        panic!("negative amount {amount_cents}");
    }
    (amount_cents % 100) as i32
}

fn main() {
    println!("score(250) = {}", score(250));
    // catch_unwind can't help: the panic reaches the extern "C" boundary first, and the process aborts there.
    let r = panic::catch_unwind(|| score(-5));
    println!("never printed: {r:?}");
}
```

```text
stdout: score(250) = 50

stderr:
thread 'main' (14) panicked at src/main.rs:7:9:
negative amount -5
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

thread 'main' (14) panicked at /rustc/48a229ceaefd4985c50990b14116b6d856af0985/library/core/src/panicking.rs:225:5:
panic in a function that cannot unwind
stack backtrace:
  ...
  18: core::panicking::panic_cannot_unwind
  19: playground::score
  20: playground::main::{closure#0}
  ...
  23: std::panicking::catch_unwind::<i32, playground::main::{closure#0}>
  ...
thread caused non-unwinding panic. aborting.
```

(Backtrace trimmed and crate hashes removed.)

1. The `catch_unwind` is on the stack (frame 23). Why didn't it catch the panic? Which frame decided to abort, and why
   does that frame exist?
2. Why is aborting better than letting the unwind continue into C frames?
3. Give two fixes: one that keeps `extern "C"` and one that changes the ABI string. When is each appropriate, and which
   one is right for Meridian's JVM case?
4. Why did the program print a full backtrace even though `RUST_BACKTRACE` wasn't set?

### 14. Design exercise

**Ferrite v1's panic policy.** Ferrite v1 (Project L4) serves `GET`/`SET`/`DEL` from a thread pool (Project L3) over a
`ShardedStore` of `Vec<RwLock<HashMap<Vec<u8>, Vec<u8>>>>`. A bug in a request handler panics while holding a shard's
write lock.

- What happens to that worker thread, and to the thread pool's capacity, with the L3 design? How do you keep the pool at
  full size?
- The shard's lock is now poisoned. Should the server recover the shard, refuse requests for keys in that shard, or shut
  down? Does the answer change once Ferrite v3 has a write-ahead log to recover from?
- Should Ferrite use `panic = "abort"`? Consider a process supervisor (systemd, Kubernetes) and the cost of restarting
  a store with 10 GB of data in memory.
- What does the client see on the connection whose handler panicked? Write the `-ERR` response, or argue for closing
  the connection instead.
