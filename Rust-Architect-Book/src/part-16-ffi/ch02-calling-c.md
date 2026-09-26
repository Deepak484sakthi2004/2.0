# Chapter 16.2 — Calling C from Rust

> **Where this sits:** Part XVI · FFI and Systems Programming · chapter 2 of 4
> **Prerequisites:** Chapter 16.1 (ABIs, `repr(C)`), Chapter 15.1 (`unsafe extern`, `safe` items, `# Safety` and
> `// SAFETY:`), Chapter 9.2 (text encodings, `OsString`), Chapter 8.1 (`Result` and `io::Error`), Chapter 3.5 (RAII).
> **After this chapter you can:** declare a C function correctly and say exactly what the declaration promises; turn C's
> contracts (NUL termination, buffer sizes, `errno`, ownership, thread affinity) into Rust types so that callers can't
> break them; convert strings across the boundary without losing or inventing data; pass a Rust callback into C; and
> lay out a binding as a raw `sys` layer plus a safe API.

---

## Pass 1 · User level — *Turning a C contract into a Rust type*

### 1. Problem

A C function's contract lives in its documentation, not in its signature. `size_t strlen(const char *s)` doesn't say
"`s` must be NUL-terminated," and `char *getenv(const char *name)` doesn't say "the result may be invalidated by the
next `setenv`." `snprintf` doesn't say that `%lld` requires a `long long`. `open` doesn't say that on failure the reason
is in a thread-local variable that the next libc call may overwrite. A C programmer carries these rules in their head.
A Rust caller can't: the compiler checks what the types say and nothing else.

