# Chapter 16.3 — Calling Rust from C (and from Java via FFM)

> **Where this sits:** Part XVI · FFI and Systems Programming · chapter 3 of 4
> **Prerequisites:** Chapters 16.1–16.2, Chapter 8.3 (`meridian_score` and `catch_unwind`), Chapter 7.2 (concrete
> exports over generic internals), Chapter 9.2 (JNI's Modified UTF-8), Chapter 2.7 (what a crate exports), Chapter 6.5
> (the C-ABI vtable).
> **After this chapter you can:** design a C API for a Rust library that a JVM, a C program, or another Rust program
> can call safely; make every entry point panic-proof and input-checked; export exactly the symbols you mean to; generate
> the header and the Java bindings from one source; call the library from Java with FFM (and know what JNI would have
> cost); and move text across the boundary without changing it.

---

## Pass 1 · User level — *A C API for a Rust library*

### 1. Problem

Chapter 16.2 wrapped someone else's C API. This chapter builds one. Meridian's fraud feature library is Rust (Chapter
7.2's port), and its callers aren't. The scoring service is Java, and it calls the library through FFM thousands of
times per second from dozens of threads. The C API it exposes is the only part of the library its callers ever see.

C can express very little of what the Rust library knows. It has no generics, no `Result`, no `Option` (except for
pointers), no slices, no strings with lengths, no ownership, no lifetimes, and no panics. A caller that is a JVM can't
catch a Rust panic, and can't be trusted to pass valid pointers, in-range enum values, or UTF-8. So the job is an
**hourglass**: rich types on both sides, and a deliberately narrow, boring waist in the middle that both sides can
check.

```text
   Rust internals                    C API (the waist)                     Java
 ─────────────────────        ────────────────────────────────       ─────────────────────
 FeatureSource trait,         meridian_scorer_new / _free            FraudLibrary class,
 generic score_all<S>,   ──►  meridian_score_batch(s, ids, n, out) ◄── AutoCloseable,
 Result<_, Invalid>,          int32_t codes: 0 / -1 / -99            exceptions from codes,
 panics, Vec, HashMap         opaque handles, (ptr, len) buffers     Arena-managed buffers
```

Chapter 8.3 built one entry point, `meridian_score`, and promised that Part XVI would build the rest. This is it.

### 2. Mental model

**The ten rules of a C API written in Rust.** Each rule answers a question the C language leaves open:

| # | Rule | Why |
|---|---|---|
| 1 | `#[unsafe(no_mangle)] pub extern "C" fn`, with a **library prefix** (`meridian_`) | C has one global namespace; the prefix is the namespace (the Part XVI review shows an unprefixed `init`) |
| 2 | **`unsafe extern "C" fn`** whenever it takes pointers it dereferences | C ignores the keyword; a *Rust* caller must see the obligation (§10) |
| 3 | **Every entry point catches panics** and returns a code | a panic can't cross `extern "C"`; the alternative is aborting the host process (16.1) |
| 4 | **One error convention**: an `int32_t` return code, results through out-pointers | C has no `Result`; mixing conventions (NULL here, -1 there, errno elsewhere) is how callers get it wrong |
| 5 | **Opaque handles** with a create/destroy pair | Rust types don't have a C layout, and Rust must stay the only one freeing them (16.4) |
| 6 | **Concrete types only** at the boundary: `u64`, `i32`, `repr(C)` structs | C has no generics; monomorphize at the waist (Chapter 7.2) |
| 7 | **Buffers as `(pointer, length)`**, checked before use | C arrays don't carry a length; `from_raw_parts` has preconditions you must establish |
| 8 | **Validate everything that arrives**: NULL, alignment, sizes, enum codes, UTF-8 | the caller isn't Rust, so its values carry no guarantees (16.1 §7) |
| 9 | **State thread safety in the header**, and make the Rust type prove it (`Sync` assertions) | the JVM *will* call from many threads |
| 10 | **Export a version** and check it at load | the header, the library, and the bindings can drift (16.1 §9) |

### 3. Rust code

**The fraud library's C API** (listing `ch03-01-fraud-c-api.rs`, verified, clean under Miri; `main` plays the C
caller). Internally, scoring is ordinary generic Rust:

```rust,ignore
pub trait FeatureSource {
    fn features(&self, id: u64) -> [f64; 3];
}

fn score_all<S: FeatureSource>(src: &S, model: &Model, ids: &[u64], out: &mut [i32]) {
    for (id, slot) in ids.iter().zip(out.iter_mut()) {
        let f = src.features(*id);
        let s: f64 = f.iter().zip(&model.weights).map(|(x, w)| x * w).sum();
        *slot = (s * 100.0).round() as i32;
    }
}
```

The waist starts with one function that implements rules 3 and 4 for every entry point:

```rust,ignore
struct Invalid;

/// One place that turns Result + panics into the header's codes.
fn ffi_guard(f: impl FnOnce() -> Result<(), Invalid>) -> i32 {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => MERIDIAN_OK,
        Ok(Err(Invalid)) => MERIDIAN_ERR_INVALID,
        Err(_) => MERIDIAN_ERR_PANIC,
    }
}
```

and one that implements rule 7, turning C's `(pointer, count)` into a slice only after checking what
`slice::from_raw_parts` requires: non-null even for length 0, aligned, and a total size no larger than `isize::MAX`:

```rust,ignore
/// C's (pointer, count) as a slice, with the checks `slice::from_raw_parts` requires of us.
/// # Safety
/// If `n > 0` and `p` is non-null, `p` must point to `n` readable, initialized `T`s that stay valid
/// and unmodified for `'a`.
unsafe fn slice_in<'a, T>(p: *const T, n: usize) -> Result<&'a [T], Invalid> {
    if n == 0 {
        return Ok(&[]); // C may pass NULL with 0; from_raw_parts may not receive NULL
    }
    if p.is_null() || !p.is_aligned() || n > isize::MAX as usize / size_of::<T>() {
        return Err(Invalid);
    }
    // SAFETY: non-null, aligned, size in range (checked above); readable for `n` (the caller's contract).
    Ok(unsafe { std::slice::from_raw_parts(p, n) })
}
```

Then the exports. The handle pair (rule 5) and the batch function (rule 6):

```rust,ignore
/// # Safety
/// `cfg` is NULL or points to a valid MeridianConfig; `out` is NULL or writable.
/// On MERIDIAN_OK, `*out` owns a scorer that must be released with `meridian_scorer_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_scorer_new(cfg: *const MeridianConfig, out: *mut *mut MeridianScorer) -> i32 {
    ffi_guard(|| {
        // SAFETY: the contract: NULL or a valid, aligned MeridianConfig.
        let cfg = unsafe { cfg.as_ref() }.ok_or(Invalid)?;
        if out.is_null() || cfg.review_at > cfg.block_at || cfg.block_at > 100 {
            return Err(Invalid);
        }
        let rows = HashMap::from([(1001, [0.9, 0.8, 0.7]), (1002, [0.1, 0.2, 0.1]), (1003, [0.6, 0.5, 0.2])]);
        let scorer = Box::new(MeridianScorer {
            model: Model { weights: [0.5, 0.3, 0.2] },
            source: InMemory { rows },
        });
        // SAFETY: `out` is non-null (checked) and writable (the contract).
        unsafe { out.write(Box::into_raw(scorer)) };
        Ok(())
    })
}

