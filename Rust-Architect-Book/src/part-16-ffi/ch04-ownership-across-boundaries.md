# Chapter 16.4 — Ownership and Allocation Across Language Boundaries

> **Where this sits:** Part XVI · FFI and Systems Programming · chapter 4 of 4
> **Prerequisites:** Chapters 16.1–16.3, Chapter 3.5 (RAII), Chapter 4.5 (higher-ranked borrows in callbacks), Chapter
> 11.5 (the owner-thread pattern), Chapter 15.2 (exposed provenance), Chapter 15.3 (`Box::into_raw`,
> `Vec::into_raw_parts`).
> **After this chapter you can:** answer, for any pointer that crosses a language boundary, who allocated it, who frees
> it and with which allocator, how long it's valid, and which threads may touch it; pick among the standard designs for
> returning data, passing callbacks, and handing out handles; express each lifetime rule as an API rule plus a Rust type
> that enforces the Rust half; and keep a thread-affine C resource behind an owner thread.

---

## Pass 1 · User level — *Four questions for every pointer*

### 1. Problem

Inside a Rust program, ownership is checked by the compiler: one owner, a `Drop` that runs once, borrows that can't
outlive their owner, and `Send`/`Sync` for threads. At a language boundary all of that turns into sentences in a
header:

- "The returned buffer must be released with `meridian_buf_free`."
- "`name` is valid only until the callback returns."
- "The error string is valid until the next call on this engine."
- "An engine must be used only on the thread that opened it."
- "Free each scorer exactly once."

Every one of those sentences is an ownership rule that two languages must follow without a shared checker. Get one
wrong and the result is Chapter 15.1's territory: memory freed twice, freed by the wrong allocator, or read after it's
gone, which is undefined behavior and, in a JVM, usually a crashed process far from the bug.

