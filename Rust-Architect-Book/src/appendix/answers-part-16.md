# Appendix A — Answer Key: Part XVI

> Model answers. Write yours first. Where several answers are defensible, the key says so. Outputs quoted here come
> from the verified listings in `listings/part-16/` (rustc 1.98.1); "predicted" marks reasoning you should check.
> Where an answer says a broken contract is undefined behavior, Chapter 15.1 explains why; the key doesn't reproduce it.

---

## Chapter 16.1 — ABIs, Calling Conventions, and `repr(C)`

### Interview & architecture questions

**1. The four agreements.** Data layout (size, alignment, offsets, enum encodings): `#[repr(C)]`,
`#[repr(transparent)]`, `#[repr(u32)]`, `#[repr(C, u32)]`. Calling convention (registers, stack slots, who saves
what): `extern "C"` (or `"system"`, `"sysv64"`, `"win64"`). Symbols (the name the loader sees): `#[unsafe(no_mangle)]`
or `#[unsafe(export_name = "...")]`. Unwinding (may an unwind cross the call?): `extern "C"` (no: a panic aborts) versus
`extern "C-unwind"` (yes).

**2. No stable Rust ABI.** It lets rustc reorder fields to remove padding, use niches, pass aggregates the fastest way
it knows (the `Pair` and `Pointer` modes in §4, even skipping a copy when the callee is read-only), change all of it
between releases, and add attributes like `noalias` freely. The cost appears at a binary boundary: a separately
compiled plugin can't safely take `Box<dyn Trait>`, `Vec`, or a default-repr struct, because nothing guarantees both
sides agree. Anything crossing must be spelled in C terms (Chapter 6.5).

**3. Linux versus Windows x64.** Argument registers: `rdi, rsi, rdx, rcx, r8, r9` versus `rcx, rdx, r8, r9`. Stack:
Windows requires 32 bytes of shadow space and has no red zone; System V has a 128-byte red zone. Structs: System V
splits aggregates up to 16 bytes into INTEGER/SSE eightbytes; Windows passes only 1-, 2-, 4-, or 8-byte structs in
registers and everything else by reference to a caller-made copy. Preserved registers differ (Windows also preserves
`rdi`, `rsi`, and `xmm6`–`xmm15`). And a C type differs: `long` is 64 bits on Linux (LP64) and 32 on Windows (LLP64),
which is why `c_long` exists.

**4. The dump.** `Point` (16 bytes, two `f64`): Rust passes it as a `Pair` of scalars; C `Cast`s it into two `Float`
registers (`xmm0`, `xmm1`), and the resulting code is identical. `Triple` (24 bytes): over System V's 16-byte limit, so
class MEMORY: the C convention copies it into the caller's outgoing argument area (`OnStack`, `byval` in IR). The Rust
convention passes a pointer (`Indirect { mode: Pointer }`). In release the parameter is marked `ReadOnly` and `NoAlias`,
so the callee promises not to write through it, and `call_rust_sum` can pass the caller's own pointer on (a single
`jmp`) instead of making a copy. The C convention can't, because System V gives the callee a private copy it may modify.

**5. Guaranteed nullable pointers.** `Option<&T>`, `Option<&mut T>`, `Option<Box<T>>` (T: Sized), `Option<NonNull<T>>`,
`Option<fn(...)>` / `Option<extern "C" fn(...)>`, `Option<NonZero*>`, and any `repr(transparent)` wrapper around one
of these: same size, alignment, and ABI as the inner type, with `None` as all zeros. `u32` has no invalid bit pattern
(no niche), so `Option<u32>` needs a separate tag, and the default-repr `Option`'s tag placement is unspecified.

**6. Two enum layouts.** `repr(C, u32)`: a `repr(C)` struct `{ tag: u32, payload: union }`. The union contains a `u64`,
so it's 8-aligned and starts at offset 8, and `Review`'s byte is at 8. `repr(u32)`: a `repr(C)` union of `repr(C)`
structs, each beginning with the tag. `Review`'s struct is `{ u32, u8 }`, so the byte is at 4, while `Block`'s `u64` is
at 8 in both. Verified: `repr(C, u32): Review payload at offset 8`, `repr(u32): Review payload at offset 4`.

**7. Send enums, receive integers.** Every value Rust produces is a valid enum value by construction. A value from C
or Java is just bits; if the parameter's type is a Rust enum and the bits aren't a discriminant, an invalid value exists
as soon as the call starts (a validity invariant, Chapter 15.1), before any code can check it. Use a
`repr(transparent)` integer newtype (`DecisionCode(u32)`) with `TryFrom` into the Rust enum, as listing
`ch01-04-explicit-encoding.rs` does: code 3 becomes `Err(UnknownDecisionCode(3))`.

