# Chapter 15.1 — What `unsafe` Means: Soundness and Invariants

> **Where this sits:** Part XV · Unsafe Rust · chapter 1 of 6
> **Prerequisites:** Chapter 1.3 (the guarantees table and the trusted computing base), Chapter 2.7 (privacy),
> Chapter 5.2 (validity, niches, the invalid-enum-tag demo), Chapter 4.6 (Miri's first appearance).
> **After this chapter you can:** name exactly what `unsafe` unlocks and what it doesn't; tell a *validity* invariant
> from a *safety* invariant and predict when breaking each becomes undefined behavior; define soundness precisely and
> decide whether an API is sound; write `# Safety` and `// SAFETY:` contracts that a reviewer can check; and explain why
> the unit of trust is the module, not the `unsafe` block.

---

## Pass 1 · User level — *What the keyword promises, and to whom*

### 1. Problem

Chapter 1.3's table said safe Rust has no use-after-free, no data races, no invalid values. Then it added the fine
print: *provided that* everything in the trusted computing base is correct, and most of that base is `unsafe` Rust.
`Vec`, `String`, `HashMap`, `Arc`, `Mutex`, the allocator glue, and every syscall wrapper are written with operations the
compiler can't check. The guarantee you've relied on for fourteen Parts is conditional.

So Part XV doesn't ask "how dangerous is `unsafe`?" It asks the two questions the brief puts at the top of this Part:

1. **What exactly does safe Rust guarantee?** Precisely enough that you could check whether a piece of code preserves it.
2. **What must `unsafe` code maintain** so that the guarantee stays true for *every* safe caller, including callers
   written years later by someone who never read your code?

A Java engineer's reflex is to file `unsafe` next to `sun.misc.Unsafe`: an API you don't touch. That's the wrong
drawer. Rust's `unsafe` is a *language-level proof obligation* with a documented contract. The standard library is
built from it, and writing it well is a normal senior skill, like writing a lock-free queue or a JNI binding is in Java.
It's just rarer, and much more reviewable.

### 2. Mental model

**`unsafe` turns nothing off. It unlocks five operations** [LANG]:

| Superpower | Example | Why the compiler can't check it |
|---|---|---|
| Dereference a raw pointer | `*p`, `p.read()` (the latter is an unsafe fn) | A raw pointer has no lifetime, may be null, dangling, or misaligned |
| Call an `unsafe fn` (including foreign functions and intrinsics) | `v.get_unchecked(i)`, `libc::strlen(p)` | The callee has a precondition only the caller can establish |
| Access a `static mut` | `COUNTER += 1` | Any thread could be touching it |
| Implement an `unsafe trait` | `unsafe impl Send for MyVec<T>` | The trait's contract is semantic (e.g., "safe to move to another thread") |
| Read a `union` field | `u.as_float` | Which field is valid depends on what was written last |

Everything else still applies *inside* an `unsafe` block: type checking, borrow checking of references, privacy.
`unsafe { v[10] }` still bounds-checks. [VERSION] Edition 2024 adds two *declarations* that must say `unsafe` because
they are promises too: `unsafe extern` blocks (a foreign signature you declared wrong is UB at every call) and unsafe
attributes such as `#[unsafe(no_mangle)]` (an unmangled symbol can collide with another one). Both syntaxes were
stabilized in Rust 1.82 and are required in edition 2024.

**The keyword is used in two directions.** One *defines* an obligation, the other *discharges* it:

```text
 DEFINES an obligation (documented in `# Safety`)          DISCHARGES it (documented in `// SAFETY:`)
 ─────────────────────────────────────────────            ─────────────────────────────────────────────
 unsafe fn get_unchecked(&self, i: usize)   ── caller ──►  // SAFETY: i < self.len(), checked two lines up
                                                            unsafe { v.get_unchecked(i) }

 unsafe trait Send                          ── implementer ► // SAFETY: MyVec<T> owns its T's; moving it moves them
                                                            unsafe impl<T: Send> Send for MyVec<T> {}

 unsafe extern "C" { fn strlen(..) }        (2024)          the declaration itself: "this signature is exactly right"
```

**Soundness** is the property that ties the two together. A safe function, type, or module is **sound** if *no safe
program*, calling it in any way the type system allows, can cause undefined behavior. If such a program exists, the
API is **unsound**, even if nobody has written that program yet. "Safe Rust can't cause UB" is really:

```text
   sound unsafe code  +  sound APIs around it   ⇒   no safe program can cause UB
```

**Two kinds of invariant.** Both are "things that must be true," but they differ in who defines them and in what
happens when you break them:

| | **Validity invariant** | **Safety invariant** |
|---|---|---|
| Defined by | The language [LANG] | The type's author [LIB] |
| Examples | `bool` is 0 or 1; `&T` is non-null, aligned, dereferenceable; `char` is a Unicode scalar value; an enum has a valid discriminant; an integer is initialized | `Vec`: `len <= cap` and `[0, len)` initialized; `str`: valid UTF-8; `Rc`: the count equals the number of live handles |
| Must hold | Every time a value of the type is produced or copied, even inside private code | Whenever *safe code* can observe the value; the owning module may break it temporarily |
| Breaking it is | **Immediate** undefined behavior | Not UB *yet*: UB happens when code that relies on it runs ("library UB") |
| Miri reports it | At the moment the invalid value is created | Only when it turns into language UB somewhere downstream |

Chapter 5.2 showed one validity violation: a `repr(u8)` enum transmuted from a byte that isn't a discriminant. This
chapter completes the family, then shows the safety kind, which is subtler and far more common in real bugs.

**The unit of trust is the module.** A safety invariant lives in private fields (Chapter 2.7), so *every* function that
can touch those fields, safe or not, is part of the proof. That's the lesson of §10.

### 3. Rust code

**An invalid value, and an optimizer that takes you at your word** (listing `ch01-01-bool-two.rs`, verified):

```rust
// A `bool` holding the byte 2 is an INVALID VALUE: undefined behavior the moment it is produced.
// Natively, the optimized `if b { 1 } else { 0 }` returns 2: a value neither branch can return.
use std::hint::black_box;

#[inline(never)]
fn to_u32(b: bool) -> u32 {
    if b { 1 } else { 0 }
}

fn main() {
    let raw: u8 = black_box(2);
    let b: bool = unsafe { std::mem::transmute::<u8, bool>(raw) };
    println!("to_u32(b) = {}", to_u32(black_box(b)));
}
```

Release build, run natively on the Playground:

```text
to_u32(b) = 2
```

A function whose two branches return 1 and 0 returned 2. It isn't a miscompilation. Because a `bool` is 0 or 1
[LANG], `if b { 1 } else { 0 }` is the identity on the byte, and the compiler generated exactly that. Under **Miri**
(the Playground's nightly toolchain), the same program is stopped where the invalid value is *created*, not where it's
used:

```text
error: Undefined Behavior: constructing invalid value of type bool: encountered 0x02, but expected a boolean
  --> src/main.rs:14:28
   |
14 |     let b: bool = unsafe { std::mem::transmute::<u8, bool>(raw) };
   |                            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Undefined Behavior occurred here
```

**The rest of the validity family**, each a separate listing, each stopped by Miri (messages verbatim, trimmed):

| Listing | The invalid value | Miri says |
|---|---|---|
| `ch01-03-invalid-char.rs` | `transmute::<u32, char>(0xD800)` | ``constructing invalid value of type char: encountered 0x0000d800, but expected a valid unicode scalar value (in `0..=0x10FFFF` but not in `0xD800..=0xDFFF`)`` |
| `ch01-04-null-reference.rs` | `&*ptr::null::<u64>()` (never read) | `constructing invalid value of type &u64: encountered a null reference` |
| `ch01-05-uninit-integer.rs` | `MaybeUninit::<u32>::uninit().assume_init()` | `constructing invalid value of type u32: encountered uninitialized memory, but expected an integer` |
| `ch01-06-unaligned.rs` | `*p` where `p: *const u32` is 4-aligned + 1 | `accessing memory based on pointer with alignment 1, but alignment 4 is required` |

Two of these deserve a second look. The null reference is UB **even though it's never dereferenced**: validity is about
the value existing, not about using it. And an integer made from uninitialized memory is invalid even though "every bit
pattern is a valid `u32`." Uninitialized memory isn't *some* bit pattern. To the abstract machine it's *no* bit
pattern, and the compiler is allowed to exploit that (§6).

The misaligned read is the one x86 engineers argue with, so listing `ch01-06-unaligned.rs` carries three checks:

```rust
// Dereferencing a misaligned *const u32 is UB in Rust even though x86-64 loads it happily.
fn main() {
    let words = std::hint::black_box([0x0403_0201u32, 0x0807_0605, 0, 0]); // 4-aligned storage
    let p = (words.as_ptr() as *const u8).wrapping_add(1) as *const u32; // aligned + 1
    let v = unsafe { *p };
    println!("{v:#010x}");
}
```

```text
release, native:   0x05040302
debug, native:     thread 'main' (13) panicked at src/main.rs:6:22:
                   misaligned pointer dereference: address must be a multiple of 0x4 but is 0x7ffc4279327d
Miri:              error: Undefined Behavior: accessing memory based on pointer with alignment 1, but alignment 4 is required
```

The release build "works," because x86-64 performs unaligned loads in hardware. The debug build aborts, because
[VERSION] since Rust 1.70 debug builds insert an alignment check before raw-pointer dereferences. Miri reports UB. §6
explains why the language keeps the rule even on hardware that doesn't need it.

**A safety invariant: `str` is UTF-8** (listing `ch01-07-str-invariant.rs`, verified):

```rust
// A non-UTF-8 `str` is a broken LIBRARY (safety) invariant: not UB the instant it exists,
// but std's code is written assuming UTF-8, and trusting it turns into language UB later.
use std::hint::black_box;

fn main() {
    let bytes: Vec<u8> = black_box(vec![b'o', b'k', 0xFF, 0xFE]);
    let s: &str = unsafe { std::str::from_utf8_unchecked(&bytes) };
    println!("len={} chars={}", s.len(), s.chars().count());
    println!("{:?}", s.chars().collect::<Vec<_>>());
}
```

Three builds, three different failures, and **none of them mentions UTF-8**:

```text
release, native:  len=4 chars=4
                  thread 'main' (14) panicked at .../alloc/src/raw_vec/mod.rs:28:5:
                  capacity overflow

debug, native:    len=4 chars=4
                  thread 'main' (12) panicked at .../core/src/str/validations.rs:55:40:
                  unsafe precondition(s) violated: hint::unreachable_unchecked must never be reached

                  This indicates a bug in the program. This Undefined Behavior check is optional, and cannot be
                  relied on for safety.

Miri:             len=4 chars=4
                  error: Undefined Behavior: entering unreachable code
                    --> .../core/src/str/validations.rs:55:27
                     |
                  55 |         let z = unsafe { *bytes.next().unwrap_unchecked() };
                     |                           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Undefined Behavior occurred here
```

The first line printed in every build: constructing the bad `str` and even counting its "chars" didn't trigger anything.
That's the safety-invariant half of the table. [LIB] The std documentation for `str` says as much: a non-UTF-8 `str` is
not immediately UB, but any function may assume UTF-8, so UB can follow later. It followed in the UTF-8 decoder, which
reads `0xFF` as the start of a four-byte sequence and calls `unwrap_unchecked()` on a continuation byte that doesn't
exist. That's the moment library UB became language UB, and Miri points at exactly that line in `core`.

When the invalid bytes are a *literal*, you don't even get that far. A deny-by-default lint catches it at compile time
(listing `ch01-08-str-literal-lint.rs`, verified):

```text
error: calls to `std::str::from_utf8_unchecked` with an invalid literal are undefined behavior
  = note: `#[deny(invalid_from_utf8_unchecked)]` on by default
```

**Defining and discharging an obligation** (listing `ch01-11-unsafe-op-in-unsafe-fn.rs`, verified):

```rust
/// # Safety
/// `p` must be non-null, aligned, and point to an initialized `u64` that no one else is writing.
unsafe fn read_counter(p: *const u64) -> u64 {
    *p // edition 2024: warning, the body of an `unsafe fn` is no longer an implicit unsafe block
}

fn main() {
    let x = 42u64;
    // SAFETY: `&x` is non-null, aligned, initialized, and not written concurrently.
    let v = unsafe { read_counter(&x) };
    println!("{v}");
}
```

Under edition 2024 this compiles with a warning that states the design principle better than most prose:

```text
warning[E0133]: dereference of raw pointer is unsafe and requires unsafe block
 --> src/main.rs:6:5
  |
6 |     *p // edition 2024: warning, the body of an `unsafe fn` is no longer an implicit unsafe block
  |     ^^ dereference of raw pointer
  |
note: an unsafe function restricts its caller, but its body is safe by default
  = note: `#[warn(unsafe_op_in_unsafe_fn)]` (part of `#[warn(rust_2024_compatibility)]`) on by default
```

*An unsafe function restricts its caller, but its body is safe by default.* [VERSION] Under edition 2021 the same file
compiles silently (the listing checks both), because an `unsafe fn` body used to be one big implicit unsafe block. The
2024 rule makes you write `unsafe { *p }` with its own `// SAFETY:` comment, so the two directions stay separate even
inside one function.

> **Safety invariant (`read_counter`).** For the duration of the call, `p` is non-null, aligned for `u64`, points to an
> initialized `u64` inside a live allocation, and no other thread writes that `u64`.

**Edition 2024 declarations** (listings `ch01-12` to `ch01-14`, verified). A plain `extern "C" { ... }` block is now a
hard error (`error: extern blocks must be unsafe`), and so is a bare `#[no_mangle]`
(`error: unsafe attribute used without unsafe`, with the suggestion `#[unsafe(no_mangle)]`). The new form also lets you
mark foreign items that have no preconditions as `safe`:

```rust
// Edition 2024: `unsafe extern` blocks, `safe` items, and `#[unsafe(...)]` attributes.
use std::ffi::{c_char, c_int};

unsafe extern "C" {
    // Declaring a signature is itself a promise (a wrong signature is UB at every call),
    // hence `unsafe extern`. Items with no preconditions may be declared `safe`:
    pub safe fn getpid() -> c_int;
    // strlen has preconditions (a valid NUL-terminated string), so it stays unsafe to call:
    pub fn strlen(s: *const c_char) -> usize;
}

// Exporting an unmangled symbol can collide with another symbol of the same name: an unsafe attribute.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_version() -> u32 {
    3
}

