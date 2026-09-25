# Chapter 18.7 — Tracing `let x = foo();` Through the Compiler

> **Where this sits:** Part XVIII · How rustc Works · chapter 7 of 7
> **Prerequisites:** Chapters 18.1–18.6, Chapter 2.3 (the `let x = 10` trace), Chapter 3.1 (drops), Chapter 8.3
> (landing pads).
> **After this chapter you can:** follow one line of Rust through every compiler stage with a real artifact at each
> step; say which stage introduced any given detail (a lifetime, a coercion, a drop, an unwind path, an attribute, an
> instruction); pick the right artifact and the right build profile for a question; and explain the whole compiler
> from memory in an interview, using one line as the thread.

---

## Pass 1 · User level — *One line, every stage*

### 1. Problem

Chapter 2.3 traced `let x = 10` from source to assembly, and found that `x` didn't exist at run time at all. That line
was deliberately simple: a constant, no calls, no heap, no drop. This chapter traces a line that exercises everything
this Part covered:

```rust,ignore
let x = foo();
```

where `foo` returns a heap-owning `String`, `x` is then lent to a function expecting `&str` (a coercion), its length is
read, and it must be dropped on every path out, including a panic. The program is listing `ch07-01-let-x-foo.rs`
(verified to build in debug and release):

```rust,ignore
#[inline(never)]
pub fn foo() -> String {
    String::from("meridian")
}

#[inline(never)]
pub fn log(s: &str) {
    std::hint::black_box(s);
}

#[inline(never)]
pub fn caller() -> usize {
    let x = foo();
    log(&x);
    x.len()
}
```

`#[inline(never)]` keeps the three functions visible as separate functions (Chapter 1.3's note on inspecting a lib
crate), and `black_box` (Chapter 4.1: an empty inline-assembly barrier) stops LLVM from deleting `log`'s body as
useless.

### 2. Mental model

The trace, with the artifact that shows each stage and whether it was verified here:

| # | Stage | What `let x = foo();` becomes | Query / crate | How to see it |
|---|---|---|---|---|
| 1 | Tokens | `let` `x` `=` `foo` `(` `)` `;` | `rustc_lexer` | Not printed by rustc; rust-analyzer's syntax-tree view (not verified here) |
| 2 | AST, expanded | Unchanged, but the crate gains an injected prelude import | `rustc_expand` (18.2) | `-Target expand` (verified) |
| 3 | HIR | `let x = foo();` with resolved paths; elided lifetimes explicit | `rustc_ast_lowering` (18.2) | `-Target hir` (verified) |
| 4 | Type check | `x: String`; `&x` adjusted to `&str` | `typeck` (18.3) | The `let () =` probe (verified) |
| 5 | THIR | The adjustment written out as a deref call | `thir_body` (18.4) | `-Zunpretty=thir-tree` (not verified here) |
| 6 | MIR, built + borrowck | `_1 = foo()`, a loan on `_1` for `log`, drop on every exit | `mir_built`, `mir_borrowck` (18.4, 18.5) | `-Z dump-mir` (not verified here) |
| 7 | MIR, optimized | Debug: calls and cleanup; release: inlined to field reads | `optimized_mir` (18.4) | `-Target mir` (verified) |
| 8 | Mono items | `foo`, `log`, `caller` plus the std instances they need | collector (18.6) | Debug IR's function list (verified) |
| 9 | LLVM IR | `alloca [24 x i8]` for `x`, `sret`, `invoke` + `landingpad` | `rustc_codegen_llvm` (18.6) | `-Target llvm-ir` (verified) |
| 10 | Assembly | Three stack slots, two calls, a conditional free | LLVM | `-Target asm` (verified) |

### 3. Rust code

**Stage 2: expansion.** The line has no macros, so the only change is to the crate around it (`tools/emit.ps1 -Target
expand`, nightly, verbatim, attributes on the functions condensed):

```text
#![feature(prelude_import)]
extern crate std;
#[prelude_import]
use std::prelude::rust_2024::*;
…
#[inline(never)]
pub fn caller() -> usize { let x = foo(); log(&x); x.len() }
```

