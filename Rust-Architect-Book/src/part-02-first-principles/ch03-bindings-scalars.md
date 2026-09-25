# Chapter 2.3 — Bindings, Mutability, and Scalar Types: Does `x` Exist at Runtime?

> **Where this sits:** Part II · Rust From First Principles · chapter 3 of 7
> **Prerequisites:** Chapters 2.1–2.2.
> **After this chapter you can:** follow `let x = 10;` from source through MIR, LLVM IR, and x86-64 assembly; say when a
> variable occupies memory and when it doesn't; explain type inference and its limits; and use Rust's integer, float,
> `char`, and conversion semantics without surprises.

---

## Pass 1 · User level — *What is a binding?*

### 1. Problem

In the JVM, a local variable is a **slot in the frame's local-variable array**. That's how the JVM specification
defines it, even though the JIT later moves hot locals into registers. Java engineers carry a quiet assumption from this:
a variable is a box somewhere in memory.

In Rust a variable is a **name for a value**. The language never promises the value is stored anywhere in particular.
Where it lives, whether that's a register, a stack slot, folded into a constant, or nowhere, is the compiler's decision,
and the answer changes between debug and release builds. This matters now because it dissolves some myths ("shadowing
allocates," "immutable variables are faster"). It also matters later, because moves and borrows (Part III) are rules
about *values and names*, not about memory boxes.

### 2. Mental model

```text
          let x = 10;
              │   │
              │   └── a VALUE, of type i32 (inferred: an unconstrained integer literal defaults to i32)
              └────── a BINDING: a compile-time name for that value; immutable unless declared `mut`

 Where does x live?   The language: it doesn't say.
                      The compiler: a constant, a register, a stack slot, or nowhere, depending on the build
                      and on what you do with x.
```

Three rules to keep straight:

1. **Mutability belongs to the binding, not the value or the type.** `let mut total = 0;` makes the *name* `total`
   reassignable. Moving the value to an immutable binding (`let total = total;`) freezes it again.
2. **Shadowing creates a new binding.** `let input = input.trim();` doesn't mutate anything. It declares a new `input`
   that hides the old one, and it can even have a different type.
3. **Types are inferred inside a function body, never across signatures.** Function parameters and return types are
   always written out.

The scalar types:

| Type | Size | Notes |
|---|---|---|
| `i8 i16 i32 i64 i128` | 1–16 bytes | Two's complement. `i32` is the default for integer literals. |
| `u8 u16 u32 u64 u128` | 1–16 bytes | Unsigned. Java has no equivalent (except `char`). |
| `isize` / `usize` | pointer width | Indices, lengths, and sizes use `usize`. |
| `f32` / `f64` | 4 / 8 bytes | IEEE 754. `f64` is the default for float literals. |
| `bool` | 1 byte | Only `0` and `1` are valid bit patterns. |
| `char` | 4 bytes | A **Unicode scalar value** (U+0000–U+10FFFF, excluding surrogates), not a UTF-16 unit. |
| `()` | 0 bytes | The unit type: "no meaningful value" (Chapter 2.4). |

### 3. Rust code

Bindings, inference, shadowing, and the scalar semantics, verified:

```rust
use std::mem::size_of;

fn main() {
    // Inference: integer literals default to i32 and floats to f64, unless context says otherwise.
    let a = 10;
    let b = 2.5;
    let c: u8 = 200;
    let d = 1_000_000u64;
    println!(
        "sizes: i32={} f64={} u8={} u64={} usize={} char={} bool={}",
        size_of::<i32>(), size_of::<f64>(), size_of::<u8>(), size_of::<u64>(),
        size_of::<usize>(), size_of::<char>(), size_of::<bool>()
    );
    println!("values: a={a} b={b} c={c} d={d}");

    // Mutability belongs to the binding, not the value.
    let mut total = 0u64;
    total += d;
    let total = total; // shadowing: from here on, `total` is an immutable binding
    println!("total={total}");

    // Shadowing can change the type: a transformation pipeline.
    let input = "  42 ";
    let input = input.trim();
    let input: u32 = input.parse().expect("a number");
    println!("parsed={input}");

    // `as` is a raw cast: it truncates, wraps, or saturates, and never fails.
    println!("300i32 as u8   = {}", 300i32 as u8);
    println!("-1i32 as u32   = {}", -1i32 as u32);
    println!("3.99f64 as i32 = {}", 3.99f64 as i32);
    println!("1e20 as i32    = {}", 1e20f64 as i32);
    println!("NaN as i32     = {}", f64::NAN as i32);

    // `From` / `TryFrom` are the checked alternatives.
    let wide: u64 = u64::from(c); // lossless: always succeeds
    let narrow = u8::try_from(300i32); // lossy: returns a Result
    println!("u64::from(200u8) = {wide}, u8::try_from(300) = {narrow:?}");

    // char is a Unicode scalar value (4 bytes), not a UTF-16 code unit.
    let ch = 'é';
    println!("'{ch}' = U+{:04X}, {} bytes in UTF-8", ch as u32, ch.len_utf8());
}
```

```text
sizes: i32=4 f64=8 u8=1 u64=8 usize=8 char=4 bool=1
values: a=10 b=2.5 c=200 d=1000000
total=1000000
parsed=42
300i32 as u8   = 44
-1i32 as u32   = 4294967295
3.99f64 as i32 = 3
1e20 as i32    = 2147483647
NaN as i32     = 0
u64::from(200u8) = 200, u8::try_from(300) = Err(TryFromIntError(PosOverflow))
'é' = U+00E9, 2 bytes in UTF-8
```

Every `as` result is **defined**: [LANG] integer-to-integer casts truncate or sign-extend (300 mod 256 = 44; the bits of
−1 reinterpreted as `u32` give 4294967295), and float-to-integer casts round toward zero and **saturate** (NaN becomes
0). That last rule is itself a piece of history. [VERSION] Before Rust 1.45, an out-of-range float-to-int `as` was
*undefined behavior*, a soundness hole in safe Rust. 1.45 defined it as saturating. `as` never fails, and that's
exactly why it's dangerous (§10). `From` exists only for conversions that can't lose information, and `TryFrom` makes
the lossy ones return a `Result`.

**Inference flows backwards as well as forwards.** Inside a function, rustc solves for types using *every* use:

```rust
fn main() {
    let mut ids = Vec::new(); // Vec<?T>: the element type is not known yet
    ids.push(7u16); // ...now it is: ?T = u16
    let first = ids[0];
    let doubled = first * 2; // u16 arithmetic
    println!("{doubled} ({})", std::any::type_name_of_val(&ids));
}
```

```text
14 (alloc::vec::Vec<u16>)
```

If nothing constrains a type, inference gives up instead of guessing:

```rust,compile_fail
fn main() {
    let items = Vec::new();
    println!("{}", items.len());
}
```

```text
error[E0282]: type annotations needed for `Vec<_>`
 --> src/main.rs:3:9
  |
3 |     let items = Vec::new();
  |         ^^^^^   ---------- type must be known at this point
  |
help: consider giving `items` an explicit type, where the type for type parameter `T` is specified
  |
3 |     let items: Vec<T> = Vec::new();
  |              ++++++++
```

A reminder from Chapter 1.3: `let _ = expr;` **doesn't bind**. The value is dropped at the end of the statement.
`let _name = expr;` does bind, and the leading underscore only silences the unused-variable warning. For RAII guards the
difference is a bug.

---

## Pass 2 · Systems level — *Follow `x` through the compiler*

### 4. Under the hood

Two tiny functions. They're marked `#[inline(never)]` so the compiler emits each one standalone and we can inspect it:

```rust,ignore
#[inline(never)]
pub fn constant() -> i32 {
    let x = 10;
    let y = x + 1;
    y * 2
}

#[inline(never)]
pub fn scale(a: i32) -> i32 {
    let x = a * 3;
    let y = x + 1;
    y
}
```

**Stage 1: MIR** (debug build, rustc 1.98.1). MIR is rustc's mid-level IR: a control-flow graph of **basic blocks**
over numbered locals (`_0` is always the return value). Borrow checking happens on MIR (Part XVIII).

```text
fn constant() -> i32 {
    let mut _0: i32;
    let mut _2: i32;
    let mut _3: (i32, bool);
    let mut _4: (i32, bool);
    scope 1 {
        debug x => const 10_i32;          // ← x has NO local at all: it is just the constant 10
        let _1: i32;
        scope 2 {
            debug y => _1;
        }
    }

    bb0: {
        _2 = const 10_i32;
        _3 = AddWithOverflow(copy _2, const 1_i32);
        assert(!move (_3.1: bool), "attempt to compute `{} + {}`, which would overflow",
               move _2, const 1_i32) -> [success: bb1, unwind continue];
    }

    bb1: {
        _1 = move (_3.0: i32);
        _4 = MulWithOverflow(copy _1, const 2_i32);
        assert(!move (_4.1: bool), "attempt to compute `{} * {}`, which would overflow",
               copy _1, const 2_i32) -> [success: bb2, unwind continue];
    }

    bb2: {
        _0 = move (_4.0: i32);
        return;
    }
}
```

Three things to notice:

- **Even in a debug build, `x` has no storage in MIR.** [RUSTC] A MIR pass noticed that `x` is a single-use constant,
  replaced it with `const 10_i32`, and kept only a *debug annotation* (`debug x => const 10_i32`) so a debugger can
  still display it.
- **Overflow checks are explicit MIR.** `AddWithOverflow` produces a `(result, overflowed)` pair, and an `assert`
  **terminator** branches to a panic if the flag is set. That's the `overflow-checks` profile setting (Chapter 2.2),
  visible as control flow.
- In `scale`, the MIR says `debug y => _0`: `y` *is* the return slot. There's no separate variable for it.

**Stage 2: LLVM IR, debug build.** rustc lowers MIR to LLVM IR, which is in **SSA form** (static single assignment:
every value is assigned exactly once). Here's `scale`:

```text
define i32 @..._10playground5scale(i32 %a) {
start:
  %y.dbg.spill = alloca [4 x i8], align 4          ; stack slots exist ONLY so the debugger
  %x.dbg.spill = alloca [4 x i8], align 4          ; can find a, x, y ("dbg.spill")
  %a.dbg.spill = alloca [4 x i8], align 4
  store i32 %a, ptr %a.dbg.spill, align 4
  %0 = call { i32, i1 } @llvm.smul.with.overflow.i32(i32 %a, i32 3)   ; a * 3, with overflow flag
  %_3.0 = extractvalue { i32, i1 } %0, 0
  %_3.1 = extractvalue { i32, i1 } %0, 1
  br i1 %_3.1, label %panic, label %bb1

bb1:
  store i32 %_3.0, ptr %x.dbg.spill, align 4       ; write x to its debug slot...
  %1 = call { i32, i1 } @llvm.sadd.with.overflow.i32(i32 %_3.0, i32 1) ; ...but compute with the SSA value
  ...
```

The computation flows through SSA values (`%_3.0`), and the stack slots are *write-only copies for the debugger*. That's
the honest answer to "does `x` exist in a debug build?": **yes, in a stack slot, but only so you can see it in gdb.**

**Stage 3: LLVM IR, release build.** No stack slots, no overflow checks:

```text
define noundef i32 @..._10playground5scale(i32 noundef %a) {
start:
  %x = mul i32 %a, 3
  %y = add i32 %x, 1
  ret i32 %y
}

define noundef i32 @..._10playground8constant() {
start:
  ret i32 22
}
```

`x` and `y` survive only as *names of SSA values*. `constant()` has been evaluated at compile time: `(10 + 1) * 2 = 22`.

**Stage 4: x86-64 assembly, release build:**

```text
playground::scale:
	lea	eax, [rdi + 2*rdi]    ; eax = a + 2a = 3a        (x exists here, for one instruction, in eax)
	inc	eax                   ; eax = 3a + 1             (y: the same register, now the return value)
	ret

playground::constant:
	mov	eax, 22               ; x and y don't exist at all
	ret
```

And the **debug** assembly of `scale`, for contrast (labels simplified):

```text
playground::scale:
	sub	rsp, 24                     ; a 24-byte stack frame
	mov	dword ptr [rsp + 12], edi   ; spill a
	mov	eax, 3
	imul	edi, eax                    ; a * 3
	mov	dword ptr [rsp + 8], edi    ; spill the product
	seto	al
	jo	<panic: mul overflow>       ; overflow check (the MIR assert)
	mov	eax, dword ptr [rsp + 8]
	mov	dword ptr [rsp + 16], eax   ; x's debug slot
	inc	eax                         ; x + 1
	mov	dword ptr [rsp + 4], eax    ; spill y
	seto	al
	jo	<panic: add overflow>
	...
```

So, **does `x` exist at run time?**

| | Debug build | Release build |
|---|---|---|
| `constant()` | A debug annotation only (no storage, even in MIR) | Nothing: the function returns 22 |
| `scale()` | A stack slot written for the debugger; the math is done in registers | Briefly, as the contents of `eax` |
| **[LANG] guarantee** | None. The language specifies values and behavior, not storage. | None. |

**What forces a variable into memory?** Taking its address in a way the optimizer can't see through. If `&x` escapes to
a function that isn't inlined, gets printed with `{:p}`, or is stored somewhere, `x` needs an address, so it gets a
stack slot. That's worth remembering for Part III: **borrowing is what gives a value a stable place.**

**SSA and `let mut`.** SSA forbids reassignment, so how does `let mut total = 0; total += d;` survive? Every assignment
becomes a *new* SSA value (`%total.1`, `%total.2`, ...). Where control flow merges, after an `if` or at a loop header, a
**φ (phi) node** picks the value from whichever predecessor block ran. Mutability is a *source-level* concept, and by the
time LLVM optimizes, it's gone. LLVM's `mem2reg` pass is what turns stack slots into SSA values in the first place. That's
why `let mut` isn't slower than shadowing and shadowing isn't slower than `let mut`. Both produce the same SSA.

**Register allocation.** The last compiler stage maps SSA values onto the machine's 16 general-purpose registers
(`rax`, `rdi`, and so on). When more values are live than registers are free, some are **spilled** to the stack. The
calling convention fixes where arguments arrive: `a` came in `edi` and the result leaves in `eax` (Chapter 2.4 covers
the ABI).

**Type inference, precisely.** [RUSTC] rustc gives each unknown type a variable, collects constraints from every
expression in the function body (a `push(7u16)` constrains `?T = u16`), and solves them by **unification**. Integer
literals get a special "integer variable" that defaults to `i32` if nothing else constrains it. [LANG] Inference never
crosses function signatures. The trade is deliberate: a signature is a contract that a reader, a caller, and the borrow
checker can rely on **without reading the body**. It keeps errors local, keeps compile times bounded, and keeps APIs from
changing type because of an edit in some unrelated function body.

### 5. Memory

**Scalar layout and valid values:**

| Type | Size / align | Valid bit patterns | Consequence |
|---|---|---|---|
| `u8`…`u128`, `i8`…`i128` | 1–16 / 1–16 | All | `Option<u8>` needs an extra byte (2 bytes) |
| `bool` | 1 / 1 | Only `0x00` and `0x01` | 254 invalid patterns form a **niche**, so `Option<bool>` is 1 byte |
| `char` | 4 / 4 | 0–0xD7FF and 0xE000–0x10FFFF | A niche, so `Option<char>` is 4 bytes |
| `f32` / `f64` | 4 / 4, 8 / 8 | All (NaN included) | No niche |
| `&T` | pointer size | Non-null, aligned | `Option<&T>` = pointer size (Chapter 1.2) |

[LANG] Producing an invalid value, such as a `bool` holding `2`, is **undefined behavior**, even if nobody reads it. Safe
Rust can't do it. `unsafe` code (Part XV) must never do it. The compiler is entitled to use invalid patterns for its own
purposes, as niches.

**Byte order.** x86-64 and most ARM targets are **little-endian**: the least significant byte comes first in memory.
Network protocols and many file formats are **big-endian**. Rust makes the conversion explicit: `u32::from_be_bytes`,
`to_le_bytes`, and so on (Chapter 2.4 uses them to parse a header).

**`scale`'s stack frame in the two builds:**

```text
 DEBUG (sub rsp, 24)                         RELEASE
 ┌──────────────────────┐ rsp+20             no stack frame at all:
 │ (padding)            │                    a arrives in edi, the result leaves in eax,
 │ x debug slot         │ rsp+16             everything happens in registers
 │ a spill              │ rsp+12
 │ product spill (x)    │ rsp+8
 │ y spill              │ rsp+4
 └──────────────────────┘ rsp
```

### 6. CPU / OS

- **Integer arithmetic** is one instruction per operation. The overflow check costs a flag test and a branch that's
  almost never taken, so it's cheap in isolation. It matters in hot loops, where it can prevent vectorization.
- **Division is checked in every build.** [LANG] Integer division by zero panics, and so does `i32::MIN / -1`, whose
  result doesn't fit. On x86-64 both would raise a hardware exception (`#DE`, delivered as `SIGFPE` on Linux), so rustc
  inserts explicit checks *before* the `idiv`. That's a panic with a message instead of a signal.
- **Floats** live in SSE registers (`xmm0`–`xmm15`) and follow IEEE 754: NaN compares unequal to everything, itself
  included; `-0.0 == 0.0`; `0.1 + 0.2 != 0.3`. That's why `f64` implements `PartialEq`/`PartialOrd` but **not**
  `Eq`/`Ord` in Rust, and why `sort()` rejects a `Vec<f64>` (the debugging exercise).
- **`i128`** occupies two 64-bit registers, and multiplication or division on it is a sequence of instructions or a
  library call, not one instruction.
- **`usize`** is 8 bytes on 64-bit targets and 4 on 32-bit (including `wasm32`). Serialized formats shouldn't use it.

---

## Pass 3 · Architect level — *Choosing types and conversions deliberately*

### 7. Trade-offs

**Choosing integer types:**

| Use | Type | Why |
|---|---|---|
| Indices, lengths, in-memory sizes | `usize` | That's what slices and `Vec` use. It avoids conversions at every index. |
| Wire formats, file formats, database columns | Explicit fixed width (`u32`, `i64`) | `usize` varies by platform, and the format mustn't. |
| Money | `i64` minor units (cents), or a decimal type (e.g. the `rust_decimal` crate) | Exact arithmetic. **Never `f64`.** |
| Counters that must not wrap | `u64` with checked or saturating arithmetic | A wrapped counter is silent corruption. |
| Bit flags, hashes, sequence numbers | Unsigned with `wrapping_*` | Wrapping is the intended semantics there. |

**Conversions:** prefer `From` (lossless) and `TryFrom` (lossy, fallible) to `as`. Keep `as` for places where
truncation *is* the intent (hashing, bit manipulation) and say so in a comment. Clippy's pedantic lints
`cast_possible_truncation`, `cast_sign_loss`, and `cast_possible_wrap` find the rest.

**Shadowing vs `mut`:** they compile identically (§4), so choose for readers. Shadowing suits *transformations* (parse,
validate, convert): each stage gets a new, immutable name, possibly with a new type. `let mut` suits *accumulation*: a
counter, a buffer being filled. Keep `mut` scopes short, and freeze the value afterwards with `let x = x;`.

> **Why immutable by default, if the compiler doesn't need it for speed?** Because it's for *people* and for the
> *borrow checker*. An immutable binding tells a reader that the value never changes in this scope, and it's a promise
> the compiler enforces: you can't take `&mut` of an immutable binding (Chapter 2.6's E0596). When mutation is the
> exception and marked with `mut`, the places where state changes stand out.