fn main() {
    println!("getpid() > 0: {}", getpid() > 0); // no unsafe block needed
    let s = c"meridian";
    // SAFETY: `s` is a valid NUL-terminated C string that outlives the call.
    let n = unsafe { strlen(s.as_ptr()) };
    println!("strlen = {n}, version = {}", meridian_version());
}
```

```text
getpid() > 0: true
strlen = 8, version = 3
```

> **Safety invariant (the `unsafe extern` block).** Each declared signature matches the C definition exactly (argument
> and return types, ABI), and each item marked `safe` has no preconditions at all. `labs` would be a trap here: it looks
> harmless, but `labs(LONG_MIN)` is undefined behavior *in C*, so declaring it `safe` would make a safe Rust call able to
> trigger UB. The unsoundness would be in the declaration, not in any call site.

---

## Pass 2 · Systems level — *Why UB is not "whatever the CPU does"*

### 4. Under the hood

**UB is a contract with the optimizer.** [LANG] The Reference lists what is undefined: data races, dangling or
misaligned accesses, breaking the aliasing rules, producing invalid values, and a few more. [RUSTC] rustc turns the
corresponding guarantees into facts LLVM can optimize with. Parts III and IV showed some of them in real IR: `noalias`,
`readonly`, `nonnull`, `align 4`, `dereferenceable(4)`, `noundef` on reference parameters. Here are the facts at work,
in the release assembly of listing `ch01-02-assumptions-asm.rs` (via `tools/emit.ps1`, rustc 1.98.1):

```text
playground::bool_to_u32:        ; if b { 1 } else { 0 }
	mov	eax, edi                ; the bool's byte IS the answer: no compare, no branch
	ret

