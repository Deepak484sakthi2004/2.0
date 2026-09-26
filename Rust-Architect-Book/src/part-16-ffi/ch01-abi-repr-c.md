# Chapter 16.1 — ABIs, Calling Conventions, and `repr(C)`

> **Where this sits:** Part XVI · FFI and Systems Programming · chapter 1 of 4
> **Prerequisites:** Chapter 5.2 (layout, niches, `repr`), Chapter 6.5 (why `dyn` isn't a plugin ABI), Chapter 8.3
> (panics at an `extern "C"` boundary), Chapter 15.1 (`unsafe extern`, `#[unsafe(no_mangle)]`, validity invariants),
> Chapter 18.6 (the `#[rustc_abi(debug)]` dump).
> **After this chapter you can:** say exactly what an ABI consists of and which Rust feature controls each part; read
> the System V classification of a struct from a compiler dump, from LLVM IR, and from assembly; design boundary types
> whose layout is pinned and tested in CI; choose between a Rust enum and an explicit integer encoding at a boundary;
> and decide where a panic may and may not travel.

---

## Pass 1 · User level — *What two compilers must agree on*

### 1. Problem

Meridian's fraud feature library is Rust, and the scoring service that calls it is Java (Chapter 1.2: 50K scores/s,
p99 under 5 ms, called through FFM). Chapter 8.3 showed one entry point, `meridian_score`, returning 0, -1, or -99.
Behind that one function sits a question neither language answers on its own: when the JVM calls a function it did
not compile, **how do the two sides agree on where the arguments are, how big each one is, which registers survive the
call, and what happens if something goes wrong inside?**

Inside one Rust program, the compiler answers all of that and never has to tell anyone. It can reorder struct fields,
pass a 24-byte struct as a pointer, change its mind in the next release, and nothing breaks, because it compiled both
the caller and the callee (Chapter 6.5: [LANG] Rust has no stable ABI). Across a language boundary that freedom is the
problem. The JVM, a C compiler, and rustc each generate code on their own, so every assumption must be written down
somewhere all three can read. That written-down agreement is the **ABI**, the application binary interface.

The brief's picture for this Part is a ladder, and this chapter is its second rung:

```text
 Rust      your types and functions             (rustc decides everything, promises nothing)
  ↓
 ABI       layout + calling convention + symbols + unwinding      ← this chapter
  ↓
 C         the lingua franca: every language can speak the C ABI  ← 16.2, 16.3
  ↓
 OS        the kernel's own convention (syscalls)                 ← §6
```

### 2. Mental model

**An ABI is four agreements.** Break any one and the call goes wrong, usually without an error message:

| Agreement | The question | Rust's default | How you pin it |
|---|---|---|---|
| **Data layout** | Size, alignment, field offsets, enum encodings | unspecified: fields may be reordered | `#[repr(C)]`, `#[repr(transparent)]`, `#[repr(u32)]`, `#[repr(C, u32)]` |
| **Calling convention** | Which registers or stack slots hold each argument and the result; who saves which registers | `extern "Rust"`: unspecified, may change between releases | `extern "C"` (the target's C convention); also `"system"`, `"sysv64"`, `"win64"` |
| **Symbols** | The name the linker and `dlsym` see | v0-mangled (`_RNvCs…`, Chapter 18.6) | `#[unsafe(no_mangle)]`, `#[unsafe(export_name = "...")]` |
| **Unwinding** | May a panic or exception cross the call? | yes, within Rust | `extern "C"`: no (a panic aborts); `extern "C-unwind"`: yes |

**Nobody checks the agreement.** [OS] The linker and the dynamic loader match **names** only. A C header, a Rust
`unsafe extern` block, and a Java `FunctionDescriptor` are three independent transcriptions of one contract, and a
mismatch between them compiles, links, loads, and runs. That's why Chapter 15.1 made the *declaration* itself
`unsafe`: a wrong signature is undefined behavior at every call, and the only defenses are writing it down once
(a header), generating the others from it (`bindgen`, `cbindgen`, `jextract`, Chapter 16.3), and testing the
parts that can be tested (§3's layout assertions).

**`extern "C"` means "the C ABI of the target you compile for."** It's System V AMD64 on x86-64 Linux and macOS, the
Microsoft x64 convention on Windows, AAPCS64 on 64-bit Arm. [LANG] Rust promises that an `extern "C"` function is
callable from C compiled for the same target, not that the registers are the same everywhere. Every register name in
this chapter is x86-64 Linux [CPU] [OS].

### 3. Rust code

**Layout: `repr(C)` versus the default** (listing `ch01-01-repr-c-layout.rs`, verified, also clean under Miri):

```rust,ignore
/// Rust's default representation: the compiler may reorder fields (and does, to cut padding).
struct TxnRust {
    flags: u8,
    amount_cents: i64,
    currency: u16,
    merchant_id: u32,
}

/// The same fields with the C layout: declaration order, each field aligned, size rounded up.
#[repr(C)]
pub struct MeridianTxn {
    pub flags: u8,         // offset 0, then 7 bytes of padding
    pub amount_cents: i64, // offset 8
    pub currency: u16,     // offset 16, then 2 bytes of padding
    pub merchant_id: u32,  // offset 20
} // size 24, align 8

/// Reordered by hand, largest first: still repr(C), still a fixed contract, now without holes.
/// (It is also a DIFFERENT contract: every C and Java caller must change with it. See 16.1 §10.)
#[repr(C)]
pub struct MeridianTxnV2 {
    pub amount_cents: i64, // offset 0
    pub merchant_id: u32,  // offset 8
    pub currency: u16,     // offset 12
    pub flags: u8,         // offset 14, then 1 byte of padding
} // size 16, align 8

// The contract as compile-time assertions: CI fails if anyone changes the layout by accident.
const _: () = {
    assert!(size_of::<MeridianTxn>() == 24);
    assert!(align_of::<MeridianTxn>() == 8);
    assert!(offset_of!(MeridianTxn, amount_cents) == 8);
    assert!(offset_of!(MeridianTxn, currency) == 16);
    assert!(offset_of!(MeridianTxn, merchant_id) == 20);
};
```

```text
default repr : size 16  flags@14 amount@0  currency@12 merchant@8
repr(C)      : size 24  flags@0  amount@8  currency@16 merchant@20
repr(C) v2   : size 16  flags@14 amount@0  currency@12 merchant@8
```

[RUSTC] rustc 1.98.1 sorted the default-repr fields by alignment, exactly as a careful C programmer would, and got 16
bytes. With `repr(C)` it can't reorder anything, and 8 of the 24 bytes are padding. That's the price of a layout you can
promise. The
`const` block turns the promise into a build failure. When a later PR inserts a `u32 channel` after `currency`
(listing `ch01-02-layout-assert-fails.rs`):

```text
error[E0080]: evaluation panicked: MeridianTxn size changed: bump MERIDIAN_ABI_VERSION
  --> src/main.rs:16:5
```

**Nullable pointers with a guarantee.** [LANG] `Option<&T>`, `Option<&mut T>`, `Option<Box<T>>`, `Option<NonNull<T>>`,
and `Option<fn ...>` have the size, alignment, and **ABI** of the inner pointer, with `None` represented as all-zero
bits: C's `NULL`. The guarantee extends through `#[repr(transparent)]` wrappers, which have exactly the layout and
calling convention of their single non-zero-sized field (Chapter 5.2). So these signatures are all FFI-safe, and with
the lint denied, the file compiling is the compiler agreeing (listing `ch01-05-nullable-pointers.rs`, verified, also
clean under Miri):

```rust,ignore
#![deny(improper_ctypes_definitions)]

/// A typed handle: repr(transparent) gives it exactly the layout AND the calling convention of the
/// NonNull inside, so `Option<ScorerRef>` is still one nullable pointer.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct ScorerRef(NonNull<c_void>);

pub type Callback = extern "C" fn(u32) -> u32;

// `const Limits *limits` in C: may be NULL, and the type says so.
pub extern "C" fn block_threshold(limits: Option<&Limits>) -> u32 {
    limits.map_or(80, |l| l.block_at)
}

// `void (*on_block)(uint32_t)` in C: a nullable function pointer.
pub extern "C" fn notify(on_block: Option<Callback>, score: u32) -> u32 {
    match on_block {
        Some(f) => f(score),
        None => 0,
    }
}
```

```text
sizes: Option<&Limits>=8 Option<NonNull<c_void>>=8 Option<Box<Limits>>=8 Option<Callback>=8 Option<ScorerRef>=8
block_threshold(NULL)=80 block_threshold(&65)=65
notify(NULL, 40)=0 notify(double, 40)=80
is_open(NULL)=false is_open(h)=true
```

The guarantee covers pointer-like types only. `Option<u32>` has no spare bit pattern, so it needs a tag, and its layout
is unspecified. The lint says so (listing `ch01-06-option-u32-lint.rs`):

```rust,compile_fail
#![deny(improper_ctypes_definitions)]

#[unsafe(no_mangle)]
pub extern "C" fn meridian_block_threshold(override_score: Option<u32>) -> u32 {
    override_score.unwrap_or(80)
}
```

```text
error: `extern` fn uses type `Option<u32>`, which is not FFI-safe
 --> src/main.rs:7:60
  |
7 | pub extern "C" fn meridian_block_threshold(override_score: Option<u32>) -> u32 {
  |                                                            ^^^^^^^^^^^ not FFI-safe
  |
  = help: consider adding a `#[repr(C)]`, `#[repr(transparent)]`, or integer `#[repr(...)]` attribute to this enum
  = note: enum has no representation hint
```

**Enums with data, two defined layouts.** [LANG] RFC 2195 gives enums with fields a defined layout under `repr(C, u32)`
and `repr(u32)`, and the two are different. Listing `ch01-03-enum-layouts.rs` spells each one out as the struct or
union it is defined to be, then reads a value through that shape (verified, clean under Miri):

```rust,ignore
#[repr(C, u32)]
#[derive(Clone, Copy)]
pub enum DecisionC {
    Allow,
    Review(u8), // risk score
    Block(u64), // id of the rule that blocked
}

// repr(C, u32) is defined as: a repr(C) struct { tag: u32, payload: repr(C) union of the variants }.
#[repr(C)]
struct DecisionCRepr {
    tag: u32,
    payload: DecisionCPayload, // the union is 8-aligned (it holds a u64), so it starts at offset 8
}

// repr(u32) is defined as: a repr(C) union of repr(C) structs, each one starting with the u32 tag.
#[repr(C)]
struct ReviewU32 {
    tag: u32,
    risk: u8, // right after the tag: offset 4
}
```

```text
size: repr(C, u32) = 16, repr(u32) = 16
repr(C, u32): Review payload at offset 8
repr(u32)   : Review payload at offset 4
repr(u32)   : Block  payload at offset 8
DecisionC::Review(70) seen as C: tag=1 risk=70
DecisionU32::Block(4017) seen as C: tag=2 rule=4017
```

Same size, same variants, different offset for `Review`'s byte. Chapter 18.6 noticed that difference in a layout dump
and called it "invisible inside Rust and decisive at a boundary." Here it is decisive: a C header must declare the one
you chose.

**An enum arriving from C is an integer until you've checked it.** Chapter 15.1 listed "an enum has a valid
discriminant" among the validity invariants. If Java sends tag 3 and the Rust signature says `DecisionC`, an invalid
value exists the moment the call starts, and no `match` afterwards can help. So incoming values use an explicit
encoding (listing `ch01-04-explicit-encoding.rs`, verified, clean under Miri):

```rust,ignore
/// The wire/ABI encoding: a plain u32 that any C or Java caller can produce.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DecisionCode(pub u32);

impl DecisionCode {
    pub const ALLOW: Self = Self(0);
    pub const REVIEW: Self = Self(1);
    pub const BLOCK: Self = Self(2);
}

/// What crosses the boundary: every field has a defined layout and every bit pattern is valid.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MeridianDecision {
    pub code: DecisionCode, // offset 0
    pub risk: u8,           // offset 4: meaningful for REVIEW
    pub rule_id: u64,       // offset 8: meaningful for BLOCK
}

impl TryFrom<&MeridianDecision> for Decision {
    type Error = UnknownDecisionCode;
    fn try_from(m: &MeridianDecision) -> Result<Self, Self::Error> {
        match m.code {
            DecisionCode::ALLOW => Ok(Decision::Allow),
            DecisionCode::REVIEW => Ok(Decision::Review { risk: m.risk }),
            DecisionCode::BLOCK => Ok(Decision::Block { rule_id: m.rule_id }),
            DecisionCode(other) => Err(UnknownDecisionCode(other)),
        }
    }
}
```

```text
size_of::<DecisionCode>() = 4, size_of::<MeridianDecision>() = 16
outgoing: MeridianDecision { code: DecisionCode(1), risk: 70, rule_id: 0 }
round trip: Ok(Review { risk: 70 })
incoming code 3: Err(UnknownDecisionCode(3))
```

The newer Java client's code 3 is now an `Err`, not undefined behavior. The Rust enum still exists; it just never
appears in a signature.

**Where a panic may travel** (listing `ch01-09-unwind-abis.rs`, verified, clean under Miri):

```rust,ignore
/// For callers that can't unwind (C, the JVM): the panic stops here and becomes a code.
pub extern "C" fn guarded(x: u32, out: &mut u32) -> i32 {
    match panic::catch_unwind(AssertUnwindSafe(|| risky(x))) {
        Ok(v) => {
            *out = v;
            MERIDIAN_OK
        }
        Err(_) => MERIDIAN_ERR_PANIC,
    }
}

/// For callers that CAN unwind (Rust, or C++ compiled with exceptions): the panic may cross.
pub extern "C-unwind" fn unguarded(x: u32) -> u32 {
    risky(x)
}
```

```text
guarded(4)   -> rc=0 out=25
guarded(0)   -> rc=-99
unguarded(0) -> the panic crossed the C-unwind boundary and was caught by the caller: true
```

What happens without either is Chapter 8.3's debugging exercise, verified there: [VERSION] since Rust 1.81 a panic
that reaches an `extern "C"` function's boundary **aborts the process** (before 1.81 it was undefined behavior). §4 shows
the code rustc generates to make that happen.

---

## Pass 2 · Systems level — *Registers, stack slots, and a landing pad*

### 4. Under the hood

**The ABI rustc computed, for both conventions.** The nightly `#[rustc_abi(debug)]` attribute from Chapter 18.6 prints
`fn_abi_of` for each function. Listing `ch01-07-abi-dump.rs` declares two `repr(C)` structs, `Point { x: f64, y: f64 }`
(16 bytes) and `Triple { a, b, c: u64 }` (24 bytes), and the same function under each convention. Debug build, trimmed
(the release build prints the same modes):

```text
error: fn_abi_of(rust_norm) = FnAbi {
           args: [ArgAbi { ty: Point, … mode: Pair(ArgAttributes { regular: NoUndef, … },
                                                   ArgAttributes { regular: NoUndef, … }) }],
           conv: Rust,
           can_unwind: true,
error: fn_abi_of(c_norm) = FnAbi {
           args: [ArgAbi { ty: Point, … mode: Cast { cast: CastTarget {
                                                prefix: [Reg { kind: Float, size: Size(8 bytes) }],
                                                rest: Uniform { unit: Reg { kind: Float, size: Size(8 bytes) },
                                                                total: Size(8 bytes), … } } } }],
           conv: C,
           can_unwind: false,
error: fn_abi_of(rust_sum) = FnAbi {
           args: [ArgAbi { ty: Triple, … mode: Indirect { attrs: ArgAttributes { …, pointee_size: Size(24 bytes), … },
                                                          mode: Pointer } }],
           conv: Rust,
           can_unwind: true,
error: fn_abi_of(c_sum) = FnAbi {
           args: [ArgAbi { ty: Triple, … mode: Indirect { …, mode: OnStack } }],
           conv: C,
           can_unwind: false,
error: fn_abi_of(c_unwind_sum) = FnAbi {
           args: [ArgAbi { ty: Triple, … mode: Indirect { …, mode: OnStack } }],
           conv: C,
           can_unwind: true,
```

(Reformatted: the fields are verbatim, the nesting condensed.) Read it as four decisions:

- **`Point`, Rust convention: `Pair`.** rustc passes the two fields as two separate scalar arguments. That's a Rust
  choice [RUSTC], not a promise.
- **`Point`, C convention: `Cast` into two `Float` registers.** That's the System V classification [CPU] [OS]: an
  aggregate of at most 16 bytes is split into 8-byte "eightbytes," each classified INTEGER or SSE by its contents.
  Both of `Point`'s eightbytes hold a `double`, so both are SSE, and the struct travels in `xmm0` and `xmm1`.
- **`Triple`, Rust convention: `Indirect { mode: Pointer }`.** The caller passes a pointer to the value.
- **`Triple`, C convention: `Indirect { mode: OnStack }`.** Larger than 16 bytes means class MEMORY: the caller
  **copies** all 24 bytes into its outgoing stack area, and the callee reads them from there.
- **`can_unwind`** is `true` for the Rust convention and for `extern "C-unwind"`, `false` for `extern "C"`. Same
  registers, different promise about exceptions.

**The same decisions in LLVM IR** (listing `ch01-08-abi-asm.rs`, release, `tools/emit.ps1 -Target llvm-ir`; symbol
names shortened):

```text
define noundef double @…rust_norm(double noundef %p.0, double noundef %p.1)
define noundef double @…c_norm({ double, double } %0)
define noundef i64 @…rust_sum(ptr dead_on_return noalias nofree noundef readonly align 8 captures(none) dereferenceable(24) %t)
define noundef i64 @…c_sum(ptr noalias nofree noundef readonly byval([24 x i8]) align 8 captures(none) dereferenceable(24) %t)
```

`byval([24 x i8])` is LLVM's spelling of "the pointee is a copy in the caller's argument area." And the assembly of the
two callers shows what that costs (comments added):

```text
playground::call_c_sum:                          ; c_sum(*t) with the C convention
	sub	rsp, 24
	mov	rax, qword ptr [rdi + 16]
	mov	qword ptr [rsp + 16], rax
	movups	xmm0, xmmword ptr [rdi]
	movups	xmmword ptr [rsp], xmm0          ; 24 bytes copied into the outgoing argument area
	call	qword ptr [rip + playground::c_sum@GOTPCREL]
	add	rsp, 24
	ret

playground::call_rust_sum:                       ; rust_sum(*t) with the Rust convention
	jmp	qword ptr [rip + playground::rust_sum@GOTPCREL]   ; the caller's own pointer, passed through

playground::c_sum:
	mov	rax, qword ptr [rsp + 16]            ; the fields sit above the return address
	add	rax, qword ptr [rsp + 8]
	add	rax, qword ptr [rsp + 24]
	ret

playground::c_norm:                              ; identical to rust_norm: xmm0 and xmm1
	mulsd	xmm0, xmm0
	mulsd	xmm1, xmm1
	addsd	xmm0, xmm1
	ret
```

The Rust convention made no copy at all. `call_rust_sum` received a pointer to a `Triple` in `rdi` and tail-jumped
with the same `rdi`. [RUSTC] That's legal because the release ABI marked the parameter `ReadOnly` and `NoAlias`: the
callee promises not to write through it, so it may read the caller's own value. The C convention can't do that: System
V says the callee owns a private copy on the stack. And for `Point` the two conventions happened to produce identical
code. That's the lesson of the whole dump in one line: **the Rust convention is often the same as C's, sometimes
better, and never promised.**

**What `can_unwind: false` compiles to.** An `extern "C"` function that can panic gets a landing pad that turns an
unwind into an abort. Listing `ch03-02-export-symbols.rs` (used again in Chapter 16.3) has one,
`meridian_ratio(a, b) = a / b`, whose division by zero panics. Its release IR:

```text
define noundef i32 @meridian_ratio(i32 noundef %a, i32 noundef %b) unnamed_addr #2 personality ptr @rust_eh_personality {
  ...
panic:
; invoke core::panicking::panic_const::panic_const_div_by_zero
  invoke void @…panic_const_div_by_zero(…)
          to label %unreachable unwind label %terminate

terminate:
  %0 = landingpad { ptr, i32 }
          filter [0 x ptr] zeroinitializer
; call core::panicking::panic_cannot_unwind
  tail call void @…panic_cannot_unwind() #6
  unreachable
```

The panic is an `invoke`, so if it unwinds, control goes to `%terminate`, whose empty filter catches anything, and
`panic_cannot_unwind` aborts with the message *panic in a function that cannot unwind*. [RUSTC] That's the whole
mechanism behind the 1.81 rule. The `catch_unwind` in `guarded` exists so that this landing pad is never reached: the
panic is caught one frame earlier and becomes `-99`.

### 5. Memory

The four boundary layouts from §3, byte by byte:

```text
 MeridianTxn (repr(C), 24 B)            0        8                16       20       24
                                         │flags│pad×7│ amount_cents │cur│pp│merchant│
 MeridianTxnV2 (repr(C), 16 B)          0                8        12   14 15 16
                                         │ amount_cents  │merchant│cur│fl│p│
 DecisionC  (repr(C, u32), 16 B)        0      4        8                16
                                         │ tag  │ pad    │ payload union  │   Review's u8 at 8, Block's u64 at 8
 DecisionU32 (repr(u32), 16 B)          0      4 5      8                16
                                         │ tag  │r│pad   │ Block's u64    │   Review's u8 at 4
```

Two consequences for anyone writing the other side:

- **Padding is part of the contract but not part of the data.** Its bytes are uninitialized in Rust (Chapter 15.1: to
  the abstract machine, *no* value), so never compare or hash a boundary struct as raw bytes, and never read the whole
  struct as bytes to "log it." Read the defined fields. Java's FFM makes the same point from the other side: its
  `MemoryLayout.structLayout` requires you to write the padding members explicitly (§8).
- **The C convention can put your struct in registers.** A 16-byte `repr(C)` struct of two `double`s never touches
  memory on the way into `c_norm`. A 24-byte one is copied into the caller's frame:

```text
 caller's frame during `call c_sum`          c_sum sees
 ┌──────────────────────────┐ rsp+24 ─────►  [rsp + 24] = t.c
 │ t.c                      │
 │ t.b                      │ rsp+16 ─────►  [rsp + 16] = t.b
 │ t.a                      │ rsp+8  ─────►  [rsp + 8]  = t.a
 │ return address           │ rsp    (pushed by `call`)
 └──────────────────────────┘
```

### 6. CPU / OS

**System V AMD64, the parts you'll meet** [CPU] [OS]:

| | Integer and pointer | Floating point | Notes |
|---|---|---|---|
| Arguments | `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`, then stack | `xmm0`–`xmm7`, then stack | Aggregates ≤ 16 B are classified per eightbyte; larger ones are copied to the stack |
| Return | `rax` (and `rdx`) | `xmm0` (and `xmm1`) | Larger results go through a hidden pointer the caller passes in `rdi`, returned in `rax` |
| Preserved across calls | `rbx`, `rbp`, `r12`–`r15`, `rsp` | none of the `xmm` registers | Everything else may be clobbered |
| Stack | 16-byte aligned at every `call` | | A 128-byte red zone below `rsp` for leaf functions |

**Windows x64 is a different contract** under the same `extern "C"`: `rcx`, `rdx`, `r8`, `r9` for the first four
arguments, 32 bytes of shadow space the caller reserves, and any struct whose size isn't 1, 2, 4, or 8 bytes passed as
a pointer to a caller-made copy. Code that is correct for one target is correct for the other only if it goes through
the declared types. Hand-written assembly or hard-coded offsets are where cross-platform FFI bugs live.
`extern "system"` differs from `"C"` only on 32-bit Windows (stdcall). That's why the JNI binding in Chapter 16.3 uses
it: `JNICALL` is stdcall there.

**The last rung: the kernel's own ABI.** Listing `ch01-10-ladder.rs` asks for the process id three ways:

```rust
// Rust -> ABI -> C -> OS: the same question asked at three levels.
fn main() {
    let from_std = std::process::id(); // std: a safe wrapper over the C library on Linux
    // SAFETY: getpid has no preconditions (the libc crate declares every foreign function unsafe).
    let from_libc = unsafe { libc::getpid() }; // the C library's wrapper, called through the C ABI
    // The kernel's own convention: syscall number 39 on x86-64 Linux, via libc's generic entry point.
    // SAFETY: SYS_getpid takes no arguments and has no preconditions.
    let from_kernel = unsafe { libc::syscall(libc::SYS_getpid) };
    println!("std::process::id() = {from_std}");
    println!("libc::getpid()      = {from_libc}");
    println!("syscall(SYS_getpid={}) = {from_kernel}", libc::SYS_getpid);
    println!("all equal: {}", from_std as i64 == from_libc as i64 && from_libc as i64 == from_kernel);
}
```

```text
std::process::id() = 15
libc::getpid()      = 15
syscall(SYS_getpid=39) = 15
all equal: true
```

The kernel's convention is yet another ABI [OS]: the syscall number in `rax`, arguments in `rdi`, `rsi`, `rdx`, **`r10`**
(not `rcx`, which the `syscall` instruction overwrites), `r8`, `r9`, and errors returned as `-errno` in `rax`. The C
library translates that into C's convention: `-1` plus a thread-local `errno`. Chapter 16.2 turns that pair into
`io::Error`. Part XIX follows the call all the way into the kernel.

---

## Pass 3 · Architect level — *Designing types for a boundary*

### 7. Trade-offs

**Which representation, where:**

| Representation | Layout promise | FFI-safe | Use it for |
|---|---|---|---|
| default | none | no | everything that never crosses a boundary (most code) |
| `repr(C)` struct | declaration order, C alignment rules | yes, if every field is | structs shared with C/Java; order fields largest-first *before* the first release |
| `repr(transparent)` | identical to the one non-ZST field, including the calling convention | yes, if the field is | typed handles, units (`Cents(i64)`), `DecisionCode(u32)` |
| `repr(u8/u32)` fieldless enum | tag of that integer type | yes, **for values Rust produces** | outgoing status values |
| `repr(C, u32)` / `repr(u32)` enum with data | RFC 2195's struct-of-union / union-of-structs | yes, **for values Rust produces** | outgoing tagged results, when the other side can mirror the union |
| integer code + `TryFrom` | a plain integer | yes, for **any** value | anything that **arrives** from outside |
| `Option<&T>` / `Option<Box<T>>` / `Option<NonNull<T>>` | a nullable pointer | yes | optional pointers, handles, callbacks |

The asymmetry in the enum rows is the design rule from §3: **Rust may send enums; it must receive integers.** Rust
controls every value it produces, so an enum it sends is always valid. It controls nothing it receives.

**Panics at the boundary:**

| Choice | What the caller sees | Choose when |
|---|---|---|
| `extern "C"` + `catch_unwind` inside | a return code; the process continues | the caller is C or the JVM: the fraud library's rule (Chapter 8.3) |
| `extern "C"` without it | the process aborts on a panic ("panic in a function that cannot unwind") | a panic is truly unrecoverable and you accept losing the host process |
| `extern "C-unwind"` | the unwind continues into the caller's frames | the caller is Rust or C++ built with exceptions and wants the panic; never a JVM |
| `panic = "abort"` | abort, with no landing pads at all | CLIs; never a library loaded into someone else's process |

> **Why not just use the Rust ABI between two Rust libraries?** It works only when both were built by the same
> compiler with compatible flags, and nothing checks that (Chapter 6.5). Two Rust crates talking through `extern "C"`
> pay for it with manual layouts and `unsafe`. That's worth it only at a real binary boundary: a plugin, a separately
> shipped library, a different language.

### 8. Java comparison

Java has three generations of answers to "call code the JVM didn't compile":

| | JNI (JDK 1.1) | FFM (JEP 454, final in JDK 22) | Rust |
|---|---|---|---|
| Who writes the glue | you, in C: `Java_pkg_Class_method(JNIEnv*, jclass, ...)` | nobody: a `MethodHandle` from a `FunctionDescriptor` | nobody: an `unsafe extern` declaration |
| Who knows the calling convention | the C compiler | the JVM's `Linker`, from the descriptor | rustc, from `extern "C"` |
| Struct layout | C structs, invisible to Java | `MemoryLayout.structLayout(...)`, padding written explicitly | `#[repr(C)]` |
| Unsigned 64-bit | no (`jlong` is signed) | no (`JAVA_LONG`); reinterpret the bits | `u64` |
| Where the contract is checked | the C compiler checks the glue | at run time: layout alignment, segment bounds, liveness | at compile time: types and lints; layout by your assertions |

A `MeridianTxn` in FFM, not verified here (requires JDK 22+; run with `java --enable-native-access=ALL-UNNAMED`):

```java
static final StructLayout MERIDIAN_TXN = MemoryLayout.structLayout(
        ValueLayout.JAVA_BYTE.withName("flags"),
        MemoryLayout.paddingLayout(7),
        ValueLayout.JAVA_LONG.withName("amount_cents"),
        ValueLayout.JAVA_SHORT.withName("currency"),
        MemoryLayout.paddingLayout(2),
        ValueLayout.JAVA_INT.withName("merchant_id"));
// MERIDIAN_TXN.byteSize() == 24; structLayout rejects members placed at misaligned offsets
```

That's `repr(C)` written by hand, with the padding visible. Leave out `paddingLayout(7)` and `structLayout` throws,
because the `long` would land at offset 1. Java is stricter than C here, and it's a good habit to copy: Meridian's
header comments name every padding gap.

> **Analogy limit.** FFM's `Linker` applies the same System V rules rustc does, so a `FunctionDescriptor` and an
> `extern "C"` signature describe the same registers. But FFM checks **memory** at run time (a `MemorySegment` knows its
> size and whether its arena is still open) and checks nothing about the **function**: declare `JAVA_INT` where the C
> side takes `size_t`, and the upper 32 bits of the register are whatever was there. Rust checks neither at run time. It
> checks types at compile time and leaves the declaration's truth to you. Neither side can see the other's source, which
> is why the header exists.

### 9. Production scenario

**Meridian's ABI contract for the fraud library.** When the fraud library moved from JNI to FFM (Chapter 9.2), the
team wrote down how the boundary is governed. It's short:

1. **One source of truth:** `include/meridian_fraud.h`, generated from the Rust crate by `cbindgen` (Chapter 16.3) and
   committed, so every change shows up in review as a header diff. The Java bindings are generated from that header
   by `jextract`.
2. **Every boundary type is `repr(C)` or `repr(transparent)`,** with an `offset_of!` assertion for every field and a
   `size_of` assertion for the whole struct, in a `const` block next to the definition (§3).
3. **Incoming enums are integers.** `DecisionCode(u32)`, never `DecisionC`, in any parameter or any struct Java
   writes. Outgoing results may use `repr(C, u32)`, and the header spells out the union.
4. **A version handshake at load.** The library exports `meridian_abi_version()`. The Java wrapper calls it once when it
   loads the library and refuses to start if it isn't the version its bindings were generated for. Any change to a
   boundary type's layout bumps the version. That's what the `E0080` message in §3 reminds the author to do.
5. **`extern "C"` plus `catch_unwind` on every export** (Chapter 8.3), and `panic = "unwind"` in the release profile so
   there's something to catch.

The rules are deliberately boring. Their job is to make a layout change *loud*: a failing `const` assertion in Rust, a
header diff in review, and a version mismatch at startup, instead of a silent reinterpretation in production.

### 10. Failure scenario

**The reorder that saved eight bytes.** In spring 2026, before rule 2 existed, an engineer noticed that `MeridianTxn`
had 8 bytes of padding and reordered its fields largest-first: exactly `MeridianTxnV2` from §3, 16 bytes instead of 24.
Every Rust test passed, since Rust code reads fields by name. The Java side still wrote the old layout.

Nothing crashed. Every field is a plain integer, so every bit pattern is valid and the library read *some* number from
every offset. The library read `amount_cents` from the bytes where Java had written `flags` and padding, and
`merchant_id` from the low half of the amount. The canary pod scored transactions against the wrong merchants' feature
histories until the score-distribution alert fired. That's the nature of a layout mismatch: the contract is broken, so
what the other side reads is meaningless (Chapter 15.1 explains why the compiler gives no guarantee at all once a
declaration is wrong), and the failure is a **plausible wrong answer**, not a crash.

The fix was the governance in §9. The part that actually catches this class of bug is small: with the `const` block in
place, the reordering PR fails to compile until its author changes the assertions, which forces the version bump,
which makes the Java wrapper refuse the new library until its bindings are regenerated. Three independent checks, all
of them cheap.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVI).*

