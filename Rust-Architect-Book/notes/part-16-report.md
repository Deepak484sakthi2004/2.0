# Part 16 report

## Status

**Written** (the narrower retry the user approved): `src/part-16-ffi/README.md`, chapters 16.1–16.4, `review.md`,
`src/appendix/answers-part-16.md`, and `listings/part-16/` (39 files, 61 checks). Scope as agreed: FFI as interface
engineering. No listing demonstrates undefined behavior. Miri is used only as `miri-ok` on correct code, and every
contract the compiler can enforce is shown through a compile error or a denied lint. Where the prose says a broken
contract is undefined, it says so in a sentence and points to Chapter 15.1. The safety classifier did not trigger.

## SUMMARY.md lines

Replace the Part XVI draft block with:

```markdown
- [Part XVI Overview](part-16-ffi/README.md)
  - [16.1 ABIs, Calling Conventions, and repr(C)](part-16-ffi/ch01-abi-repr-c.md)
  - [16.2 Calling C from Rust](part-16-ffi/ch02-calling-c.md)
  - [16.3 Calling Rust from C (and from Java via FFM)](part-16-ffi/ch03-calling-rust-from-c-and-java.md)
  - [16.4 Ownership and Allocation Across Language Boundaries](part-16-ffi/ch04-ownership-across-boundaries.md)
  - [Part XVI Review: The Explain-API PR & Interview Mode](part-16-ffi/review.md)
```

Appendix entry, after the Part XV answers line:

```markdown
  - [Part XVI Answers](appendix/answers-part-16.md)
```

## PROGRESS concepts

