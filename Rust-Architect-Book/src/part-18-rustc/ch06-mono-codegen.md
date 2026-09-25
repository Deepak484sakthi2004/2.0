# Chapter 18.6 — Monomorphization and Codegen Backends (LLVM, Cranelift)

> **Where this sits:** Part XVIII · How rustc Works · chapter 6 of 7
> **Prerequisites:** Chapter 7.1 (instances, v0 symbols), Chapter 7.3 (codegen units, instance growth), Chapter 6.4
> (vtables in IR), Chapter 5.2 (layout and niches), Chapters 3.3 and 4.1 (`noalias`), Chapter 18.4 (optimized MIR).
> **After this chapter you can:** explain how rustc decides which functions exist in the binary; trace an attribute
> such as `noalias` from rustc's ABI computation to LLVM IR to the machine code it enables; read symbol names, vtable
> entries, and type layouts straight from the compiler; explain function merging and why Rust never promises distinct
> function addresses; and choose codegen settings and backends for development and release builds.

---

## Pass 1 · User level — *From one generic body to machine code*

### 1. Problem

After borrow checking and MIR optimization (18.4, 18.5), rustc holds one optimized MIR body per function. Most of them
are generic, and none of them is machine code. Between here and an object file, the compiler must decide:

- **which concrete functions exist at all**: `fee::<Card>`, `fee::<Wallet>`, `drop_glue::<String>`, the vtable for
  `Circle as Shape`;
- **how each one is called**: which arguments go in registers, which by hidden pointer, and which promises (`noalias`,
  `readonly`, `nonnull`) the backend may rely on;
- **how each type is laid out** in memory, and what each function is called in the symbol table;
- **which backend** turns it into machine code.

You've seen the results of these decisions throughout the book: `noalias` in Chapters 3.3 and 4.1, instances and v0
symbols in 7.1, vtables in 6.4, niches in 5.2. This chapter asks the compiler for the decisions themselves, and shows
one decision with a production consequence: in release builds, **two different functions can have the same address**.

### 2. Mental model

```text
 optimized MIR (one generic body per function)
   │  MONOMORPHIZATION COLLECTOR          [RUSTC] rustc_monomorphize
   │    start from ROOTS: main (binaries); non-generic public items (libraries); statics
   │    walk each root's MIR: every call with concrete generic args → an INSTANCE
   │                          every cast to dyn Trait              → a VTABLE (+ its methods)
   │                          every drop of a type                  → its DROP GLUE
   │                          every generic const                   → evaluate it (post-mono errors, 7.1)
   │    repeat until no new items: the set of MONO ITEMS
   ▼
 PARTITION into codegen units (CGUs)  (7.3: 16 non-incremental, 256 incremental by default)
   │
   ▼  per CGU, possibly in parallel — rustc_codegen_ssa (backend-independent) + one backend:
 for each instance:  layout_of(each local's type)   fn_abi_of_instance(signature)   symbol_name(instance)
   │                 translate MIR statements and terminators into backend IR
   ▼
 LLVM IR ──► LLVM optimization passes ──► machine code ──► object file (.o)  ──► linker (Part XIX)
        (or Cranelift IR, or GCC's IR, with the other backends)
```

Two ideas carry the chapter.

1. **Reachability decides existence.** A generic function that is never instantiated produces no code (7.1). A
   concrete instance exists because the collector found a path to it from a root. That's closer to a garbage
   collector's mark phase than to a compiler that "compiles every function."
2. **The ABI is rustc's decision; the optimization is LLVM's.** rustc computes, per function, how each argument is
   passed and what may be assumed about it. LLVM only optimizes what it's told. `noalias` exists in LLVM IR because
   rustc proved exclusivity (the borrow checker) and wrote it down (the ABI computation).

### 3. Rust code

**Asking for the ABI.** A nightly testing attribute, `#[rustc_abi(debug)]`, prints what `fn_abi_of` computed for a
function (listing `ch06-01-fn-abi.rs`, two verified checks):

```rust,ignore
#[rustc_abi(debug)]
fn add_into(dst: &mut i32, src: &i32) {
    *dst += *src;
}
```