playground::is_null_ref:        ; (r as *const u64).is_null() where r: &u64
	xor	eax, eax                ; always false: references are never null
	ret

playground::is_null_raw:        ; p.is_null() where p: *const u64
	test	rdi, rdi                ; a real test: raw pointers may be null
	sete	al
	ret
```

The compiler even warns about the middle one (`warning: references are not nullable, so checking them for null will
always return false`, lint `useless_ptr_null_checks`). This is why "UB means the CPU does something weird" is the wrong
mental model. UB means **the compiler was told something false**, and code generated from false facts can do anything:
return 2 from a boolean function, delete a null check (CVE-2009-1897, Chapter 1.1), or skip a bounds check it "proved"
unnecessary.

**Why the UTF-8 program panicked with "capacity overflow."** [RUSTC] This one is inferred, not traced, and that's
the point. `unwrap_unchecked()` on a `None` is UB, so the optimized decoder assumes the continuation byte exists and
advances its slice pointer past the end of the buffer. The iterator's remaining length (`end - ptr`) then underflows to
an enormous number, `collect` asks `Vec` to reserve that much, and the allocation-size check panics. A broken invariant
surfaces wherever the false assumption happens to feed. Here it was an allocation check three abstractions away, in a
message that sends you in the wrong direction.

**The debug-build safety net.** [VERSION] Since Rust 1.78, many standard-library `unsafe fn`s check their
preconditions whenever the *calling* crate has debug assertions on: `slice::from_raw_parts` (null, alignment, size),
`ptr::copy_nonoverlapping` (overlap), `Vec::set_len` (`new_len <= capacity`), `unreachable_unchecked`, and more. The
check is decided at code generation in your crate, so it works even though std itself ships compiled in release mode.
The message's second line is honest about what it is: *"This Undefined Behavior check is optional, and cannot be relied
on for safety."* It's a smoke detector, not a sprinkler. Release builds don't have it (the release UTF-8 run sailed past
the same line), and it only covers std's own preconditions, not yours.

**Soundness holes in the compiler.** [RUSTC] Chapter 1.3 put rustc itself in the trusted base and cited
rust-lang/rust#25860, open since 2015. The reproduction usually quoted from that issue is now **rejected** by rustc
1.98.1 (listing `ch01-10-classic-25860.rs`, `error: lifetime may not live long enough`). A higher-ranked variant still
gets through. It was popularized in 2024 by a tongue-in-cheek crate called `cve-rs` (listing
`ch01-09-soundness-hole.rs`, verified):

```rust
// rust-lang/rust issue 25860 (open since 2015): SAFE code, no `unsafe` anywhere, that the compiler
// accepts and that produces a dangling reference. This higher-ranked variant compiles on rustc 1.98.1.
const STATIC_UNIT: &&() = &&();