| Concept | Where | Full treatment planned |
|---|---|---|
| ABI = layout + calling convention + symbols + unwinding; the pinning feature for each; nobody checks (loader matches names) | 16.1 | — |
| `repr(C)` vs default layout measured (24 vs 16 B); `const` + `offset_of!` layout assertions; failing assertion = `E0080 evaluation panicked` | 16.1 | — |
| `Option<&T>`/`Option<Box<T>>`/`Option<NonNull<T>>`/`Option<extern "C" fn>`/`repr(transparent)` newtype = 8 B nullable pointer, FFI-safe with the lint denied; `Option<u32>` rejected | 16.1 | — |
| RFC 2195 layouts verified: `repr(C, u32)` payload at 8 vs `repr(u32)` at 4 (both 16 B), read through the defined struct/union shapes (Miri-clean) | 16.1 | — |
| "Send enums, receive integers": `DecisionCode(u32)` + `TryFrom`; code 3 → `Err` | 16.1 | — |
| `#[rustc_abi(debug)]`: `Point` Pair (Rust) vs Cast to 2 Float regs (C); `Triple` Indirect Pointer (Rust) vs OnStack (C); `can_unwind` true/false/true (Rust/C/C-unwind) | 16.1 | — |
| Release IR `byval([24 x i8])` (C) vs `dead_on_return … readonly` (Rust); asm: C caller copies 24 B, Rust caller tail-`jmp`s with its own pointer | 16.1 | XIX |
| `extern "C"` panic path in IR: `invoke` → `landingpad filter []` → `panic_cannot_unwind` | 16.1 | XIX (unwind tables) |
| System V vs Windows x64 conventions; LP64 vs LLP64 `long`; syscall ABI (`r10`, `-errno`); getpid three ways (std / libc / `syscall(39)`) | 16.1 | XIX |
| `improper_ctypes_definitions` checks by-value types only (by-reference/pointer struct: no warning, verified) | 16.1 | — |
| Declaration / call / safe wrapper; C-contract → Rust-type translation table | 16.2 | — |
| `strlen`/`getenv` wrappers (copy out; `set_var` unsafe in 2024), CStr/CString/OsStr conversions, `c"…"` literals | 16.2 | — |
| `snprintf` wrapper with truncation retry; `E0617` for `f32` to variadics | 16.2 | — |
| `qsort_r` with an `extern "C"` comparator + user data; why a generic safe comparator wrapper is unsound (C11 7.22.5); `total_cmp` ranks NaN first | 16.2 | — |
| `open` + `io::Error::last_os_error()` + `OwnedFd`; errno clobbered by logging (verified wrong error) | 16.2 | — |
| Foreign call asm: `jmp/call [rip + strlen@GOTPCREL]`; `&CStr` is two words; LLVM libcall attributes on `declare @strlen` | 16.2 | XIX (GOT/PLT) |
| Lints: `improper_ctypes`, `dangling_pointers_from_temporaries` (denied) | 16.2 | — |
| `CBuf`: malloc-owned buffer with `Drop` → `free`, `into_raw` hand-off (Miri-clean) | 16.2 | — |
| `sys` + safe binding layers; opaque `repr(C)` type with `PhantomData<(*mut u8, PhantomPinned)>`; `safe fn` extern items in a real binding | 16.2 | XXII (-sys crates) |
| Ten rules of a C API written in Rust; `ffi_guard`; checked `slice_in`/`slice_out`; `Sync` assertion for "thread-safe" headers | 16.3 | — |
| `#[unsafe(no_mangle)]` in asm; cdylib exports vs executable (`dlsym(this program)`: undefined symbol); dlopen/dlsym; `Symbol<'lib, F>` | 16.3 | XIX |
| FFM binding (`Linker`, `downcallHandle`, `Arena`, `allocateFrom`), jextract, cbindgen header; JNI `extern "system"` | 16.3 | XX (downcall cost) |
| One Java name, three encodings: FFM UTF-8 and JNI UTF-16 give the same hash; Modified UTF-8 rejected; lossy "fix" changes the hash | 16.3 | — |
| Safe `extern "C" fn` with raw-pointer params is unsound for Rust callers → `unsafe extern "C" fn` | 16.3 | — |
| Rust code called from Java runs on the Java thread's stack (`-Xss`) | 16.3 | — |
| Four questions per pointer; five ownership rules | 16.4 | — |
| `Option<Box<T>>` create/destroy with no `unsafe`; RAII wrapper on the Rust-caller side | 16.4 | — |
| Caller buffer + size query vs library `MeridianBuf` via `into_raw_parts` (len 53, cap 98) + `meridian_buf_free` | 16.4 | — |
| Callback trampolines with `void *user`; higher-ranked `&CStr`; panics parked and `resume_unwind`ed after C returns | 16.4 | — |
| Two heaps: counting global allocator vs `malloc`; allocators per cdylib (two Rust runtimes in one JVM) | 16.4 | XV.5 |
| Handles as integers: exposed provenance (Miri warning) vs generational registry (misuse = `-1`) | 16.4 | — |
| Thread-affine engine: `!Send` (E0277) + owner thread + bounded queue + one-shot replies; `EngineDown` | 16.4 | — |
| FFM arenas mapped to Rust ownership; `reinterpret(len, arena, cleanup)` adopting a Rust buffer | 16.4 | — |

## Promises to later Parts

- **Part XIX (Binary/OS):** the dynamic symbol table and the version script rustc uses for `cdylib` exports (`nm -D`),
  GOT/PLT and `strlen@GOTPCREL`, `dlopen` search paths, `RTLD_LOCAL`/`RTLD_GLOBAL` and symbol interposition,
  `-rdynamic`, unwinding across languages (`libgcc_s`, `.eh_frame`), reading a JVM `hs_err` log with native frames.
- **Part XX (Performance):** measure FFM downcall overhead against batch size (JMH; `Linker.Option.critical`), predicted
  here as tens of nanoseconds per call; `qsort_r` vs `sort_by` + `total_cmp` (indirect vs inlined comparator).
- **Part XXII (Ecosystem):** `bindgen`/`cbindgen` in build scripts, `-sys` crate conventions and the `links` key,
  cross-language LTO, `jextract` in the Java build, `staticlib` for cgo.
- **Part XV (unwritten 15.5–15.6):** `GlobalAlloc`/mimalloc as used by a `cdylib` (one allocator per library);
  sanitizers for bound C code; Miri's limits with FFI (`qsort`, `snprintf`, `strdup` unsupported).

## Promises kept