Rust code still has to call C. The operating system's interface is C (Chapter 16.1's ladder). So are most vendor SDKs,
compression and crypto libraries, and database drivers. Meridian's fraud library calls a vendor's C scoring engine
(Chapter 11.2's design exercise). The job of this chapter is the discipline that makes those calls routine:

> **Each C function gets one small, reviewed `unsafe` block, and a safe Rust function around it whose *parameter and
> return types* carry the C contract, so that no caller can break it without writing `unsafe` themselves.**

That's Chapter 15.1's soundness rule, applied to a foreign function.

### 2. Mental model

**Three promises, three places.** Calling C involves three different acts, and each carries its own obligation:

```text
 unsafe extern "C" { fn strlen(s: *const c_char) -> usize; }    DECLARATION: "this signature matches the C
                                                                  definition exactly" (wrong = UB at every call)
 unsafe { strlen(p) }                                            CALL: "the preconditions hold right now"
                                                                  (p is non-null, NUL-terminated, alive)
 pub fn c_len(s: &CStr) -> usize                                 SAFE WRAPPER: "the types make the preconditions
                                                                  impossible to violate" (&CStr can't be anything else)
```

**The translation table.** Nearly every clause of a C contract has a Rust type or pattern that enforces it:

| The C documentation says... | The Rust wrapper uses... | Why it's enforced |
|---|---|---|
| "a NUL-terminated string, valid during the call" | `&CStr` parameter | a `&CStr` is NUL-terminated by construction and borrowed for the call |
| "a buffer of `n` bytes" | `&[u8]` / `&mut [u8]`, passing `(as_ptr(), len())` | the slice carries its own length |
| "may be NULL" | `Option<&T>`, or a null check that becomes `None` | Chapter 16.1's nullable-pointer guarantee |
| "returns -1 and sets `errno`" | `io::Result<T>` via `io::Error::last_os_error()` | the error can't be ignored or misread |
| "the caller must `free()` / `close()` it" | an owning type with `Drop` | release happens exactly once, on every path |
| "valid until the next call on this object" | copy it out before returning; take `&mut self` | no pointer escapes, and no other call can intervene |
| "not thread-safe" / "use on one thread" | a `!Send` / `!Sync` type | the compiler rejects crossing threads (Chapter 16.4) |
| "no preconditions" | a `safe fn` item in the `unsafe extern` block (edition 2024) | callers need no `unsafe` at all |
| "the format must match the arguments" | a fixed format inside the wrapper | callers never supply one |

When a clause has no type that can enforce it, the wrapper is an `unsafe fn` with a `# Safety` section, and the
obligation moves to the caller. That's the honest outcome, not a failure (§7).

### 3. Rust code

**Two functions, two wrappers** (listing `ch02-01-strlen-getenv.rs`, verified, clean under Miri):

```rust
// Two C functions, declared by hand, each wrapped in a safe function whose TYPES carry the contract.
use std::ffi::{CStr, OsString, c_char};
use std::os::unix::ffi::OsStringExt;

mod sys {
    use std::ffi::c_char;

    unsafe extern "C" {
        /// # Safety
        /// `s` must point to a NUL-terminated byte string, readable up to and including the NUL.
        pub fn strlen(s: *const c_char) -> usize;

        /// # Safety
        /// `name` must be NUL-terminated. The result is NULL or points into the process environment,
        /// and it's only valid until the environment is next modified (setenv/putenv/unsetenv).
        pub fn getenv(name: *const c_char) -> *mut c_char;
    }
}

/// Safe: a `&CStr` is NUL-terminated by construction and stays alive for the whole call.
pub fn c_len(s: &CStr) -> usize {
    // SAFETY: `s.as_ptr()` points to a NUL-terminated string borrowed for the duration of the call.
    unsafe { sys::strlen(s.as_ptr()) }
}

/// Safe: copies the value out before returning, so no pointer into the environment escapes.
/// Contract kept by this program: nothing modifies the environment concurrently (in edition 2024
/// `std::env::set_var` is `unsafe` for exactly this reason).
pub fn env_var(name: &CStr) -> Option<OsString> {
    // SAFETY: `name` is NUL-terminated.
    let p: *mut c_char = unsafe { sys::getenv(name.as_ptr()) };
    if p.is_null() {
        return None;
    }
    // SAFETY: non-null means a NUL-terminated string inside the environment block, and it's still
    // valid because nothing has run since the call. `to_bytes().to_vec()` copies it immediately.
    let bytes = unsafe { CStr::from_ptr(p) }.to_bytes().to_vec();
    Some(OsString::from_vec(bytes)) // Unix environment values are bytes, not necessarily UTF-8
}

fn main() {
    println!("c_len(c\"meridian\") = {}", c_len(c"meridian"));
    println!("env_var(MERIDIAN_MODEL_DIR) = {:?}", env_var(c"MERIDIAN_MODEL_DIR"));
    println!("env_var(PATH) is set: {}", env_var(c"PATH").is_some());
    // std's own wrapper, for comparison: same answer, plus a lock shared with std's set_var.
    println!("std::env::var_os(MERIDIAN_MODEL_DIR) = {:?}", std::env::var_os("MERIDIAN_MODEL_DIR"));
}
```

```text
c_len(c"meridian") = 8
env_var(MERIDIAN_MODEL_DIR) = None
env_var(PATH) is set: true
std::env::var_os(MERIDIAN_MODEL_DIR) = None
```

Look at what each wrapper's *signature* does. `c_len` takes `&CStr`, so the only way to call it is with something
NUL-terminated that outlives the call. `env_var` returns an owned `OsString`, so the caller never holds a pointer into
the environment block. That's the whole reason it copies: `getenv`'s pointer is valid "until the environment is next
modified," a lifetime Rust can't express, so the wrapper doesn't try. [LIB] std's `env::var_os` does the same and
also takes an internal lock shared with `env::set_var`. The lock only covers *Rust's* own calls: a C library calling
`setenv` on another thread doesn't take it. [VERSION] That's why edition 2024 made `std::env::set_var` itself
`unsafe`.

**Strings at the boundary** (listing `ch02-02-c-strings.rs`, verified, clean under Miri). Every conversion that can
fail says so in its type:

```text
CString::new("merchant-42") -> 12 bytes with the NUL
CString::new("a\0b") -> Err(1)                        interior NUL at byte 1: C would see "a"
c"fraud" -> "fraud", count_bytes() = 5                a C-string literal (Rust 1.77+), NUL included, no allocation
from_bytes_with_nul(b"ok\0")    -> Ok("ok")
from_bytes_with_nul(b"ok\0xx")  -> is_err: true       the NUL must be exactly at the end...
from_bytes_until_nul(b"ok\0xx") -> Ok("ok")           ...or use the "stop at the first NUL" variant
to_str() on Latin-1 bytes -> Err(2)                   C promises bytes, not UTF-8
to_string_lossy()         -> "Zo�"                    replaced: fine for a log line, wrong for data (9.2)
as a Path (exact bytes)   -> "Zo\xEB"                 OsStr keeps the bytes exactly
```

(Annotations added on the right.) The types line up with Chapter 9.2's rule: `CString`/`&CStr` for C's bytes-plus-NUL,
`OsString`/`&OsStr` for OS data that is bytes on Unix, and `String`/`&str` only after validation.

**A variadic function** (listing `ch02-03-snprintf.rs`, verified in debug and release). Variadic C functions check
nothing: the format string decides how many arguments are read and as what type. The wrapper owns the format, so
callers can't get it wrong, and it uses the return value to detect truncation:

```rust,ignore
unsafe extern "C" {
    /// # Safety
    /// `buf` must be writable for `size` bytes; `fmt` must be NUL-terminated; each variadic argument
    /// must have exactly the type its conversion specifier expects (C checks none of this).
    fn snprintf(buf: *mut c_char, size: usize, fmt: *const c_char, ...) -> c_int;
}

/// "EUR 1234.05" from minor units, formatted by the C library.
pub fn format_amount(currency: &CStr, cents: i64) -> Result<String, FormatError> {
    let sign: &CStr = if cents < 0 { c"-" } else { c"" };
    let abs = cents.unsigned_abs();
    let (major, minor) = (abs / 100, abs % 100);
    let mut buf = vec![0u8; 8]; // deliberately small: the first attempt truncates for large amounts
    loop {
        // SAFETY: `buf` is writable for `buf.len()` bytes. The format is a literal, and each argument
        // matches its conversion: %s <- NUL-terminated *const c_char (x2), %llu <- c_ulonglong (x2).
        let n = unsafe {
            snprintf(
                buf.as_mut_ptr().cast::<c_char>(),
                buf.len(),
                c"%s %s%llu.%02llu".as_ptr(),
                currency.as_ptr(),
                sign.as_ptr(),
                major as c_ulonglong,
                minor as c_ulonglong,
            )
        };
        if n < 0 {
            return Err(FormatError); // an encoding error: C's only failure signal here
        }
        let needed = n as usize; // the length it WANTED to write, excluding the NUL
        if needed < buf.len() {
            buf.truncate(needed); // drop the NUL and the unused tail
            return String::from_utf8(buf).map_err(|_| FormatError);
        }
        println!("  (buffer of {} bytes was too small: snprintf needs {} + NUL; retrying)", buf.len(), needed);
        buf.resize(needed + 1, 0);
    }
}
```

```text
  (buffer of 8 bytes was too small: snprintf needs 9 + NUL; retrying)
Ok("EUR 10.50")
  (buffer of 8 bytes was too small: snprintf needs 15 + NUL; retrying)
Ok("USD -1234567.89")
```

The compiler does apply one of C's rules for variadics: the default argument promotions. A `float` passed to `...` is
promoted to `double` in C, and Rust refuses to guess (listing `ch02-04-variadic-f32.rs`):

```text
error[E0617]: can't pass `f32` to variadic function
  --> src/main.rs:13:85
   |
13 |     let n = unsafe { snprintf(buf.as_mut_ptr().cast(), buf.len(), c"%.2f".as_ptr(), score) };
   |                                                                                     ^^^^^
   |
help: cast the value to `c_double`
```

**A callback into Rust: `qsort_r`** (listing `ch02-06-qsort-r.rs`, verified in debug and release). glibc's `qsort_r`
sorts an array with a comparator you supply, plus a `void *` it passes back to every comparator call. That's the
standard C shape for a callback with state:

```rust,ignore
/// The C type of the comparator: (a, b, user data) -> <0, 0, >0.
type Compar = unsafe extern "C" fn(*const c_void, *const c_void, *mut c_void) -> c_int;

unsafe extern "C" {
    /// glibc's signature. (BSD and macOS have a qsort_r with a DIFFERENT argument order: the libc
    /// crate declares each platform's version; a hand-written declaration is per-platform.)
    ///
    /// # Safety
    /// `base` must point to `nmemb` elements of `size` bytes each, valid for reads and writes;
    /// `compar` must define a consistent total order (C11 7.22.5: anything else is undefined);
    /// `arg` is passed through to every `compar` call unchanged.
    fn qsort_r(base: *mut c_void, nmemb: usize, size: usize, compar: Compar, arg: *mut c_void);
}

struct RankCtx<'a> {
    scores: &'a [f64],
}

/// Descending by score, ties broken by index: a total order, because `f64::total_cmp` is one
/// (NaN included) and indices are distinct.
unsafe extern "C" fn by_score_desc(a: *const c_void, b: *const c_void, arg: *mut c_void) -> c_int {
    // SAFETY: qsort_r passes back the `arg` given below: a `RankCtx` that outlives the sort.
    let ctx = unsafe { &*(arg as *const RankCtx<'_>) };
    // SAFETY: a and b point to elements of the u32 index array being sorted.
    let (ia, ib) = unsafe { (*(a as *const u32), *(b as *const u32)) };
    let (sa, sb) = (ctx.scores[ia as usize], ctx.scores[ib as usize]);
    match sb.total_cmp(&sa).then(ia.cmp(&ib)) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// Safe API: indices of `scores`, highest score first.
pub fn rank_by_score(scores: &[f64]) -> Vec<u32> {
    assert!(scores.len() <= u32::MAX as usize);
    let mut idx: Vec<u32> = (0..scores.len() as u32).collect();
    let ctx = RankCtx { scores };
    // SAFETY: `idx` holds `idx.len()` u32s, valid for reads and writes, and is not otherwise borrowed
    // during the call; the comparator is a total order over those indices (see above), and every
    // index is in bounds for `scores`; `ctx` lives until qsort_r returns, and C never keeps `arg`.
    unsafe {
        qsort_r(
            idx.as_mut_ptr().cast(),
            idx.len(),
            size_of::<u32>(),
            by_score_desc,
            (&ctx as *const RankCtx<'_>).cast_mut().cast(),
        )
    };
    idx
}
```

```text
scores  = [0.12, 0.97, 0.45, 0.97, NaN, 0.03]
ranking = [4, 1, 3, 2, 0, 5]
```

Three details carry the design:

- **The comparator is the wrapper's own code.** C11 §7.22.5 requires the comparison to be a consistent total order
  and leaves everything else undefined. A safe `sort_by(v, |a, b| ...)` over `qsort_r` would hand that requirement to
  arbitrary safe closures, and safe code can write an inconsistent one. Chapter 15.1's rule applies: unsafe code may not
  rely on a *safe* caller-supplied ordering. So the public API accepts data (`&[f64]`), not behavior, and the one
  comparator it passes is provably total. A generic comparator version would have to be an `unsafe fn`.
- **`total_cmp` is total, NaN included.** Positive NaN sorts above every number, which is why index 4 ranks first.
  For a fraud ranking, that's a reason to reject non-finite scores *before* the call (exercise).
- **The comparator can't panic across C.** It's `extern "C"`, so a panic inside would abort the process (Chapter
  16.1), and C's `qsort_r` gives no way to report "stop." That's acceptable here only because the comparator does
  nothing that can panic: all its indices are in bounds by construction. Chapter 16.4 shows the pattern for callbacks
  whose C API *does* allow stopping.

**Errors: `-1` plus `errno`** (listing `ch02-07-last-os-error.rs`, verified):

```rust,ignore
pub fn open_readonly(path: &CStr) -> io::Result<OwnedFd> {
    // SAFETY: `path` is NUL-terminated and outlives the call; O_RDONLY | O_CLOEXEC need no mode argument.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        // Read errno IMMEDIATELY: the next libc call on this thread may overwrite it.
        return Err(io::Error::last_os_error());
    }
    // SAFETY: open succeeded, so `fd` is a fresh descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}
```

```text
error: No such file or directory (os error 2); kind=NotFound; raw_os_error=Some(2)
opened fd 3 (closed when `fd` is dropped)
```

Two conversions in five lines: C's `(-1, errno)` pair becomes `io::Error` with a portable `ErrorKind`, and a bare
integer descriptor becomes `OwnedFd`, which closes itself exactly once (Chapter 3.5's RAII, applied to a kernel
resource). §10 shows what "immediately" protects against.

---

## Pass 2 · Systems level — *What a foreign call compiles to*

### 4. Under the hood

**A declaration is an undefined symbol.** `unsafe extern "C" { fn strlen(...) }` emits no code. It tells rustc the
signature and tells the linker that some other object will define `strlen`. On Linux, std already links the C library,
so libc's functions resolve without any `#[link]` attribute. A vendor library needs one: `#[link(name = "vse")]` on
the extern block, or a build script printing `cargo::rustc-link-lib=vse` (not verified here: it needs the library
installed; Part XXII covers build scripts and `-sys` crates).

Listing `ch02-12-extern-call-asm.rs` wraps `strlen` twice. Release assembly (`tools/emit.ps1`, comments added):

```text
playground::c_len_plus_one:
	push	rax
	call	qword ptr [rip + strlen@GOTPCREL]    ; through the GOT: the address is filled in at load time
	inc	rax
	pop	rcx
	ret

playground::c_len:
	jmp	qword ptr [rip + strlen@GOTPCREL]    ; a tail call: the wrapper costs nothing
```

A foreign call is an ordinary indirect `call` through the Global Offset Table. The dynamic loader writes `strlen`'s real
address into the GOT slot when the program starts ([OS], Part XIX). There's no marshaling layer, no thread-state
transition, and no copying: the safe wrapper `c_len` compiled to a single jump.

The LLVM IR shows two quieter facts:

```text
define noundef i64 @…c_len(ptr noalias nofree noundef nonnull readonly captures(none) %s.0, i64 noundef %s.1)
  %_0 = tail call noundef i64 @strlen(ptr noundef nonnull dereferenceable(1) %s.0) #2

; Function Attrs: mustprogress nocallback nofree nounwind nonlazybind willreturn memory(argmem: read) uwtable
declare noundef i64 @strlen(ptr noundef captures(none)) unnamed_addr #1
```

- **`&CStr` is two words in Rust and one in C.** [LIB] `CStr` is currently an unsized type, so `c_len` receives a
  pointer and a length (`%s.0`, `%s.1`), and `as_ptr()` passes only the pointer. The std docs reserve the right to
  make `&CStr` a thin pointer later. Nothing in C-facing code should depend on either.
- **LLVM knows what `strlen` is.** The `declare` line carries `memory(argmem: read)`, `nofree`, and `willreturn`,
  which rustc didn't write: it can't know what a foreign function does. (It does add `nounwind`, because an
  `extern "C"` callee may not unwind.) [RUSTC] LLVM recognizes the names of standard C library functions and applies their
  documented semantics, which is how it can constant-fold `strlen("literal")`. The flip side: a vendor function LLVM
  doesn't recognize is fully opaque. The optimizer must assume it reads and writes any memory whose address has
  escaped, so values are reloaded after the call and it can't be inlined, unless you build with cross-language LTO
  (Part XXII).

**Lints that check the declaration and the call.** Two contracts are checked at compile time when you let them:

- `improper_ctypes` (warn by default) checks `unsafe extern` declarations. Declaring `fn puts(s: &str)` gets
  (listing `ch02-10-improper-ctypes.rs`, denied):

```text
error: `extern` block uses type `str`, which is not FFI-safe
 --> src/main.rs:8:16
  |
8 |     fn puts(s: &str) -> c_int;
  |                ^^^^ not FFI-safe
  |
  = help: consider using `*const u8` and a length instead
  = note: string slices have no C equivalent
```

- `dangling_pointers_from_temporaries` (warn by default) catches the most common C-string mistake, taking `as_ptr()`
  of a `CString` that's dropped at the end of the statement (listing `ch02-09-dangling-temporary.rs`, denied):

```rust,compile_fail
#![deny(dangling_pointers_from_temporaries)]
use std::ffi::{CString, c_char};

unsafe extern "C" {
    fn strlen(s: *const c_char) -> usize;
}

fn model_path_len(dir: &str) -> usize {
    let p = CString::new(format!("{dir}/model.bin")).unwrap().as_ptr();
    // SAFETY (intended): `p` is a NUL-terminated string... except the CString is already gone.
    unsafe { strlen(p) }
}
```

```text
error: this creates a dangling pointer because temporary `CString` is dropped at end of statement
  --> src/main.rs:12:63
   |
12 |     let p = CString::new(format!("{dir}/model.bin")).unwrap().as_ptr();
   |             ------------------------------------------------- ^^^^^^ pointer created here
   |             |
   |             this `CString` is dropped at end of statement
   |
   = help: bind the `CString` to a variable such that it outlives the pointer returned by `as_ptr`
   = note: a dangling pointer is safe, but dereferencing one is undefined behavior
```

The note says it precisely: creating the pointer is safe, and using it after the `CString` is freed would be undefined
behavior (Chapter 15.1). The fix is one line, `let path = CString::new(...)?;` followed by `path.as_ptr()` inside the
call, so the owner outlives the use. Meridian's FFI crates deny both lints.

### 5. Memory

**Who owns which bytes, in the four string types:**

```text
 String / &str           heap buffer, UTF-8, length-delimited, may contain '\0'           owned by Rust
 CString                 heap buffer: bytes + trailing NUL, no interior NUL               owned by Rust's allocator
 &CStr                   a borrowed view of bytes + NUL (currently a fat pointer)         owned by someone else
 getenv's result         a pointer into libc's environment block                          owned by libc; valid until
                                                                                           the environment changes
```

`env_var` exists because of that last line. The environment block belongs to the C library, and `setenv` may replace
or reallocate the string the pointer points into. Rust has no lifetime for "until someone, somewhere, calls `setenv`,"
so the wrapper copies the bytes into Rust-owned memory before anything else can run.

**Memory from the C allocator, owned by a Rust value.** Some C APIs take a buffer that *they* will later `free()`.
The allocation must come from `malloc`, and the Rust side should still own it until it's handed over (listing
`ch02-05-malloc-buffer.rs`, verified, clean under Miri):