/// # Safety
/// `s` is NULL (a no-op, like free(NULL)) or came from `meridian_scorer_new` and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_scorer_free(s: *mut MeridianScorer) {
    if !s.is_null() {
        // SAFETY: the contract: `s` came from Box::into_raw in meridian_scorer_new, released once.
        drop(unsafe { Box::from_raw(s) });
    }
}

/// Concrete signature over the generic `score_all`: C has no generics, so the boundary picks the types.
/// # Safety
/// `s` came from `meridian_scorer_new`; `ids` points to `n` u64s and `out` to room for `n` i32s
/// (either may be NULL when `n == 0`). On an error return the contents of `out` are unspecified.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_score_batch(s: *const MeridianScorer, ids: *const u64, n: usize, out: *mut i32) -> i32 {
    ffi_guard(|| {
        // SAFETY: the contract: NULL or a live scorer, shared (read-only) for the call.
        let s = unsafe { s.as_ref() }.ok_or(Invalid)?;
        // SAFETY: the contract on `ids` / `out`, checked for NULL, alignment and size inside.
        let (ids, out) = unsafe { (slice_in(ids, n)?, slice_out(out, n)?) };
        score_all(&s.source, &s.model, ids, out);
        Ok(())
    })
}
```

And rule 9, the header's thread-safety promise, as a compile-time proof:

```rust,ignore
/// Opaque to C (`typedef struct MeridianScorer MeridianScorer;`). Immutable after creation, and the
/// header promises it may be shared across threads, so it must be Sync. The compiler checks that:
pub struct MeridianScorer {
    model: Model,
    source: InMemory,
}
const _: () = {
    const fn assert_sync<T: Sync>() {}
    assert_sync::<MeridianScorer>();
};
```

If someone later adds a `RefCell` cache to `MeridianScorer`, the header's promise becomes false, and this block turns
that into a compile error instead of a data race in production. The run, with `main` calling exactly as C would:

```text
meridian_abi_version() = 3
scorer_new(80/50)         -> rc=0
score_batch([1001, 1002, 1003]) -> rc=0 scores=[83, 13, 49]
score_batch(NULL, n=3)    -> rc=-1
score_batch(NULL, n=0)    -> rc=0
score_batch([1001, 4242]) -> rc=-99  (a bug inside, contained)
scorer_new(40/60)         -> rc=-1
```

Every outcome is a code. The id 4242 isn't in the feature store, and `InMemory::features` indexes a `HashMap` with
`self.rows[&id]`, which panics. That bug is kept in the listing on purpose: it's the kind a real library has, and here
it becomes `-99` and an alert in Java, not a dead JVM.

**The header, and a real C caller.** C and Java callers see only the header. In production it's generated from the
crate by `cbindgen` (not verified here: requires `cbindgen`; run
`cbindgen --config cbindgen.toml --crate meridian-fraud --output include/meridian_fraud.h`). The version below is
written by hand, and it *is* verified. The Playground's container has `rustc`, `gcc`, and binutils, so listing
`ch03-06-c-calls-rust.rs` writes this header, the same exports as above, and a C client to `/tmp`, builds the library
as a real `cdylib` with `rustc --crate-type cdylib`, compiles the client with `gcc -std=c11 -Wall -Wextra -Werror`,
runs it, and asserts that every step succeeded:

```c
#ifndef MERIDIAN_FRAUD_H
#define MERIDIAN_FRAUD_H
#include <stddef.h>
#include <stdint.h>