fn translate<'a, 'b, T: ?Sized>(_witness: &'a &'b (), v: &'b T) -> &'a T {
    v
}

fn expand<'a, 'b, T: ?Sized>(x: &'a T) -> &'b T {
    let f: for<'x> fn(_, &'x T) -> &'b T = translate;
    f(STATIC_UNIT, x)
}

fn main() {
    let dangling: &'static String = {
        let s = String::from("freed");
        expand(&s)
    };
    println!("{}", dangling.len());
}
```

```text
native:  5
Miri:    error: Undefined Behavior: constructing invalid value of type &std::string::String: encountered a dangling
         reference (use-after-free)
```

`translate` is sound on its own: the type `&'a &'b ()` can only exist if `'b: 'a`, and that *implied bound* is what
makes returning `&'b T` as `&'a T` legal. The coercion to a higher-ranked function pointer loses the implied bound
without re-checking it, and the witness argument no longer constrains anything. Three honest conclusions:

- It takes deliberately contrived code, and Chapter 1.3's point stands: it has not been reported in real code.
- It's tracked as a compiler bug (`I-unsound`), not accepted behavior. The fix belongs to the ongoing trait-solver and
  implied-bounds work. [VERSION] Check the issue before quoting its status: the classic snippet's rejection shows the
  area is moving.
- It's a real reminder that "safe" means "sound *relative to* a correct compiler."

### 5. Memory

A safety invariant is a statement about memory that only the owning module can keep true. The inline header block
from §9 and §10, drawn with its invariant:

```text
 HeaderBlock<8>   (slots: [MaybeUninit<(String, String)>; 8] = 8 × 48 B, then len)
 ┌──────────┬──────────┬──────────┬──────────┬──────────┬─────┬──────────┬─────┐
 │ slot 0   │ slot 1   │ slot 2   │ slot 3   │ slot 4   │ ... │ slot 7   │ len │
 │ ("host", │ ("x-req",│ ("x-ten",│  ??????  │  ??????  │     │  ??????  │  3  │
 │  "api.") │  "r-42") │  "t-7")  │ (uninit) │ (uninit) │     │ (uninit) │     │
 └──────────┴──────────┴──────────┴──────────┴──────────┴─────┴──────────┴─────┘
 INVARIANT: len <= 8, slots[..len] initialized, slots[len..] NOT initialized
            get(i) trusts it (assume_init_ref for i < len), Drop trusts it (drops exactly slots[..len])

 after restore(5):  len = 5  ──►  get(4) treats 48 bytes of garbage as two Strings (ptr, cap, len) × 2
                                 Drop would free two garbage pointers