### 8. Java comparison

| Java | Rust | Note |
|---|---|---|
| `int`, `long` signed only; `char` is the only unsigned type | `i*` and `u*` at every width | Java's `byte` is signed (-128..127), so byte-level protocol code is full of `& 0xFF`. Rust has `u8`. |
| `char`: a UTF-16 code unit (2 bytes) | `char`: a Unicode scalar value (4 bytes) | A Java `char` can hold half a surrogate pair. A Rust `char` is always a complete code point. |
| `final int x` | `let x` | Java's `final` is *shallow* (a `final List` can still be modified). Rust immutability is inherited through what the binding owns (Chapter 1.3). |
| `var x = ...` (Java 10) | `let x = ...` | Java's `var` infers from the initializer only. Rust infers from all later uses too (the `Vec::new()` example). |
| Integer overflow wraps silently; `Math.addExact` throws | Panics in debug, wraps in release; `checked_*`, `wrapping_*`, `saturating_*` | Java's default is Rust's release default, without the debug safety net. |
| `(int) 1e20` → `Integer.MAX_VALUE`; `(int) NaN` → 0 | The same saturating semantics | JLS §5.1.3 and Rust (since 1.45) agree. |
| `Double.compare` / `Comparator.naturalOrder()` gives a total order | `f64::total_cmp` | Java's natural ordering for boxed doubles is already total. Rust makes you choose. |
| `Integer` boxing, the Integer cache | No boxing of scalars unless you ask for `Box<i32>` | A `Vec<i32>` is contiguous `i32`s. An `ArrayList<Integer>` is references to objects. |