This chapter doesn't add new mechanisms. It gives a small set of **designs** that make each rule either impossible to
break (on the Rust side, by types) or unambiguous to follow (on the other side, by the API's shape), so the prose in
the header is short and hard to misread.

### 2. Mental model

**Four questions for every pointer that crosses the boundary:**

| Question | Rust inside a program | Across the boundary |
|---|---|---|
| **Who allocated it?** | the type knows (`Box`, `Vec`, `Arc`) | the header must say |
| **Who frees it, with which allocator?** | `Drop`, with the allocator that made it | "whoever allocates provides the free function" |
| **How long is it valid?** | a lifetime, checked | "for this call," "until X is called," "until the handle is freed" |
| **Which threads may touch it?** | `Send` / `Sync`, checked | "thread-safe," "one thread at a time," "only the creating thread" |

**Five rules that answer them:**

1. **Memory returns to the allocator that made it.** Rust's global allocator and C's `malloc` are different systems
   (§3 shows it with a counter). A `Box` goes back through `Box::from_raw`, a `malloc` block through `free`, and a
   `Vec` through `Vec::from_raw_parts` with its exact `(ptr, len, cap)`.
2. **Whoever allocates exports the free.** A library that returns owned memory also exports the only function that may
   release it. The caller never guesses.
3. **Borrowed means "for the duration of the call"** unless the header explicitly says otherwise. Anything the other
   side wants to keep, it copies.
4. **A lifetime the compiler can't see becomes an API rule on the far side and a type on the near side.** The header
   says "don't use the symbol after closing the library." The Rust wrapper makes it `Symbol<'lib, F>`.
5. **Thread rules are type properties.** "Only the creating thread" is `!Send`. "Many threads at once" is `Sync`, and
   you prove it with an assertion (Chapter 16.3).

### 3. Rust code

**Handles: `Box::into_raw` out, the matching free in.** Chapter 16.3's scorer used raw pointers on both sides. The
free function can be simpler, because [LANG] `Option<Box<T>>` (for `T: Sized`) is guaranteed to have the ABI of a
nullable C pointer (listing `ch04-01-handles.rs`, verified, clean under Miri):

```rust,ignore
/// C: `MeridianScorer *meridian_scorer_create(uint32_t block_at);` NULL on invalid input.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_scorer_create(block_at: u32) -> Option<Box<MeridianScorer>> {
    (block_at <= 100).then(|| Box::new(MeridianScorer { block_at }))
}

/// C: `void meridian_scorer_destroy(MeridianScorer *s);` NULL is a no-op.
/// Taking `Option<Box<_>>` by value means Rust frees it when this function returns. The one rule
/// the type can't enforce, and the header must state: C passes each pointer here at most once.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_scorer_destroy(s: Option<Box<MeridianScorer>>) {
    drop(s);
}
```

No `unsafe` block anywhere: the ownership transfer is in the types. `create` returns a `Box` that the ABI turns into a
pointer, and `destroy` receives one back. What the types can't say is the one rule a C caller can still break: *pass
each pointer to `destroy` exactly once*. On the Rust-caller side, the same pair wrapped in an owning type makes even
that rule structural:

```rust,ignore
/// A Rust caller of a C-style API (say, a Rust service linking the same library through its header)
/// wraps the pair so that ownership is back in the type system.
pub struct Scorer(NonNull<MeridianScorer>);

impl Drop for Scorer {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from meridian_scorer_create and is destroyed once, here.
        meridian_scorer_destroy(Some(unsafe { Box::from_raw(self.0.as_ptr()) }));
    }
}
```

```text
create(150) is NULL: true
block_at = 80
```

**Returning variable-size data: two designs, two owners** (listing `ch04-02-returning-bytes.rs`, verified, clean under
Miri). Design 1: the **caller** allocates, and the library writes into it. It's the `snprintf` protocol from Chapter
16.2, with the size reported back when the buffer is too small:

```rust,ignore
/// (1) C: `int32_t meridian_explain_into(uint64_t txn, uint8_t *buf, size_t cap, size_t *needed);`
/// Writes at most `cap` bytes (no NUL). If they don't fit, returns MERIDIAN_ERR_TOO_SMALL and sets
/// `*needed`; the caller retries with a bigger buffer. The library never allocates caller memory.
/// # Safety
/// `buf` is writable for `cap` bytes (may be NULL if cap == 0); `needed` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_explain_into(txn: u64, buf: *mut u8, cap: usize, needed: *mut usize) -> i32 {
    guard(|| {
        let Some(text) = explain(txn) else { return MERIDIAN_ERR_INVALID };
        if needed.is_null() || (cap > 0 && buf.is_null()) {
            return MERIDIAN_ERR_INVALID;
        }
        // SAFETY: `needed` is non-null and writable (the contract).
        unsafe { needed.write(text.len()) };
        if text.len() > cap {
            return MERIDIAN_ERR_TOO_SMALL;
        }
        // SAFETY: `buf` is writable for `cap >= text.len()` bytes, and can't overlap our String.
        unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), buf, text.len()) };
        MERIDIAN_OK
    })
}
```

Design 2: the **library** allocates, and exports the only function that may free it:

```rust,ignore
/// (2) Library-owned bytes. C reads `ptr[0..len]`, must not change any field, and must hand the
/// struct back to `meridian_buf_free` exactly once. `cap` is there only so Rust can free it.
#[repr(C)]
pub struct MeridianBuf {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

/// # Safety
/// `out` is writable. On MERIDIAN_OK, `*out` owns a buffer to be released with `meridian_buf_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_explain(txn: u64, out: *mut MeridianBuf) -> i32 {
    guard(|| {
        let Some(text) = explain(txn) else { return MERIDIAN_ERR_INVALID };
        if out.is_null() {
            return MERIDIAN_ERR_INVALID;
        }
        let (ptr, len, cap) = text.into_bytes().into_raw_parts(); // no longer freed by Rust's Drop
        // SAFETY: `out` is non-null and writable (the contract).
        unsafe { out.write(MeridianBuf { ptr, len, cap }) };
        MERIDIAN_OK
    })
}

/// # Safety
/// `buf` was produced by `meridian_explain`, is unmodified, and is freed only once.
/// A zeroed MeridianBuf (ptr NULL) is accepted and ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_buf_free(buf: MeridianBuf) {
    if !buf.ptr.is_null() {
        // SAFETY: (ptr, len, cap) came from Vec::<u8>::into_raw_parts and come back exactly once,
        // so the Rust global allocator that made the buffer is the one that frees it.
        drop(unsafe { Vec::from_raw_parts(buf.ptr, buf.len, buf.cap) });
    }
}
```

```text
explain_into(cap=16) -> rc=-2, needed=53
explain_into(cap=53) -> rc=0, "txn 7001: amount +0.41, velocity +0.22, country -0.05"
explain -> rc=0, len=53 cap=98, "txn 7002: amount +0.41, velocity +0.22, country -0.05"
explain(0) -> rc=-1
```

Look at `len=53 cap=98`. The `String` built by `format!` grew by doubling, so its buffer is 98 bytes holding 53. The
allocator was asked for 98, and [LANG] `Vec::from_raw_parts` requires the *same* capacity back, because deallocation
must use the layout the block was allocated with. That's why `cap` is in the struct, and why the header says C "must
not change any field." A design that returned only `(ptr, len)` would call `into_boxed_slice()` first, which
reallocates to the exact length whenever `cap != len`.

**Callbacks: user data, borrowed arguments, and panics** (listing `ch04-03-callbacks.rs`, verified, clean under Miri).
The C shape is a function pointer plus a `void *user` that the library passes back untouched. The library side:

```rust,ignore
/// C: `typedef int32_t (*meridian_feature_cb)(void *user, const char *name, double value);`
/// Return 0 to continue, anything else to stop (the value is returned to the caller).
pub type FeatureCb = unsafe extern "C" fn(user: *mut c_void, name: *const c_char, value: f64) -> i32;