```

Nothing about the bytes changed. Only the *meaning* the module assigns to them did. That's why the field is private,
and why the invariant is written next to the field, where the next person to touch it will see it.

The "trusting safe code" bug from the debugging exercise (§13) has the same shape one level lower:

```text
 Vec::with_capacity(2):  heap block of 16 bytes    [ x0 | x1 ] [ allocator metadata / another object ... ]
 the lying iterator yields 4 items; the unsafe loop writes x2, x3 past the end   ──►  heap corruption
 Miri: memory access failed: attempting to access 8 bytes, but got alloc317+0x10 which is at or beyond the end
       of the allocation of size 16 bytes
```

### 6. CPU / OS

- **Alignment.** [CPU] x86-64 performs misaligned scalar loads, usually at no extra cost unless they cross a cache line
  (the release run printed `0x05040302`). The language still makes misalignment UB [LANG], for two reasons. Some targets
  fault on it or need slower instruction sequences. And even on x86 the compiler, knowing a type's alignment, may pick
  instructions that *require* it: `movaps` and other aligned SIMD loads raise a general-protection fault on a misaligned
  address. A misaligned `&T` is a promise the compiler may cash in with such an instruction long after your cast.
- **Uninitialized memory.** [OS] Fresh pages from the kernel are zero-filled, so a new allocation often *reads* as
  zeros. Recycled heap memory holds whatever the last owner left there: another request's data, a token (Chapter 5.2's
  padding leak). [RUSTC] To LLVM, reading uninitialized memory can produce a value that isn't even consistent between
  two uses. "It's just stale bytes" is not a safe assumption at either level.
- **Null.** [OS] Address 0 is unmapped on Linux (`vm.mmap_min_addr`), so a null *dereference* that survives to machine
  code faults with SIGSEGV. But the optimizer may delete a dereference, or a null check, it has proven unnecessary.
  The fault is the lucky outcome.
- **Why the debug checks are cheap enough.** The alignment check is an `and` + compare + branch per raw dereference,
  and the precondition checks are similar. That's fine for debug and test builds, and wrong for a hot release loop.
  That's why they're debug-only and explicitly "not relied on for safety."

---

## Pass 3 · Architect level — *Where does each invariant get enforced?*

### 7. Trade-offs

Every invariant has to be enforced somewhere. The choice is the design:

| Enforcement | Who is responsible | Run-time cost | When it fails | Choose when |
|---|---|---|---|---|
| **Type system** (private fields + safe constructors, newtypes, type-state) | The module author, once | None | Compile error | The default: Parts V–VI |
| **Run-time check** (`assert!`, `Result`, bounds checks) | The module author | A compare + branch, often optimized away | Panic or `Err` (safe) | The check is cheap relative to the work, or the input is untrusted |
| **Debug-only check** (`debug_assert!`, std's `ub_checks`) | The module author, backed by tests | None in release | Tests catch it, or production gets UB | Never alone for a soundness condition. Fine as an *extra* net under an `unsafe fn` precondition |
| **`unsafe fn` precondition** (`# Safety` section) | Every caller, forever | None | UB | The check is expensive or impossible for the callee (e.g., "this pointer came from `Box::into_raw`"), and callers can prove it cheaply |
| **`unsafe trait`** (`Send`, `Sync`, `GlobalAlloc`, `TrustedLen` in std) | Every implementer | None | UB | Unsafe code needs to *rely* on a property of arbitrary types |