**8. What rustc emits.** The panic is emitted as an LLVM `invoke` whose unwind edge goes to a landing pad with an
empty `filter`, which calls `core::panicking::panic_cannot_unwind` (verified in `meridian_ratio`'s IR). At run time the
unwinder finds that pad, and the process aborts with "panic in a function that cannot unwind." [VERSION] Before Rust
1.81, a panic unwinding out of an `extern "C"` function was undefined behavior; since 1.81 it's a guaranteed abort.

**9. `C-unwind`.** When the caller is built to receive an unwind: Rust code calling through a function pointer, or C++
compiled with exceptions that wants the panic to propagate (and to run its own destructors). Never for a JVM: its frames
aren't unwindable by Rust's unwinder, and an unwind reaching them isn't a Java exception.

**10. A field in the padding.** Yes, it's an ABI change: the *meaning* of bytes changes even though no offset does. An
old Java writer leaves the bytes zero (FFM zeroes new allocations), and a new library reads them as a real value; a
new writer sets a field an old library ignores. The `const` assertions still pass, because every existing offset and
the size are unchanged. What catches it: the header diff in review, and the rule that *any* change to a boundary type
bumps `MERIDIAN_ABI_VERSION`, which the load-time handshake then enforces.

### Debugging exercise (`ScoreRequest`)

1. The warning points at `ScoreRequest` in `meridian_score_req`'s by-value parameter, and it's about the field
   `override_block_at: Option<u32>` ("enum has no representation hint"). `Option<u32>` needs a tag, and where the tag
   sits and how big it is are unspecified for the default-repr `Option`, so no C declaration can mirror it.
2. `improper_ctypes_definitions` checks types passed **by value**. Behind a reference or raw pointer it doesn't inspect
   the pointee's fields, because a pointee may legitimately be opaque to C. Most C APIs pass structs by pointer, so for
   them the lint is silent (listing `ch01-11-lint-blind-spot.rs`, verified: one warning, not three). A cheap trick is a
   never-called `extern "C" fn _check(_: ScoreRequest) {}` that passes each boundary struct by value, so the lint does
   inspect it.
3. `mode: ScoreMode` (a `repr(u8)` enum). The lint considers it FFI-safe, which is true for values Rust writes. A Java
   client that writes `2` creates an invalid `ScoreMode` inside a struct Rust reads through a reference, and that's
   undefined behavior. `&ScoreRequest` also promises Rust that the pointer is non-null and aligned and that *every
   field* is valid, which Java can't be held to. Rewrite (a sketch):
   ```rust,ignore
   #[repr(C)]
   pub struct MeridianScoreRequest {
       pub amount_cents: i64,
       pub mode: u32,              // MERIDIAN_MODE_NORMAL = 0, MERIDIAN_MODE_STRICT = 1
       pub override_block_at: u32, // 0 = no override, else 1..=100
   }
   pub unsafe extern "C" fn meridian_score_req(req: *const MeridianScoreRequest, out: *mut i32) -> i32
   ```
   The checks now live in Rust code: NULL and alignment of both pointers, `mode` through `TryFrom<u32>`, the override
   range, and `catch_unwind` around the body.

### Selected exercises

- **Beginner.** `{ u8, u32, u8 }`: 12 bytes, align 4 (offsets 0, 4, 8, then 3 bytes of tail padding).
  `{ u32, u8, u8 }`: 8 bytes, align 4 (0, 4, 5, then 2). `{ u16, f64, u16 }`: 24 bytes, align 8 (0, 8, 16, then 6).
  `{ [u8; 3], u64 }`: 16 bytes, align 8 (0, 8). Order fields largest-first before the first release, and never after.
- **Intermediate.** `risk: u16` stays inside the padding (offset 4, next field still at 8, size still 16), so the size
  and `rule_id` assertions pass. Only an assertion on `risk`'s own size or the header diff notices, and old C code that
  writes one byte leaves the other byte as whatever was there. `risk: u32` also fits (4..8). Both change the contract
  without failing a size/offset check, which is why the version rule covers any change, not only the ones assertions see.
- **Advanced (predicted; check on nightly).** `{ f32, f32, u32 }` under C: first eightbyte (two floats) is SSE, the
  second (the `u32`) INTEGER, so a `Cast` to one float register (two packed floats) plus one integer register.
  `{ f64, u64 }`: SSE + INTEGER, `xmm0` and `rdi`. Under the Rust convention, the two-scalar struct is a `Pair`; the
  three-field one is likely `Cast` to integer registers. Returning `Triple`: both conventions return through a hidden
  pointer (`ret` is `Indirect`), with `sret` in C's IR.
- **Systems (predicted).** When the argument is a fresh local built from parameters, the Rust convention must
  materialize the struct somewhere to pass a pointer, so it writes it to the caller's stack and passes its address;
  the C convention writes it into the outgoing argument area. The costs converge. The zero-copy trick in §4 depends on
  the caller already holding a pointer to an immutable value.
- **Architecture.** Export a version number *and* a small layout table (`meridian_abi_info(out)` filling
  `{ version, sizeof_txn, offsetof_amount, ... }`). The Java wrapper compares both at load and refuses to start on
  mismatch. During a rolling deploy each JVM loads its own library copy from its own image, so old and new never mix
  inside one process; what must stay compatible is the *wire* (requests and stored data), not the in-process ABI.

### Design exercise (a stable plugin ABI for fraud rules)

- **Entry symbol:** `meridian_rule_plugin_v1` returning a `repr(C)` descriptor `{ abi_version: u32, name: *const
  c_char, vtable: *const RuleVTable, state: *mut c_void }`. The versioned symbol name means a plugin built for another
  major version simply isn't found.
- **Version:** the host checks `abi_version` against the versions it supports, and refuses (with a log line naming the
  plugin) anything else. Adding functions to the vtable means a new minor version with a `size` field so the host knows
  which entries exist.
- **Signatures:** only C types: integers, `f64`, `repr(C)` structs, pointers with documented lifetimes. "No opinion" is
  an explicit return code, with the score in an out-parameter, never a magic score value.
- **Panics:** every plugin function is `extern "C"` with `catch_unwind` inside and returns a code. The host can't catch
  a plugin's panic, and mustn't try: the plugin may use a different Rust version, with its own panic runtime and
  unwinder. A plugin that aborts takes the process down, so rule plugins run in the out-of-process tier (Chapter 6.5's
  third row) if analysts write them.
- **Who checks what:** the compiler checks each side's Rust against its own declarations; `const` assertions check each
  side's struct layouts against the header's numbers; only the load-time handshake checks that host and plugin agree
  with *each other*.

---

## Chapter 16.2 — Calling C from Rust

### Interview & architecture questions

**1. Three promises.** The declaration (`unsafe extern "C" { fn f(...) }`): the signature exactly matches the C
definition, documented by `# Safety` on the item. The call (`unsafe { f(...) }`): the preconditions hold now,
documented by a `// SAFETY:` comment. The safe wrapper's signature: its types make the preconditions impossible to
violate from safe code, documented by the wrapper's rustdoc.

**2. `env_var` copies** because `getenv`'s pointer is valid "until the environment is next modified," by any thread
and any code. A borrowed result would need a lifetime that ends at the next `setenv` anywhere in the process, which
isn't a region of the caller's code, so Rust can't express it. Copying replaces an inexpressible lifetime with
ownership.

**3. The lints.** `improper_ctypes` checks foreign declarations for types with no C equivalent: it catches
`fn puts(s: &str)`; it can't catch a declaration that uses valid C types with the *wrong meaning* (`i32` where C
takes `size_t`). `dangling_pointers_from_temporaries` catches `CString::new(..).unwrap().as_ptr()` on a temporary; it
can't catch a named local `CString` that is dropped before a pointer to it is used later, or returned from a function.