/// C: `int32_t meridian_for_each_feature(uint64_t txn, meridian_feature_cb cb, void *user);`
/// `name` is valid only until the callback returns. `user` is passed through, never dereferenced.
/// # Safety
/// `cb`, if non-null, is safe to call with `user` and any valid NUL-terminated `name`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_for_each_feature(txn: u64, cb: Option<FeatureCb>, user: *mut c_void) -> i32 {
```

And a safe Rust wrapper over it, for Rust callers of the C API. The closure travels as `user`, and a generic
`extern "C"` trampoline turns the call back into a closure call:

```rust,ignore
struct Ctx<F> {
    f: F,
    panic: Option<Box<dyn Any + Send>>,
}

unsafe extern "C" fn trampoline<F>(user: *mut c_void, name: *const c_char, value: f64) -> i32
where
    F: FnMut(&CStr, f64) -> ControlFlow<()>,
{
    // SAFETY: `user` is the `&mut Ctx<F>` passed below, alive and unaliased for the whole call.
    let ctx = unsafe { &mut *(user as *mut Ctx<F>) };
    // SAFETY: the library promises a NUL-terminated `name`, valid until we return.
    let name = unsafe { CStr::from_ptr(name) };
    match panic::catch_unwind(AssertUnwindSafe(|| (ctx.f)(name, value))) {
        Ok(ControlFlow::Continue(())) => 0,
        Ok(ControlFlow::Break(())) => 1,
        Err(payload) => {
            ctx.panic = Some(payload); // can't unwind through C: park it, stop the iteration
            2
        }
    }
}

/// `f` gets `&CStr` for the duration of each call only (a higher-ranked borrow: it can't keep it).
pub fn for_each_feature<F>(txn: u64, f: F) -> Result<(), i32>
where
    F: FnMut(&CStr, f64) -> ControlFlow<()>,
{
    let mut ctx = Ctx { f, panic: None };
    // SAFETY: trampoline::<F> matches FeatureCb and expects exactly `&mut Ctx<F>` as `user`;
    // `ctx` outlives the call, and the library doesn't keep `user` after returning.
    let rc = unsafe { meridian_for_each_feature(txn, Some(trampoline::<F>), (&raw mut ctx).cast()) };
    if let Some(payload) = ctx.panic {
        panic::resume_unwind(payload); // back in Rust frames: continue the original panic
    }
    match rc {
        MERIDIAN_OK | 1 => Ok(()),
        other => Err(other),
    }
}
```

```text
all features: ["amount=2.87", "velocity=1.54", "country=-0.35"]
first negative feature: Some("country")
panic in the callback reached the Rust caller intact: "bad feature \"velocity\""
```

Each of this chapter's rules shows up once:

- **Borrowed for the call (rule 3).** `F: FnMut(&CStr, f64)` is a higher-ranked bound (Chapter 4.5): the closure must
  accept a `&CStr` of *any* lifetime, so it can't store one. The first closure copies what it keeps
  (`format!` into a `String`). The header's "valid only until the callback returns" became a type.
- **`user` is owned by the caller.** It points at `ctx` on `for_each_feature`'s stack. The library never frees it and
  never keeps it after returning. Both are header rules, and the Rust side relies on them in its `// SAFETY:` comment.
- **A panic can't cross C, but it can wait.** The trampoline is `extern "C"`, so an unwind there would abort (Chapter
  16.1). It catches the panic, parks the payload in `ctx`, and returns a nonzero "stop" code, which this API allows,
  unlike `qsort_r` in Chapter 16.2. Once `meridian_for_each_feature` has returned and no C frames are left on the
  stack, `resume_unwind` continues the original panic. The Rust caller sees exactly the panic its closure raised.

**Two heaps in one process** (listing `ch04-04-two-heaps.rs`, verified, clean under Miri). A counting global allocator,
the Part III instrumentation, shows which allocator each operation uses:

```rust,ignore
fn main() {
    let t = counting::counts();
    let owned = CString::new("merchant-42").unwrap(); // Rust's allocator
    let raw: *mut c_char = owned.into_raw(); // ownership leaves the type system...
    report("CString::new + into_raw", t);

    let t = counting::counts();
    // SAFETY: `raw` came from CString::into_raw and returns exactly once, unmodified in length.
    drop(unsafe { CString::from_raw(raw) }); // ...and comes back to the allocator that made it
    report("CString::from_raw + drop", t);

    let t = counting::counts();
    // SAFETY: malloc has no preconditions; the result is checked and freed once, by free().
    unsafe {
        let p = malloc(64);
        assert!(!p.is_null());
        free(p);
    }
    report("malloc(64) + free", t);
}
```