> **Analogy limit.** "A Rust `let` is a Java local variable" holds for scoping. It fails for storage. The JVM
> specification *defines* locals as frame slots, and optimizing them away is the JIT's business. The Rust language
> defines no storage for bindings at all. Storage is purely a compilation decision, which is why the debug and release
> answers in §4 can differ completely.

### 9. Production scenario

**Money at Meridian's JSON boundary.** The payments service represents amounts as `i64` minor units, which is exact and
fast, with checked arithmetic in the billing crate. The storefront talks to browser clients in JSON, and **JavaScript
numbers are IEEE 754 doubles**: integers are exact only up to 2⁵³ − 1 (about 9 × 10¹⁵). An `i64` amount in minor units
of a high-denomination currency, or a 64-bit ID, can exceed that and be silently rounded by `JSON.parse`. The
architecture decision:

- Amounts cross the JSON boundary as **strings** (`"12500"`) or as `{ "units": ..., "nanos": ... }` pairs. Never as bare
  JSON numbers above 2⁵³.
- 64-bit IDs go out as strings too.
- Inside Rust, `i64` with `checked_add` / `checked_mul`. At the boundary, `TryFrom` with explicit rejection.

It's a numeric-representation decision that no Rust type can enforce once data has left the process. It has to be part
of the API contract.