**4. `qsort_r` soundness.** C11 §7.22.5 makes an inconsistent comparator undefined behavior. A safe `sort_by(v, f)`
lets any safe closure be that comparator, and safe code can write an inconsistent one, so a safe program could cause UB
through it: unsound. `rank_by_score` takes data, and the only comparator it passes is its own `total_cmp`-plus-index
order, which is total by construction. That's Chapter 15.1's rule: unsafe code may rely only on things it controls or on
`unsafe` contracts, never on the good behavior of a safe caller-supplied function.

**5. What a foreign call compiles to.** An indirect `call` (or a tail `jmp`) through the GOT slot for `strlen`, which
the dynamic loader fills in. The optimizer can't inline the callee (its body isn't in the module), must assume an
unknown foreign function reads and writes any escaped memory (so values are reloaded afterwards), and can't specialize
it. Recognized C library functions (`strlen`, `memcpy`) get known attributes from LLVM, which helps a little.

**6. `errno`.** It's one slot per thread, overwritten by any later failing libc call on that thread, including calls
hidden in logging, formatting, or allocation. Read it on the line after the call that failed. It isn't shared between
threads (it's thread-local), but it isn't private to your call either.

**7. `safe` items.** Correct only when the function has no preconditions for *any* argument values and any program
state, like `getpid`. `abs(INT_MIN)` and `labs(LONG_MIN)` overflow, which is undefined behavior in C, so declaring them
`safe` would let safe Rust trigger UB: the declaration itself would be the unsound part.

**8. Strings in three ways.** Rust `&CStr`: zero copies, the caller's bytes (any encoding, NUL-terminated), freed by the
Rust owner. FFM `allocateFrom(String)`: one copy into native memory, standard UTF-8 plus NUL, freed when the arena
closes. JNI `GetStringUTFChars`: a copy (or a pinned view) in Modified UTF-8, freed by `ReleaseStringUTFChars`, which you
must remember to call.

**9. "Not thread-safe."** Either *not reentrant* (no concurrent calls on one object, but any thread may call it one at a
time), which is `Send` but not `Sync`: wrap it in a `Mutex` or give each thread its own. Or *thread-affine* (only the
creating thread may ever touch it), which is `!Send` and `!Sync`: use it where it was created, or behind an owner thread.
Ask the vendor which one they mean; the header rarely says.

### Debugging exercise (`model_dir`)

1. Three violations:
   - **No NULL check.** When `MERIDIAN_MODEL_DIR` is unset, `getenv` returns NULL, and `CStr::from_ptr` requires a
     non-null pointer. The CI test always had the variable set, so the path was never exercised.
   - **An invented lifetime.** The result borrows the environment block, valid only until the environment changes, but
     the signature promises `'static`. Any later `setenv` can invalidate a string callers keep forever.
   - **Concurrency.** It's called from several worker threads during startup. Concurrent `getenv` calls are fine, but
     if anything calls `setenv` concurrently (a C library, or `set_var`), `getenv` races with it: the reason
     `set_var` is `unsafe` in edition 2024. The code carries no `// SAFETY:` argument that rules it out.
   And a design issue: `to_str().ok()` conflates "unset" with "set but not UTF-8."
2. `CStr::from_ptr<'a>` returns `&'a CStr` for a lifetime the caller chooses, because a raw pointer carries no
   lifetime. The function's return type chose `'static`. It's the most dangerous part because it's invisible at every
   call site: callers see a `'static` reference and store it in long-lived config.
3. `pub fn model_dir() -> Option<PathBuf> { std::env::var_os("MERIDIAN_MODEL_DIR").map(PathBuf::from) }`. std handles
   NULL, copies the bytes, and takes its environment lock. A path is bytes on Unix, so keep it as a `PathBuf`, log it
   with `.display()`, and use it as is. Reject a non-UTF-8 value only if something downstream really needs text (say,
   echoing it into a JSON status page), and then fail startup with a clear error rather than replacing characters.

### Selected exercises

- **Beginner.** `getpid`: `safe fn getpid() -> c_int;` no clause. `strnlen`: "`s` readable for `min(maxlen, position of
  the first NUL + 1)` bytes"; the wrapper takes `&[u8]` and passes `(as_ptr(), len())`, so the whole slice is
  readable. `gethostname`: "`name` writable for `len` bytes"; the wrapper passes a stack buffer and its length, checks
  for `-1` (then `last_os_error()`), and copies up to the first NUL (POSIX doesn't guarantee a NUL on truncation, so
  handle "no NUL found" as an error or retry with a larger buffer).
- **Intermediate.** Reject non-finite scores before the call (`scores.iter().all(|s| s.is_finite())`). The `sort_by`
  version (predicted, then check the asm) inlines the comparator into the sort; the `qsort_r` version makes an indirect
  call per comparison. That's the performance side of "don't call C for what Rust has."
- **Advanced.** `# Safety: cmp must be a consistent total order on the elements for the whole call (for all a, b, c:
  exactly one of <, =, > holds; the results are antisymmetric and transitive; repeated calls on the same pair agree),
  and must not panic.` No safe wrapper can remove the obligation, because the closure's behavior can't be checked by the
  wrapper. `slice::sort_by` with an inconsistent order is a *logic error*: the result is an unspecified permutation, and
  since Rust 1.81 it may panic, but it never causes UB.
- **Architecture.** A `vendor-sys` crate (bindgen output, `links = "vendor"`, a build script that finds the library)
  and a `vendor` crate with the safe API. Wrap first what the service calls, in order of call volume; leave the rest in
  `sys`. For callbacks, one trampoline per callback type, with the patterns of Chapter 16.4. Miri can't run the C code:
  test the safe layer against a Rust re-implementation of the C functions exported with the same symbols (the approach
  of listing `ch02-11-vse-binding.rs`) under Miri, and run the real C library under sanitizers in a separate CI job.

### Design exercise (`libgeo`)

- **Handle:** `GeoDb { raw: NonNull<sys::geo_db> }` with `Drop` → `geo_close`. The header says lookups are thread-safe,
  so `unsafe impl Send + Sync` with a `// SAFETY:` citing that sentence (verify it with the vendor, and consider a
  stress test).