```text
CString::new + into_raw                      Rust allocator: +1 allocs, +0 frees
CString::from_raw + drop                     Rust allocator: +0 allocs, +1 frees
malloc(64) + free                            Rust allocator: +0 allocs, +0 frees
```

`malloc` never touched Rust's allocator. They're two independent heaps that happen to live in one address space. On
this Playground build Rust's allocator forwards to the system `malloc` underneath (`System`), which is exactly what
makes mixing them *seem* to work until someone changes the global allocator (§10).

---

## Pass 2 · Systems level — *What crosses, and what it's made of*

### 4. Under the hood

**`into_raw` and `from_raw` are ownership, not memory operations.** [LIB] `Box::into_raw` returns the pointer and
forgets the `Box`: nothing is copied, nothing freed. `Box::from_raw` rebuilds a `Box` that will free the pointer when
dropped. `Vec::into_raw_parts` (stable on 1.98) does the same for the `(ptr, len, cap)` triple, and `CString::into_raw`
for a C string. In each pair, the pointer in the middle is *owned* by whoever holds it, with no type to say so. That's
why the header sentence "release with `meridian_buf_free`, exactly once" carries the whole ownership proof for the time
the pointer spends on the C side (Chapter 15.3 §7's table: "the pointer, length, and capacity must round-trip exactly,
once").

**`Option<Box<T>>` in a C signature.** [LANG] The `Box` documentation guarantees that for `T: Sized`, `Box<T>` is
represented as a single non-null pointer and is ABI-compatible with a C `T*` in `extern "C"` functions, and the
`Option` niche guarantee (Chapter 16.1) adds `NULL` as `None`. So `meridian_scorer_destroy(s: Option<Box<_>>)` has the
exact ABI of `void f(MeridianScorer *)`. What the type adds on the Rust side is that the function *owns* its argument:
the drop at the end is the free.

**Handles that travel as integers.** Some callers can only store an integer: a JNI class with a `long nativeHandle`
field, a Java record, a database row. Two designs, both in listing `ch04-08-handle-registry.rs` (verified, clean under
Miri). The first sends the address itself, and uses Chapter 15.2's **exposed provenance** to turn the integer back into
a usable pointer:

```rust,ignore
#[unsafe(no_mangle)]
pub extern "C" fn meridian_session_open_addr() -> u64 {
    let p = Box::into_raw(Box::new(Session { txns: 0 }));
    p.expose_provenance() as u64 // the integer may later be turned back into this pointer
}

/// # Safety
/// `h` came from `meridian_session_open_addr`, has not been closed, and isn't used concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_session_record_addr(h: u64) -> u64 {
    let p = std::ptr::with_exposed_provenance_mut::<Session>(h as usize);
    // SAFETY: the contract: a live, exclusively used Session whose provenance was exposed above.
    let s = unsafe { &mut *p };
    s.txns += 1;
    s.txns
}
```

`expose_provenance` marks the allocation as reachable from integers, and `with_exposed_provenance_mut` picks up that
permission again. Under Miri this is correct and runs clean, with a warning that states the cost honestly:

```text
warning: integer-to-pointer cast
  = help: this program is using integer-to-pointer casts or (equivalently) `ptr::with_exposed_provenance`, which
          means that Miri might miss pointer bugs in this program
```

That warning is the design argument. With pointer-shaped integers, *every* misuse by the caller (a stale handle, a
handle closed twice, a number that was never a handle) is undefined behavior that no tool can reliably detect. The
second design never gives the caller an address:

```rust,ignore
fn handle(index: usize, generation: u32) -> u64 {
    ((generation as u64) << 32) | (index as u64 + 1) // index + 1: the handle 0 is never valid
}

/// Safe to call with ANY integer: unknown, closed, or reused handles are MERIDIAN_ERR_INVALID.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_session_record(h: u64, out_txns: Option<&mut u64>) -> i32 {
    let mut slots = SESSIONS.lock().unwrap_or_else(|p| p.into_inner());
    let Some((i, generation)) = split(h) else { return MERIDIAN_ERR_INVALID };
    match (slots.get_mut(i), out_txns) {
        (Some(Slot { generation: g, value: Some(s) }), Some(out)) if *g == generation => {
            s.txns += 1;
            *out = s.txns;
            MERIDIAN_OK
        }
        _ => MERIDIAN_ERR_INVALID,
    }
}
```

```text
(A) pointer handle: txns = 2
(B) handle 0x1: record -> 0 (txns 1)
    close -> 0, close again -> -1
    record after close -> -1
    reopened slot: new handle 0x100000001; old handle -> -1, new -> 0
    handle 0 -> -1, handle 12345 -> -1
```

A registry handle is an index into a table plus a generation counter. Closing a slot and reopening it bumps the
generation, so the old handle no longer matches. Double close, use after close, and made-up numbers are all *defined*:
they return `-1`. The `main` of design A exercises only correct use, because its misuse can't be demonstrated safely
at all. That asymmetry is the whole comparison. (`meridian_session_record` also takes `Option<&mut u64>` for its
out-parameter, the Chapter 16.1 guarantee again: NULL becomes `None`, and a non-null pointer is trusted to be valid and
exclusive for the call.)

**Lifetimes of code, not just data.** A function pointer from `dlsym` points into the library's code, which disappears
when the library is unloaded. Chapter 16.3's `Symbol<'lib, F>` borrows the `Library`, so the compiler rejects unloading
while a symbol is still in use (listing `ch04-07-symbol-outlives-library.rs`):

```text
error[E0505]: cannot move out of `lib` because it is borrowed
  --> src/main.rs:46:10
   |
43 |     let lib = Library::open(c"libc.so.6").expect("dlopen");
   |         --- binding `lib` declared here
44 |     // SAFETY: strlen's exact C type.
45 |     let strlen = unsafe { lib.get::<StrlenFn>(c"strlen") }.expect("dlsym");
   |                           --- borrow of `lib` occurs here
46 |     drop(lib); // "we're done with the library"...
   |          ^^^ move out of `lib` occurs here
47 |     // SAFETY (intended): a NUL-terminated literal.
48 |     println!("{}", unsafe { (strlen.f)(c"meridian".as_ptr()) }); // ...but not with its code
   |                             ---------- borrow later used here
```

It's an ordinary E0505 (Chapter 4.6's A–B–C: the borrow, the move, the later use), and it's rule 4 in action: the
header's "don't use a symbol after `dlclose`" became a lifetime parameter, and the compiler checks it like any other
borrow.

### 5. Memory

**Three ways to hand back bytes, drawn:**

```text
 (1) caller-allocated                  (2) library-allocated                  (3) borrowed for a callback
 ┌────────────── C / Java ─────┐       ┌────────── C / Java ──────────┐       ┌──────── library ───────────┐
 │ buf[cap]  ◄── library writes│       │ MeridianBuf {ptr,len,cap} ───┼──┐    │ name bytes (its own memory)│
 │ (stack or arena)            │       │ reads ptr[0..len]            │  │    │   │  lent for ONE call        │
 │ owner: the caller, always   │       │ meridian_buf_free(buf) ──────┼─┐│    │   ▼                         │
 └─────────────────────────────┘       └──────────────────────────────┘ ││    │ cb(user, name, value) ─────►│ caller copies
                                        Rust heap: [53 used | 45 spare] ◄┘│    │                             │ what it keeps
                                        owner: C until free, then Rust ◄──┘    └─────────────────────────────┘
```

**The registry handle, drawn:**

```text
 handle (u64) = generation << 32 | (index + 1)          SESSIONS: Vec<Slot>
 0x0000_0001_0000_0001  → generation 1, index 0 ───────► [0] { generation: 1, value: Some(Session) }   match: OK
 0x0000_0000_0000_0001  → generation 0, index 0 ───────► [0] { generation: 1, ... }                    stale: -1
 0x0000_0000_0000_3039  → index 12344 ─────────────────► out of range                                   -1
```

The table owns every `Session`. The caller owns only a number, and a number can't dangle. The cost is a lookup and a
lock per call (or a sharded or lock-free table, Part XI's options), and one extra level of indirection.

### 6. CPU / OS

- **Allocators are per library, not per process.** [RUNTIME] Every Rust `cdylib` contains its own copy of std, with
  its own global allocator, its own panic hook, and its own thread-locals. Two Rust libraries loaded into one JVM are
  two Rust runtimes that don't know about each other: a `Box` created in one and freed by the other is an allocator
  mismatch, even though both are "Rust." Rule 1 applies between Rust libraries exactly as between Rust and C.
- **Mixing allocators is undefined, not "slightly wrong."** An allocator keeps metadata next to or around each block,
  in a format only it understands, so handing a block to a different allocator's `free` makes it misread that metadata
  (Chapter 15.1 for why "undefined" means anything can follow). It can appear to work when both allocators are the same
  code underneath, which is the trap.
- **`malloc` is thread-agnostic; many C resources aren't.** Memory from `malloc` can be freed on any thread [OS]. A
  vendor engine that keeps per-engine state in thread-local storage, a GUI toolkit, or a single-threaded embedded
  runtime can't be touched from another thread at all. That's a property of the *resource*, and Rust expresses it as
  `!Send` (§9).
- **Where the bytes physically are.** A Java `Arena` hands out native memory from the C heap (`malloc`, which
  itself uses `mmap` for large blocks) [RUNTIME]. Rust borrowing it as `&[u64]` for a call reads the same cache lines Java wrote, with no copy. A
  copy happens only where a design asks for one: Java's `long[]` into the arena (the GC heap can move), or a Rust
  callback's `to_owned()`.

---

## Pass 3 · Architect level — *Choosing who owns what*

### 7. Trade-offs

**Returning variable-size data:**

| Design | Who allocates / frees | Extra call | Best for | Watch out for |
|---|---|---|---|---|
| Caller buffer + size query (`_into`) | caller / caller | on truncation (retry) | small, bounded outputs; callers with arenas or stack buffers (Java, C) | two calls when the size is unknown; computing twice or caching |
| Library buffer + `*_free` (`MeridianBuf`) | library / library, via its own free | the free | unbounded outputs; one-shot results | a missing free = a leak; the wrong free = UB |
| Borrowed during a callback | library / library | none | streaming many items without materializing them | callers keeping pointers past the callback |
| Pointer into the handle, "valid until the next call" | library / library | none | error messages (`vse_last_error`) | every caller on every thread must copy first; review each use |

**Handles:**

| Design | Stale handle | Double free | Cost | Use when |
|---|---|---|---|---|
| Raw pointer (`Box::into_raw`) | UB | UB | none | callers are trusted C/C++ with RAII wrappers |
| Pointer as integer (exposed provenance) | UB | UB | none | the caller can only hold integers and you trust it completely |
| Registry index + generation | error code | error code | a lookup (and a lock or atomic) per call | the caller is Java, a script, or anything you don't control |

**Thread-affine resources:**

| Design | Callers | Cost | Failure mode |
|---|---|---|---|
| `!Send` handle, used where created | code already on that thread | none | can't be shared at all |
| Owner thread + request channel (§9) | any thread | a channel round trip per call (µs, order of magnitude) | the owner dies: callers get `EngineDown` |
| One handle per worker thread (`thread_local!`) | the pool's threads | N handles, N× memory, N× load time | uneven load; handles live as long as the threads |

### 8. Java comparison

FFM's `Arena` is the closest thing Java has to Rust ownership, and it's worth comparing precisely:

| Java FFM | Rust | Checked |
|---|---|---|
| `Arena.ofConfined()`: one owner thread; `close()` frees everything | an owned value that is `!Send`, dropped at scope end | Java: at run time (`WrongThreadException`, `IllegalStateException`). Rust: at compile time |
| `Arena.ofShared()`: any thread; `close()` waits until no thread is accessing | `Arc<T>` with `T: Sync`, dropped by the last owner | Java: run time. Rust: compile time plus a reference count |
| `Arena.ofAuto()`: freed when the GC finds it unreachable | no direct equivalent (Rust has no tracing GC) | Java: the GC |
| `Arena.global()`: never freed | `'static` / `Box::leak` | nothing to check |
| a `MemorySegment` from native code: size 0, no arena | a raw pointer | nothing: `reinterpret(...)` is a restricted method |

The last row is where the two meet. A `MeridianBuf` returned from Rust arrives in Java as a pointer with no size and no
owner. FFM lets Java **adopt** it into an arena and attach the matching free as the cleanup, which is rule 2 expressed
in Java (not verified here: requires JDK 22+ and a `FunctionDescriptor` for `meridian_buf_free` taking the 24-byte
struct by value):

```java
MemorySegment text = buf.get(ADDRESS, 0)                          // MeridianBuf.ptr: a zero-length segment
        .reinterpret(len, arena, seg -> freeBuf(buf));            // size it, and free it WITH the Rust function
// ... read `text` ...
// when `arena` closes, the cleanup calls meridian_buf_free(buf): memory returns to Rust's allocator
```

JNI has no such hook. The usual pattern is a `long nativeHandle` field plus `java.lang.ref.Cleaner` to call the native
free when the object becomes unreachable, with all the timing uncertainty of GC-driven cleanup.

> **Analogy limit.** An `Arena` frees memory when *Java* closes it, and it knows nothing about memory Rust allocated.
> Rust's `Drop` frees memory when *Rust* drops the owner, and it knows nothing about Java's arenas. At the boundary each
> side's ownership system covers only its own allocations. The rules in §2 are what connect them, and every one of
> them is enforced by at most one side.

### 9. Production scenario

**The vendor engine behind an owner thread.** Chapter 11.2's design exercise asked how to use a thread-affine native
handle from a pool of worker threads. Chapter 16.2 bound the engine and made `Engine` `!Send`. The compiler enforces
the vendor's rule (listing `ch04-06-engine-not-send.rs`):

```text
error[E0277]: `*const ()` cannot be sent between threads safely
  --> src/main.rs:26:19
   |
26 |     thread::spawn(move || engine.score(&[0.9, 0.5, 0.1]))
   |     ------------- -------^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |     |             |
   |     |             `*const ()` cannot be sent between threads safely
   |     |             within this `{closure@src/main.rs:26:19: 26:26}`
   |     required by a bound introduced by this call
   |
note: required because it appears within the type `PhantomData<*const ()>`
```

(The same call also reports `NonNull<VseEngine>`: either field alone would make `Engine` `!Send`.) So the fraud
service does what Chapter 11.5's owner-thread pattern prescribes. One thread opens the engine and owns it for its whole
life, and every other thread sends it requests over a bounded channel, each with a one-shot reply channel (listing
`ch04-05-engine-owner-thread.rs`, verified, clean under Miri; the vendor library is simulated as in Chapter 16.2):

```rust,ignore
/// Send + Sync: share it with `Arc` across every worker thread. The Engine itself never moves.
pub struct EngineService {
    tx: Option<mpsc::SyncSender<Request>>,
    owner: Option<JoinHandle<()>>,
}

impl EngineService {
    pub fn start(model: PathBuf, queue: usize) -> Result<EngineService, VseError> {
        let (tx, rx) = mpsc::sync_channel::<Request>(queue); // bounded: backpressure, Chapter 11.5
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let owner = thread::Builder::new()
            .name("vse-owner".into())
            .spawn(move || {
                let mut engine = match Engine::open(&model) {
                    Ok(e) => e, // opened HERE, so every vse_* call below happens on this thread
                    Err(e) => return drop(ready_tx.send(Err(e))),
                };
                let _ = ready_tx.send(Ok(()));
                for req in rx {
                    assert!(req.features.iter().all(|x| x.is_finite()), "non-finite feature reached the engine");
                    let _ = req.reply.send(engine.score(&req.features));
                }
            }) // loop ends when every sender is gone; `engine` drops (vse_close) on this thread
            .expect("spawn");
        ready_rx.recv().expect("owner thread reports")?;
        Ok(EngineService { tx: Some(tx), owner: Some(owner) })
    }

    pub fn score(&self, features: Vec<f64>) -> Result<f64, ServiceError> {
        let (reply, answer) = mpsc::sync_channel(1);
        let tx = self.tx.as_ref().expect("present until Drop");
        tx.send(Request { features, reply }).map_err(|_| ServiceError::EngineDown)?;
        answer.recv().map_err(|_| ServiceError::EngineDown)?.map_err(ServiceError::Engine)
    }
}
```

```text
12 requests from 4 threads -> 12 scored on the owner thread
wrong feature count -> Err(Engine(Score { code: 1, message: "expected 3 features, got 1" }))
NaN feature (a bug) -> Err(EngineDown)
after the owner thread died -> Err(EngineDown)
```

Every ownership question has a structural answer:

- **Which thread touches the engine?** Only `vse-owner`. `Engine` can't leave it (`!Send`), and the simulated vendor
  library, which checks the calling thread and would return `VSE_EWRONGTHREAD`, scored all 12 requests.
- **Who closes it, and where?** `Engine`'s `Drop`, on the owner thread, when the loop ends. It ends when
  `EngineService` is dropped (its `Drop` closes the channel and joins), or when the thread panics. The `assert!` on NaN
  stands in for a bug in that thread. The unwind drops `engine` on its own thread, so `vse_close` still runs where the
  vendor requires.
- **How do callers learn it's gone?** Their reply sender was dropped, so `recv()` fails, and every later `send` fails
  too: `EngineDown`, never a hang. The service layer above turns that into a restart of the owner thread and a metric.

In production Meridian runs a small pool of these owner threads, each with its own engine (the vendor allows several
engines per process, each bound to one thread), behind a least-loaded dispatcher. The review queue is a small fraction
of scoring traffic, so the channel round trip, microseconds (order of magnitude), is invisible next to the engine's own
latency.

### 10. Failure scenario

**It worked until the allocator changed.** The fraud library once exported a convenience function,
`meridian_version_string()`, that returned `CString::new(...).into_raw()`, and its header comment said *"free the
result with free()"*. A small C++ tool used it for years and called `free()`, and nothing went wrong, because Rust's
global allocator in that library was the default `System` allocator, which forwards to the same `malloc` (§3's
listing shows the two are different *APIs* even so). In 2026 the fraud library adopted mimalloc as its global
allocator, like the gateway (Chapter 2.1). The next release of the C++ tool crashed intermittently inside `free()`.
Nothing in the tool had changed: a pointer from mimalloc was being handed to glibc's `free`, which reads allocator
metadata that isn't there. That is undefined behavior (Chapter 15.1), and "worked for years" had only ever meant "both
allocators happened to be the same one."

The contract had been wrong from the start. It told the caller to use an allocator the library never promised to use.
The fix followed rules 1 and 2:

- the function now returns a `MeridianBuf` (§3), and the header's only instruction is *release with
  `meridian_buf_free`*;
- no exported function returns memory without a matching `*_free` in the same header, and the review checklist asks
  "which free function?" for every pointer-returning export;
- the Java wrapper adopts returned buffers with `reinterpret(..., cleanup)` (§8), so the right free is attached at the
  single place the pointer enters Java.

The general lesson: **an allocator is an implementation detail of a library, and a boundary contract must never expose
it.** "Free with `free()`" exposes it. "Free with `meridian_buf_free`" hides it, and the library can change allocators
without breaking anyone.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVI).*