### 10. Failure scenario

**The truncated length prefix.** A framing layer writes a 16-bit length before each message body:

```rust
fn main() {
    let payload = vec![0u8; 70_000];

    // BUG: `as` silently truncates: 70_000 mod 65_536 = 4_464.
    let len_field = payload.len() as u16;
    println!("length field written: {len_field}");

    // FIX: make the narrowing explicit and fallible.
    match u16::try_from(payload.len()) {
        Ok(len) => println!("length field: {len}"),
        Err(e) => println!("rejecting frame: {} bytes does not fit in u16 ({e})", payload.len()),
    }
}
```

```text
length field written: 4464
rejecting frame: 70000 bytes does not fit in u16 (out of range integral type conversion attempted)
```

In production, the buggy version is worse than a crash. The receiver reads a 4,464-byte body, then parses the remaining
65,536 bytes of *this* message as the next frame's header. The stream is desynchronized, and every later message on the
connection is garbage. If an attacker controls payload sizes, the same pattern is how request-smuggling attacks work:
one layer and the next disagree about where a message ends. The rule: **at every trust or format boundary, narrowing
conversions go through `TryFrom`, and failure is an explicit branch.**

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part II).*

1. Does `x` in `let x = 10;` exist at run time? Answer for debug and release builds, and name something that forces a
   variable into memory.