```rust,ignore
/// INVARIANT: `ptr` came from `malloc(len.max(1))`, is freed exactly once (by Drop or by whoever
/// receives it from `into_raw`), and `[ptr, ptr + len)` is initialized.
pub struct CBuf {
    ptr: NonNull<u8>,
    len: usize,
}

impl CBuf {
    /// A zero-filled buffer from the C heap, or None if malloc fails.
    pub fn zeroed(len: usize) -> Option<CBuf> {
        // malloc(0) may return NULL or a unique pointer; asking for at least 1 byte avoids the ambiguity.
        // SAFETY: malloc has no preconditions.
        let raw = unsafe { malloc(len.max(1)) }.cast::<u8>();
        let ptr = NonNull::new(raw)?;
        // SAFETY: `ptr` is valid for writes of `len` bytes (we asked for at least that many).
        unsafe { ptr.as_ptr().write_bytes(0, len) };
        Some(CBuf { ptr, len })
    }

    /// Hands ownership to C. The receiver must release it with `free()`, exactly once.
    pub fn into_raw(self) -> *mut u8 {
        let p = self.ptr.as_ptr();
        std::mem::forget(self); // don't run Drop: ownership moved to the caller
        p
    }
}

impl Drop for CBuf {
    fn drop(&mut self) {
        // SAFETY: the invariant: `ptr` came from malloc and this is its only release.
        unsafe { free(self.ptr.as_ptr().cast()) }
    }
}
```