1. State the four questions for a pointer crossing a boundary, and the five rules that answer them.
2. Why must `Vec::from_raw_parts` receive the original capacity? What happens to that requirement if you return only
   `(ptr, len)`?
3. Why is `extern "C" fn destroy(s: Option<Box<T>>)` a correct C destructor with no `unsafe` block? Which rule can its
   type still not enforce?
4. Two Rust `cdylib`s are loaded into one JVM. Can a `Box` created by one be freed by the other? Why or why not?
5. Compare raw-pointer handles, pointer-as-integer handles, and registry handles for a JNI caller. What does each do
   with a stale handle?
6. How does the callback wrapper in §3 keep a panic from crossing C and still deliver it to the Rust caller? Why
   couldn't `qsort_r` in Chapter 16.2 use the same technique?
7. How does `F: FnMut(&CStr, f64)` enforce "valid only during the callback"? Which Part IV chapter explains why?
8. An owner thread panics while holding a thread-affine engine. Walk through what happens to the engine, the pending
   caller, and later callers.
9. Map FFM's four arena kinds onto Rust ownership constructs. Where does the analogy break?

### 12. Exercises

- **Beginner.** For each function, write the header comment that answers all four questions:
  `meridian_scorer_create`, `meridian_explain`, `meridian_for_each_feature`, `vse_last_error`.