2. Why is mutability a property of the binding rather than the type? What does `let mut` change in the generated code?
3. What is SSA? How do `let mut` reassignments and loops appear in SSA form?
4. Why does Rust limit type inference to function bodies? Compare with Java's `var`.
5. Why can't you call `sort()` on a `Vec<f64>`? What are the alternatives, and what do they do with NaN and `-0.0`?
6. What do `300i32 as u8`, `-1i32 as u32`, `1e20 as i32`, and `f64::NAN as i32` produce, and why? Why did Rust 1.45
   change float-to-int casts?
7. Why is `Option<bool>` 1 byte but `Option<u8>` 2 bytes?
8. How would you represent money in a Rust service with JavaScript clients? Where does the representation change, and
   why there?

### 12. Exercises

- **Beginner.** Before running anything, predict: `(-1i8) as u8`, `256u16 as u8`, `-3.7f64 as i32`, `(-3.7f64) as u8`,
  `u16::try_from(-1i32)`. Then check on the Playground.
- **Intermediate.** Write `fn average(xs: &[u32]) -> Option<u32>` that can't overflow for any input (hint: the sum's
  type) and returns `None` for an empty slice. Test it with `u32::MAX` values.
- **Advanced.** Compile `fn sum_to(n: u64) -> u64 { let mut t = 0; let mut i = 0; while i < n { t += i; i += 1; } t }`
  to LLVM IR in release mode. Find the loop, or explain why there isn't one. (LLVM may have used a closed-form formula.)
  Then find the φ nodes in the debug IR.
