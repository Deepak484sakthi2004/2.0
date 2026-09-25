# Chapter 3.5 — Drop, RAII, and Deterministic Destruction

> **Where this sits:** Part III · Ownership · chapter 5 of 6
> **Prerequisites:** Chapters 1.3 and 3.1–3.4.
> **After this chapter you can:** state every rule for *when* and *in what order* destructors run (and when they don't);
> read unwind cleanup paths in MIR; design RAII guards that are correct on every exit path; and explain the limits of
> `Drop`: it can't fail, can't block on the network, can't be async, and doesn't run on `process::exit`.

---

## Pass 1 · User level — *Cleanup you can't forget*

### 1. Problem

Memory is only one resource. Services also hold file descriptors, sockets, locks, pooled connections, database
transactions, temporary files, and tracing spans, and each must be released on **every** exit path: normal return,
early return, `?`, `break`, and panic. Java handles memory with GC and everything else with `try`/`finally` or
`try-with-resources`, which works only if every caller remembers to write it.

Rust handles all of it with one mechanism. A type implements `Drop`, and the compiler inserts the call wherever the
value's owner goes away (Chapter 3.1). Chapter 1.3 introduced it. This chapter gives the exact rules, the machinery, and
the sharp edges.

### 2. Mental model

**When a destructor runs:**

| Situation | Destructor runs? | When |
|---|---|---|
| The owning variable goes out of scope | Yes | At the end of the scope, **locals in reverse declaration order** |
| The containing value is dropped | Yes | After the container's own `Drop::drop`, **fields in declaration order** |
| The place is assigned a new value (`x = new`) | Yes, for the **old** value | At the assignment |
| `drop(x)` | Yes | Immediately (it's just a move into a function that returns) |
| A temporary (`make().len()`) | Yes | At the end of the enclosing statement (details in §4) |
| Panic, with `panic = "unwind"` | Yes | During unwinding, frame by frame |
| The value was **moved** | No, not here | Its new owner drops it |
| `mem::forget(x)`, `Box::leak`, an `Rc` cycle | **Never** | Memory and resource leak (safe) |
| `std::process::exit`, `panic = "abort"`, a signal kill, a crash | **Never** | The process just ends |

**Order, and why:**

- **Locals: reverse declaration order.** A later local may *borrow* an earlier one (`let guard = mutex.lock();`), so
  the borrower must die first.
- **Fields: declaration order**, after the struct's own `drop`. That's simple, predictable, and specified [LANG].
- **Tuple, array, and `Vec` elements: front to back.**

### 3. Rust code

Every rule above, in one run (verified):

```rust
struct Noisy(&'static str);

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("  drop {}", self.0);
    }
}

fn make(name: &'static str) -> Noisy {
    Noisy(name)
}

fn main() {
    println!("1. a temporary dies at the end of its statement:");
    let len = make("temp").0.len();
    println!("  len = {len}");

    println!("2. assigning over a value drops the old one:");
    let mut slot = make("old");
    slot = make("new");
    println!("  slot now holds {}", slot.0);

    println!("3. explicit drop(): just a move into a function that does nothing:");
    let early = make("early");
    drop(early);
    println!("  after drop(early)");

    println!("4. tuple fields and Vec elements drop front to back; locals in reverse:");
    {
        let _pair = (make("pair.0"), make("pair.1"));
        let _v = vec![make("v[0]"), make("v[1]")];
    }

    println!("5. mem::forget: never dropped (safe; it leaks):");
    std::mem::forget(make("forgotten"));

    println!("6. end of main: remaining locals in reverse declaration order:");
    let _last = make("last");
}
```

```text
1. a temporary dies at the end of its statement:
  drop temp
  len = 4
2. assigning over a value drops the old one:
  drop old
  slot now holds new
3. explicit drop(): just a move into a function that does nothing:
  drop early
  after drop(early)
4. tuple fields and Vec elements drop front to back; locals in reverse:
  drop v[0]
  drop v[1]
  drop pair.0
  drop pair.1
5. mem::forget: never dropped (safe; it leaks):
6. end of main: remaining locals in reverse declaration order:
  drop last
  drop new
```

In block 4, `_v` was declared *after* `_pair`, so it's dropped *first*, and within each, elements go front to back.
`forgotten` never prints. At the end, `_last` goes before `slot` (which now holds "new").

**Temporaries and guards: where the scope really ends.** Lock guards make temporary lifetimes observable. `try_lock`
tells us, without blocking, whether the lock is still held (verified in both editions):

```rust
use std::sync::Mutex;

fn state<T>(m: &Mutex<T>) -> &'static str {
    // try_lock never blocks: it tells us whether the lock is currently held
    if m.try_lock().is_ok() { "free" } else { "STILL LOCKED" }
}

fn main() {
    let queue = Mutex::new(vec![1, 2, 3]);

    // A temporary created in a `match` scrutinee lives until the END of the match.
    match queue.lock().unwrap().len() {
        n => println!("inside match (len {n}):       {}", state(&queue)),
    }

    // Binding the value first ends the temporary at the end of the `let` statement.
    let n = queue.lock().unwrap().len();
    println!("after `let n = ...` (len {n}): {}", state(&queue));

    // `if let`: the guard lives through the body...
    if let Some(&first) = queue.lock().unwrap().first() {
        println!("inside if-let body ({first}):  {}", state(&queue));
    }

    // ...but in edition 2024 it is dropped BEFORE the `else` block (in 2021 it was still held).
    if let Some(&x) = queue.lock().unwrap().get(99) {
        println!("unreachable {x}");
    } else {
        println!("inside if-let else:        {}", state(&queue));
    }
    println!("done"); // keeps the `if let` from being main's tail expression (see ch05-09)
}
```

Edition 2024:

```text
inside match (len 3):       STILL LOCKED
after `let n = ...` (len 3): free
inside if-let body (1):  STILL LOCKED
inside if-let else:        free
done
```

The same code compiled as **edition 2021**:

```text
inside match (len 3):       STILL LOCKED
after `let n = ...` (len 3): free
inside if-let body (1):  STILL LOCKED
inside if-let else:        STILL LOCKED
done
```

Two facts that matter in production. First, **a guard created in a `match` scrutinee is held for the whole `match`** in
every edition. Taking the same lock again inside an arm, with `lock()` instead of `try_lock()`, is a **self-deadlock**
(the debugging exercise). Second, [VERSION] **edition 2024 releases an `if let` scrutinee's temporaries before `else`**,
fixing a long-standing deadlock trap.

Edition 2024 changed one more temporary rule, and verifying this chapter hit it directly. In the first draft, the final
`if let` was `main`'s **tail expression**, with no `println!("done")` after it, and under edition 2021 it didn't even
compile. The minimal reproduction (listing `ch05-09-tail-expression.rs`), compiled as edition 2021:

```text
error[E0597]: `queue` does not live long enough
   |
 6 |     let queue = Mutex::new(vec![1, 2, 3]);
   |         ----- binding `queue` declared here
...
11 |     if let Some(&first) = queue.lock().unwrap().first() {
   |                           ^^^^^----------------
   |                           |
   |                           borrowed value does not live long enough
   |                           a temporary with access to the borrow is created here ...
...
14 | }
   | -
   | |
   | `queue` dropped here while still borrowed
   | ... and the borrow might be used here, when that temporary is dropped and runs the `Drop` code for type `std::sync::MutexGuard`
```

[VERSION] In edition 2021, temporaries in a block's *tail expression* live until after the block's *locals* are
dropped, so the `MutexGuard` would outlive the `Mutex` it borrows. Edition 2024 drops tail-expression temporaries first,
and the same code compiles (listing `ch05-09-tail-expression.rs` checks both editions).

---

## Pass 2 · Systems level — *Cleanup paths in the compiler*

### 4. Under the hood

**Every call that can panic gets two exits.** The debug MIR of a function holding two `String`s (listing
`ch05-08-drop-mir.rs`):

```rust,ignore
pub fn two_names(a: &str, b: &str) -> usize {
    let first = a.to_string();
    let second = b.to_string(); // if this allocation panics, `first` must still be dropped
    first.len() + second.len()
}
```

```text
fn two_names(_1: &str, _2: &str) -> usize {
    ...                                     // _3 = first, _4 = second
    bb0: {
        _3 = <str as ToString>::to_string(copy _1) -> [return: bb1, unwind continue];
    }
    bb1: {
        _4 = <str as ToString>::to_string(copy _2) -> [return: bb2, unwind: bb9];
    }
    bb2: {
        _6 = &_3;
        _5 = String::len(move _6) -> [return: bb3, unwind: bb8];
    }
    ...
    bb5: {
        _0 = move (_9.0: usize);
        drop(_4) -> [return: bb6, unwind: bb9];     // normal path: second...
    }
    bb6: {
        drop(_3) -> [return: bb7, unwind continue]; // ...then first (reverse order)
    }
    bb7: {
        return;
    }
    bb8 (cleanup): {
        drop(_4) -> [return: bb9, unwind terminate(cleanup)];
    }
    bb9 (cleanup): {
        drop(_3) -> [return: bb10, unwind terminate(cleanup)];
    }
    bb10 (cleanup): {
        resume;                                     // continue unwinding into the caller
    }
}
```

Read the edges:

- If the **first** `to_string` panics (`bb0`), nothing is live yet: `unwind continue`, so there's nothing to clean up.
- If the **second** panics (`bb1`), only `first` exists: unwind goes to `bb9`, which drops `_3`, then `resume`.
- If anything after that panics (`bb2`–`bb4`), both exist: `bb8` drops `_4`, `bb9` drops `_3`, then `resume`. That's
  the same reverse order as the normal path.
- **`unwind terminate(cleanup)`**: if a destructor *itself* panics while already unwinding, the process **aborts**.
  [LANG] A double panic doesn't unwind again.

This is how RAII is "exception-safe" with no `finally` in sight. The compiler generates the `finally` blocks, exactly
matching which values are initialized at each point.

**Which types have destructors at all?** `std::mem::needs_drop::<T>()` (verified):

```text
u64:             false
(u32, bool):     false
[u64; 1024]:     false
&String:         false
String:          true
Vec<u8>:         true
Option<Box<u8>>: true
```

A reference never needs drop: borrowing never frees (Chapter 3.3). A 1024-element array of `u64` doesn't either: going
out of scope is free. Generic code uses `needs_drop` to skip work. `Vec<T>::clear()` for a `T` without drop glue just
sets the length to zero.

**`Copy` and `Drop` are mutually exclusive:**

```text
error[E0184]: the trait `Copy` cannot be implemented for this type; the type has a destructor
```

If bitwise copies were allowed for a type with a destructor, each copy would run the destructor, freeing a handle twice.
`Copy` means "a bitwise duplicate is a complete, independent value," which is exactly the condition under which no
cleanup is needed.

**Drop check (brief).** [LANG] If a type implements `Drop`, the compiler assumes `drop` might use every reference the
value contains, so those references must still be valid when the value is dropped. This "drop check" is behind some
surprising borrow errors (Part IV). `std` uses an unstable escape hatch (`#[may_dangle]`) in its collections, so a
`Vec<&T>` doesn't impose that restriction unnecessarily.

### 5. Memory

Unwinding walks back up the stack, running each frame's cleanup blocks:

```text
 panic!() in handler
     │  unwind
     ▼
 frame: handler      cleanup: drop(request_buffer)  → free heap buffer
     │  resume
     ▼
 frame: route        cleanup: drop(span_guard)      → close the tracing span
     │  resume
     ▼
 frame: serve_conn   cleanup: drop(pooled_conn)     → return the connection to the pool
     │  resume
     ▼
 catch point (catch_unwind / a Tokio task boundary / thread::spawn's join)
```

Each frame releases exactly what it owned, in reverse order. Nothing is freed twice, because moved values have no cleanup
in the frame they left.

### 6. CPU / OS

- **Unwinding is zero-cost until it happens.** [RUSTC] On mainstream targets the normal path contains no cleanup
  bookkeeping. Cleanup blocks are "landing pads" described by unwind tables (`.eh_frame` plus language-specific data),
  and when a panic happens, the unwinder consults those tables frame by frame. That's slow (microseconds and more), and
  that's fine, because panics are for bugs. The cost on the happy path is binary size and some inhibited optimization,
  which `panic = "abort"` removes (Chapter 2.2).
- **When the process ends, the OS reclaims what it knows about**: memory, file descriptors, sockets, and kernel locks
  (`flock`). It **doesn't** know about user-space buffers (a `BufWriter`'s unflushed bytes), external state (temporary
  files on disk, a lock row in a database, a lease in a coordination service), or remote peers expecting a graceful
  close. §10 shows the first case.

---

## Pass 3 · Architect level — *Designing guards, and knowing their limits*

### 7. Trade-offs

**`Drop` can't return an error, can't be `async`, and shouldn't block.** Those constraints shape three different API
designs for "clean this up":

| Design | Example | Errors | Can be forgotten? | Best for |
|---|---|---|---|---|
| **RAII guard** (`Drop`) | `MutexGuard`, `PooledConn`, `Tx` with rollback-on-drop | Swallowed or logged | No | Releases that can't fail meaningfully (unlock, return to pool, rollback) |
| **Explicit close/commit returning `Result`** | `tx.commit()?`, `file.sync_all()?`, `writer.flush()?` | Reported to the caller | Yes, but pair it with a guard as a backstop | Operations whose *failure matters* (durability, commit) |
| **Closure-scoped API** | `with_transaction(\|tx\| ...)`, `thread::scope(\|s\| ...)` | Returned by the scope function | No | When early drop or leaking the guard must be impossible (Chapter 1.3's scoped threads) |

The robust pattern combines the first two: **an explicit, fallible operation for the success path, and `Drop` as a
safety net for every other path.** The transaction guard in §9 does exactly that: `commit()` consumes the guard, and
`Drop` rolls back if commit never happened.

**The fallible-close problem, concretely.** [LIB] Dropping a `File` closes it and **ignores** any error from `close`.
Dropping a `BufWriter` flushes and **ignores** flush errors. On many filesystems, a write can appear to succeed and fail
only at `close` or `fsync`, so for data that must be durable, call `flush()` and then `sync_all()` explicitly and handle
their `Result`s. Part XXIII builds on this. **No async drop**: a `Drop` can't `.await`, so graceful network shutdown
(sending a close frame, draining a queue) needs an explicit `async fn shutdown()`, with `Drop` as a best-effort backstop
(Part XIII).

### 8. Java comparison

| Java | Rust | Where each is stronger |
|---|---|---|
| `try-with-resources` + `AutoCloseable` | `Drop` | **Java's `close()` can throw**, and suppressed exceptions preserve secondary failures. Rust's `Drop` can't report errors, so it needs explicit fallible methods. |
| The *caller* must write `try (...)` | The *type* guarantees cleanup; nothing to forget | **Rust**: forgetting is impossible |
| `finally` | Compiler-generated cleanup blocks | Equivalent power. Rust's are exact per initialized value. |
| `synchronized` releases on exception | `MutexGuard` releases on unwind | Equivalent |
| Finalizers (deprecated), `Cleaner` (GC-timed) | No GC-timed cleanup | **Rust**: timing is deterministic |

> **Analogy limit.** "`Drop` is `close()` in `try-with-resources`" holds for *when* cleanup runs. It fails for errors.
> Java's `close()` is a full method that can fail and report it. Rust's `drop()` must succeed or swallow. Design as if
> `Drop` were a `finally` block that isn't allowed to throw.

### 9. Production scenario

**Meridian's transaction guard: rollback unless committed.** A database transaction must be rolled back on every path
except a successful commit: early returns from validation failures, `?` on errors, and panics. Committing consumes the
guard, so a committed transaction can't be touched again (verified):

```rust,ignore
struct Tx<'db> {
    db: &'db mut Db,
    pending: Vec<String>,
    done: bool,
}

impl Db {
    fn begin(&mut self) -> Tx<'_> {
        self.events.push("BEGIN".into());
        Tx { db: self, pending: Vec::new(), done: false }
    }
}

impl Tx<'_> {
    fn insert(&mut self, row: &str) {
        self.pending.push(row.to_string());
    }

    /// Consumes the transaction: after commit, it cannot be used (or committed) again.
    fn commit(mut self) {
        self.db.committed.append(&mut self.pending);
        self.db.events.push("COMMIT".into());
        self.done = true;
    } // `self` is dropped here, and Drop sees done == true
}

impl Drop for Tx<'_> {
    fn drop(&mut self) {
        if !self.done {
            let discarded = self.pending.len();
            self.db.events.push(format!("ROLLBACK ({discarded} pending row(s) discarded)"));
        }
    }
}

fn transfer(db: &mut Db, fail: bool) -> Result<(), String> {
    let mut tx = db.begin();
    tx.insert("debit acct-1 100");
    if fail {
        return Err("credit side rejected".into()); // early return: `tx` dropped → rollback
    }
    tx.insert("credit acct-2 100");
    tx.commit();
    Ok(())
}
```

Run with a success, an early-return failure, and a panic caught by `catch_unwind`:

```text
Ok(())
Err("credit side rejected")
panicked: true
events:    ["BEGIN", "COMMIT", "BEGIN", "ROLLBACK (1 pending row(s) discarded)", "BEGIN", "ROLLBACK (1 pending row(s) discarded)"]
committed: ["debit acct-1 100", "credit acct-2 100"]
```

Three details make it sound. `Tx` **borrows** the database mutably (`&'db mut Db`), so nothing else can touch the
database while a transaction is open (Chapter 3.3). `commit(self)` **consumes** the guard, so double commit is a compile
error (Chapter 3.1). And `Drop` handles *every* non-commit path, including the panic, with no `try`/`finally` anywhere.
In a real driver, `commit` would return `Result`, because commit can fail. That's the combined pattern from §7:
explicit and fallible for success, `Drop` for everything else.

### 10. Failure scenario

**The export that was always empty.** A Meridian CLI exported rows through a `BufWriter` and exited with an explicit
code (verified; the program prints **nothing**):

```rust
use std::io::{BufWriter, Write};

fn main() {
    let mut out = BufWriter::new(std::io::stdout());
    for i in 0..3 {
        writeln!(out, "exported row {i}").unwrap();
    }
    // BUG: process::exit ends the process WITHOUT running destructors,
    // so the BufWriter is never flushed and the rows are lost.
    std::process::exit(0);
}
```

Output: *(empty)*. The rows sat in the `BufWriter`'s buffer. `process::exit` terminates immediately. [LANG] It doesn't
unwind and doesn't run destructors for anything on any thread's stack, so the flush in `BufWriter`'s `Drop` never
happened. Small exports that happened to fit the buffer produced empty files, and larger ones were silently truncated
to a multiple of the buffer size. It exited 0 either way.

The fix is to flush explicitly, the only way to *see* a write error anyway, before any exit (verified; prints all three
rows):

```rust,ignore
    if let Err(e) = out.flush() {
        eprintln!("export failed: {e}");
        std::process::exit(1);
    }
    std::process::exit(0);
```

Better still, don't call `process::exit` from deep inside the program. Return a `std::process::ExitCode` from `main`,
as `logstat` does, so every destructor runs normally on the way out.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part III).*

1. List every situation in which a destructor runs, and every situation in which it doesn't.
2. State the drop order for locals, struct fields, and tuple, array, and `Vec` elements. Why are locals dropped in
   *reverse*?
3. Why is a guard created in a `match` scrutinee held for the whole `match`? What did edition 2024 change for `if let`
   and for tail expressions?
4. In the `two_names` MIR, what happens if the second `to_string` panics? If `String::len` panicked?
5. What happens if a destructor panics during unwinding?
6. Why can't `Drop` return an error? What does that mean for `File`, `BufWriter`, and database transactions?
7. Why can't a type be both `Copy` and `Drop`?
8. When does `process::exit` lose data, and what's the correct way for a program to exit with a status code?

### 12. Exercises

- **Beginner.** Predict the output order for a struct with three `Noisy` fields whose own `Drop` prints "drop outer,"
  stored in a `Vec` of two, inside a function that returns early with `?`. Then run it.
- **Intermediate.** Write a `TempDir` guard that creates a directory on construction and deletes it recursively on
  drop. Add `fn persist(self) -> PathBuf` that keeps the directory. How do you stop `Drop` from deleting it after
  `persist`? (Hint: `mem::forget`, or a flag, or `ManuallyDrop`. Compare them.)
- **Advanced.** Make the transaction guard's `commit` return `Result<(), CommitError>`. What should happen to the guard
  if commit *fails*: roll back, or leave the database in an unknown state? Encode your decision in the types.
- **Systems.** Write a program that panics inside a destructor while another panic is already unwinding. Observe the
  abort message and exit status. Then do the same with `panic = "abort"` and compare.
- **Architecture.** List every "cleanup" responsibility in a service you know (connections, locks, temp files, leases,
  metrics flushes, graceful shutdown). For each, choose RAII, an explicit fallible method, a scoped API, or a
  combination, and say what happens on `SIGKILL` (when nothing runs at all).

### 13. Debugging exercise

A Meridian worker deadlocks under load. The code:

```rust,ignore
fn process_next(queue: &Mutex<Vec<Job>>, done: &mut Vec<Job>) {
    match queue.lock().unwrap().pop() {
        Some(job) => {
            if job.retries > 3 {
                queue.lock().unwrap().push(job.escalated()); // re-queue for the escalation handler
            } else {
                done.push(job);
            }
        }
        None => {}
    }
}
```

1. Explain precisely why this deadlocks the thread against *itself*. Which temporary is alive, and until when?
2. `std::sync::Mutex` isn't reentrant. What does the documentation say happens when a thread locks a mutex it already
   holds? Why isn't it guaranteed to be a clean panic?
3. Fix it two ways: by binding the popped value first (`let next = queue.lock().unwrap().pop();`), and by restructuring
   the lock scope explicitly. Which fix makes the lock's scope obvious to the next reader?
4. Would edition 2024's `if let` change have saved this code if it had been written with `if let` instead of `match`?
   Why or why not?

### 14. Design exercise

**Leases in a distributed job scheduler.** Meridian's scheduler grants workers a *lease* on a job. The worker must
renew the lease every 10 seconds and release it when done, or another worker takes the job after the lease expires.
Someone proposes a `Lease` guard whose `Drop` sends a "release" RPC.

Evaluate the proposal: what happens on panic, on `panic = "abort"`, on `SIGKILL`, on a network partition, and when the
release RPC is slow? (`Drop` blocks the thread, and can't `.await` in async code.) Propose a design that combines an
explicit async `release()`, a `Drop` backstop, and **server-side lease expiry** as the only guarantee that survives
every failure. State which component is responsible for correctness and which only for promptness.