#define MERIDIAN_ABI_VERSION 3
#define MERIDIAN_OK 0
#define MERIDIAN_ERR_INVALID -1
#define MERIDIAN_ERR_PANIC -99

typedef struct MeridianScorer MeridianScorer; /* opaque */

typedef struct MeridianConfig {
    uint32_t block_at;  /* 0..=100 */
    uint32_t review_at; /* <= block_at */
} MeridianConfig;
_Static_assert(sizeof(MeridianConfig) == 8, "MeridianConfig layout changed");
_Static_assert(offsetof(MeridianConfig, review_at) == 4, "MeridianConfig layout changed");

uint32_t meridian_abi_version(void);
int32_t meridian_scorer_new(const MeridianConfig *cfg, MeridianScorer **out);
void meridian_scorer_free(MeridianScorer *s); /* NULL is a no-op; free each scorer once */
/* Thread-safe. ids[n] and out[n] must not overlap. On error, out is unspecified. */
int32_t meridian_score_batch(const MeridianScorer *s, const uint64_t *ids, size_t n, int32_t *out);
#endif
```

The two `_Static_assert`s are the C half of Chapter 16.1's layout rule: the Rust crate asserts its offsets with
`offset_of!`, and every C translation unit that includes the header asserts the same numbers. The client (excerpt):

```c
int main(void) {
    if (meridian_abi_version() != MERIDIAN_ABI_VERSION) {
        fprintf(stderr, "meridian ABI mismatch\n");
        return 1;
    }
    MeridianConfig cfg = { .block_at = 80, .review_at = 50 };
    MeridianScorer *s = NULL;
    printf("scorer_new(80/50)         -> rc=%d\n", meridian_scorer_new(&cfg, &s));

    uint64_t ids[3] = { 1001, 1002, 1003 };
    int32_t out[3] = { 0 };
    int32_t rc = meridian_score_batch(s, ids, 3, out);
    printf("score_batch(3 ids)        -> rc=%d scores=[%d, %d, %d]\n", rc, out[0], out[1], out[2]);
```

The run, with the first two lines of the client's stderr:

```text
built /tmp/meridian-16-3/libmeridian_fraud.so (rustc) and client (gcc -Werror): OK

scorer_new(80/50)         -> rc=0
score_batch(3 ids)        -> rc=0 scores=[83, 13, 49]
score_batch(NULL, n=3)    -> rc=-1
score_batch(NULL, n=0)    -> rc=0
score_batch([1001, 4242]) -> rc=-99
scorer_new(40/60)         -> rc=-1, out left NULL: yes
C main returns normally after a contained Rust panic
  client stderr | thread '<unnamed>' (69) panicked at /tmp/meridian-16-3/meridian_fraud.rs:103:27:
  client stderr | no entry found for key
```

Same codes as the Rust-played caller, now from a C program that knows nothing about Rust. Two details only a real
foreign caller shows. [RUNTIME] The panic hook still runs inside the library and writes to the *host process's*
stderr, which for the JVM is the service's console log. And the thread is `'<unnamed>'`: Rust's std didn't create C's
main thread, so it has no name for it. The fraud library therefore installs its own hook when the first scorer is
created, routing panic messages to the service's structured log with the thread and transaction.

**The lint that guards the waist.** An export that takes a Rust type only works when the caller is Rust (listing
`ch03-05-improper-ctypes-definitions.rs`, lint denied):

```rust,compile_fail
#![deny(improper_ctypes_definitions)]

#[unsafe(no_mangle)]
pub extern "C" fn meridian_merchant_known(merchant: &str) -> bool {
    merchant.starts_with("m-")
}
```

```text
error: `extern` fn uses type `str`, which is not FFI-safe
 --> src/main.rs:7:53
  |
7 | pub extern "C" fn meridian_merchant_known(merchant: &str) -> bool {
  |                                                     ^^^^ not FFI-safe
  |
  = help: consider using `*const u8` and a length instead
  = note: string slices have no C equivalent
```

The help text is rule 7. Remember its blind spot from Chapter 16.1's debugging exercise (listing
`ch01-11-lint-blind-spot.rs`): the lint checks what's passed *by value*, and doesn't look inside a struct passed by
pointer. Rules 5 to 8 still need review.

---

## Pass 2 · Systems level — *Symbols, loaders, and whose stack this is*

### 4. Under the hood

**What `#[unsafe(no_mangle)]` changes.** Listing `ch03-02-export-symbols.rs` has the same function twice. Release
assembly:

```text
playground::abi_version_rust:
	mov	eax, 3
	ret

meridian_abi_version:
	mov	eax, 3
	ret
```

Same code, different names. The first is v0-mangled in the object file (`_RNvCs…16abi_version_rust`, shown demangled
here; Chapter 18.6), a name that encodes the crate hash and changes with the compiler, so no C program or JVM could
look it up. The second is the literal string `meridian_abi_version`. [LANG] That's all `no_mangle` does, and it's why
it's an **unsafe** attribute in edition 2024: two definitions of `meridian_abi_version` in one link can collide, and
which one wins isn't something the type system can check (Chapter 15.1).

**Exported versus merely unmangled.** A name in the object file isn't enough. The host finds a function at run time
by asking the dynamic loader, which only sees symbols in the library's **dynamic symbol table**. Listing
`ch03-04-dlopen.rs` does what the JVM does when it loads a library: `dlopen`, then `dlsym`:

```rust,ignore
/// A function pointer that can't outlive the library it points into.
pub struct Symbol<'lib, F> {
    f: F,
    _lib: PhantomData<&'lib Library>,
}

impl Library {
    pub fn open(name: &CStr) -> Result<Library, String> {
        // SAFETY: `name` is NUL-terminated. (Loading runs the library's initializers: only load
        // libraries you trust, exactly as with System.loadLibrary.)
        let h = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        NonNull::new(h).map(|handle| Library { handle }).ok_or_else(dl_error)
    }

    /// # Safety
    /// `F` must be a function-pointer type exactly matching the symbol's C signature and ABI.
    pub unsafe fn get<F: Copy>(&self, name: &CStr) -> Result<Symbol<'_, F>, String> {
        assert_eq!(size_of::<F>(), size_of::<*mut c_void>(), "F must be a function pointer");
        // SAFETY: a live handle (the invariant) and a NUL-terminated name.
        let p = unsafe { libc::dlsym(self.handle.as_ptr(), name.as_ptr()) };
        if p.is_null() {
            return Err(dl_error());
        }
        // SAFETY: the caller guarantees that F is the symbol's exact function-pointer type.
        let f = unsafe { std::mem::transmute_copy::<*mut c_void, F>(&p) };
        Ok(Symbol { f, _lib: PhantomData })
    }
}
```

```text
strlen via dlsym(libc.so.6) = 8
direct call: meridian_abi_version() = 3
dlsym(this program) failed: target/debug/playground: undefined symbol: meridian_abi_version
```

`strlen` is found in `libc.so.6`. But `meridian_abi_version`, defined `#[unsafe(no_mangle)] pub extern "C"` in the very
program doing the lookup, is not: an **executable** doesn't put its functions in the dynamic symbol table unless it's
linked with `-rdynamic` or an explicit export list. A **`cdylib`** does: [RUSTC] when you build a C-compatible shared library,
rustc exports the `#[no_mangle]` / `#[export_name]` functions and hides everything else: every mangled Rust symbol,
the crate's own and its dependencies'. That's Chapter 2.7's "a `cdylib` exports only what you mark," now observable.
Listing `ch03-06-c-calls-rust.rs` ends by listing the dynamic symbols of the library it built (`nm -D --defined-only`):

```text
nm -D --defined-only libmeridian_fraud.so:
  0000000000014d30 T meridian_abi_version
  0000000000014d40 T meridian_score_batch
  0000000000014f60 T meridian_scorer_free
  0000000000014fb0 T meridian_scorer_new
```

Four symbols, exactly the four `#[unsafe(no_mangle)]` functions. The library also contains `ffi_guard`, `slice_in`,
the `HashMap` code, and a whole copy of std's panic machinery, and none of it is visible to the loader (Part XIX reads
symbol tables properly). In a Cargo project the same result comes from the manifest (not verified here: needs a local
Cargo build):

```toml
[lib]
crate-type = ["cdylib", "rlib"]   # the .so for the JVM; the rlib for Rust tests and tools

[profile.release]
panic = "unwind"                  # the default, written down on purpose: catch_unwind needs it
```

**What `catch_unwind` costs at the waist.** Nothing on the happy path [RUSTC]: the landing pads from Chapter 16.1 §4
are only reached when a panic unwinds, and the zero-cost tables that find them are consulted only then. Chapter 8.3
measured the unhappy path: about 1.8 µs for a panic caught one frame up (one run, noisy), which is fine for a code that
means "a bug happened, alert."

**The FFM side of a downcall.** [RUNTIME] When Java calls `invokeExact` on a downcall handle, the JVM runs a stub it
generated from the `FunctionDescriptor`: it moves each Java argument into the register or stack slot the platform's C
convention assigns (the same System V classification as Chapter 16.1, done by the JVM's `Linker`), switches the thread
from "running Java" to "running native" so the GC doesn't wait for it, and calls the symbol's address. When the call
returns, it switches back and converts the return value. No C glue, no JNI function table.

### 5. Memory

A batch call, with every byte's owner:

```text
 JVM heap                    native memory (Java's confined Arena)           Rust heap (the library)
 ┌──────────────┐            ┌──────────────────────────────┐              ┌───────────────────────────┐
 │ long[] ids   │ ─copy────► │ ids segment: n × u64         │ ◄── &[u64] ──│                           │
 │ int[] scores │ ◄─copy──── │ out segment: n × i32         │ ◄── &mut [i32]│ MeridianScorer (Box)      │
 │ FraudLibrary │            └──────────────────────────────┘   borrowed   │   Model, HashMap rows     │
 │   scorer ────┼──────── a MemorySegment holding the address ────────────►│                           │
 └──────────────┘            freed when the Arena closes                   └───────────────────────────┘
                                                                           freed by meridian_scorer_free
```

Three owners, three lifetimes, and the call borrows across all of them:

- **Java owns the buffers.** They live in an `Arena` that the Java code closes after the call. Rust only *borrows* them
  as `&[u64]` and `&mut [i32]` for the duration of `meridian_score_batch`, which is exactly what `slice_in`'s unbounded
  `'a` must be restricted to: the slices never escape the closure.
- **Rust owns the scorer.** Java holds its address, not the object. Rust's allocator made it, and only
  `meridian_scorer_free` releases it (Chapter 16.4 makes this a rule).
- **The copies are Java's choice.** `long[]` lives on the GC heap, which can move. FFM passes native memory by default,
  so the Java side copies in and out. Where that copy matters, `Linker.Option.critical(true)` lets a short downcall read
  heap arrays directly [VERSION: JDK 22], at the price of blocking the GC for the call's duration.

The `&[u64]` / `&mut [i32]` pair also explains a line in the header: *ids and out must not overlap*. Two Rust slices,
one mutable, over the same memory would break Chapter 4.1's aliasing rule. Rust can't check that for pointers from
Java, so the header states it.

### 6. CPU / OS

- **Loading.** [OS] `dlopen` maps the library's segments, resolves its own imports (libc, libm, `libgcc_s` for
  unwinding), and runs its initializers. A Rust `cdylib` has almost none of its own, which keeps loading predictable. The
  JVM loads the library once and keeps it, and so should any host: see §9 and Chapter 16.4 on unloading.
- **Whose thread, whose stack.** Rust code called through FFM runs **on the calling Java thread**, on that thread's
  native stack: 1 MiB by default on Linux x64, set by `-Xss` (the table in Part IX's Interlude). A deep recursion in the
  library can exhaust a stack it didn't size. That's one more reason the linked-accounts traversal in that Interlude uses
  a heap-allocated frontier instead of recursion.
- **Concurrency.** Dozens of JVM threads call `meridian_score_batch` at once with the same scorer. That's safe because
  `MeridianScorer: Sync` (proved in §3) and the function only takes `&MeridianScorer`. Any per-call scratch space comes
  from the calling thread (a `thread_local!` buffer or a stack array), never from shared mutable state behind a lock.
- **Per-call overhead and batching.** Predicted from mechanism: a downcall's fixed cost (the stub, the thread-state
  transitions, the arena) is tens of nanoseconds, order of magnitude, while scoring one transaction costs microseconds.
  At 50K scores/s the fixed cost is noise either way, but for cheap functions it isn't, which is why the API is
  `score_batch` and not `score_one`. The exercise below asks you to measure it; Part XX benchmarks it properly.

---

## Pass 3 · Architect level — *Designing the waist*

### 7. Trade-offs

**Error conventions for a C API:**

| Convention | Example | Pros | Cons |
|---|---|---|---|
| int code + out-params | `int32_t f(..., T *out)` | explicit, thread-safe, trivially bound from Java | no message; codes must be documented |
| code + thread-local "last error" string | `meridian_last_error()` | human-readable messages | a hidden global, per-thread lifetime rules for the string; easy to misuse across threads |
| error struct out-param | `f(..., MeridianError *err)` | code + message + detail, no globals | the caller allocates it and frees the message |
| sentinel return (NULL / -1) + errno | POSIX style | familiar | the errno pitfalls of 16.2 §10, now in *your* API |

Meridian uses int codes, logs the detail inside the library (with the transaction ID), and keeps codes coarse: `-1`
means "your input was wrong," `-99` means "our bug." The Java side can act on those two differently, and it can't act on
anything finer.

**How Java should call the library:**

| | FFM (JDK 22+) | JNI | Out of process (gRPC/HTTP) |
|---|---|---|---|
| Glue code | none (or `jextract`-generated) | C/Rust functions with `Java_...` names, per method | a service definition |
| Strings | standard UTF-8 via `allocateFrom` | Modified UTF-8 or UTF-16 (Chapter 9.2) | whatever the protocol says |
| A native crash | kills the JVM | kills the JVM | kills the sidecar; the JVM sees an error |
| Per-call cost | a downcall (tens of ns, order of magnitude) | similar | a network round trip (µs to ms) |
| Deploy coupling | library and bindings must match (version check) | same | independent deploys |

Meridian chose FFM for the fraud library because the latency budget (p99 under 5 ms, Chapter 1.2) rules out a network
hop per score, and FFM removes the C glue and the string-encoding trap that JNI had. The price is that a library bug
that escapes `catch_unwind` (an abort, or a memory-safety bug in `unsafe` code) takes the JVM with it. That's why the
library's `unsafe` is confined to the waist and runs under Miri in CI (Chapter 15.1 §9's discipline).