- **Systems.** Compile `pub fn div(a: i32, b: i32) -> i32 { a / b }` in release mode and find the two checks rustc
  inserted before `idiv`. What would the CPU do without them?
- **Architecture.** Write a one-page **numeric policy** for a payments codebase: types for money, rates, quantities, and
  IDs; overflow behavior per type; allowed conversions; and representations at the JSON, database, and FFI boundaries.

### 13. Debugging exercise

```rust,compile_fail
fn main() {
    let mut prices = vec![19.99, 5.25, 12.0];
    prices.sort();
    println!("{prices:?}");
}
```

```text
error[E0277]: the trait bound `{float}: Ord` is not satisfied
   --> src/main.rs:4:12
    |
  4 |     prices.sort();
    |            ^^^^ the trait `Ord` is not implemented for `{float}`
    |
note: required by a bound in `slice::<impl [T]>::sort`
    |
131 |     pub fn sort(&mut self)
    |            ---- required by a bound in this associated function
132 |     where
133 |         T: Ord,
    |            ^^^ required by this bound in `slice::<impl [T]>::sort`
```

1. Why doesn't `f64` implement `Ord`? Give a concrete pair of values that breaks a total order under `<`.
2. What is `{float}` in the message, and why does the error say `{float}` instead of `f64`?
3. Fix it. The verified fix below uses `total_cmp`:

```rust
fn main() {
    let mut prices = vec![19.99, f64::NAN, 5.25, -0.0, 0.0, 12.0];
    prices.sort_by(|a, b| a.total_cmp(b)); // IEEE 754 totalOrder: every f64 has a place
    println!("{prices:?}");
    println!("NaN == NaN:       {}", f64::NAN == f64::NAN);
    println!("0.1 + 0.2 == 0.3: {}", 0.1 + 0.2 == 0.3);
}
```

```text
[-0.0, 0.0, 5.25, 12.0, 19.99, NaN]
NaN == NaN:       false
0.1 + 0.2 == 0.3: false
```

rustc also warns on the second `println!`:

```text
warning: incorrect NaN comparison, NaN cannot be directly compared to itself
  = note: `#[warn(invalid_nan_comparisons)]` on by default
help: use `f32::is_nan()` or `f64::is_nan()` instead
```

Where did `total_cmp` put `-0.0` relative to `0.0`, and NaN relative to everything else? For a list of **prices**, is
sorting NaN to the end what you want, or should NaN have been rejected at the boundary?

### 14. Design exercise

**Numeric types for Meridian's order book.** Orders carry a price, a quantity, a notional (price × quantity), and a fee
(basis points of notional). Prices have instrument-specific tick sizes (0.01 for equities, 0.00001 for FX). Quantities
can be fractional for some instruments. The matching engine does about 2 million price comparisons per second per core.

Choose representations (scaled integers, decimals, or floats) for each field. Define the overflow behavior of
`price × quantity` and where it's checked. Specify the conversions at the JSON API, the database, and the market-data
feed. Justify each choice on correctness, speed, and how mistakes would show up. State the largest notional your design
can represent without overflow.