Release build, trimmed to the argument attributes (the full dump also prints each argument's layout):

```text
error: fn_abi_of(add_into) = FnAbi {
           args: [
               ArgAbi { ty: &mut i32, …
                   mode: Direct(
                       ArgAttributes {
                           regular: NoAlias | NonNull | NoUndef | NoFree,
                           pointee_size: Size(4 bytes),
                           pointee_align: Some(Align(4 bytes)),
               ArgAbi { ty: &i32, …
                   mode: Direct(
                       ArgAttributes {
                           regular: CapturesReadOnly | NoAlias | NonNull | ReadOnly | NoUndef | NoFree,
           ret: ArgAbi { ty: (), … mode: Ignore },
           conv: Rust,
           can_unwind: true,
```

(Reformatted: the fields are verbatim, the nesting condensed.) The debug build of the same function prints
`regular: NonNull | NoUndef` for both arguments and nothing else.

That's the whole `noalias` story in one dump:

- `&mut i32` is `NoAlias`: [LANG] a `&mut` is the only way to reach its target while it's live (Part IV), so rustc
  can promise LLVM that no other pointer used by the function touches that memory.
- `&i32` is `NoAlias` **and** `ReadOnly`: the target of a shared reference to a type without `UnsafeCell` can't change
  while the reference is live. (Chapter 4.1 showed `&Cell<i32>` losing `noalias` because `Cell` is built on
  `UnsafeCell`.)
- `NonNull`, and `pointee_size`/`pointee_align`, which become `dereferenceable(4)` and `align 4`: a reference is always
  valid to read (Chapter 5.2's validity invariant).
- `conv: Rust`: the unspecified Rust calling convention [LANG: no stable Rust ABI, Chapter 6.5], and `can_unwind:
  true` (Chapter 8.3).
- The `()` return is `Ignore`: a zero-sized value isn't passed at all.

**The attributes in LLVM IR, and what they buy.** Listing `ch06-02-noalias-ir.rs` (the same signature, with the body
doing `*dst += *src` twice) in release LLVM IR:

```text
define void @_RNvCsjau7DNBNlby_10playground8add_into(ptr noalias nofree noundef align 4 captures(none) dereferenceable(4) %dst, ptr noalias nofree noundef readonly align 4 captures(none) dereferenceable(4) %src) unnamed_addr #0 {
  %_3 = load i32, ptr %src, align 4, !noundef !4
  %0 = load i32, ptr %dst, align 4, !noundef !4
  %reass.add = shl i32 %_3, 1
  %1 = add i32 %0, %reass.add
  store i32 %1, ptr %dst, align 4
  ret void
}
```

and the assembly:

```text
playground::add_into:
	mov	eax, dword ptr [rsi]
	add	eax, eax
	add	dword ptr [rdi], eax
	ret
```

`*src` is loaded **once**, doubled (`shl` by 1, `add eax, eax`), and added to `*dst` in one store. Without `noalias`,
the first store to `*dst` could have changed `*src` (they might point to the same `i32`), and LLVM would have to
reload. The debug IR of the same function declares its parameters as just `ptr align 4 %dst, ptr align 4 %src`: no
`noalias`, no `readonly`, no `nonnull`. [RUSTC, observed on 1.98.1] Attributes that only matter to an optimizer
aren't emitted when the optimizer isn't running. The chain is: borrow checker proves it (18.5) → ABI computation
records it → LLVM IR carries it → LLVM exploits it. Chapter 3.3's `add_twice` showed the last step. Now you've seen
all four.

**Symbol names, from the compiler** (listing `ch06-03-symbol-names.rs`, nightly, verified):

```text
error: symbol-name(_RNvCsafzbsTfqYtn_10playground9fee_cents)
  = note: demangling(playground[776698e31d5be5cf]::fee_cents)
  = note: demangling-alt(playground::fee_cents)

error: symbol-name(_RNvMCsafzbsTfqYtn_10playgroundINtB2_7WrapperpE3getB2_)
  = note: demangling(<playground[776698e31d5be5cf]::Wrapper<_>>::get)
  = note: demangling-alt(<playground::Wrapper<_>>::get)
```

(Spans trimmed.) The `symbol_name` query produces the v0 mangling you met in Chapter 7.1. Two details:

- The bracketed hash in `playground[776698e31d5be5cf]` is the **crate disambiguator**, derived from the crate's name,
  version, and metadata. It's why two semver-incompatible versions of one crate can be linked into the same binary
  without symbol clashes (Chapter 2.1).
- The generic method's symbol has a `p` where the type argument goes (`Wrapper<_>`): a placeholder, because an
  uninstantiated generic has no code. Each instance the collector creates (`Wrapper<u64>::get`, …) gets its own
  concrete symbol.

**Vtables and layouts, from the compiler** (listing `ch06-04-vtable-layout.rs`, nightly, verified):

```text
error: vtable entries: [
           MetadataDropInPlace,
           MetadataSize,
           MetadataAlign,
           Method(<Circle as Shape>::area),
           Method(<Circle as Shape>::name),
       ]
  --> src/main.rs:19:1
   |
19 | impl Shape for Circle {
```

That's the `vtable_entries` query behind Chapter 6.4's IR constant: drop glue, size, alignment, then one slot per
method, **including** `name`, which `Circle` didn't override (the default method's instance for `Circle` goes in the
slot). And the layout of a small enum, the answer to Chapter 5.2's `size_of` question straight from `layout_of`:

```rust,ignore
#[rustc_dump_layout(debug)]
enum Reply {
    Ok(u8),
    Value(u32),
}
```

```text
error: layout_of(Reply) = Layout {
           size: Size(8 bytes),
           align: AbiAlign { abi: Align(4 bytes) },
           …
           variants: Multiple {
               tag: u8 is 0..=1,
               tag_encoding: Direct,
               tag_field: 0,
               variants: [
                   VariantLayout { size: Size(2 bytes), … field_offsets: [ Size(1 bytes) ], … },
                   VariantLayout { size: Size(8 bytes), … field_offsets: [ Size(4 bytes) ], … },
               ],
           },
```

(Condensed.) An 8-byte enum with a one-byte tag at offset 0 whose valid values are `0..=1`; `Ok`'s `u8` sits at offset
1, `Value`'s `u32` at offset 4 (aligned). [RUSTC] Every one of those choices is the default representation's, not a
promise: Chapter 5.2's rule stands (`repr(C)` marks a boundary).

**Two functions, one body.** Listing `ch06-05-merged-fns.rs` has two instances of one generic function whose type
arguments happen to produce identical code (both payment methods charge 290 basis points today):

```rust,ignore
impl Method for Card {
    const BPS: u64 = 290;
}
impl Method for Wallet {
    const BPS: u64 = 290; // same rate as cards today, so the two instances compile to identical code
}

#[inline(never)]
pub fn fee<M: Method>(amount: u64) -> u64 {
    amount * M::BPS / 10_000
}
```

The release assembly:

```text
playground::fee::<playground::Card>:
	imul	rax, rdi, 290
	movabs	rcx, 3777893186295716171
	mul	rcx
	mov	rax, rdx
	shr	rax, 11
	ret

playground::fee_wallet:
	jmp	qword ptr [rip + playground::fee::<playground::Card>@GOTPCREL]
playground::fee::<playground::Wallet> = playground::fee::<playground::Card>
playground::fee_card = playground::fee_wallet
```

Two monomorphized instances became **one body and an alias** (`fee::<Wallet> = fee::<Card>`), and the two non-generic
wrappers merged the same way. (The `imul`/`movabs`/`mul`/`shr` sequence is `* 290 / 10_000` with the division turned
into a multiply by a magic constant, Chapter 7.2's strength reduction.) The debug assembly has all four functions
separately.

The consequence is observable (listing `ch06-06-fn-addr-eq.rs`, verified in both profiles):

```rust
pub trait Method {
    const BPS: u64;
}
pub struct Card;
pub struct Wallet;
impl Method for Card {
    const BPS: u64 = 290;
}
impl Method for Wallet {
    const BPS: u64 = 290;
}

#[inline(never)]
pub fn fee<M: Method>(amount: u64) -> u64 {
    amount * M::BPS / 10_000
}

fn main() {
    let card: fn(u64) -> u64 = fee::<Card>;
    let wallet: fn(u64) -> u64 = fee::<Wallet>;
    println!("fee(10_000): card={} wallet={}", card(10_000), wallet(10_000));
    println!("same address? {}", std::ptr::fn_addr_eq(card, wallet));
}
```

Debug:

```text
fee(10_000): card=290 wallet=290
same address? false
```

Release:

```text
fee(10_000): card=290 wallet=290
same address? true
```

Same source, same compiler, different answer. [LANG] Rust doesn't promise that distinct functions have distinct
addresses, or that one function has one address (`std::ptr::fn_addr_eq`'s documentation says both). The IR says so
too: the `define` line for `add_into` above carries `unnamed_addr`, rustc's statement to LLVM that the function's
address isn't significant, which is what permits a merge.

---

## Pass 2 · Systems level — *Collector, CGUs, backends*

### 4. Under the hood

**The collector.** [RUSTC] (rustc-dev-guide, *Monomorphization*.) The query `collect_and_partition_mono_items` starts
from the roots and walks optimized MIR:

- **Roots.** For a binary: `main` (really the `lang_start` wrapper you can see in any binary's asm) and anything with
  `#[no_mangle]` or `#[used]`. For a library: every non-generic item that another crate could call, since the library
  can't know which ones will be used.
- **Edges.** A call to `fee::<Card>` adds that instance. A cast from `&Circle` to `&dyn Shape` adds the vtable and
  every method in it (the collector can't know which will be called through the pointer). A `drop` of a `String` adds
  `drop_glue::<String>` (you'll see it in 18.7's IR as `core::ptr::drop_glue::<alloc::string::String>`). Reading a
  generic `const` evaluates it for the concrete type: that's where Chapter 7.1's post-monomorphization error came from.
- **Instance kinds.** Most instances are ordinary items with generic arguments, but the collector also creates shims:
  drop glue, vtable shims (the capstone's release asm contains one named `call_once::{shim:vtable#0}`), function-pointer
  shims for calling a closure through `fn()`, and so on.

**Where instances are compiled.** [RUSTC] A generic function's MIR comes from the defining crate's metadata (18.4), and
its instances are compiled in the crate that uses them (7.1). Two refinements:

- **Shared generics.** In unoptimized builds, rustc by default reuses an instance an upstream crate already compiled
  (`fee::<u64>` compiled in a dependency) instead of compiling its own copy (7.3's "shared generics in unoptimized
  builds").
- **`#[inline]` functions** get a local copy wherever they're used, which is how cross-crate inlining works (2.7,
  7.3). In Chapter 18.7's debug IR, `String::len` and `<String as Deref>::deref` (both non-generic `#[inline]` std
  functions) appear as function definitions in *our* crate.

**Partitioning.** [RUSTC] Mono items are split into CGUs, each an independent LLVM module that can be optimized in
parallel and cached by incremental compilation (18.1). Generic instances are placed near their users; `#[inline]`
items may be duplicated into several CGUs so each can inline them. More CGUs mean more parallelism and less
cross-function optimization, which is why release profiles that care about peak performance set `codegen-units = 1` or
use LTO (Chapter 2.2's gateway profile).

**Codegen: the backend-independent part.** [RUSTC] `rustc_codegen_ssa` walks each instance's MIR, using two queries for
every decision:

- `layout_of(T)`, whose `backend_repr` decides how a value is held: `Scalar` (one register, like a reference, as the
  ABI dump showed), `ScalarPair` (two registers, like `&str`'s pointer and length), or `Memory` (a stack slot, like a
  `String`, which Chapter 18.7 shows as `alloca [24 x i8]`).
- `fn_abi_of_instance`, whose **pass mode** per argument you saw above: `Ignore` (zero-sized), `Direct` (one register),
  `Pair` (two registers: 18.7's debug IR calls `log(ptr %_3.0, i64 %_3.1)` for a `&str`), `Cast` (packed into integer
  registers), or `Indirect` (by hidden pointer: a `String` return becomes `sret([24 x i8])`).

It then emits backend IR through a trait interface that each backend implements.

**The LLVM backend.** [RUSTC] `rustc_codegen_llvm` builds LLVM IR through LLVM's C API (plus a thin C++ wrapper for
what the C API lacks), runs LLVM's optimization pipeline on each CGU, and asks LLVM to emit an object file per CGU.
Function merging is one of those LLVM passes (LLVM's `MergeFunctions`, observed in release builds here).
Link-time optimization (thin or fat, Chapter 2.2) lets LLVM see across CGUs and crates afterward.

**Other backends.** [VERSION] (Not verified here: the Playground uses LLVM.)

- **Cranelift** (`rustc_codegen_cranelift`), a code generator written in Rust and designed for fast compilation rather
  than peak code quality, is distributed for some targets as a nightly rustup component
  (`rustup component add rustc-codegen-cranelift-preview --toolchain nightly`) and selected with
  `-Zcodegen-backend=cranelift` or Cargo's unstable `codegen-backend` profile option. Its intended use is faster debug
  builds.
- **GCC** (`rustc_codegen_gcc`, via libgccjit) targets platforms GCC supports and LLVM doesn't.

Both plug into the same `rustc_codegen_ssa` layer, which is why every stage before this one is shared. Check each
project's current status before adopting either.

> **What actually happens?** Why does the collector add *every* method of a vtable, even ones never called through a
> `dyn`? Because the vtable is data: a table of function pointers that must all point to real functions. Which slot a
> call reads is decided at run time, so the compiler must assume any of them can be called. That's the code-size cost of
> `dyn Trait` that Chapter 6.5's decision matrix listed, seen from inside the compiler.

### 5. Memory

- **Code size is instances times their size.** [RUSTC] The collector decides the count, and Chapter 7.3 measured how
  it grows. Function merging claws some back when instances are byte-for-byte identical (merged helpers in 7.3,
  `fee::<Card>` here).
- **Vtables are read-only data**, one per (type, trait) pair used, possibly duplicated across CGUs or crates (6.4, and
  why `ptr::addr_eq` exists). Each costs `3 + methods` pointer-sized words.
- **Layout decides memory and registers.** The `Reply` dump shows 8 bytes where a C-like declaration with a 4-byte tag
  would also take 8, but with different offsets. That difference is invisible inside Rust and decisive at a boundary
  (§13).
- **Symbols and debug info are file size, not memory.** Long v0 symbols for deeply generic types, and DWARF for every
  instance, can make binaries large. Cargo strips debug info from release builds by default since 1.77 (Chapter 2.2);
  `strip = true` removes the symbol table too, and `split-debuginfo` keeps debug info beside the binary instead of in
  it. Part XIX covers the binary itself.

### 6. CPU / OS

**The build:** codegen is the parallel part of a crate's compilation. LLVM optimizes CGUs on several threads, which is
why Chapter 18.1's `--timings` bars show a long single-threaded front end followed by a burst of parallel work.

**The running program:** the ABI decisions are the calling convention your CPU executes. On x86-64 Linux, rustc's
`Rust` convention currently places the first integer and pointer arguments in the System V registers
(`rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`), returns large values through a hidden pointer in `rdi`, and returns small
pairs in `rax:rdx` (Chapter 2.4's `(u64, u64)`). The capstone's `post` shows all of it: `rdi` is the hidden `Result`
pointer, then `rsi`, `rdx`, `rcx` are its three arguments. [RUSTC] Unlike `extern "C"` (Part XVI), none of that is a
promise.

**Profiling a release binary:** merged functions share an address, so a sampling profiler attributes their time to one
symbol. §9 shows what that looks like in practice.

---

## Pass 3 · Architect level — *Codegen settings as architecture*

### 7. Trade-offs

| Setting | Build time | Run time | Other effects |
|---|---|---|---|
| LLVM, `opt-level = 0` (dev) | Fastest LLVM builds | Slow code; no `noalias`-style attributes | Best debugging; shared generics on by default |
| Cranelift backend (nightly) | Faster than LLVM at opt-level 0 (the project's stated goal) | Slower code than LLVM | Nightly only, target-dependent; check status |
| `codegen-units = 16` / `256` (defaults) | Parallel, incremental-friendly | Some lost cross-function optimization | 256 for incremental dev builds |
| `codegen-units = 1` | Serial codegen | Best intra-crate optimization | Chapter 2.2's gateway release profile |
| Thin LTO | Moderate cost | Cross-crate inlining | Good default for services |
| Fat LTO | Slowest | Sometimes slightly better than thin | Worth measuring, not assuming |
| `debug = "line-tables-only"` | Less DWARF to emit | None | Symbolized backtraces without full debug info |

> **Why not ship Rust libraries as compiled generic code, like Java ships bytecode?** Because a generic function has no
> code until it's instantiated, and it can't be instantiated without knowing the types, which only the user of the
> library knows. That's why `.rlib` files carry MIR and why the collector runs in the crate that builds the binary. The
> alternative (one shared body with type information passed at run time, like Java erasure or Swift's witness tables)
> trades the instance count for indirection on every generic call (Chapter 7.2).

### 8. Java comparison

The JVM does its "monomorphization" at run time. HotSpot's C2 inlines through call sites it has observed to be
monomorphic, guarded by type checks and deoptimization (Chapter 7.2's type profiles). Code lives in the code cache,
not in a symbol table, and "linking" is class loading.

The closer comparison is **GraalVM Native Image**, Java's ahead-of-time compiler. It performs a **closed-world
reachability analysis** from `main`: starting at the entry point, it discovers which classes, methods, and fields can be
reached and compiles only those. That's the same idea as rustc's collector.

| | rustc | GraalVM Native Image |
|---|---|---|
| Starting point | Roots: `main`, public items, `#[no_mangle]` | `main` plus configured entry points |
| What makes it hard | Nothing dynamic: generics resolved statically | Reflection, dynamic proxies, class loading (need configuration) |
| Precision | Exact for generics; conservative for vtables | Points-to analysis over a dynamic language |
| Result | One instance per (function, type arguments) | One compiled method per reachable method |

> **Analogy limit.** Native Image must *approximate* a dynamic language: anything reached only by reflection must be
> declared, or it's missing at run time. rustc's collector has no such gap, because Rust has no reflection and every
> generic instantiation is visible in MIR. The one place rustc is conservative is the same place Native Image is,
> dynamic dispatch: every method in a vtable is compiled because any may be called.

### 9. Production scenario

**The profile that blamed cards for wallet traffic.** During a load test of payments-core with *only* wallet payments,
a flame graph of the release binary showed `fee::<Card>` as a hot frame and no `fee::<Wallet>` at all. The first
reading was a routing bug: "wallet payments are being charged card fees." The fee amounts in the test results were
correct, though, and both methods charged 290 basis points at the time.

The engineer asked the compiler instead of guessing. The release assembly (as in listing `ch06-05`) showed
`fee::<Wallet> = fee::<Card>`: one merged body, profiled under the name LLVM kept. The team added a note to their
profiling runbook:

- In release profiles, a symbol stands for **all functions merged into it**. Before concluding that code path A runs
  instead of B, check the release asm or the symbol table for aliases.
- For investigations that need exact attribution, profile a build with merging disabled. (Not verified here: rustc has
  an unstable `-Z merge-functions=disabled` flag; check your toolchain.)
- Never use a function's address to identify anything. That rule turned out to matter more than the profile (§10).

### 10. Failure scenario

**The chargeback handler that disappeared in release.** Meridian's webhook intake receives card-network dispute
notifications and routes each by event kind to a handler. While the refunds and disputes teams rebuilt their flows,
both handlers were placeholders that just acknowledged the event, with identical bodies. The registry had a guard
against registering the same handler twice, keyed on the handler's function pointer (listing
`ch06-07-handler-registry.rs`, verified):

```rust,ignore
fn register(&mut self, kind: &'static str, h: Handler) {
    // BUG: "don't register the same handler twice", keyed on the function's address.
    if self.handlers.iter().any(|&(_, existing)| std::ptr::fn_addr_eq(existing, h)) {
        return;
    }
    self.handlers.push((kind, h));
}
```

The first version compared with `existing == h`, which draws a warning on 1.98.1:

```text
warning: function pointer comparisons do not produce meaningful results since their addresses are not guaranteed to be unique
  = note: the address of the same function can vary between different codegen units
  = note: furthermore, different functions could have the same address after being merged together
  = note: `#[warn(unpredictable_function_pointer_comparisons)]` on by default
help: refactor your code, or use `std::ptr::fn_addr_eq` to suppress the lint
```

(Trimmed; from listing `ch06-09-fn-ptr-eq-lint.rs`, two different functions with identical bodies compared with `==`,
which prints `true` in release.) The developer took the second half of the help ("use `std::ptr::fn_addr_eq` to suppress the lint") and
missed the first ("refactor your code"). The tests ran under `cargo test`, which builds in debug:

```text
registered: ["refund", "chargeback"]
chargeback -> Some(Ack)
```

Production runs release:

```text
registered: ["refund"]
chargeback -> None
```

`ack_refund` and `ack_chargeback` were merged into one function, so the chargeback registration looked like a
duplicate and was silently skipped. Chargeback notifications fell through to the "unknown event" path, which logged at
debug level and returned 200. The card network stopped retrying, and the disputes were discovered only when the
network's deadline reports arrived: several responses were late.

The fix (listing `ch06-08-handler-registry-fixed.rs`, verified in both profiles) uses the identity the code actually
means, the event kind, and makes a duplicate an error:

```text
Err("handler for \"refund\" already registered")
registered: ["refund", "chargeback"]
chargeback -> Some(Ack)
```

The review rules that followed:

1. **Function and vtable addresses are not identities.** Key registries on names, enums, or `TypeId`s.
2. **A lint's "to suppress" suggestion is a last resort**, and a PR that suppresses a correctness lint needs a comment
   saying why the warning doesn't apply.
3. **CI runs the test suite in release as well** (`cargo test --release` in the nightly job), because optimization can
   change observable behavior wherever the language leaves it unspecified.
4. Unknown event kinds are an error with an alert, not a 200 (Chapter 8.4's taxonomy).

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVIII).*

1. What are the collector's roots in a binary and in a library, and what edges does it follow? Why does a `dyn` cast
   pull in every method of the trait?
2. Trace `noalias` from the borrow checker to the machine code. Which component makes each decision?
3. Why does the debug build's IR lack `noalias` and `readonly`, and what does that tell you about benchmarking debug
   builds?
4. What does the bracketed hash in a demangled v0 symbol represent, and what problem does it solve?
5. What is a codegen unit? What do more CGUs buy, and what do they cost?
6. Explain function merging, the role of `unnamed_addr`, and why `fn_addr_eq` can return different answers in debug and
   release.
7. What do the Cranelift and GCC backends share with the LLVM backend, and what would you use each for?
8. Compare rustc's monomorphization collector with GraalVM Native Image's reachability analysis.

### 12. Exercises

- **Beginner.** Use `#[rustc_dump_vtable]` on nightly for a trait with a supertrait (`trait Shape: Debug`). Where do the
  supertrait's methods go? Compare with Chapter 6.4's layout.
- **Intermediate.** Use `#[rustc_abi(debug)]` in release on functions taking `&[u8]`, `String` (by value), `(u64,
  u64)`, `Option<&u8>`, and `[u64; 8]`. Record each argument's pass mode and attributes, and relate them to Chapter
  2.4's calling-convention facts.
- **Advanced.** Change `Wallet::BPS` in listing `ch06-06-fn-addr-eq.rs` to 250 and predict the release output. Then find
  two *different* generic instances in a real program (hint: identically laid-out types, as in Chapter 7.3) that merge,
  and show it in the release asm.
- **Systems.** On a local toolchain, build a crate with LLVM and with Cranelift (nightly component) in debug. Compare
  wall-clock build time and the run time of a CPU-bound test (not verifiable on the Playground).
- **Architecture.** Your gateway's release build takes 14 minutes with `codegen-units = 1` and fat LTO. Propose
  experiments to find a profile that keeps at least 97% of throughput with a much shorter build, what you'd measure, and
  how you'd make the result stick in CI.

### 13. Debugging exercise

The market-data team streams `Reply` values to a C++ consumer by copying their bytes into a shared-memory ring. The C++
side declares:

```cpp
struct Reply {           // written from the Rust enum "by eye"
    uint32_t tag;        // 0 = Ok, 1 = Value
    union { uint8_t ok; uint32_t value; };
};
```

`Value` replies decode correctly about half the time, and `Ok` replies sometimes show a tag of 0x2A00 or other
nonsense. Using the `layout_of(Reply)` dump in §3:

1. Where do the tag and each payload live in the Rust layout, and how many bytes of the tag field does the C++ reader
   interpret? Explain both symptoms.
2. Why can't you "just match" the C++ struct to the Rust layout you see today?
3. Fix it two ways: with a `repr` attribute (say which one, and what `layout_of` would then print), and with an
   explicit encoding function. Which does Chapter 5.2's rule recommend, and when is the other justified?

### 14. Design exercise

**A codegen policy for Meridian's Rust estate.** Design the build profiles for three kinds of artifacts: developer
builds of the gateway workspace, CI test builds for pull requests, and release builds of the gateway and the fraud
feature library (which is loaded into the JVM via FFM). Decide backend (LLVM or Cranelift, and whether you'd accept a
nightly dependency for it), opt-level, codegen units, LTO, debug info, panic strategy (Chapter 8.3), symbol mangling and
demangling support in your profilers, and whether tests run in release. For each choice, state what you gain, what it
costs, and which observable behaviors (function addresses, overflow checks, `noalias`-dependent performance) differ
between the profiles, and how you'll keep those differences from hiding bugs.