Two rules fall out of the table:

- **Unsafe code may rely only on things it controls or on `unsafe` contracts.** It may trust its own private fields
  (the module is one proof) and the promises of `unsafe fn`s and `unsafe trait`s. It may **not** trust a *safe* trait
  implemented by someone else: `Ord`, `Hash`, `Eq`, `Iterator::size_hint`, `ExactSizeIterator::len`, `Deref`. A buggy
  `Ord` may make a sort return garbage, but it must never make it write out of bounds (Chapter 9.3 and 9.4 called
  these "logic errors, not UB"). That's why std has an unstable **`unsafe trait TrustedLen`** next to the safe
  `ExactSizeIterator`: the unsafe version is the one whose answer may decide memory accesses.
- **Every `unsafe fn` is a tax on all future callers.** An `unsafe fn` is justified when its precondition is cheap for
  the *caller* to establish and expensive or impossible for the *callee* to check. Otherwise, take the check and make
  it safe.

> **Why not reach for `unsafe` for speed?** Measure first. The common case, `get_unchecked` instead of indexing, saves
> one compare and a predictable branch, *if* the optimizer hadn't already removed the check (Chapter 1.3 showed it
> removing one). Predicted from mechanism: a measurable win only in tight loops where the bounds check blocks
> vectorization. The exercise below asks you to find such a loop and measure it before and after.

> **Why not make leaking unsafe?** Chapter 1.3 told the Leakpocalypse story: `mem::forget` became safe because leaks
> are always possible anyway (reference cycles), so **no safe API may rely on a destructor running for soundness**.
> Chapter 15.4 shows the discipline this forces on unsafe code (*leak amplification* in `Drain`).

### 8. Java comparison

| Java | Rust |
|---|---|
| `sun.misc.Unsafe`: `getLong(addr)`, `putLong`, `allocateMemory`, `compareAndSwapInt`: an internal API with no language-level contract | `unsafe`: a keyword with a checked scope, plus a contract convention (`# Safety`, `// SAFETY:`) and tooling (Miri, debug precondition checks) |
| Its memory-access methods are being retired: deprecated for removal in JDK 23 (JEP 471), warned about at run time from JDK 24 (JEP 498); replacements are `VarHandle` (JDK 9) and the FFM API (JDK 22) | `unsafe` is permanent and central: std is built from it; the goal is to *encapsulate* it, not remove it |
| JNI: native code that can corrupt the heap; `-Xcheck:jni` catches some misuse | FFI calls are `unsafe`; the declaration itself is `unsafe extern` (2024) |
| No undefined behavior in the language: the JVM verifies bytecode and checks every array access and cast; even data races have defined (if weak) semantics (JLS §17, Chapter 1.1) | Undefined behavior exists and is *exploited* by the optimizer; safe Rust rules it out, `unsafe` Rust must not cause it |
| "Soundness" is the JVM's job | "Soundness" is the job of every author of an `unsafe` block and of every module around one |