- `extern "C"` stable ABI, `catch_unwind` at FFM entry points, `extern "C-unwind"` (2.2, 8.3): 16.1 §3–4, 16.3.
- `cdylib` exported symbols (2.7): 16.3 §4 (unmangled symbol in asm; executable doesn't export; `cdylib` does).
- The fraud library via FFM: `meridian_score` codes 0/-1/-99 (8.3), concrete `score_batch` exports over generic
  internals (7.2), Java string transfer FFM vs JNI (9.2): 16.3 §3, §8, §9.
- `Option<&T>`/`repr(transparent)` guarantees, `repr(C)` enums (RFC 2195) (5.2, 6.5): 16.1 §3.
- Loading a `cdylib` plugin with the C-ABI vtable of 6.5: **partly**. `dlopen`/`dlsym` verified on `libc.so.6` and the
  `Symbol<'lib, F>` lifetime (plus its E0505) verified; the plugin load itself is an unverified `libloading` sketch, and
  16.3 §9 corrects `FfiRule`'s `&'static RuleVTable` (a lifetime or "never unload").
- The fraud library's thread-affine native scoring handle and the owner-thread design (11.2): 16.2 §9 + 16.4 §9.
- `unsafe extern` blocks with `safe` items in real bindings (15.1): 16.2 §9 (`vse_version`).
- `Vec::into_raw_parts`/`from_raw_parts` and `Box::into_raw` across the C boundary (15.3): 16.3, 16.4.
- Exposed provenance for addresses from C (15.2): 16.4 §4.
- `repr(C, u32)` enums and explicit encodings at boundaries, `conv: Rust` vs `extern "C"` in the ABI dump (18.6): 16.1.

## Meridian facts introduced

| System | Facts established | Where |
|---|---|---|
| Fraud library ABI governance | one `cbindgen`-generated, committed header; boundary types only `repr(C)`/`repr(transparent)` with `offset_of!`/`size_of` const assertions; incoming enums as `u32` codes (`DecisionCode`); `meridian_abi_version()` handshake at Java load; `extern "C"` + `catch_unwind`; `panic = "unwind"` | 16.1 §9 |
| `MeridianTxn` reorder incident (spring 2026) | Rust side reordered 24 B → 16 B largest-first; Java kept the old layout; no crash; the canary scored against wrong merchants' histories until the score-distribution alert fired; fixed by the governance rules | 16.1 §10 |
| Vendor scoring engine `libvse` | second opinion for transactions the in-house model flags for review (a small fraction of traffic); thread-affine handles; bound as `sys` (bindgen) + safe `Engine` (`!Send`, `Drop` → `vse_close`, `&mut self` + copy for `vse_last_error`); `vse_version` declared `safe` | 16.2 §9 |
| Model-loader errno incident (2026) | pod failed to start; log said "Is a directory (os error 21)"; real error ENOENT; a logging call clobbered errno; ~30 minutes lost on the volume mount; rule: `last_os_error()` is the first statement of a failure branch | 16.2 §10 |
| Fraud library C API v3 | `meridian_abi_version()` = 3; `meridian_scorer_new(cfg, **out)`/`meridian_scorer_free` (NULL no-op); `meridian_score_batch(s, ids, n, out)` thread-safe (`Sync` asserted), `ids`/`out` must not overlap; `MeridianConfig { block_at, review_at }`; codes 0 / -1 / -99, plus -2 = buffer too small (16.4) | 16.3 §3 |
| Fraud FFM binding (Java) | `FraudLibrary implements AutoCloseable`; `jextract` bindings in the repo, regenerated in CI; library loaded once into `Arena.global()`; ABI check at startup; batches ≤ 256 IDs per downcall, per-call confined arenas; one scorer per process, hourly model reload inside Rust (`ArcSwap<Model>`); -1 → `IllegalArgumentException` + metric, -99 → `IllegalStateException` + alert + circuit breaker to fallback rules | 16.3 §9 |
| Rule plugin host | rule `cdylib`s loaded once at startup, never unloaded (documented at `FfiRule`'s `'static` vtable) | 16.3 §9 |
| Backfill null out-pointer crash | the Rust backfill job linked the `rlib` and called the (then safe) exported `meridian_score` with `null_mut()` for `out` in warm-up; segfault in a crate with no `unsafe`; fix: `unsafe extern "C" fn` + null check → -1; Clippy `not_unsafe_ptr_arg_deref` enabled | 16.3 §10 |
| Vendor engine owner threads | a small pool of owner threads, one engine each (vendor allows several per process, each thread-bound), least-loaded dispatcher, bounded queues, one-shot replies; `EngineDown` → restart + metric | 16.4 §9 |
| `meridian_version_string` allocator incident (2026) | returned `CString::into_raw`, header said "free with free()"; a C++ tool worked for years under the `System` allocator; the fraud library adopted mimalloc in 2026 and the tool crashed intermittently in `free()`; fix: `MeridianBuf` + `meridian_buf_free`, every pointer-returning export has a `*_free`, Java adopts buffers via `reinterpret(..., cleanup)` | 16.4 §10 |
| Explain API (risk console) | Part XVI review capstone: PR adding `init`/`explain`/`last_explanation` (one lint warning, 16 defects); rewrite: `meridian_explainer_new/free`, `meridian_explain` → `MeridianBuf`, `MeridianExplainOptions { u32 top, u32 format, u8 include_negative }`, ABI version 4 | review |
| Geolocation library `libgeo` | design exercise: risk team's IP geolocation via a C library; reload semantics | 16.2 §14 |
| Tokenization library rewrite | design exercise: Rust library called from Java (FFM), Go (cgo), and Rust | 16.3 §14 |

## Verification

- `listings/part-16/`: **39 files, 61 checks, all PASS** in one final full-folder run (output saved to scratch
  `part-16/final/verify-final.txt`). rustc 1.98.1 stable, edition 2024;
  18 `miri-ok` runs (all on correct code, Stacked Borrows); 9 intended compile failures (`E0080`, `E0617`, `E0277`,
  `E0505`, and denied lints `improper_ctypes_definitions` ×3, `improper_ctypes`, `dangling_pointers_from_temporaries`);
  the nightly `#[rustc_abi(debug)]` dump checked in debug (`error:OnStack`) and release (`error:Cast`); 3 `release build`
  files used only for `tools/emit.ps1`; one `test` check (3 tests).
- **No UB demonstrations.** No listing violates a contract at run time. Contract-breaking code appears only as (a)
  compile-fail listings, (b) clearly labeled review sketches in debugging exercises (`model_dir`,
  `meridian_top_features`, `meridian_feature_name`), which are not listings and are never run, or (c) prose.
- Artifacts (`tools/emit.ps1`, quoted trimmed, symbols shortened and labeled): release asm + LLVM IR of
  `ch01-08-abi-asm.rs`; release asm + IR of `ch02-12-extern-call-asm.rs`; release asm + IR of
  `ch03-02-export-symbols.rs` (the `panic_cannot_unwind` landing pad); the nightly dump of `ch01-07-abi-dump.rs`.
- Every `rust` and `rust,compile_fail` block in the chapters and review was machine-checked line by line against the
  listings (a perl script over `listings/part-16/*.rs`); all `rust,ignore` blocks either name their listing or are
  labeled "not verified here" / "review sketch". Answer-key rewrites are labeled sketches.
- Where C code is required, listings simulate it with Rust functions exported under the same C ABI and symbol names
  (the `vendor` module of `ch02-11` and `ch04-05`), and say so.
- Unverifiable here and labeled with the local command: Java FFM/JNI code (JDK 22+, `--enable-native-access`), C
  headers, `cbindgen`, `bindgen`, `jextract`, Cargo `cdylib` settings, `nm -D`, the `libloading` plugin load, the
  `jni`-crate signature, `LD_DEBUG=bindings`.

## Word count

| File | Words (`wc -w`, code included) |
|---|---|
| README.md | 784 |
| ch01-abi-repr-c.md | 5,634 |
| ch02-calling-c.md | 6,264 |
| ch03-calling-rust-from-c-and-java.md | 5,469 |
| ch04-ownership-across-boundaries.md | 5,886 |
| review.md | 2,075 |
| answers-part-16.md | 7,382 |
| **Total** | **≈ 33,500** |

## Tooling notes

New in this Part:

- `improper_ctypes_definitions` checks only types passed **by value**. A non-`repr(C)` struct (or one with an
  `Option<u32>` field) behind `&T` or `*const T` gets no warning (`ch01-11-lint-blind-spot.rs`). Flagged by value:
  `Option<u32>`, `&str`/`str`, `String`, `Vec<T>`, `&[T]`, tuples, `char`, default-repr enums. Not flagged: `u128`,
  `Option<Box<T>>`, `repr(u32)` enums, `*mut c_char`.
- `#[rustc_abi(debug)]` (nightly): the argument *modes* for `Point`/`Triple` are the same in debug and release; the
  attributes differ (release adds `ReadOnly`/`CapturesNone` on the Rust `Indirect` pointer). `extern "C-unwind"` prints
  `conv: C, can_unwind: true`. The dump is ~750 lines for five functions: grep for `mode:|conv:|can_unwind`.
- Release asm: the Rust-convention caller of a 24-byte read-only struct is a single `jmp` (no copy); the C caller copies
  24 bytes to `[rsp]..[rsp+24]`.
- LLVM adds known-libcall attributes to `declare @strlen` (`memory(argmem: read)`, `nofree`, `willreturn`); rustc adds
  `nounwind`.
- Simulating a C library: `#[unsafe(no_mangle)] pub unsafe extern "C" fn` definitions plus `unsafe extern "C"`
  declarations (with opaque pointer types) in the same crate work natively and under Miri.
- Miri: `with_exposed_provenance_mut` prints the "integer-to-pointer cast … Miri might miss pointer bugs" warning but
  passes `miri-ok`; threads + `mpsc::sync_channel` one-shot replies are Miri-clean; `getenv("PATH")` is non-NULL under
  Miri too. Don't add Miri checks for `qsort`/`qsort_r`/`snprintf` (unsupported foreign functions).
- The Bash tool's heredoc turned `\\0` into `\0` inside a Rust string (an actual NUL in a label); write listings that
  contain backslashes with the Write tool.
- The Playground's `/execute` endpoint twice timed out ("The operation timed out: deadline has elapsed") under shared
  load while `/miri` succeeded; a retry passed.

From the earlier attempt (kept for reference):

- Miri on the Playground runs these libc calls: `strlen`, `getenv`, `malloc`, `free`, `memcpy`, `getpid`. It reports
  "unsupported operation" for `abs`, `qsort` and `strdup`, which is a Miri limitation, not UB. Declare `malloc`/`free`
  with `*mut c_void`: other pointer types trigger the `suspicious_runtime_symbol_definitions` warning.
- Calling a `#[unsafe(no_mangle)] extern "C"` function defined in the same crate, through an `unsafe extern "C"`
  declaration, works natively and under Miri.
- `libc::qsort_r` works natively. `dlopen`/`dlsym` on `libc.so.6` work. `dlsym` can't find a `#[no_mangle]` symbol
  in the Playground's own binary ("undefined symbol"), because executables don't export it.
- Lints: `improper_ctypes_definitions` (deny it to turn it into an error) flags `String` and `&str` parameters.
  `dangling_pointers_from_temporaries` is warn-by-default, so deny it to get an error. A literal null passed to
  `slice::from_raw_parts` is caught by the deny-by-default `invalid_null_arguments`.
- `E0617` rejects passing an `f32` to a C variadic function (with a hint to cast to `c_double`).
- In edition 2024 `std::env::set_var` is `unsafe` (E0133); in edition 2021 it compiles.
- `Vec::into_raw_parts` is stable on 1.98.1. `extern "C-unwind"` panics are caught by `catch_unwind` in the Rust
  caller. `io::Error::last_os_error()` after a failed `open` prints
  `No such file or directory (os error 2)`, `kind=NotFound`.
- A `const` layout assertion using `offset_of!` fails with `error[E0080]: evaluation panicked: <message>`, which
  suits CI ABI checks.
- An inline-asm `syscall` for `getpid`, libc's `getpid()` and `std::process::id()` all return the same pid (this Part
  uses `libc::syscall(SYS_getpid)` instead of inline asm).
- Nightly `#[rustc_abi(debug)]` contrasts the two conventions directly (see above).
- Byte offsets for `enum { Allow, Review(u8), Block(u64) }`: with `#[repr(C, u32)]` the `Review` payload sits at
  offset 8, with `#[repr(u32)]` at offset 4. Both are 16 bytes. Read only the defined offsets (as listing
  `ch01-03-enum-layouts.rs` does, through the documented struct/union shapes).