The **prelude** is injected here: `String` in `foo`'s signature resolves because of `use std::prelude::rust_2024::*`.
[VERSION] The prelude module is chosen by edition (`rust_2024` here), which is how editions can add names like
`FromIterator` to the prelude (2021) without breaking older crates.

**Stage 3: HIR** (`-Target hir`):

```text
#[attr = Inline(Never)]
fn foo() -> String { String::from("meridian") }

#[attr = Inline(Never)]
fn log(s: &'_ str) { std::hint::black_box(s); }

#[attr = Inline(Never)]
fn caller() -> usize { let x = foo(); log(&x); x.len() }
```

Two changes, both from 18.2. `log`'s parameter gained its elided lifetime, `&'_ str` (Chapter 4.3's elision rules,
applied during lowering). And `#[inline(never)]` became `#[attr = Inline(Never)]`: [RUSTC] built-in attributes are
parsed into structured values during lowering, and the printer shows the parsed form. The line itself is unchanged:
`let` statements with a simple binding have nothing to desugar.

**Stage 4: type checking.** What did `typeck` decide `x` is? On stable, the oldest trick is to bind the value to a
pattern of the wrong type (listing `ch07-02-ask-the-type.rs`, verified):

```rust,compile_fail
fn foo() -> String {
    String::from("meridian")
}

fn main() {
    let () = foo();
}
```

```text
error[E0308]: mismatched types
 --> src/main.rs:9:9
  |
9 |     let () = foo();
  |         ^^   ----- this expression has type `String`
  |         |
  |         expected `String`, found `()`
```

Type checking also recorded something you can't see yet: at `log(&x)`, the argument has type `&String` and the
parameter wants `&str`, so it inserted a **deref coercion** as an adjustment (18.3). Watch for it in stage 7.

---

## Pass 2 · Systems level — *The back half of the compiler*

### 4. Under the hood

**Stages 5–6: THIR, built MIR, borrow checking.** Not visible on the Playground, but predictable from 18.4 and 18.5. THIR
writes the adjustment out as a call to `Deref::deref` on `&x`. MIR building gives `x` a local, schedules its drop at
the end of `caller`'s scope, and adds a cleanup path from every call after `x` exists. The borrow checker creates one
shared loan on `x` for `&x`, whose region covers the call to `log` and ends there (nothing keeps the `&str` afterward),
and a second for `x.len()`. No conflicts, no moves, and `x` is initialized at every use. Then regions are erased.

**Stage 7: optimized MIR, debug** (`-Target mir -Mode debug`, verbatim):

```text
fn caller() -> usize {
    let mut _0: usize;
    let _1: std::string::String;
    let _2: ();
    let _3: &str;
    let _4: &std::string::String;
    let mut _5: &std::string::String;
    scope 1 {
        debug x => _1;
    }

    bb0: {
        _1 = foo() -> [return: bb1, unwind continue];
    }

    bb1: {
        _4 = &_1;
        _3 = <String as Deref>::deref(copy _4) -> [return: bb2, unwind: bb6];
    }

    bb2: {
        _2 = log(copy _3) -> [return: bb3, unwind: bb6];
    }

    bb3: {
        _5 = &_1;
        _0 = String::len(move _5) -> [return: bb4, unwind: bb6];
    }

    bb4: {
        drop(_1) -> [return: bb5, unwind continue];
    }

    bb5: {
        return;
    }

    bb6 (cleanup): {
        drop(_1) -> [return: bb7, unwind terminate(cleanup)];
    }

    bb7 (cleanup): {
        resume;
    }
}
```

Everything this Part described is on the page:

- `x` is **local `_1`** (`debug x => _1`), unlike Chapter 2.3's constant `x`, which had no local at all. A `String`
  needs a place to live.
- `foo()`'s call has `unwind continue`: if `foo` panics, `x` doesn't exist yet, so there's nothing to clean up.
- The **deref coercion** is `bb1`: `_3 = <String as Deref>::deref(copy _4)`. The `&str` is produced by a function call,
  in debug.
- Every call *after* `_1` exists unwinds to `bb6 (cleanup)`, which drops `_1` and resumes unwinding: Chapter 8.3's
  landing pad, as MIR.
- `drop(_1)` in `bb4` is the normal-path drop. No drop flag: `x` is definitely initialized on every path (18.4's
  drop elaboration).