- **`country`'s lifetime:** it points into the database, valid until `geo_close`, so the Rust API can return
  `&'db str` borrowed from `&'db GeoDb` (after validating UTF-8 once, or returning `&CStr`). That's rule 4 of Chapter 16.4:
  the lifetime the header describes becomes a Rust borrow.
- **`geo_reload`:** it must not run concurrently with any lookup, and it may invalidate every `country` pointer. That's
  exactly `&mut self`, which a shared, `Sync` database can't offer to callers holding `&GeoDb`. Better: never reload in
  place. Open a *new* `GeoDb` and swap it in with `ArcSwap<GeoDb>` (Chapter 14.5); old lookups finish on the old
  handle, and it's closed when its last `Arc` drops.
- **Errors:** `geo_open`'s return is an errno value, so `io::Error::from_raw_os_error(rc)`. `geo_lookup`'s failure is
  "not found," so `Option<Location>`.
- **At 400K req/s:** predicted from mechanism, the FFI call itself is cheap. What matters is the library's lookup cost and
  whether a Rust-native reader of the same format would avoid allocation and inline better. Measure p99 lookup latency and
  CPU per lookup for both under the gateway's real IP mix before choosing.

---

## Chapter 16.3 — Calling Rust from C (and from Java via FFM)

### Interview & architecture questions

**1. The ten rules and their enforcement.** The compiler enforces: `extern "C"` (convention), `repr(C)` field order,
`Sync` via the assertion (rule 9), and `unsafe` on pointer-taking functions once written (rule 2's effect on Rust
callers). Lints enforce: FFI-safe by-value types (`improper_ctypes_definitions`, denied), dangling temporaries, and
Clippy's `not_unsafe_ptr_arg_deref` for the simplest rule-2 violations. Only review (and tests) enforce: the prefix,
`catch_unwind` in every export, one error convention, opaque handles, input validation, a version function, and the
correctness of the header's prose.

**2. `no_mangle` and `cdylib`.** It emits the item under its literal name instead of a v0-mangled one. It's unsafe
because two definitions of one unmangled name can collide at link or load time, and which one is used isn't something
the type system checks. A `cdylib` puts exactly the `#[no_mangle]`/`#[export_name]` functions in its dynamic symbol table
and hides all mangled Rust symbols: `nm -D --defined-only` on the library built in listing `ch03-06-c-calls-rust.rs`
lists exactly its four `meridian_` functions. An executable exports nothing dynamically by default: `dlsym(this
program)` failed with `undefined symbol: meridian_abi_version` in listing `ch03-04-dlopen.rs`.

**3. `n == 0`.** `slice::from_raw_parts` requires a non-null, aligned pointer *even for length 0*, and C callers
routinely pass NULL with a zero count. Returning `&[]` (whose pointer is a dangling, aligned, non-null one Rust
makes) satisfies both. `from_raw_parts(NULL, 0)` would be undefined behavior.

**4. No overlap.** The function turns `ids` into `&[u64]` and `out` into `&mut [i32]`. If they overlapped, a mutable
and a shared reference would cover the same bytes, breaking aliasing XOR mutation (Chapter 4.1), which rustc relies on
(`noalias`). Rust can't check it because both come from raw pointers supplied by the caller. The header states it (and
exercise 3 considers checking it).

**5. A downcall.** Java evaluates arguments (the arena segments' addresses, the `long` count). `invokeExact` enters the
JVM's generated downcall stub, which places them per System V (`rdi` = scorer, `rsi` = ids, `rdx` = n, `rcx` = out),
switches the thread's state to "in native," and calls `meridian_score_batch`. Rust runs **on the Java thread's native
stack**: the ffi_guard closure, `slice_in`/`slice_out`, then the generic `score_all`, writing into Java's `out` segment.
The return code comes back in `eax`; the stub switches the thread back to Java and returns the `int`.

**6. Proving thread safety.** `const _: () = { const fn assert_sync<T: Sync>() {} assert_sync::<MeridianScorer>(); };`
fails to compile unless every field is `Sync`, and the exports take `*const`/`&` only. Adding a `RefCell` or `Cell`
cache, an `Rc`, or a raw pointer field breaks the proof (a compile error); so would changing an export to mutate through
the handle.

**7. `C-unwind` and the JVM.** The unwind would continue past the downcall stub into JVM frames, which Rust's unwinder
can't process and the JVM doesn't expect. It wouldn't become a Java exception; the behavior is undefined (or the runtime
aborts). `C-unwind` is only for callers built to receive the unwind.

**8. FFM, JNI, or a sidecar.** For a 5 ms p99 with scoring in the low milliseconds: FFM. It adds tens of nanoseconds per
call (order of magnitude), needs no C glue, and passes standard UTF-8; JNI costs similar per call plus glue code and
encoding traps; a sidecar adds a network round trip and serialization per score. What would change it: frequent crashes
in the native code (the sidecar isolates them), a need to deploy the library independently of the JVM service, or a
language team that can't own native code.