> **Analogy limit.** Misusing `Unsafe` in Java typically corrupts memory and crashes the JVM, sometimes much later. But
> the JIT doesn't reason from `Unsafe` misuse the way LLVM reasons from UB. In Rust, UB is a license for the compiler to
> *rewrite your program under false assumptions*: a boolean function returning 2, a UTF-8 bug reported as "capacity
> overflow." The failure can appear in a different function, a different form, and only in release builds. That's why
> Rust's answer is prevention by proof and by tooling, not crash dumps.

### 9. Production scenario

**Meridian's gateway workspace draws its soundness boundaries on purpose.** The workspace from Chapter 2.7
(`gateway-core`, `gateway-proto`, `gateway-io`, and the `gateway` binary) has one rule written at the top of each crate
root:

```rust,ignore
// gateway-core/src/lib.rs, gateway/src/main.rs, gateway-io/src/lib.rs
#![forbid(unsafe_code)] // not "deny": `forbid` can't be re-allowed further down
```

Only `gateway-proto` may contain `unsafe`, and inside it only one module, `headers`. Upstream responses are capped at 16
headers by proxy policy, so the module parses them into an inline `HeaderBlock<16>`: a fixed-capacity array of
`MaybeUninit` slots and a length, with no heap allocation for the block itself (Chapter 15.3 explains `MaybeUninit`).
The module's rules, enforced in CI and in review:

1. **The invariant is written on the field** (`/// INVARIANT: len <= N, slots[..len] initialized, slots[len..] not`)
   and every `unsafe` block's `// SAFETY:` comment cites it. [LIB] Clippy enforces the comments
   (`clippy::undocumented_unsafe_blocks`) and `# Safety` sections on public `unsafe fn`s (`clippy::missing_safety_doc`).
2. **`#![deny(unsafe_op_in_unsafe_fn)]`** even on older editions, so every operation gets its own justification.
3. **The public API is entirely safe.** `push`, `get`, `len`, and `Drop` are the only ways in.
4. **CODEOWNERS** routes any change to `headers.rs` to the two engineers who own the proof, and the module's tests run
   under Miri nightly (Chapter 15.6).

The payoff is in the numbers a reviewer cares about: one module, about 120 lines, holds all of the workspace's
`unsafe`. The other roughly 40,000 lines of gateway code can't cause UB unless that module or the compiler is wrong.

### 10. Failure scenario

**The PR that had no `unsafe` in it.** Months later, the retry path needed to roll a header block back to a checkpoint.
A developer added a small, obviously harmless method in the same module. There was no `unsafe` keyword in the diff, so
the "unsafe changes need an owner's review" rule never fired (listing `ch01-17-module-scope-bug.rs`):

```rust,ignore
        /// Added in a later "no unsafe" PR: roll back to a checkpoint taken with `len()`.
        pub fn restore(&mut self, checkpoint: usize) {
            self.len = checkpoint; // no `unsafe` here... and the invariant is gone
        }
```

On the retry path, a pooled request context sometimes carried a checkpoint from a *different* request with more
headers. `restore(5)` on a block holding three headers made `len` claim two slots that were never written. Miri, on the
listing's `main` (three pushes, `restore(5)`, `get(4)`):

```text
error: Undefined Behavior: reading memory at alloc227[0xc8..0xd0], but memory is uninitialized at [0xc8..0xd0], and
this operation requires initialized memory
```

In production the symptom was rarer and stranger: occasional crashes deep inside the allocator when the block was
dropped (garbage "String pointers" being freed), and once a response carrying a header value from a previous request.
That last one is the dangerous kind of UB: the kind that doesn't crash.

The fix (listing `ch01-18-module-scope-fixed.rs`, verified clean under Miri) makes `restore` part of the proof. It
can only shrink, it drops what it removes, and it updates `len` *before* each drop, so a panicking destructor can't
leave a dropped slot inside `[..len]`:

```rust,ignore
        /// Roll back to a checkpoint: can only SHRINK, and drops what it removes.
        pub fn restore(&mut self, checkpoint: usize) {
            while self.len > checkpoint {
                self.len -= 1; // shrink first: if a drop panics, the slot is already outside [..len]
                // SAFETY: slot `len` was initialized and is now outside [..len]; dropped exactly once.
                unsafe { self.slots[self.len].assume_init_drop() }
            }
        }
```

```text
len=1 get(0)=Some(("host", "api.meridian.example")) get(1)=None
```