- **Intermediate.** Change `meridian_explain` to return `(ptr, len)` only, using `into_boxed_slice`. Measure with the
  counting allocator from listing `ch04-04-two-heaps.rs` how many allocations the change adds, and when.
- **Advanced.** Make the registry in listing `ch04-08-handle-registry.rs` lock-free for lookups: a fixed-capacity slab
  with `AtomicU64` generation counters. What does a lookup have to do so that a concurrent close can't free the session
  while it's being used? (Hint: Part XIV's reclamation problem.)
- **Systems.** Write a program with two different global allocators in two *modules* of one binary (you can't, and
  explain why), then describe how you'd reproduce the two-runtimes situation of §6 with two `cdylib`s locally, and what
  `LD_DEBUG=bindings` would show.
- **Architecture.** The owner-thread design serializes all calls to one engine. The vendor's engine takes about 200 µs
  per call, and the review queue peaks at 2,000 requests/s. Size the pool, the queue bounds, and the timeout, and say
  what the service does when every queue is full.

### 13. Debugging exercise

A Java team asks for a zero-copy way to read the model's feature names, and gets this export (a review sketch, not a
listing):

```rust,ignore
pub struct MeridianScorer {
    names: Vec<String>,
    /* ... */
}

/// Returns a pointer to the `i`-th feature name (NUL-terminated) and its length.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_feature_name(s: *mut MeridianScorer, i: usize, len: *mut usize) -> *const u8 {
    let s = unsafe { &mut *s };
    let name = &mut s.names[i];
    name.push('\0'); // make it a C string
    unsafe { *len = name.len() - 1 };
    name.as_ptr()
}
```