**9. The safe `meridian_score`.** Not sound: a safe Rust caller can pass a null or dangling `out` without writing
`unsafe`, and the function writes through it. For C and Java callers it makes no difference, since they don't see the
keyword and must follow the header anyway. It matters for Rust callers of the `rlib`, like the backfill job in 16.3 §10.

### Debugging exercise (`meridian_top_features`)

1. **Rule 3 is missing.** `top_features` panics for an unknown `id`; the unwind reaches the `extern "C"` boundary, the
   landing pad calls `panic_cannot_unwind`, and the process aborts (SIGABRT). The JVM dies with it, so there's no Java
   stack trace, only a native crash log (an `hs_err` file) and a Kubernetes restart.
2. **Nobody owns the bytes after return.** `joined` is a local `CString`, dropped at the end of the function, so `*out`
   points into freed memory: using it is undefined behavior, and "sometimes garbage" is what that looked like.
   `dangling_pointers_from_temporaries` doesn't fire, because `joined` is a named local, not a temporary.
3. **A reference parameter** promises rustc that the pointer is non-null, aligned, points to a live, valid
   `MeridianScorer`, and that nothing mutates it during the call; rustc marks it `nonnull`/`noalias` and may delete null
   checks. Java passes whatever address it holds (possibly NULL, possibly freed). Rewrite (a sketch, following listing
   `ch03-01-fraud-c-api.rs`):
   ```rust,ignore
   /// # Safety
   /// `s` is NULL or a live scorer; `out` is NULL or writable. On MERIDIAN_OK, `*out` owns a buffer to be
   /// released with `meridian_buf_free`.
   #[unsafe(no_mangle)]
   pub unsafe extern "C" fn meridian_top_features(s: *const MeridianScorer, id: u64, out: *mut MeridianBuf) -> i32 {
       ffi_guard(|| {
           let s = unsafe { s.as_ref() }.ok_or(Invalid)?;
           if out.is_null() { return Err(Invalid); }
           let names = s.model.try_top_features(id).ok_or(Invalid)?; // no panic for an unknown id
           let (ptr, len, cap) = names.join(",").into_bytes().into_raw_parts();
           unsafe { out.write(MeridianBuf { ptr, len, cap }) };
           Ok(())
       })
   }
   ```
   The library buffer suits a data-dependent length read once; the caller-buffer design (`_into` + size query) is
   equally valid if the console reuses buffers.

### Selected exercises

- **Beginner.** `/// # Safety: s is NULL or a live scorer; out is NULL or writable for one int32_t.` Header:
  `int32_t meridian_score_one(const MeridianScorer *s, uint64_t id, int32_t *out);` The body calls `score_all` with
  one-element slices, inside `ffi_guard`.
- **Intermediate.** `thread_local! { static LAST: RefCell<String> }`, set by `ffi_guard` on every error path (the
  message, or "panic: <payload>"). `int32_t meridian_last_error(char *buf, size_t cap)` copies it with the size-query
  protocol. Header rules: "per thread: describes the most recent failing call *on the calling thread*"; "copied into
  your buffer, so no lifetime to manage"; "unchanged by successful calls" (or cleared: pick one and state it). Copying
  avoids the "valid until the next call" pointer entirely.
- **Advanced.** Check `ids_start < out_end && out_start < ids_end` on the byte ranges using `addr()` values. For these
  two ranges it's complete. Comparing addresses from different allocations *as integers* is fine (Chapter 15.2: it's
  deriving pointers from them that needs provenance). Worth it: two comparisons against a class of caller bug that
  would otherwise be undefined behavior.
- **Architecture.** Keep one C header with `#ifdef __cplusplus extern "C" { #endif` guards, and provide a small
  header-only C++ wrapper (RAII class owning the scorer, a `std::span` overload) owned by the fraud team, since they own
  the contract. `C-unwind` is still a no: the C++ caller catches nothing useful from a Rust panic, and return codes
  already cover it.

### Design exercise (tokenization library for three callers)

- **Crates:** `tokenize-core` (the Rust API: generic, `Result`, no `unsafe`), and `tokenize-ffi` (`crate-type =
  ["cdylib", "staticlib"]`, all `#[no_mangle]` functions, `ffi_guard`, the conversions). Rust services depend on
  `tokenize-core` directly and never see the C API. Go links the `staticlib` through cgo.
- **Errors and handles:** int codes plus out-params. Handles as registry indices, since Java and Go are both callers the
  library doesn't control. cgo calls cost more than FFM downcalls (order of magnitude: around a hundred nanoseconds
  versus tens; measure), so offer batch functions from day one. cgo's pointer-passing rules forbid C code from keeping
  Go pointers after the call returns: the library copies anything it keeps.
- **Secrets:** the caller provides both input and output buffers, so the caller controls their lifetime and zeroing.
  The library copies nothing it doesn't need, zeroes its own scratch buffers before freeing them (commonly with the
  `zeroize` crate, not on the Playground), never logs inputs, and documents that output buffers hold secrets.
- **Release:** `cbindgen` header committed and diffed in CI; `jextract` bindings for Java; cgo uses the header
  directly; `tokenize_abi_version()` checked by every binding at load; the header versioned with the library.

---

## Chapter 16.4 — Ownership and Allocation Across Language Boundaries

### Interview & architecture questions

**1. Four questions, five rules.** Who allocated it? Who frees it, with which allocator? How long is it valid? Which
threads may touch it? Rules: memory returns to the allocator that made it; whoever allocates exports the free; borrowed
means "for the call" unless stated otherwise; lifetimes the compiler can't see become an API rule on the far side and a
type on the near side; thread rules are type properties (`!Send`, a `Sync` assertion).

**2. Capacity.** Deallocation must use the same layout (size and alignment) as the allocation, and a `Vec`'s allocation
size is `cap * size_of::<T>()`, not `len`. Verified: the explanation had `len=53 cap=98`, so freeing with 53 would
describe a different block. Returning only `(ptr, len)` requires `into_boxed_slice()` first, which reallocates to the
exact length whenever `cap != len`: one more allocation and copy for that design.