The process fix mattered more than the code fix. The ownership rule changed from "PRs containing `unsafe`" to
**"PRs touching a module that contains `unsafe`."** CODEOWNERS was already path-based, so the only real change was
moving the unsafe module into its own file, so the path identifies the proof. The lesson generalizes: **`unsafe` is a
property of modules.** Any code that can write the fields an `unsafe` block trusts is part of the unsafe code, whatever
keyword it contains.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XV).*

1. List the operations `unsafe` unlocks. Name three checks that still happen inside an `unsafe` block.
2. What's the difference between `unsafe fn` and an `unsafe { }` block? Why did edition 2024 stop treating an
   `unsafe fn` body as an unsafe block?
3. Define *soundness*. Can a function containing no `unsafe` be unsound? Can a function containing `unsafe` be sound?
   Give an example of each.
4. Validity invariant vs safety invariant: define both, give two examples of each, and say when violating each one
   becomes undefined behavior.
5. Why is a null `&T` UB even if it's never dereferenced? Why is an uninitialized `u32` UB when every bit pattern is a
   valid `u32`?
6. A `bool`-returning helper returned 2 in production. Walk through how that's possible without a compiler bug.
7. Why may unsafe code trust its own private fields but not a caller-supplied `Ord` implementation? What does std do
   when it genuinely needs to trust an iterator's length?
8. Why is `#[no_mangle]` an unsafe attribute in edition 2024? Why is a foreign function *declaration* an unsafe act?
9. rust-lang/rust#25860: what is it, and what does it mean for the claim "safe Rust can't cause UB"?

### 12. Exercises

- **Beginner.** For each value, say whether producing it is UB and which invariant it breaks: `3u8` transmuted to
  `bool`; `0x110000` transmuted to `char`; a `&[u8]` with length 0 and a null pointer; a `String` whose bytes are
  not UTF-8 but which is never read; `MaybeUninit::<[u8; 4]>::uninit().assume_init()`. Check the UB ones under Miri.
- **Intermediate.** Write `fn ascii_lower(s: &str) -> String` twice: once with `from_utf8_unchecked` on the lowered
  bytes, once safely. Write the `// SAFETY:` comment for the first. Then change the input type to `&[u8]` and show that
  your comment becomes false.
- **Advanced.** Take listing `ch01-01-bool-two.rs` and change `to_u32` to `if b { 10 } else { 20 }`. Predict the
  release output for the byte 2, then check. Emit the assembly and explain the result from the instructions.
- **Systems.** Find a loop where replacing `v[i]` with `v.get_unchecked(i)` changes the release assembly (hint: an
  index computed from data, not from the loop counter). Measure both versions (one run is noisy; do several), and
  write down the smallest set of facts under which the unchecked version is sound. Would an `assert!` before the loop
  give you the same code safely?
- **Architecture.** Inventory `unsafe` in a Rust workspace you know (or a popular crate): how many modules, how many
  blocks, and which ones rely on invariants held by *safe* functions in the same module? Propose a boundary rule like
  §9's and estimate its review cost.

### 13. Debugging exercise

A utility module in a shared crate (listing `ch01-15-trusting-safe-trait.rs`):

```rust,ignore
fn collect_exact<I: ExactSizeIterator<Item = u64>>(it: I) -> Vec<u64> {
    let n = it.len();
    let mut out: Vec<u64> = Vec::with_capacity(n);
    let p = out.as_mut_ptr();
    let mut written = 0;
    for x in it {
        unsafe { p.add(written).write(x) }; // no bounds check: "len() told us"
        written += 1;
    }
    unsafe { out.set_len(written) };
    out
}
```

1. The function is safe to call. Is it sound? Write a *safe* program, with no `unsafe` anywhere in your code, that
   makes it cause undefined behavior.
2. In a debug build, what do you predict happens, and which std check fires? What does Miri report, and where?
3. Fix it without giving up the single up-front allocation in the common case. Then explain why `ExactSizeIterator` is
   a safe trait and what std uses when it needs a length it can *trust*.

### 14. Design exercise

**An `AsciiStr` for header names.** The gateway team wants a type for header names that are guaranteed ASCII
(lowercasing, hashing, and comparing become byte operations). Proposals:

- (a) `AsciiStr::new(&[u8]) -> Option<&AsciiStr>` validating every byte (safe);
- (b) (a) plus `unsafe fn new_unchecked(&[u8]) -> &AsciiStr` for the parser, which "already knows" the bytes are ASCII;
- (c) a safe `new_unchecked` guarded by `debug_assert!(bytes.is_ascii())`;
- (d) no new type: `&str` plus a comment.

For each, state the invariant, where it's enforced (§7's table), what code must be reviewed to trust it, and what goes
wrong if it's violated: is it UB, and where? One of the four is unsound. Say which, and write the safe program that
proves it. Recommend one for a parser that handles 400K requests/s, and name the measurement that would change your
mind.