1. Name the four parts of an ABI. For each, give the Rust attribute or keyword that pins it.
2. Why does Rust have no stable ABI, and what does it gain from that? What does it cost at a plugin boundary?
3. `extern "C"` on x86-64 Linux and on x86-64 Windows: same syntax, different contract. Name three differences.
4. Walk through the `#[rustc_abi(debug)]` output for `Point` and `Triple` under both conventions. Why is `Triple`
   `OnStack` for C and `Pointer` for Rust, and why could `call_rust_sum` pass its own pointer without copying?
5. Which `Option<T>` types are guaranteed to be a nullable C pointer? Why isn't `Option<u32>`?
6. `repr(C, u32)` versus `repr(u32)` on `enum { Allow, Review(u8), Block(u64) }`: where does `Review`'s byte live in each,
   and why?
7. Why may Rust send a `repr(u32)` enum to C but not receive one? What do you use instead?
8. What does rustc generate for an `extern "C"` function that can panic? What changed in Rust 1.81?
9. When is `extern "C-unwind"` the right choice, and why is it never right for a function the JVM calls?
10. A C library's struct gains a field *in its padding*, so size and every existing offset stay the same. Is that an
    ABI change? Which of §9's checks would catch it?

### 12. Exercises

- **Beginner.** For each type, give its size and alignment under `repr(C)` on x86-64, then check with `size_of`,
  `align_of`, and `offset_of!`: `{ u8, u32, u8 }`, `{ u32, u8, u8 }`, `{ u16, f64, u16 }`, `{ [u8; 3], u64 }`.