> **Why not `extern "C-unwind"` and let the panic become a Java exception?** Because it doesn't. The JVM's frames
> between the downcall and the Java caller can't be unwound by Rust's unwinder, and an unwind reaching them is not an
> exception, it's a broken contract. `C-unwind` is for callers that are Rust or C++ built to receive the unwind.

### 8. Java comparison

The Java side of §3, with FFM (not verified here: requires JDK 22+ and the built library; run with
`java --enable-native-access=ALL-UNNAMED`):

```java
import java.lang.foreign.*;
import java.lang.invoke.MethodHandle;
import java.nio.file.Path;
import static java.lang.foreign.ValueLayout.*;

public final class FraudLibrary implements AutoCloseable {
    private static final Linker LINKER = Linker.nativeLinker();
    private static final SymbolLookup LIB =                 // loaded once, never unloaded (Arena.global)
            SymbolLookup.libraryLookup(Path.of("/opt/meridian/lib/libmeridian_fraud.so"), Arena.global());
    private static final MethodHandle ABI_VERSION = LINKER.downcallHandle(
            LIB.find("meridian_abi_version").orElseThrow(), FunctionDescriptor.of(JAVA_INT));
    private static final MethodHandle SCORER_NEW = LINKER.downcallHandle(
            LIB.find("meridian_scorer_new").orElseThrow(), FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS));
    private static final MethodHandle SCORER_FREE = LINKER.downcallHandle(
            LIB.find("meridian_scorer_free").orElseThrow(), FunctionDescriptor.ofVoid(ADDRESS));
    private static final MethodHandle SCORE_BATCH = LINKER.downcallHandle(
            LIB.find("meridian_score_batch").orElseThrow(),
            FunctionDescriptor.of(JAVA_INT, ADDRESS, ADDRESS, JAVA_LONG, ADDRESS)); // size_t = JAVA_LONG here

    private final MemorySegment scorer;                    // the Rust Box's address; Rust owns the object

    public FraudLibrary(int blockAt, int reviewAt) throws Throwable {
        if ((int) ABI_VERSION.invokeExact() != 3) throw new IllegalStateException("meridian ABI mismatch");
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment cfg = arena.allocate(8, 4);      // MeridianConfig: two uint32_t
            cfg.set(JAVA_INT, 0, blockAt);
            cfg.set(JAVA_INT, 4, reviewAt);
            MemorySegment out = arena.allocate(ADDRESS);
            check((int) SCORER_NEW.invokeExact(cfg, out));
            scorer = out.get(ADDRESS, 0);
        }
    }

    public int[] scoreBatch(long[] ids) throws Throwable {  // safe to call from many threads at once
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment in = arena.allocateFrom(JAVA_LONG, ids);
            MemorySegment out = arena.allocate(JAVA_INT, ids.length);
            check((int) SCORE_BATCH.invokeExact(scorer, in, (long) ids.length, out));
            return out.toArray(JAVA_INT);
        }
    }

    @Override public void close() {                         // after every scoreBatch has returned
        try { SCORER_FREE.invokeExact(scorer); } catch (Throwable t) { throw new IllegalStateException(t); }
    }

    private static void check(int rc) {
        switch (rc) {
            case 0 -> { }
            case -1 -> throw new IllegalArgumentException("meridian: invalid input");
            case -99 -> throw new IllegalStateException("meridian: internal error (library bug; alerted)");
            default -> throw new IllegalStateException("meridian: unknown code " + rc);
        }
    }
}
```