```text
[104, 101, 108, 108, 111, 0, 0, 0]
both buffers released by the allocator that created them
```

> **Safety invariant (`CBuf`).** `ptr` was returned by `malloc` for at least `len` bytes, the first `len` bytes are
> initialized, and exactly one of `Drop` or the receiver of `into_raw` calls `free` on it, once.

`as_slice` and `as_mut_slice` rely on the invariant, so every function in the module is part of the proof (Chapter
15.1's "the module is the unit of trust"). The general rule it enforces, *memory returns to the allocator that made
it*, is Chapter 16.4's subject.

### 6. CPU / OS

- **An FFI call costs a call.** [CPU] Calling C from Rust is the same instruction sequence as calling Rust from Rust:
  argument registers, a `call` (through the GOT for a shared library), a `ret`. The real costs are what the optimizer
  loses: no inlining across the boundary, and an opaque callee that forces reloads (§4). For a function doing
  microseconds of work that's nothing. For a tiny function called in a hot loop, it's the reason to batch or to port
  it (§7).
- **`errno` is thread-local.** [OS] Each thread has its own `errno` (glibc exposes it through
  `__errno_location()`), so one thread's failure can't corrupt another's error. It isn't *call*-local: any later libc
  call on the same thread may overwrite it, including calls you didn't know were libc calls (allocation, logging,
  formatting). `io::Error::last_os_error()` reads it, so call it first (§10).