- **Intermediate.** Write `const` assertions for `MeridianDecision` (every field and the size). Then change `risk` to
  `u16` and to `u32`. Which changes break an assertion, and which don't but still break the contract?
- **Advanced.** Extend listing `ch01-07-abi-dump.rs` with a 12-byte `repr(C)` struct `{ f32, f32, u32 }`, a 16-byte
  `{ f64, u64 }`, and a function returning `Triple`. Predict each `mode` from the System V classification before running
  it on nightly, then explain any surprise.
- **Systems.** Emit release assembly for a caller of `c_sum` and `rust_sum` whose argument is a local `Triple` built
  from three parameters, not a reference. Does the Rust convention still avoid the copy? Why or why not?
- **Architecture.** Design the version handshake of §9 in detail: what the library exports (one version number, or a
  table of struct sizes and offsets?), what the Java side checks at load, and what happens during a rolling deploy when
  old and new library versions coexist in one fleet.

### 13. Debugging exercise

A teammate adds an optional override to the scoring call, in two variants while they decide how to pass the request
(listing `ch01-11-lint-blind-spot.rs`, excerpt):

```rust,ignore
#[repr(u8)]
#[derive(Clone, Copy)]
pub enum ScoreMode {
    Normal = 0,
    Strict = 1,
}

#[repr(C)]
pub struct ScoreRequest {
    pub amount_cents: i64,
    pub mode: ScoreMode,
    pub override_block_at: Option<u32>,
}

#[unsafe(no_mangle)]
pub extern "C" fn meridian_score_req(req: ScoreRequest, out: &mut i32) -> i32 {
    *out = (req.amount_cents / 100) as i32;
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn meridian_score_req_ref(req: &ScoreRequest, out: &mut i32) -> i32 {
    *out = (req.amount_cents / 100) as i32;
    0
}
```