**3. `Option<Box<T>>` destroy.** `Box<T>` (T: Sized) is guaranteed ABI-compatible with `T*`, and `Option` adds NULL as
`None`, so the function has the exact ABI of `void destroy(T *)`. Taking it by value means the parameter owns the
allocation, and dropping it frees it with Rust's allocator. No raw pointer is dereferenced, so no `unsafe` is needed.
What the type can't enforce: that C passes each pointer at most once, and only pointers that came from the matching
create function.

**4. Two Rust `cdylib`s.** No. Each contains its own std and its own global allocator (they may even be configured
differently, one with mimalloc). A `Box` from one freed by the other hands a block to an allocator that didn't create it:
undefined behavior. Listing `ch03-07-rust-plugin.rs` measured the separation: the plugin's `Box` and its free added
+0 to the host's counting allocator. Each library must export its own free function, and objects go back to the library
that made them.

**5. Handle designs with a stale handle.** Raw pointer: using a freed handle is undefined behavior (use after free).
Pointer-as-integer with exposed provenance: the same, and Miri warns it may miss such bugs. Registry index plus
generation: the generation no longer matches, and the call returns `-1` (verified: "record after close -> -1", "old
handle -> -1"). For JNI callers you don't control, use the registry.

**6. Parked panics.** The trampoline wraps the closure call in `catch_unwind`. On a panic it stores the payload in
`ctx.panic` and returns a nonzero code, which the C API defines as "stop." When `meridian_for_each_feature` returns,
no C frames remain, and `for_each_feature` calls `resume_unwind(payload)`, so the Rust caller sees its closure's original
panic. `qsort_r` has no "stop" result, and returning a made-up comparison to end early would make the comparator
inconsistent, which is itself undefined behavior in C.

**7. `FnMut(&CStr, f64)`.** The elided lifetime in a closure bound is higher-ranked: `for<'a> FnMut(&'a CStr, f64)`. The
closure must work for a borrow of *any* lifetime, including one that ends when the call returns, so it can't store the
reference anywhere that outlives the call (E0521 if it tries). Chapter 4.5 explains it.

**8. The owner thread panics.** The unwind runs destructors on that thread: `engine` is dropped there, so `vse_close`
runs on the thread that opened it, as the vendor requires. The pending request's reply sender is dropped with the
request, so its caller's `recv()` fails: `EngineDown`. The request receiver is dropped too, so every later `send` fails
immediately: `EngineDown`, never a hang (verified). The service layer restarts the owner thread and counts the event.

**9. Arenas as ownership.** `ofConfined`: an owned, `!Send` value dropped at scope end. `ofShared`: an `Arc<T>` with
`T: Sync`. `ofAuto`: no Rust equivalent (GC-driven release). `global`: `'static` or `Box::leak`. The analogy breaks on
*when* the rules are checked (Java at run time, per access, with exceptions; Rust at compile time, with nothing at run
time) and on *what* they cover: an arena knows nothing about memory Rust allocated, and `Drop` knows nothing about
arenas.

### Debugging exercise (`meridian_feature_name`)

1. Who allocated: Rust's allocator, as the buffer of `names[i]`. Who frees: the scorer, when that `String` reallocates,
   when `names` is replaced, or when the scorer is freed. How long valid: until the next `push` onto the same name
   reallocates it (any call for the same index, from any thread) or the hourly reload replaces `names`. Threads: any
   thread may call, and nothing coordinates them. The validity answer depends on other threads and the reload because
   the pointer's lifetime is tied to the *scorer's mutable state*, which other callers change, not to anything this
   caller does.
2. A second read of the same name returns it with the first call's `'\0'` included: `push` runs on every call, so the
   string grows by one NUL each time, and `len` grows with it. Two threads reading names at once both create a
   `&mut MeridianScorer` from the same pointer, two exclusive references to one value, which breaks aliasing XOR
   mutation (Chapter 4.1); for the same index they also write the same `String` concurrently, a data race. Both are
   undefined behavior. `&mut *s` let the author skip the proof of exclusivity that the borrow checker would have
   demanded: from a raw pointer, the compiler takes the author's word for it.
3. Three redesigns:
   - Caller buffer: `meridian_feature_name_into(s, i, buf, cap, needed)`, with names stored immutably and copied out.
   - Library buffer: `meridian_feature_name(s, i, out: *mut MeridianBuf)` plus `meridian_buf_free`.
   - Callback: `meridian_for_each_feature_name(s, cb, user)`, lending each name for one call.
   For a Java caller reading all names once at startup, the callback: one downcall and N upcalls, no buffer sizing, and
   Java copies each name into a `String`. (A fourth design is also sound: make names immutable, stored once as
   `Box<[CString]>` at load, and return pointers valid "until `meridian_scorer_free`." That's zero-copy, and the reload
   creates a new scorer instead of mutating this one.)

### Selected exercises

- **Beginner (one of four).** `meridian_explain`: "Allocated by the library. Release with `meridian_buf_free`, exactly
  once; don't modify any field. Valid until freed, independent of the scorer. May be read and freed on any thread."
  `vse_last_error`: "Owned by the engine. Valid until the next call on this engine. Copy it before calling anything else
  on the engine; only the engine's thread may call."
- **Intermediate.** `into_boxed_slice` reallocates when `cap != len`, so it adds one allocation (and a copy of `len`
  bytes) whenever `format!` over-allocated, which is most of the time (verified example: 98 versus 53). The free
  function then rebuilds `Box<[u8]>` from `(ptr, len)`.
- **Advanced (predicted design).** A fixed-capacity slab with a per-slot `AtomicU64` holding the generation and a "live"
  bit. A lookup must also stop a concurrent close from freeing the session *while it's being used*: pin it with a
  per-slot reference count (increment, re-check the generation, use, decrement; close marks dead and the last user
  frees), or defer frees with epochs (crossbeam-epoch, Chapter 14.4). Reading the generation alone isn't enough, which is
  Part XIV's reclamation problem.
- **Systems (predicted, then run it).** Plugin allocator: +1 alloc during `new_large_amount` (the `Box`), +1 free during
  the rule's drop. Host allocator: +0 and +0, as the listing already measured. One binary can have only one
  `#[global_allocator]` (rustc rejects a second), because the allocation shim functions every `Box` and `Vec` call are
  defined once per final artifact. A process can hold several, because each `cdylib` is its own final artifact whose
  shim functions stay local to it: they're not in its dynamic symbol table (16.3's `nm -D` output lists only the
  `#[no_mangle]` functions), so nothing outside the library can bind to them. `LD_DEBUG=bindings` on a local machine
  shows each library binding only its own imports.