In production the `MethodHandle`s come from `jextract` instead of being written by hand (not verified here: requires
the `jextract` tool; run `jextract --output src/main/java -t com.meridian.fraud.ffi -l meridian_fraud
include/meridian_fraud.h`), which removes the one part of this class that can silently disagree with the header: the
`FunctionDescriptor`s.

**What JNI would have required** for the same call: a Rust function per Java `native` method, named after the Java
class, using `extern "system"` (JNI's `JNICALL`, which is stdcall on 32-bit Windows, Chapter 16.1 §6):

```rust,ignore
// Not verified here: requires the `jni` crate and a JVM.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_meridian_fraud_Native_scoreBatch(
    env: JNIEnv, _class: JClass, scorer: jlong, ids: JLongArray, out: JIntArray,
) -> jint { /* copy the arrays through JNIEnv calls, then call the same score_all */ }
```

Every array and string goes through `JNIEnv` calls, each with its own copy-or-pin semantics, and strings arrive in
Modified UTF-8 unless you ask for UTF-16. That last point is Chapter 9.2's incident, and listing `ch03-03-java-strings.rs`
(verified, clean under Miri) replays it from the library's side. One name, three encodings, two entry points that
validate and never repair:

```text
FFM, standard UTF-8   [5A, 6F, C3, AB, 20, F0, 9F, 98, 80]
    -> rc=0 hash=b1b67e19b6344899
JNI GetStringChars    [005A, 006F, 00EB, 0020, D83D, DE00]
    -> rc=0 hash=b1b67e19b6344899
JNI GetStringUTFChars [5A, 6F, C3, AB, 20, ED, A0, BD, ED, B8, 80]
    -> rc=-1 (rejected: not UTF-8)
the lossy "fix": 6 x U+FFFD instead of the emoji -> hash=34698a2334c1683e
```

FFM's bytes and JNI's UTF-16 code units produce the same feature hash, because both are exact encodings of "Zoë 😀".
Modified UTF-8 encodes the emoji as two 3-byte surrogate sequences (`ED A0 BD ED B8 80`), which isn't UTF-8, so the
FFM entry point rejects it with `-1`. The last line is the incident: a "fix" that swallowed the error by replacing the
bytes and silently changed the feature for exactly the customers with emoji in their names. **Entry points validate;
they never repair.**

> **Analogy limit.** Java's scorer handle is a `MemorySegment` holding an address, and its `close()` is a method anyone
> can call at any time, from any thread, twice. The Java type system knows nothing about "don't call `scoreBatch` after
> `close`." In Rust the same rule is ownership (`Drop` takes the value; nothing can use it afterwards), checked at compile
> time. At the waist, neither side's rules exist: the header's prose is the only contract. Chapter 16.4 shows how to
> make a stale handle an error code instead of undefined behavior.

### 9. Production scenario

**Meridian's fraud library in the scoring service.** The pieces from this Part, as deployed:

1. **Build:** `crate-type = ["cdylib", "rlib"]`, `panic = "unwind"`, the header generated by `cbindgen` in CI and
   diffed against the committed one. The job fails if they differ, so a signature change can't reach Java unreviewed.
2. **Bindings:** `jextract` output checked in next to `FraudLibrary`, regenerated by the same CI job.
3. **Load:** once per JVM, into `Arena.global()`, so the library is never unloaded while handles or function pointers
   into it exist. The constructor calls `meridian_abi_version()` and refuses to start on a mismatch (16.1 §9).
4. **Calls:** batches of up to 256 IDs per downcall, from the scoring pool's threads, each with its own confined arena
   per call. One `MeridianScorer` for the process, shared by all threads. The hourly model reload happens *inside* it
   (Chapter 14.5's `ArcSwap<Model>`, which keeps it `Sync`), so Java never replaces a handle while batches are in
   flight.
5. **Errors:** `-1` becomes `IllegalArgumentException` and a metric; `-99` becomes `IllegalStateException`, an alert,
   and the scoring path's circuit breaker trips to the fallback rules (Chapter 8.3's policy).

**Plugins, finishing Chapter 6.5.** The same loader works for Rust-to-Rust plugins, and the container makes it
verifiable. Listing `ch03-07-rust-plugin.rs` writes Chapter 6.5's plugin side (the `LargeAmount` rule, its
`RuleVTable`, and an exported `new_large_amount`, plus a `rule_plugin_abi_version`) to `/tmp`, builds it with
`rustc --crate-type cdylib`, and loads it into the running program with `dlopen`. Chapter 6.5's `FfiRule` stored
`vtable: &'static RuleVTable`, and with a plugin that `'static` is false: the table, the `score` code, and the drop
function all live in the library's memory and disappear at `dlclose`. So the host's type borrows the library:

```rust,ignore
/// A rule whose code, vtable, and state live in a loaded library: it can't outlive that library.
pub struct FfiRule<'lib> {
    raw: RawRule,
    _lib: PhantomData<&'lib Library>,
}

impl Drop for FfiRule<'_> {
    fn drop(&mut self) {
        println!("  rule dropped: the plugin frees its own state");
        // SAFETY: the vtable is live (see `score`); `data` came from the plugin's constructor and is
        // released exactly once, by the plugin's own drop entry (so by the plugin's allocator).
        unsafe { ((*self.raw.vtable).drop)(self.raw.data) }
    }
}

fn load_large_amount(lib: &Library, over_cents: i64) -> Result<FfiRule<'_>, String> {
    // SAFETY: the plugin ABI (version 1) declares exactly these two signatures.
    let version = unsafe { lib.get::<extern "C" fn() -> u32>(c"rule_plugin_abi_version")? };
    if version() != 1 {
        return Err(format!("unsupported plugin ABI {}", version()));
    }
    let ctor = unsafe { lib.get::<extern "C" fn(i64) -> RawRule>(c"new_large_amount")? };
    Ok(FfiRule { raw: ctor(over_cents), _lib: PhantomData })
}
```

The host also carries the counting global allocator from Part III, to watch where the plugin's memory comes from:

```text
plugin exports: ["new_large_amount", "rule_plugin_abi_version"]
host allocator during load + new_large_amount: +0 allocs
score(1_200) = 0, score(750_000) = 60
  rule dropped: the plugin frees its own state
host allocator during the rule's drop: +0 frees
  dlclose(plugin)
```

`new_large_amount` executes `Box::new`, and the drop entry frees that `Box`, yet the host's allocator counted neither.
[RUNTIME] The plugin is a complete Rust program image with its own copy of std and its own global allocator, and the
two never meet. That's why Chapter 6.5 made the plugin supply the drop function, and it's Chapter 16.4's first rule,
measured. The order of the last two lines is the lifetime at work: every `FfiRule<'_>` is dropped before the
`Library`, and code that tries otherwise gets Chapter 16.4's E0505.

In production, the `libloading` crate provides the same `Library`/`Symbol<'lib>` pair (not on the Playground; the
listing's thirty lines of `dlopen` wrapping stand in for it). Meridian's rule host takes the other honest option for
the `'static` problem, never unloading: rules are loaded once at startup and every `Library` stays alive for the
process, as the JVM does with `Arena.global()`. It's documented where the `'static` is.

### 10. Failure scenario

**The safe signature.** Chapter 8.3's first `meridian_score` was declared as a *safe* function (excerpt of
`listings/part-08/ch03-06-ffi-boundary.rs`):

```rust,ignore
#[unsafe(no_mangle)]
pub extern "C" fn meridian_score(amount_cents: i64, out: *mut i32) -> i32 {
```

For the JVM that made no difference: C and Java don't see the `unsafe` keyword either way. Then the fraud-model backfill
job (Chapter 11.6's design exercise), written in Rust, linked the library's `rlib` and called the same exported entry
points "to score exactly the way production does." Its warm-up loop passed `std::ptr::null_mut()` for `out`, because it
didn't need the result. Because the function was safe, that line needed no `unsafe` block, and nothing in review drew
anyone's eye to it. The job crashed on its first call with a segmentation fault, in a crate containing no `unsafe` code
at all.

That's Chapter 15.1's definition of **unsound**: a safe function that a safe caller can make misbehave. The function
wrote through a pointer it had never checked, and its declaration told Rust callers they didn't need to care. The fix
was rules 2 and 8 of §2:

- declare it `pub unsafe extern "C" fn`, with a `# Safety` section, so a Rust caller has to write `unsafe` and read the
  contract (C and Java are unaffected: the symbol and the ABI are the same);
- check what can be checked (`out.is_null()` → `MERIDIAN_ERR_INVALID`), exactly as `meridian_scorer_new` does.

Clippy's `not_unsafe_ptr_arg_deref` lint flags the simplest form of this (a public safe function dereferencing a raw
pointer argument). Meridian's FFI crates enable it, and the review checklist has the general rule: **an exported
function that trusts a pointer is `unsafe` to call, even if its only intended caller is Java.**

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVI).*