It passes the Java team's single-threaded test. The hourly model reload replaces `names`.

1. Answer the four questions for the returned pointer. Why does the validity answer depend on calls the caller doesn't
   make itself: other threads, and the hourly reload?
2. The function mutates shared state on every call. What does a second read of the same name return? What happens
   when two Java threads read names at the same time? Which Rust rule did `&mut *s` let the author skip?
3. Redesign it three ways: caller buffer, library buffer with a free, and a callback. Pick one for a Java caller that
   reads all names once at startup, and justify it.

### 14. Design exercise

**Streaming results to Java.** The fraud team wants a function that scores a day's transactions (millions of rows)
and streams `(txn_id, score)` pairs back to a Java batch job as they're computed, without materializing all of them.
Design the boundary:

- callback (an FFM upcall stub) versus a pull API (`meridian_stream_next(handle, buf, cap, *n)`) versus a shared ring
  buffer in memory both sides map;
- who allocates each buffer, and how long each pointer is valid;
- how a Java exception in an upcall is handled (the JDK documents that an exception escaping an upcall terminates the
  JVM), and how a Rust panic is handled;
- backpressure: what happens when Java consumes more slowly than Rust produces.

For your chosen design, write the header, the four answers for every pointer in it, and the Rust types that enforce the
Rust half.