There's one more thing in the same output, printed after `foo`:

```text
alloc1 (size: 8, align: 1) {
    6d 65 72 69 64 69 61 6e                         │ meridian
}
```

That's the string literal as a **const-evaluation allocation** (18.4): eight bytes with alignment 1, which codegen
will turn into read-only data.

**Stage 7, release.** The same function after the MIR inliner (trimmed to the body; the output also lists sixteen
`scope N (inlined …)` entries, from `<String as Deref>::deref` down through `Vec::as_ptr`, `RawVecInner::non_null`, and
`from_utf8_unchecked`):

```text
    bb0: {
        StorageLive(_1);
        _1 = foo() -> [return: bb1, unwind continue];
    }

    bb1: {
        …
        _8 = copy (((((_1.0: std::vec::Vec<u8>).0: alloc::raw_vec::RawVec<u8>).0: alloc::raw_vec::RawVecInner).0: std::ptr::Unique<u8>).0: std::ptr::NonNull<u8>);
        _6 = copy _8 as *const u8 (Transmute);
        …
        _7 = copy ((_1.0: std::vec::Vec<u8>).1: usize);
        _5 = *const [u8] from (copy _6, move _7);
        …
        _4 = &(*_5);
        …
        _3 = copy _4 as &str (Transmute);
        …
        _2 = log(move _3) -> [return: bb2, unwind: bb4];
    }

    bb2: {
        StorageDead(_3);
        _0 = copy ((_1.0: std::vec::Vec<u8>).1: usize);
        StorageLive(_9);
        _9 = Le(copy _0, const 9223372036854775807_usize);
        assume(move _9);
        StorageDead(_9);
        drop(_1) -> [return: bb3, unwind continue];
    }
```

The deref coercion and `x.len()` are gone as calls. The `&str` is built from two field reads (the buffer pointer, five
projections deep inside `String → Vec → RawVec → RawVecInner → Unique → NonNull`, and the length) and two
`Transmute`s that change only the type. `x.len()` is the same length field again, with Chapter 18.4's
`assume(len <= isize::MAX)`. The drop and its cleanup path remain: dropping a `String` means freeing memory, which is
a real call.

**Stage 8: mono items.** The debug LLVM IR defines **18** functions. Their names, from the IR's comment lines:

```text
core::ptr::drop_glue::<alloc::vec::Vec<u8>>
core::ptr::drop_glue::<alloc::raw_vec::RawVec<u8>>
core::ptr::drop_glue::<alloc::string::String>
core::hint::black_box::<&str>
<u8 as <[_]>::to_vec_in::ConvertVec>::to_vec::<alloc::alloc::Global>
playground::foo
playground::log
playground::caller
<alloc::string::String>::len
<*const ()>::is_aligned_to
<alloc::raw_vec::RawVecInner>::with_capacity_in
core::intrinsics::cold_path
<alloc::vec::Vec<_, _>>::set_len::precondition_check
core::ptr::copy_nonoverlapping::precondition_check
core::hint::assert_unchecked::precondition_check
core::ub_checks::maybe_is_nonoverlapping::runtime
<alloc::string::String as core::convert::From<&str>>::from
<alloc::string::String as core::ops::deref::Deref>::deref
```