- **C library functions have their own thread-safety rules.** POSIX marks some functions not thread-safe
  (`strerror`, `localtime`, `getenv` racing with `setenv`) and provides `_r` variants for several of them. [LIB] Rust's
  std uses the safe variants internally. A binding must decide per function, and encode the answer in `Send`/`Sync`.
- **Libraries can be thread-affine.** A vendor library that keeps per-engine state in thread-local storage, or uses a
  single-threaded runtime inside, requires every call on an engine to come from the thread that created it. That's a
  property of the library, invisible in its header, and it's the constraint §9's binding encodes.

---

## Pass 3 · Architect level — *Where the unsafety lives*

### 7. Trade-offs

**Where declarations come from:**

| Source | Pros | Cons | Use for |
|---|---|---|---|
| `std` | safe, portable, maintained | covers only what std needs | anything std already wraps (files, env, processes, sockets) |
| the `libc` crate | every platform's declarations, including per-platform differences like `qsort_r` | all raw and `unsafe`; you still write wrappers | OS APIs std doesn't wrap |
| hand-written `unsafe extern` | no dependency, exactly the functions you need | you own correctness per platform | a handful of functions from a stable, small C API |
| `bindgen` from the header | mechanically faithful to the header; regenerates on upgrade | needs libclang at build time; output is raw | a vendor SDK or any header with more than a few items |