1. List the rules of a C API written in Rust. Which of them does the compiler enforce, which does a lint enforce, and
   which only review can enforce?
2. What exactly does `#[unsafe(no_mangle)]` do, and why is it an unsafe attribute? What does a `cdylib` export that an
   executable doesn't?
3. Why does `slice_in` return an empty slice for `n == 0` without looking at the pointer? What would
   `slice::from_raw_parts(NULL, 0)` be?
4. Why must `ids` and `out` not overlap in `meridian_score_batch`, and why can't Rust check it?
5. Walk through a downcall from Java's `invokeExact` to Rust's `score_all` and back. Where are the arguments at each
   step, and on which stack does the Rust code run?
6. The header says a scorer is thread-safe. How does the Rust code *prove* that, and what would break the proof?
7. Why is `extern "C-unwind"` wrong for a function the JVM calls, even though it would let the panic "escape"?
8. FFM versus JNI versus a sidecar for a fraud library with a 5 ms p99 budget: argue for one, and say what would change
   your answer.
9. Chapter 8.3's `meridian_score` was a safe `extern "C" fn` that wrote through a raw pointer. Is it sound? For which
   callers does the answer matter?

### 12. Exercises

- **Beginner.** Add `meridian_score_one(s, id, out)` to listing `ch03-01-fraud-c-api.rs` as a thin wrapper over
  `score_all`. Write its `# Safety` section and its header line.
- **Intermediate.** Add a thread-local "last error" facility: `meridian_last_error(char *buf, size_t cap)` copying the
  most recent error message *for the calling thread* into a caller buffer. Justify each lifetime rule in its header
  comment.
- **Advanced.** Make `meridian_score_batch` reject overlapping `ids`/`out` ranges by comparing addresses. Is that check
  complete? Is it worth doing? What does Chapter 15.2 say about comparing addresses from different allocations?
- **Systems.** With JDK 22+ locally, benchmark `scoreBatch` with batch sizes 1, 16, 256, and 4,096 using JMH. Plot
  nanoseconds per ID and find the batch size where the per-call overhead stops mattering. Repeat with
  `Linker.Option.critical(true)` and heap arrays.
- **Architecture.** The C++ market-data team wants to call the fraud library directly from C++. Decide what changes in
  the header (namespaces? `extern "C"` guards? RAII wrappers?), whether they may use `extern "C-unwind"`, and who owns
  the C++ wrapper.

### 13. Debugging exercise

A new export, reviewed and merged on a Friday (a review sketch, not a listing):

```rust,ignore
#[unsafe(no_mangle)]
pub extern "C" fn meridian_top_features(s: &MeridianScorer, id: u64, out: *mut *const c_char) -> i32 {
    let names = s.model.top_features(id);          // Vec<String>, panics if the model has no row for `id`
    let joined = CString::new(names.join(",")).unwrap();
    unsafe { *out = joined.as_ptr() };
    MERIDIAN_OK
}
```

The Java team reports that it "usually works," that the strings are sometimes garbage, and that a few scoring pods were
restarted over the weekend with no Java stack trace.

1. Explain the restarts. Which rule of §2 is missing, and what exactly happens in the process when `top_features`
   panics?
2. Explain the garbage strings: who owns the bytes `*out` points to, and when are they freed?
3. `s: &MeridianScorer` compiles and has no lint warning. What does a reference parameter in an exported function
   promise the Rust compiler, and why can't Java keep that promise? Rewrite the function following all ten rules, and
   choose between the two output designs of Chapter 16.4 for the string.

### 14. Design exercise

**A Rust library for three callers.** Meridian's tokenization library (card numbers in, tokens out) is being rewritten
in Rust. It will be called by the Java payments service (FFM), by a Go service (cgo), and by Rust services directly.
Design:

- the crate structure: one crate with a `ffi` module, or a core crate plus a separate `-ffi` crate? Where do the
  `#[no_mangle]` functions live, and what does the Rust-native API look like?
- the error convention and the handle design, given that cgo calls cost more than FFM downcalls (so batching matters
  more) and that Go's garbage collector forbids C code from keeping Go pointers after a call returns;
- how secrets (card numbers) cross the boundary: who allocates the buffers, and who is responsible for zeroing them;
- the release process: header generation, bindings for each language, and the version check.