That's the collector's output (18.6) made concrete. Your three functions are roots. Everything else was reached from
them: drop glue for `String` and the types inside it, `black_box::<&str>` (a generic instance), `String::from` and the
`to_vec` it uses to copy the literal, and `#[inline]` std functions such as `String::len` compiled locally. The
`precondition_check` functions are the debug-build **UB checks** in `unsafe` std code (Chapter 15.1's "debug aborts on a
std precondition check"): the standard library checks its own preconditions when debug assertions are on. The release
IR defines **3** functions: everything else was inlined.

**Stage 9: LLVM IR, debug** (the `caller` function, `!dbg` metadata and debug-info intrinsics removed):

```text
define i64 @_RNvCs8wKijPXT76U_10playground6caller() unnamed_addr #2 personality ptr @rust_eh_personality {
start:
  %0 = alloca [16 x i8], align 8
  %x = alloca [24 x i8], align 8
; call playground::foo
  call void @_RNvCs8wKijPXT76U_10playground3foo(ptr sret([24 x i8]) align 8 %x) #16
; invoke <alloc::string::String as core::ops::deref::Deref>::deref
  %1 = invoke { ptr, i64 } @_RNvXsx_NtCs6i54tJFfzR_5alloc6stringNtB5_6StringNtNtNtCsgxBkk5gSRhY_4core3ops5deref5Deref5derefCs8wKijPXT76U_10playground(ptr align 8 %x)
          to label %bb2 unwind label %cleanup

cleanup:                                          ; preds = %bb3, %bb2, %start
  %2 = landingpad { ptr, i32 }
          cleanup
  …

bb2:                                              ; preds = %start
  %_3.0 = extractvalue { ptr, i64 } %1, 0
  %_3.1 = extractvalue { ptr, i64 } %1, 1
; invoke playground::log
  invoke void @_RNvCs8wKijPXT76U_10playground3log(ptr %_3.0, i64 %_3.1)
          to label %bb3 unwind label %cleanup

bb3:                                              ; preds = %bb2
; invoke <alloc::string::String>::len
  %_0 = invoke i64 @_RNvMNtCs6i54tJFfzR_5alloc6stringNtB2_6String3lenCs8wKijPXT76U_10playground(ptr align 8 %x)
          to label %bb4 unwind label %cleanup

bb4:                                              ; preds = %bb3
; call core::ptr::drop_glue::<alloc::string::String>
  call void @_RINvNtCsgxBkk5gSRhY_4core3ptr9drop_glueNtNtCs6i54tJFfzR_5alloc6string6StringECs8wKijPXT76U_10playground(ptr align 8 %x)
  ret i64 %_0
```

(Block order and the terminate/resume blocks trimmed.) Each MIR concept has an LLVM counterpart:

| MIR | LLVM IR |
|---|---|
| local `_1: String` (24 bytes, `Memory` repr) | `%x = alloca [24 x i8], align 8` |
| `_1 = foo()` returning a `String` | `call @foo(ptr sret([24 x i8]) %x)`: `foo` writes directly into `x`'s slot (18.6's `Indirect` pass mode) |
| call with `unwind: bb6` | `invoke … to label %bb2 unwind label %cleanup` |
| `bb6 (cleanup)` | `landingpad { ptr, i32 } cleanup` |
| `&str` (a `ScalarPair`) | `{ ptr, i64 }`, split into `%_3.0`, `%_3.1` and passed as two arguments (the `Pair` pass mode) |
| `drop(_1)` | `call @drop_glue::<String>(ptr %x)` |

The function has a `personality ptr @rust_eh_personality`: the routine the unwinder calls to decide what each landing
pad does (Chapter 8.3, Part XIX).