**What shape the safe wrapper takes:**

| Wrapper style | Example | Choose when |
|---|---|---|
| safe function, copy out | `env_var` → `OsString` | the C result's lifetime can't be expressed in Rust |
| safe function, borrow in | `c_len(&CStr)`, `write(&[u8])` | inputs only need to live for the call |
| owning type with `Drop` | `OwnedFd`, `CBuf`, `Engine` | C hands you something to release |
| `&mut self` methods | `Engine::score` | C invalidates something "on the next call" |
| `unsafe fn` with `# Safety` | a generic comparator for `qsort_r` | the precondition can't be encoded in types |
| don't call C | `slice::sort_by` instead of `qsort_r` | the Rust version is safe, inlinable, and generic |

That last row is not a joke. `rank_by_score` exists to show the callback pattern. In production, `sort_by` with
`total_cmp` does the same job with no `unsafe` at all, and inlines the comparator. **Call C for what only C has**:
the OS, a vendor SDK, a battle-tested codec you don't want to reimplement.

> **Why not make the whole binding `unsafe fn`s and let callers sort it out?** Because every caller then carries the
> proof, forever (Chapter 15.1's "every `unsafe fn` is a tax on all future callers"). A binding with thirty call
> sites and one safe wrapper has one proof. A binding of `unsafe fn`s has thirty.

### 8. Java comparison

The same call from Java, with FFM, not verified here (requires JDK 22+; run `java --enable-native-access=ALL-UNNAMED
Strlen.java`):

```java
import java.lang.foreign.*;
import java.lang.invoke.MethodHandle;
import static java.lang.foreign.ValueLayout.*;

public class Strlen {
    public static void main(String[] args) throws Throwable {
        Linker linker = Linker.nativeLinker();
        MethodHandle strlen = linker.downcallHandle(
                linker.defaultLookup().find("strlen").orElseThrow(),
                FunctionDescriptor.of(JAVA_LONG, ADDRESS));
        try (Arena arena = Arena.ofConfined()) {
            MemorySegment s = arena.allocateFrom("meridian"); // UTF-8 + NUL, freed when the arena closes
            System.out.println((long) strlen.invokeExact(s));  // 8
        }
    }
}
```

And the JNI version, which needs C glue compiled per platform:

```c
JNIEXPORT jlong JNICALL Java_Strlen_strlen(JNIEnv *env, jclass cls, jstring s) {
    const char *utf = (*env)->GetStringUTFChars(env, s, NULL); /* Modified UTF-8 (Chapter 9.2) */
    jlong n = (jlong) strlen(utf);
    (*env)->ReleaseStringUTFChars(env, s, utf);
    return n;
}
```

| | Rust | Java FFM | Java JNI |
|---|---|---|---|
| Declaring the function | `unsafe extern "C"` block | `FunctionDescriptor` + `downcallHandle` | C glue + `native` method |
| Passing a string | `&CStr` (borrowed, zero-copy) | `arena.allocateFrom(s)` (a copy into native memory) | `GetStringUTFChars` (a copy, Modified UTF-8) |
| Freeing native memory | `Drop` at scope end | `Arena.close()` at the end of `try` | `Release...` calls you must not forget |
| Checked at run time | nothing | segment bounds, arena liveness, confinement thread | almost nothing (`-Xcheck:jni` helps) |
| Cost per call | a `call` | a downcall stub with a thread-state transition (order of tens of ns, not measured here) | a JNI transition, similar order |

> **Analogy limit.** The try-with-resources `Arena` and a Rust scope look alike: native memory lives exactly as long as
> the block. But the Arena enforces it **at run time**, per access: touching a segment after `close()` throws
> `IllegalStateException`, and touching a confined segment from another thread throws `WrongThreadException`. Rust
> enforces the same rules **at compile time**, with lifetimes and `Send`, and checks nothing at run time. And neither
> checks the *C function's* contract. A `FunctionDescriptor` that says `JAVA_INT` for a `size_t`, or a Rust declaration
> that says `i32`, is trusted by both.

### 9. Production scenario

**Binding the vendor scoring engine.** Meridian's fraud library sends transactions that its own model flags for review
to a vendor's scoring engine, `libvse`, for a second opinion. The vendor ships a C header, a shared library, and one
rule in bold: *an engine handle must be used only on the thread that opened it.* The binding has two layers (listing
`ch02-11-vse-binding.rs`, verified, clean under Miri; the vendor library is simulated at the bottom of the file by
Rust functions exported with the same C ABI, so the binding runs on the Playground).

Layer 1, `sys`, is the header transcribed (in production, generated by `bindgen`):

```rust,ignore
/// `typedef struct vse_engine vse_engine;` an opaque C type: never constructed or read in Rust.
#[repr(C)]
pub struct VseEngine {
    _private: [u8; 0],
    _marker: PhantomData<(*mut u8, PhantomPinned)>, // !Send, !Sync, !Unpin: we know nothing about it
}

unsafe extern "C" {
    /// No preconditions at all, so it can be declared `safe` (edition 2024).
    pub safe fn vse_version() -> c_int;
    /// `path` NUL-terminated; `out` writable. On VSE_OK, `*out` is an engine owned by the caller.
    pub fn vse_open(path: *const c_char, out: *mut *mut VseEngine) -> c_int;
    /// `e` from vse_open, used on the thread that opened it; `features` readable for `n` f64s.
    pub fn vse_score(e: *mut VseEngine, features: *const f64, n: usize, out: *mut f64) -> c_int;
    /// Valid until the next call on `e`. Never NULL for a valid `e`.
    pub fn vse_last_error(e: *const VseEngine) -> *const c_char;
    /// Releases `e`; `e` must not be used afterwards.
    pub fn vse_close(e: *mut VseEngine);
}
```

Layer 2 is the safe API. Each clause of the vendor's contract maps to one line of §2's table:

```rust,ignore
/// INVARIANT: `raw` came from a successful vse_open and has not been closed.
/// `PhantomData<*const ()>` makes Engine !Send and !Sync: the vendor requires one thread per engine.
pub struct Engine {
    raw: NonNull<sys::VseEngine>,
    _thread_affine: PhantomData<*const ()>,
}

impl Engine {
    /// `&mut self`: one call at a time, which is also what keeps `vse_last_error`'s pointer valid
    /// until we've copied it.
    pub fn score(&mut self, features: &[f64]) -> Result<f64, VseError> {
        let mut out = 0.0;
        // SAFETY: the invariant (a live engine); Engine is !Send, so this is the opening thread;
        // `features` is readable for `len` f64s for the call; `&mut out` is writable.
        let rc = unsafe { sys::vse_score(self.raw.as_ptr(), features.as_ptr(), features.len(), &mut out) };
        if rc == sys::VSE_OK {
            return Ok(out);
        }
        // SAFETY: a live engine; the pointer is valid until the next call on it, and none can happen
        // before `to_string_lossy().into_owned()` has copied the text (we hold `&mut self`).
        let message = unsafe { CStr::from_ptr(sys::vse_last_error(self.raw.as_ptr())) }
            .to_string_lossy() // an error message is for humans: lossy is the right policy here
            .into_owned();
        Err(VseError::Score { code: rc, message })
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: the invariant; Drop runs once, and the engine is never used again.
        unsafe { sys::vse_close(self.raw.as_ptr()) }
    }
}
```

```text
vse_version() = 402 (a `safe` foreign item: no unsafe block)
open(missing.bin): Some(Open { code: 2 })
score([0.9, 0.5, 0.1]) = Ok(0.62)
score([0.9]) = Err(Score { code: 1, message: "expected 3 features, got 1" })
```

The mapping, clause by clause: "an engine you must close" → `Drop`. "Use only on the opening thread" →
`PhantomData<*const ()>`, which makes `Engine` neither `Send` nor `Sync` (Chapter 16.4 shows the compiler rejecting a
move to another thread, and the owner-thread design the fraud service actually runs). "The error string is valid until
the next call" → `&mut self` plus an immediate copy. "`vse_version` has no preconditions" → a `safe` item, callable
without `unsafe`. That's Chapter 15.1's edition-2024 feature doing the job it was designed for in a real binding. And
`to_string_lossy` is right *here*, for a human-readable error message, for exactly the reason it was wrong in Chapter
9.2's hashed name feature.

The `sys` layer would be generated like this (not verified here: requires `bindgen-cli` and libclang):

```text
bindgen include/vse.h -o src/sys.rs \
    --allowlist-function 'vse_.*' --allowlist-var 'VSE_.*' --opaque-type vse_engine
```

### 10. Failure scenario

**"Is a directory."** In 2026 a fraud-library pod failed to start after a deploy, and its log said the model file
couldn't be opened: *Is a directory (os error 21)*. The on-call engineer spent the first half hour on the volume mount,
looking for a directory where the model file should be. There wasn't one. The model file simply wasn't there. The real
error was `ENOENT`, and the logged one came from the *logging code*. The model loader looked like this (listing
`ch02-08-errno-clobbered.rs`, verified):

```rust,ignore
fn log_failure(what: &str) {
    // Looks harmless. It also calls into libc, which is allowed to set errno.
    let _ = std::fs::OpenOptions::new().append(true).open("/"); // e.g. probing a log target
    let _ = what;
}

/// Buggy: logs first, reads errno second.
pub fn open_model_buggy(path: &CStr) -> io::Result<i32> {
    // SAFETY: `path` is NUL-terminated and outlives the call.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        log_failure("model open failed");
        return Err(io::Error::last_os_error()); // errno now belongs to log_failure's call
    }
    Ok(fd)
}
```

```text
buggy: Err("Is a directory (os error 21)")
fixed: Err("No such file or directory (os error 2)")
```

It isn't a memory bug and no `unsafe` rule was broken, which is why nothing flagged it. `errno` is a single slot per
thread, and "the most recent failure" changed between the call and the read. The fix (`open_model_fixed` in the same
listing) captures the error first: `let err = io::Error::last_os_error();` on the line right after the failing call,
then logs, then returns `err`. The review rule Meridian adopted is mechanical: **in a binding, the `last_os_error()`
call is the first statement of the failure branch.** Better still, return `io::Result` from the smallest possible
wrapper, as `open_readonly` in §3 does, so the error is captured inside the function that made the call and nothing
can run in between.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVI).*