The crate compiles with exactly one warning (verified), which CI doesn't treat as an error:

```text
warning: `extern` fn uses type `Option<u32>`, which is not FFI-safe
  --> src/main.rs:22:43
   |
22 | pub extern "C" fn meridian_score_req(req: ScoreRequest, out: &mut i32) -> i32 {
   |                                           ^^^^^^^^^^^^ not FFI-safe
```

1. Which field is the warning really about, and what exactly is unspecified about it?
2. Why is there no warning for `meridian_score_req_ref`, which passes the same struct? What does that tell you about
   relying on this lint for structs passed by pointer, which is how most C APIs pass structs?
3. Which field does the lint **never** warn about, in either function, that is still wrong for a struct Java writes?
   What happens when a Java client sends `mode = 2`? Rewrite the struct and the signature so that every value Java can
   write is valid, and say which checks now live in Rust code rather than in types.

### 14. Design exercise

**A stable plugin ABI for fraud rules.** Chapter 6.5 built `FfiRule`, a hand-written C-ABI vtable, and deferred loading
to this Part. The analytics team wants to ship rule plugins as separately compiled `cdylib`s that the fraud library
loads at startup (Chapter 16.3 shows the loading). Design the ABI:

- the entry symbol every plugin exports, and the struct it returns (vtable, version, name);
- how host and plugin agree on the version, and what the host does with a plugin built for an older version;
- which types may appear in the vtable's function signatures, and how a rule reports "no opinion" versus a score;
- what the host does if a plugin's function panics, given that the host and the plugin may have been built with
  different Rust versions (and so different panic runtimes).

State which parts of your design the compiler checks, which a `const` assertion checks, and which only a load-time
handshake can check.
