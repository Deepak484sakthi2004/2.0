# Part XVI — FFI and Systems Programming

> **Part question:** *When Rust code and code the Rust compiler didn't build must call each other, what exactly must
> both sides agree on, who checks it, and how do you design the boundary so that the rules the compiler can't see are
> either enforced by types or impossible to misread?*

For fifteen Parts, every call was Rust calling Rust, and the compiler saw both ends. Part XVI is about the calls where
it sees only one: Rust calling the C library and vendor SDKs, and C, C++, or the JVM calling Rust. Meridian's fraud
feature library, called from Java through FFM since Chapter 1.2, is the running example. Chapter 8.3 gave it one
entry point, and this Part gives it a complete, reviewed C API.

The Part treats FFI as **interface engineering**. An ABI is a contract made of four agreements: data layout, calling
convention, symbol names, and unwinding. Each is shown with a real compiler artifact: a `#[rustc_abi(debug)]` dump that
puts `conv: Rust` next to `conv: C`, the LLVM IR attribute that copies a struct onto the stack, and the landing pad
that turns a panic into an abort. Each rule of a boundary is then expressed twice: as a sentence in a header for the
side Rust can't check, and as a Rust type (`&CStr`, `Option<Box<T>>`, `Drop`, `&mut self`, `!Send`, a lifetime on a
symbol) for the side it can.

The Part keeps to one discipline throughout. **Every listing is correct code.** Where a contract can be broken, the
Part shows the compiler or a lint rejecting the mistake, or a design that turns the mistake into an error code, and
explains in a sentence what would go wrong otherwise, pointing to Chapter 15.1 for why it's undefined. Miri appears
only to show that correct code is clean.

## Chapter map

```text
16.1 ABIs, Calling Conventions,      the four agreements; repr(C) layouts pinned by const assertions; Option<&T> and
     and repr(C)                     repr(transparent) guarantees; repr(C, u32) vs repr(u32) enums, and why incoming
                                     enums are integers; System V in a rustc_abi dump, LLVM IR, and asm; extern "C"
                                     vs "C-unwind"; the Rust → ABI → C → OS ladder
      │
16.2 Calling C from Rust             declaration / call / safe wrapper; strlen, getenv, snprintf, malloc/free,
                                     qsort_r with a callback, open + errno; CStr/CString/OsStr; the lints; a vendor
                                     engine bound as `sys` + safe layers with `safe` extern items
      │
16.3 Calling Rust from C             ten rules for a C API written in Rust; the fraud library's exports with
     (and from Java via FFM)         catch_unwind, checked (ptr, len) inputs, and a Sync assertion; cdylib symbols and
                                     dlopen/dlsym; cbindgen and jextract; the FFM binding; JNI's Modified UTF-8
      │
16.4 Ownership and Allocation        four questions for every pointer; Box/Vec ownership transfer with matching free
     Across Language Boundaries      functions; caller vs library buffers; callbacks with user data and parked
                                     panics; two heaps in one process; registry handles; a thread-affine engine
                                     behind an owner thread
      │
Part XVI Review                      a boundary API review of an "explain" PR (one lint warning, a dozen defects);
                                     interview mode
```

## What you'll be able to do after Part XVI

- Say what an ABI consists of, which Rust feature pins each part, and read a struct's calling convention from a
  compiler dump, IR, or assembly.
- Design boundary types whose layouts are fixed, tested in CI, and versioned, and accept enum-like values from outside
  without ever creating an invalid value.
- Bind a C library as a raw `sys` layer plus a safe API whose types carry the C contract.
- Build a C API for a Rust library that a JVM can call from many threads without a panic, a bad pointer, or bad text
  taking the process down.
- Answer, for any pointer crossing the boundary, who allocated it, who frees it and with which allocator, how long it's
  valid, and which threads may touch it, and enforce the Rust half of each answer with a type.
- Integrate a thread-affine native resource into a multi-threaded service.

## Listings

`listings/part-16/`: 43 files, 65 checks, all verified on rustc 1.98.1 (edition 2024) with `tools/verify.ps1`: 18
runs under **Miri** (every one clean, on correct code), 9 intended compile errors or denied lints, the
`#[rustc_abi(debug)]` dump on nightly (checked in debug and release), 3 release builds used for `tools/emit.ps1`
artifacts, and one test suite.

Four listings cross a real language boundary. The Playground's container has `rustc`, `gcc`, and binutils, so they
write C and Rust sources to `/tmp`, build them (a `cdylib` with `rustc --crate-type cdylib`, C with
`gcc -Wall -Wextra -Werror`), run the result, and inspect exported symbols with `nm -D`, asserting that every inner
step succeeded: a Rust binding over a gcc-built C library (16.2), a C program calling the Rust fraud library (16.3), a
Rust plugin loaded with `dlopen` (16.3), and a C client following every ownership rule, also run under
AddressSanitizer and LeakSanitizer (16.4). The C headers and C code shown in those sections come from these listings.
Java code, `cbindgen`/`bindgen`/`jextract` commands, and Cargo settings are marked "not verified here," each with the
command to run locally. Listings that need a C library *and* Miri simulate it with Rust functions exported under the
same C ABI and symbol names, and say so.