1. Name the three distinct promises involved in calling a C function from Rust, and where each is written.
2. Why does `env_var` copy the value instead of returning `&CStr`? What lifetime would a borrowed result need, and why
   can't Rust express it?
3. What do `improper_ctypes` and `dangling_pointers_from_temporaries` check? Give one mistake each catches and one it
   can't.
4. Why is a safe, generic `sort_by(v, closure)` over `qsort_r` unsound, while `rank_by_score` is sound? Which Chapter
   15.1 rule is this?
5. What does a call through an `unsafe extern` declaration compile to on x86-64 Linux? What does the optimizer lose
   compared to a call to a Rust function in the same crate?
6. Why must `io::Error::last_os_error()` be called immediately? Is `errno` shared between threads?
7. When is a `safe fn` item in an `unsafe extern` block correct, and why would declaring `abs` or `labs` safe be a
   mistake?
8. Compare passing a string to C from Rust (`&CStr`), from Java FFM (`allocateFrom`), and from JNI
   (`GetStringUTFChars`): copies, encoding, and who frees.
9. A vendor library is documented "not thread-safe." What are the two different things that can mean, and how do you
   encode each one in Rust types?

### 12. Exercises

- **Beginner.** Write safe wrappers for `getpid` (declared `safe`), `strnlen(const char *, size_t)` taking `&[u8]`, and
  `gethostname(char *, size_t)` returning `io::Result<OsString>`. State the `# Safety` clause of each declaration.