- **Architecture.** At 200 µs per call, one engine does at most about 5,000 calls/s (order of magnitude). A 2,000/s peak
  needs one busy engine, so run three or four for headroom and isolation. A queue bound of about 50 per engine (10 ms of
  work) keeps queueing delay inside the latency budget. Time out waiting for the reply at around 50 ms. When every queue
  is full, return "review unavailable" immediately and fall back to the in-house decision (a load-shedding policy, as in
  Chapter 13.5).

### Design exercise (streaming results to Java)

A defensible design is the **pull API with a caller buffer**: `meridian_stream_open(scorer, day, *handle)`,
`meridian_stream_next(handle, MeridianScored *buf, size_t cap, size_t *n)` filling up to `cap` `(txn_id, score)`
records, and `meridian_stream_close(handle)`. The four answers: Java allocates `buf` (a reused arena segment) and frees
it; Rust borrows it only during `next`; the handle is a registry index (defined misuse). Backpressure is natural: Rust
produces only when Java asks, from a bounded internal channel fed by a producer thread. A callback (upcall) design
streams with fewer calls, but an exception escaping an upcall terminates the JVM, so every upcall target needs a Java
try/catch, and backpressure requires blocking inside the upcall. A shared ring buffer is the fastest and hardest to get
right (memory ordering across languages, Part XIV). Rust panics in every design stop at `ffi_guard` and become `-99`.

---

## Part XVI Review — Capstone: the explain-API PR

### 1. The defects

1. **`init(model_name: String)`** (16.3 rule 7, the one lint warning). Java can't construct a Rust `String`. It would
   pass a pointer, and the Rust side would read whatever occupies the 24 bytes its convention expects as a `String`,
   then drop it: undefined behavior.
2. **Unprefixed exported names** `init`, `explain`, `last_explanation` (16.3 rule 1). They collide with any other
   library's `init` found first in a lookup (`SymbolLookup.loaderLookup()` searches every library the class loader
   loaded), and in profiles and crash logs they identify nothing.
3. **No `catch_unwind` anywhere** (16.1, 16.3 rule 3). Panic sites: `lock().unwrap()` (three, including poisoning),
   `guard.as_mut().unwrap()` before `init`, `partial_cmp().unwrap()` on a NaN feature, `e.weights[*n]` if names and
   weights ever diverge. Each aborts the JVM: pods restart with no Java stack trace.
4. **Safe `extern "C" fn` dereferencing raw pointers** (16.3 §10). `explain` is callable from safe Rust with bad
   pointers: unsound.
5. **`from_raw_parts(features, n as usize)` unchecked** (16.3 rule 7). NULL (even with `n == 0`) or a misaligned pointer
   violates `from_raw_parts`'s preconditions, and a negative `n` becomes an enormous length.
6. **No check that `n` equals the model's feature count.** `zip` silently truncates, so a caller that sends two features
   gets a plausible, partial explanation with no error.
7. **`ExplainOptions` has no `repr(C)`** (16.1). Its layout is unspecified, and the lint is silent because it's passed
   by pointer (listing `ch01-11-lint-blind-spot.rs`). Java's layout may match today by luck.
8. **`opts` dereferenced without a NULL check** (16.3 rule 8).
9. **`format: Format` as a parameter** (16.1 §7). A Java caller passing `2` creates an invalid enum value at the call:
   undefined behavior. Receive a `u32` and validate it.
10. **`include_negative: bool`** in a struct other languages write. Any byte other than 0 or 1 is an invalid `bool`.
    Use `u8` and validate.
11. **"The caller frees it with `free()`"** (16.4 §10). The string came from Rust's allocator. With the system allocator
    it happens to work, and it breaks the day the library adopts mimalloc. There's no `meridian_string_free`.
12. **`last_explanation()` returns a pointer into shared state after releasing the lock** (16.4 rule 3). It's valid only
    until the next `explain` or `init` on any thread, which frees the old `CString`. A console thread reading it while
    another thread explains is reading memory that may be freed under it.
13. **A process-wide singleton behind one `Mutex`** (Chapter 11.3). Every explain from every thread serializes on it,
    including the formatting done under the lock. `init` silently replaces the model under concurrent callers. There's
    one model per process and no way to free it.
14. **Poisoning** (Chapter 11.3). Once `catch_unwind` exists, a panic under the lock poisons it, and every later
    `lock().unwrap()` panics: the library turns into a permanent `-99`.
15. **No usable error reporting.** `init` returns a `bool` with no reason, and `explain` has no error path at all: it
    returns a pointer or aborts.
16. **No ABI version, no generated header, no layout assertions** (16.1 §9), and `n: i32` where the rest of the library
    uses `size_t`.

### 2. Why the lint said so little

`improper_ctypes_definitions` checks types passed **by value**. `String` was by value, so it was flagged. `ExplainOptions`
is behind `*const`, and the lint doesn't inspect pointees. `Format` is `repr(u32)`, which is FFI-safe for values Rust
produces, and the lint can't know that Java produces this one. One warning out of sixteen defects is typical: the lint
checks *representability*, not *validity* or *ownership*.

### 3. What a Rust demo can't reveal