**Stage 9, release** (the whole function, symbols shortened by hand; the `; call` comments are rustc's):

```text
define noundef range(i64 0, -9223372036854775808) i64 @caller() unnamed_addr #0 personality ptr @rust_eh_personality {
start:
  %x = alloca [24 x i8], align 8
  call void @llvm.lifetime.start.p0(ptr nonnull %x)
; call playground::foo
  call void @foo(ptr noalias nofree noundef nonnull sret([24 x i8]) align 8 captures(none) dereferenceable(24) %x) #10
  %0 = getelementptr inbounds nuw i8, ptr %x, i64 8
  %_8 = load ptr, ptr %0, align 8, !nonnull !9, !noundef !9
  %1 = getelementptr inbounds nuw i8, ptr %x, i64 16
  %_7 = load i64, ptr %1, align 8, !noundef !9
; call playground::log
  tail call void @log(ptr noalias nofree noundef nonnull readonly captures(address, read_provenance) %_8, i64 noundef %_7)
  %_9 = icmp sgt i64 %_7, -1
  tail call void @llvm.assume(i1 %_9)
  %x.val2 = load i64, ptr %x, align 8
  %2 = icmp eq i64 %x.val2, 0
  br i1 %2, label %drop_glue.exit, label %deallocate
deallocate:
; call __rustc::__rust_dealloc
  tail call void @__rust_dealloc(ptr noundef nonnull %_8, i64 noundef %x.val2, i64 noundef range(i64 1, -9223372036854775807) 1) #8
  br label %drop_glue.exit
drop_glue.exit:
  call void @llvm.lifetime.end.p0(ptr nonnull %x)
  ret i64 %_7
}
```

(Block labels shortened; the originals are long mangled names from inlined functions.) Read it against the MIR:

- The `String`'s fields are at offsets 8 (the pointer) and 16 (the length), so offset 0 is the capacity: the
  `(cap, ptr, len)` order Chapter 9.1 read from `Vec::push`'s asm. [RUSTC] That order is the compiler's choice for the
  default representation, not a promise.
- `log` gets exactly two values, the pointer and the length, with `noalias readonly` on the pointer (18.6).
- **The length is loaded once, before `log`, and returned after it** (`ret i64 %_7`). LLVM knows `log` can't change
  `x`'s length: `x`'s slot was never passed to `log`, only the heap pointer, and that as `readonly`.
- `range(i64 0, -9223372036854775808)` on the return value is the MIR `assume(len <= isize::MAX)`, now a return-value
  fact the *callers* of `caller` can use.
- The drop is inlined `drop_glue::<String>`: free the buffer only if the capacity is non-zero (an empty `String`
  doesn't allocate, Chapter 9.2), then `__rust_dealloc(ptr, capacity, align 1)`.
- There's no cleanup block any more. The only calls left while `x` is alive are to `log` and to the deallocator, and
  the release IR marks both `nounwind` (`; Function Attrs: noinline nounwind …` above `log`'s definition: it contains
  only `black_box`'s inline assembly). A call that can't unwind needs no landing pad, so the `invoke` became a plain
  `call` and the cleanup path was deleted. [RUSTC/LLVM, observed]
- `llvm.lifetime.start`/`end` are MIR's `StorageLive`/`StorageDead` (18.4).

**Stage 10: assembly, release** (verbatim):

```text
playground::foo:
	push	rbx
	mov	rbx, rdi
	call	qword ptr [rip + __rustc::__rust_no_alloc_shim_is_unstable_v2@GOTPCREL]
	mov	edi, 8
	mov	esi, 1
	call	qword ptr [rip + __rustc::__rust_alloc@GOTPCREL]
	test	rax, rax
	je	.LBB0_2
	movabs	rcx, 7953754296899757421
	mov	qword ptr [rax], rcx
	mov	qword ptr [rbx], 8
	mov	qword ptr [rbx + 8], rax
	mov	qword ptr [rbx + 16], 8
	mov	rax, rbx
	pop	rbx
	ret

.LBB0_2:
	mov	edi, 1
	mov	esi, 8
	call	qword ptr [rip + alloc::raw_vec::handle_error@GOTPCREL]

playground::log:
	mov	qword ptr [rsp - 16], rdi
	mov	qword ptr [rsp - 8], rsi
	lea	rax, [rsp - 16]
	#APP
	#NO_APP
	ret

playground::caller:
	push	r14
	push	rbx
	sub	rsp, 24
	mov	rdi, rsp
	call	qword ptr [rip + playground::foo@GOTPCREL]
	mov	r14, qword ptr [rsp + 8]
	mov	rbx, qword ptr [rsp + 16]
	mov	rdi, r14
	mov	rsi, rbx
	call	qword ptr [rip + playground::log@GOTPCREL]
	mov	rsi, qword ptr [rsp]
	test	rsi, rsi
	je	.LBB2_2
	mov	edx, 1
	mov	rdi, r14
	call	qword ptr [rip + __rustc::__rust_dealloc@GOTPCREL]

.LBB2_2:
	mov	rax, rbx
	add	rsp, 24
	pop	rbx
	pop	r14
	ret
```

`let x = foo();` at the end of the pipeline:

- **In `caller`**: `sub rsp, 24` reserves `x`'s 24 bytes; `mov rdi, rsp` passes their address to `foo` as the hidden
  return pointer (`sret`); after the call, `x` *is* `[rsp]`, `[rsp + 8]`, `[rsp + 16]`: capacity, pointer, length.
- **In `foo`**: allocate 8 bytes with alignment 1 (`__rust_alloc(8, 1)`), store the eight bytes of `"meridian"` with one
  64-bit immediate (7953754296899757421 is `6d 65 72 69 64 69 61 6e` read as a little-endian integer: the const
  allocation from stage 7, folded into an instruction), then fill in `x` through `rbx`, the saved `sret` pointer:
  capacity 8, pointer, length 8. If allocation fails, `handle_error` is called and never returns (Chapter 9.1: OOM
  aborts). The first call, to `__rust_no_alloc_shim_is_unstable_v2`, is an internal symbol rustc's allocator shim
  emits for linking reasons [RUSTC]; it isn't part of your program's logic.
- **`log(&x)`** is two register moves: pointer in `rdi`, length in `rsi`. The whole deref coercion cost nothing.
- **`x.len()`** is `rbx`, loaded before `log` and kept in a callee-saved register across the call (`push rbx` saves the
  caller's value), exactly as the release IR promised.
- **The drop** is `test rsi, rsi` on the capacity and a call to `__rust_dealloc(ptr, cap, 1)` only if it's non-zero.
- **No landing pad**, as in the release IR.
- `log` itself spills its `&str` to the stack and executes an empty `#APP`/`#NO_APP` block: `black_box` is inline
  assembly the optimizer must assume reads the value (Chapter 4.1).

Compared with Chapter 2.3's `let x = 10`, which vanished into a constant, `let x = foo();` becomes a 24-byte stack
object, a heap allocation, an `sret` pointer, and a conditional free. That's the difference between a `Copy` scalar and
an owning type, measured at every stage.

### 5. Memory

Where `x` lives at each stage:

| Stage | `x` is… | Size |
|---|---|---|
| HIR | A binding in a `let` statement | — |
| Type check | Type `String` | — |
| MIR | Local `_1: String`, `debug x => _1` | 24 bytes (3 × `usize`) |
| Debug LLVM IR | `%x = alloca [24 x i8], align 8` | 24 bytes on the stack |
| Release LLVM IR | The same `alloca`, bracketed by lifetime markers, written by `foo` through `sret` | 24 bytes |
| Release asm | `[rsp]`, `[rsp + 8]`, `[rsp + 16]` in a 24-byte frame area | 24 bytes |
| Heap | One 8-byte block from `__rust_alloc(8, 1)`, holding `meridian` | 8 bytes requested (the allocator may round up, Chapter 1.1) |

Two observations for an architect. First, the heap block is exactly the string's length: `String::from(&str)` allocates
the capacity it needs, no growth policy involved (compare Chapter 9.1's `push` growth). Second, `x`'s stack slot
isn't released by any instruction: it's part of `caller`'s frame, reclaimed by `add rsp, 24`. Only the heap block needs
an explicit free, which is why `Drop` for `String` is one conditional call.

### 6. CPU / OS

- **Calls go through the GOT.** `call qword ptr [rip + playground::foo@GOTPCREL]` is an indirect call through the
  global offset table, the position-independent code sequence the Playground's build produces for a library crate.
  Part XIX explains relocations and when the linker relaxes these into direct calls.
- **The allocator is outside your code.** `__rust_alloc` and `__rust_dealloc` are shims that forward to the global
  allocator (the system `malloc` by default on Linux, Chapter 9.1; the counting allocator of Part III when you install
  one). The actual cost of `let x = foo();` is dominated by that `malloc`/`free` pair, not by anything the compiler
  generated around it.
- **Register discipline.** `r14` and `rbx` are callee-saved in the System V ABI, so `caller` saves them (`push`) to keep
  the pointer and length alive across calls. That's the register allocator (Chapter 17.8) choosing registers that
  survive calls for values needed after them.

---

## Pass 3 · Architect level — *Artifact literacy as a team skill*

### 7. Trade-offs

**Which stage, which profile, which tool?**

| You want to know… | Stage | Profile | Tool (verified here = Playground) |
|---|---|---|---|
| What a macro or `derive` generates | Expansion | Either | `-Target expand` (verified), `cargo expand` (third-party) |
| What `for`, `?`, `.await` became | HIR | Either | `-Target hir` (verified), `-Zunpretty=hir` locally |
| What type something has | Type check | Either | `let () =` probe (verified), rust-analyzer inlay hints |
| Where drops, unwinds, and moves happen | MIR | **Debug** | `-Target mir` (verified), `--emit=mir` |
| Why the borrow checker rejects something | Borrowck-stage MIR | Either | `-Z dump-mir` + `-Z nll-facts` locally (not verified here) |
| Which instances exist; binary-size questions | Mono items | Both | debug IR function list (verified), `cargo llvm-lines` (third-party) |
| Which guarantees LLVM gets (`noalias`, ranges) | LLVM IR | **Release** | `-Target llvm-ir` (verified), `--emit=llvm-ir` |
| What actually executes; performance | Assembly | **Release** | `-Target asm` (verified), `cargo-show-asm`, Compiler Explorer |

The profile column is the one teams get wrong: **debug artifacts answer semantic questions** (when, where, in what
order), **release artifacts answer performance questions** (how much, how fast). §10 is what happens when you mix
them up.

> **Why not always read assembly, since it's "the truth"?** Because it's the truth about *one* compiler version, target,
> and profile, after every optimization has had its way. It tells you what ran, not what the language guarantees. A drop
> that disappeared from the asm still happens semantically (it was inlined), and a check that vanished may have been
> proven unnecessary or may depend on an `assume`. Read the earliest stage that answers your question, and the asm when
> the question is about cost.

### 8. Java comparison

The same line in Java, `String x = foo(); log(x); return x.length();`, compiled by javac to bytecode (illustrative, not
verified here: `javac` + `javap -c`):

```text
invokestatic  foo:()Ljava/lang/String;
astore_1
aload_1
invokestatic  log:(Ljava/lang/String;)V
aload_1
invokevirtual java/lang/String.length:()I
ireturn
```

| | Java | Rust |
|---|---|---|
| `x` is | A reference in local slot 1; the object is on the heap with a header | 24 bytes on the stack (cap, ptr, len); only the bytes are on the heap |
| Passing `x` to `log` | Copies the reference | Passes a pointer and length (a `&str` view) |
| `x.length()` | A virtual call (devirtualized and inlined by the JIT once `String` is known) | A register read, decided at compile time |
| End of scope | Nothing; the GC reclaims the object later | An inlined `free`, if capacity is non-zero |
| Where optimization happens | C1/C2 at run time, guided by profiles | MIR passes and LLVM at build time |
| What you can inspect | Bytecode (`javap`), JIT output (`-XX:+PrintAssembly` with hsdis) | Every stage above |

> **Analogy limit.** Tracing a Java line ends at bytecode unless you attach a disassembler to the JIT, and even then the
> machine code depends on the run's profile and can be deoptimized and regenerated while the program runs. A Rust trace
> ends at machine code that is fixed at build time. That's why Rust performance reviews can quote assembly in a pull
> request and Java ones usually quote benchmarks.

### 9. Production scenario

**Artifact-backed performance reviews.** Meridian's gateway and payments-core teams adopted a rule after the incidents
in 18.4, 18.6, and §10 below: a pull request that claims a performance effect on a hot path includes the evidence at
the right stage.

- Claims about **allocations** attach a counting-allocator test (the Part III instrument) or the release asm showing
  the `__rust_alloc` calls, as in stage 10.
- Claims about **dispatch or inlining** attach release asm for the function, generated with the pinned toolchain
  (1.98.1, Chapter 2.1) and the service's real profile. Symbol names are v0 and must be demangled.
- Claims about **drop timing or lock scope** attach debug MIR (18.4's `let _` and `match` cases).
- Small reproductions go through the Playground (as every artifact in this Part did). Real crates use `cargo rustc
  --release -- --emit=asm` or `cargo-show-asm` locally.

Reviewers push back on performance claims made from debug artifacts, or from benchmarks without the artifact that
explains them.

### 10. Failure scenario

**The length cache that truncated messages.** An engineer on the market-data team was optimizing a hot path that
formats outgoing frames. Reading the **debug** MIR and LLVM IR of a function like `caller`, they saw
`String::len(move _5)` as a call, and in the IR an `invoke` of `String::len` with its own landing pad. They concluded
that `len()` was an expensive call and changed an internal API to pass a cached length alongside each buffer:
`fn encode(buf: &mut String, len: usize)`.

Two weeks later, a change inside `encode` appended a sequence number to `buf` with `push_str` and then used the cached
`len` to write the frame's length prefix. The prefix was now short by the sequence number's length, and a downstream
consumer truncated frames. Replaying a morning's capture showed about 1 in 400 frames affected (those carrying the new
field).

The trace in this chapter shows what the engineer should have read. In the **release** MIR, `len()` is a field read
(`copy ((_1.0: Vec<u8>).1: usize)`); in the release asm, it's a register that was loaded once (`rbx`). The "call" only
exists in debug builds, where nothing is inlined. The optimization saved nothing and created a second source of truth
for a value the `String` already carries.

The team's fixes:

1. The cached-length parameter was removed; `encode` computes the prefix from `buf.len()` after all appends.
2. The review guideline in §7 ("release artifacts for performance questions") was added to the performance-review
   rule in §9.
3. A property test now checks that every encoded frame's length prefix equals the frame's actual length.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVIII).*

1. Trace `let x = foo();` through the compiler out loud, naming each stage and one thing that changes at it.
2. Where does the deref coercion in `log(&x)` come from, where does it become visible, and where does it disappear?
3. Why does `caller`'s debug MIR have a cleanup block and its release asm have no landing pad?
4. Explain `sret`. Where is it decided, and what does it look like in IR and in assembly?
5. The debug IR defines 18 functions and the release IR 3. Where did the other 15 come from, and where did they go?
6. What does `range(i64 0, -9223372036854775808)` on `caller`'s return value mean, and where did it originate?
7. Which artifact and which profile would you use to answer: "when is this lock released?", "does this loop allocate?",
   "is this call devirtualized?"
8. Compare the lifecycle of `x` in this Rust function and in the equivalent Java method, from allocation to
   reclamation.

### 12. Exercises

- **Beginner.** Replace `log(&x)` with `log(x.as_str())` in listing `ch07-01-let-x-foo.rs`. Predict the debug MIR and
  the release asm, then check both. What changed at which stage?
- **Intermediate.** Make `foo` return `Box<str>` instead of `String`. Predict the size of `x`'s stack slot, the pass
  mode of the return value, and the drop code, then check with the IR and asm.
- **Advanced.** Add a `panic!` path to `log` (for example, if the string is empty). Predict how the release IR and asm of
  `caller` change (landing pad, `_Unwind_Resume`), then verify. Relate it to Chapter 8.3's landing-pad listing.
- **Systems.** Change `caller` to `let x = foo(); let y = x.clone(); log(&y); x.len()`. Count allocations with the
  counting allocator (Part III), then find each `__rust_alloc`/`__rust_dealloc` in the release asm.
- **Architecture.** Pick one hot function in a service you know. Write the one-page "artifact brief" a reviewer would
  need for it: which stages to capture, with which toolchain and profile, which facts to extract, and how to keep the
  brief current when the compiler version changes.

### 13. Debugging exercise

A reviewer blocks a pull request with this comment: "`log(&x)` converts `String` to `&str`. That's a conversion, so it
must copy the bytes. Pass `x.clone()` into a `log_owned(String)` instead, so the cost is at least explicit."

1. Using stage 7 (debug and release MIR) and stage 10 (asm), show what `&x` → `&str` does and doesn't do. Quote the
   instructions that implement it.
2. What would the reviewer's suggestion cost, in allocations and instructions? Point to where each would appear in the
   release asm.
3. What *would* justify an owned parameter here (think about lifetimes, threads, and Chapter 4.3)? How would you write
   the review reply?

### 14. Design exercise

**An artifact-regression check in CI.** The gateway team wants CI to catch performance regressions in about twenty hot
functions *before* benchmarks run: for example, a new `__rust_alloc` call in the request path, a lost inlining, or a
bounds check that reappeared. Design the check: how you pin the toolchain and target, how you extract a function's
assembly from a real crate (symbol names are v0-mangled and generic instances multiply), what you compare (instruction
counts, specific calls, a golden file), how you handle legitimate changes (compiler upgrades, intentional refactors),
and who owns the golden files. Name the false positives and false negatives your design accepts, and compare it with
running the benchmark suite on every pull request.