- **Intermediate.** Change `rank_by_score` to return `Result<Vec<u32>, NonFiniteScore>` and reject NaN and infinities
  before calling C. Then write the same function with `sort_by` and `total_cmp`, and compare the two in release
  assembly: which one inlines the comparator?
- **Advanced.** Write `unsafe fn sort_by_c<T, F: FnMut(&T, &T) -> Ordering>(v: &mut [T], cmp: F)` over `qsort_r` with
  a generic trampoline. Write its `# Safety` section precisely. Then explain why no safe wrapper can remove the
  obligation, and what `slice::sort_by` does differently when the ordering is inconsistent (Chapters 9.3 and 9.4 call
  that a logic error, not UB).
- **Systems.** Build `format_amount` with a 64-byte stack buffer on the first attempt and a heap buffer only on
  truncation. Use a counting allocator (Chapter 16.4 has one) to show that the common case allocates exactly once (the
  returned `String`).
- **Architecture.** Your team must bind a C compression library with 60 functions, three of which have callbacks. Plan
  the crate structure (`-sys` crate, safe crate, features), the generation step, which functions get safe wrappers
  first, and how you'll test the wrappers under Miri when Miri can't run the C code.

### 13. Debugging exercise

A helper in the fraud library's config module (a review sketch, not a listing: it's here to be read, not run):

```rust,ignore
/// Returns the model directory from the environment, if set.
pub fn model_dir() -> Option<&'static str> {
    let name = CString::new("MERIDIAN_MODEL_DIR").unwrap();
    let p = unsafe { libc::getenv(name.as_ptr()) };
    unsafe { CStr::from_ptr(p) }.to_str().ok()
}
```

It passes its unit test (the variable is set in CI), and it's called from several worker threads during startup.

1. Find three contract violations in the last two lines of the body. For each, name the C or Rust rule and say, in
   one sentence, what goes wrong when it's violated.
2. Where did the `'static` lifetime come from? Which function let the author pick it, and why is that the most
   dangerous part of the signature?
3. Rewrite it as a safe function with no `'static` and no UTF-8 assumption, and say what a startup-time caller should do
   with a non-UTF-8 value.

### 14. Design exercise

**Binding a geolocation library.** Meridian's risk team wants IP geolocation from a C library, `libgeo`:

```c
typedef struct geo_db geo_db;
int  geo_open(const char *path, geo_db **out);              /* 0 or an errno value */
int  geo_lookup(const geo_db *db, const uint8_t ip[16],
                const char **country, double *lat, double *lon);
                /* country points into db; valid until geo_close. Thread-safe for lookups. */
void geo_close(geo_db *db);
int  geo_reload(geo_db *db, const char *path);               /* NOT thread-safe: no lookups may run */
```

Design the Rust API: the handle type and its auto traits, the lifetime of `country` in your API, how `geo_reload`'s
rule is enforced (by `&mut self`? by a lock? by a new handle and a swap?), and error mapping. Then decide whether
lookups should go through this library at all at the gateway's 400K req/s, or through a Rust-native reader of the same
file format, and name the measurement that would settle it.