A Rust caller satisfies every Rust-side assumption by construction: it passes a real `String`, valid enum values, valid
`bool`s, a properly laid-out `ExplainOptions` (same compiler, same struct), non-null pointers, and it frees with
`CString::from_raw`, the correct function, whatever the doc comment says. It also runs single-threaded. Invisible to
the demo: defects 1, 2, 5, 7, 9, 10, 11, 12, 13 (under concurrency), and 14. The demo tested the API against the one
caller for which the API was never meant.

### 4. The four answers

- `model_name`: nothing crosses correctly (defect 1).
- `features`: allocated by Java, borrowed for the call. The header omits NULL/length rules.
- `opts`: allocated by Java, borrowed for the call. The layout is undefined, so no header can state it.
- The returned explanation: allocated by Rust, "freed with `free()`": **wrong** (defect 11); valid until freed; any
  thread.
- `last_explanation()`: owned by the global; the header implies "valid until you're done," the truth is "until the next
  `explain`/`init` on any thread" (**wrong**); thread rules not stated.

### 5. `catch_unwind` alone makes poisoning worse

Without it, the first panic aborts the process, and Kubernetes restarts it: an outage, but a self-healing one. With it,
the panic is caught while `EXPLAINER`'s lock is held, the lock is poisoned, and every later call panics on
`lock().unwrap()` and returns `-99`, forever, in a process that looks healthy. The fix removes the global lock entirely
(an immutable, `Sync` handle); where a lock is unavoidable, decide the poisoning policy explicitly (Chapter 11.3).

### 6. The rewrite

Listing `review-03-explain-fixed.rs`: a `MeridianExplainer` handle (create/free, `Option<Box<_>>` destroy),
immutable and asserted `Sync`, so no lock; `unsafe extern "C" fn meridian_explain(e, features, n: usize, opts, out:
*mut MeridianBuf) -> i32` inside a `guard`; a `repr(C)` `MeridianExplainOptions` with `u32 format` and `u8
include_negative`, both validated, and `const` layout assertions; the feature count and finiteness checked (NaN → `-1`,
and `total_cmp` so sorting can't panic); the result returned as a `MeridianBuf` released by `meridian_buf_free`; no
`last_explanation`, since the console keeps the buffer it received; `meridian_abi_version()` returning 4; and the lint
denied for the crate. The caller-buffer variant (`meridian_explain_into`) is an equally good answer.

---

## Part XVI Review — Interview mode

1. **Layout guarantees:** Chapter 16.1 §3 and question 5. `repr(C)`: C's order and alignment rules. `repr(transparent)`:
   the single field's layout *and* calling convention. `Option<&T>`: a nullable pointer. The default repr guarantees
   nothing about order, offsets, or even that two identical definitions get the same layout.
2. **Unsafe declarations:** a wrong foreign signature makes every call undefined, and an unmangled symbol can collide, so
   both are promises the compiler can't check (Chapter 15.1). A `safe` item is callable without `unsafe`; declaring one
   is a soundness bug if the function has *any* precondition (`abs(INT_MIN)`). Chapter 16.2, question 7.
3. **`unsafe extern "C" fn`:** Chapter 16.3 §10 and question 9. Soundness is about safe callers; C and Java don't see
   the keyword, Rust callers do.
4. **`conv: Rust` versus `conv: C`:** Chapter 16.1 question 4: `Pointer` (and a pass-through `jmp` in release) versus
   `OnStack` (a 24-byte copy into the outgoing argument area).
5. **Panicking `extern "C"`:** Chapter 16.1 question 8.
6. **Foreign calls:** Chapter 16.2 question 5.
7. **FFM overhead at 2M calls/s:** predicted from mechanism, tens of nanoseconds of fixed cost per downcall against
   50 ns of work, so overhead of the same order as the work. Batch (one downcall per N items), keep buffers in reused
   arenas, consider `Linker.Option.critical` for short calls, and measure with JMH at several batch sizes (Chapter
   16.3 exercise, Part XX).
8. **Output designs:** Chapter 16.4 §7. Small results: a caller buffer (no allocation, one call). Large or unbounded: a
   library buffer and free (one allocation, no retry). Streams: callbacks or a pull API (no materialization).
9. **A C API for three runtimes:** Chapter 16.3 §2 and its design exercise. Java: FFM rules and arenas. Go: cgo's
   pointer rules and higher per-call cost. C++: exceptions must not meet `extern "C"`, and RAII wrappers on its side.
10. **A thread-affine vendor library:** Chapter 16.4 §9: `!Send` handle, owner threads with bounded queues and one-shot
    replies, `EngineDown` on failure, a pool sized by measurement.
11. **JNI, FFM, or sidecar:** Chapter 16.3 question 8.
12. **Keeping three artifacts in agreement:** Chapter 16.1 §9 and 16.3 §9: generate the header (`cbindgen`) and bindings
    (`jextract`) in CI and diff them; `const` layout assertions; `meridian_abi_version()` checked at load; any boundary
    change bumps the version. Rolling deploys are safe because each process loads its own library.
13. **A JVM that dies with no Java stack trace:** read the `hs_err` log's native frames (a Rust symbol and
    `panic_cannot_unwind` means a missing `catch_unwind`, 16.1/16.3); check for `SIGABRT` versus `SIGSEGV`; look for
    allocator frames in `free` (an allocator mismatch, 16.4 §10); check whether the crash follows a library or model
    upgrade (layout drift, 16.1 §10); and run the library's tests under Miri and the C side under sanitizers.
14. **An FFI review checklist:** prefixed name; `unsafe extern "C" fn` if it takes pointers; `# Safety` states NULL,
    length, alignment, overlap, and lifetime for every pointer; `ffi_guard`/`catch_unwind`; only C types, no incoming
    enums or `bool`s from outside; `repr(C)` structs with `const` assertions; every returned allocation has a matching
    `*_free`; no pointers into shared mutable state; thread safety stated and `Sync` asserted; header regenerated and
    diffed; version bumped if any boundary type changed; the lints denied.
