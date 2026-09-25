# Chapter 5.2 — Type Layout: Size, Alignment, Padding, and Niches

> **Where this sits:** Part V · Types as Architecture · chapter 2 of 4
> **Prerequisites:** Chapter 1.2 (`Option<u64>` is 16 bytes), Chapter 2.3 (validity invariants), Chapter 2.6 (default
> layout reorders fields; enum tags; the `Message` and `MessageBoxed` sizes), Chapter 5.1.
> **After this chapter you can:** predict the size, alignment, and field offsets of a struct or enum; choose between the
> default representation and `repr(C)`, `repr(transparent)`, `repr(u8)`, `repr(packed)`, and `repr(align)`; say which
> niche optimizations are *guaranteed* and which are compiler behavior; count niche values to predict enum sizes;
> read the assembly a niche produces; and keep padding bytes out of files and network packets.

---

## Pass 1 · User level — *Where the bytes go*

### 1. Problem

Chapter 2.6 established the basics with verified numbers. rustc reorders fields (`{u8, u64, u8}` is 16 bytes by
default and 24 as `repr(C)`), enums carry a tag unless a niche hides it (`Option<Shape>` = 24, the same as `Shape`), and
one huge variant makes every value huge (`Message` = 4,097 bytes, boxed = 8).

An architect needs the rest of the model, because layout decisions show up far from the code:

- **Memory budgets.** Ten million resting orders at 24 or 32 bytes each is an 80 MB difference, and a cache-line
  difference on every scan.
- **Wire and disk formats.** A struct written "as bytes" carries its padding with it. Padding bytes are uninitialized
  memory.
- **Concurrency.** Two counters on one cache line ping-pong between cores, even though no data is shared.
- **Guarantees.** Some niche optimizations are promised by the language. Others are things the compiler happens to do
  today. Unsafe code and FFI may rely only on the first kind.

### 2. Mental model

**Every type has three layout facts** [LANG]:

1. **Alignment** (a power of two): every value of the type lives at an address that's a multiple of it.
2. **Size**: a multiple of the alignment, so that in an array, element *i+1* is aligned too. Arrays have no gaps
   between elements; any padding lives *inside* the element type.
3. **Validity**: the set of bit patterns that are legal values. `bool` allows 2 of 256, `char` allows about 1.1M of
   2³², `&T` excludes 0, and `NonZeroU32` excludes 0. The illegal patterns are **niches**, and an enum can use them to
   encode its tag without extra bytes.

**Who decides the layout** depends on the `repr`:

```text
 default (repr(Rust))   rustc chooses field order, padding, and tag encoding. UNSPECIFIED: may change between
                        compiler versions, and between two generic instantiations of the same struct.
 repr(C)                fields in declaration order, each at the next offset aligned for it; size rounded up to
                        alignment. What C does. Required for FFI and for reading raw bytes of known layout.
 repr(transparent)      exactly the layout AND calling convention of the single non-zero-sized field.
 repr(u8) / repr(i32)…  on an enum: the tag is exactly that integer type (and a data-carrying enum gets a
                        defined, C-like layout: RFC 2195).
 repr(packed)           alignment 1 (or N): no padding; fields may be misaligned.
 repr(align(N))         raise the alignment to at least N (and therefore the size to a multiple of N).
```

A good default is to let rustc choose, and pin the layout only where bytes cross a boundary: FFI, files, the network,
memory-mapped structures, and `unsafe` code that casts pointers.

### 3. Rust code

**Sizes, alignments, offsets** (listing `ch02-01-layout-table.rs`, verified). The same five fields, default and
`repr(C)`:

```rust
#![allow(dead_code)]
use std::mem::{align_of, offset_of, size_of};

struct Order {
    side: u8,
    qty: u32,
    id: u64,
    price_cents: i64,
    tif: u8,
}

#[repr(C)]
struct OrderC {
    side: u8,
    qty: u32,
    id: u64,
    price_cents: i64,
    tif: u8,
}

fn main() {
    println!("{:<18} size align", "type");
    println!("{:<18} {:>4} {:>5}", "u8", size_of::<u8>(), align_of::<u8>());
    println!("{:<18} {:>4} {:>5}", "u32", size_of::<u32>(), align_of::<u32>());
    println!("{:<18} {:>4} {:>5}", "u64", size_of::<u64>(), align_of::<u64>());
    println!("{:<18} {:>4} {:>5}", "u128", size_of::<u128>(), align_of::<u128>());
    println!("{:<18} {:>4} {:>5}", "(u8, u64)", size_of::<(u8, u64)>(), align_of::<(u8, u64)>());
    println!("{:<18} {:>4} {:>5}", "[u16; 3]", size_of::<[u16; 3]>(), align_of::<[u16; 3]>());
    println!("{:<18} {:>4} {:>5}", "Order (Rust)", size_of::<Order>(), align_of::<Order>());
    println!("{:<18} {:>4} {:>5}", "OrderC (repr C)", size_of::<OrderC>(), align_of::<OrderC>());

    println!("\nfield offsets        Order   OrderC");
    println!("side                 {:>5}   {:>6}", offset_of!(Order, side), offset_of!(OrderC, side));
    println!("qty                  {:>5}   {:>6}", offset_of!(Order, qty), offset_of!(OrderC, qty));
    println!("id                   {:>5}   {:>6}", offset_of!(Order, id), offset_of!(OrderC, id));
    println!("price_cents          {:>5}   {:>6}", offset_of!(Order, price_cents), offset_of!(OrderC, price_cents));
    println!("tif                  {:>5}   {:>6}", offset_of!(Order, tif), offset_of!(OrderC, tif));

    let n = 10_000_000usize;
    println!(
        "\n10M orders: {} MB (Rust layout) vs {} MB (repr C)",
        n * size_of::<Order>() / 1_000_000,
        n * size_of::<OrderC>() / 1_000_000
    );
}
```

```text
type               size align
u8                    1     1
u32                   4     4
u64                   8     8
u128                 16    16
(u8, u64)            16     8
[u16; 3]              6     2
Order (Rust)         24     8
OrderC (repr C)      32     8

field offsets        Order   OrderC
side                    20        0
qty                     16        4
id                       0        8
price_cents              8       16
tif                     21       24

10M orders: 240 MB (Rust layout) vs 320 MB (repr C)
```

Drawn out:

```text
 Order (default)   0        8        16    20 21 22  24
                   │ id     │ price  │ qty │s │t │pad│         22 bytes of data + 2 of padding

 OrderC (repr C)   0  1   4     8        16       24 25        32
                   │s │pad│ qty │ id     │ price  │t │ pad     │   22 bytes of data + 10 of padding
```

[RUSTC] The default layout sorted the fields by alignment, largest first. That's the current strategy, not a promise
[LANG]. `u128` has alignment 16 on x86-64 since Rust 1.77–1.78, a [VERSION] change made to match C's ABI. It used to be 8.
`offset_of!` has been stable since 1.77.

**Niches, counted** (listing `ch02-02-niches.rs`, verified):

```text
u32                                          4
Option<u32>                                  8
NonZeroU32                                   4
Option<NonZeroU32>                           4
Option<NonZeroU64>                           8
Option<Option<NonZeroU64>>                  16
char / Option<char>                          4
Option<Option<Option<bool>>>                 1
Tri / Option<Tri>                            1
Shape / Option<Shape>                       24
Payload                                     16
Msg { Big(Box, u64), Small(u32), Empty }    24
Msg2 { Big(Box, u64), Small(u32) }          16
TwoU64 { A(u64), B(u64) }                   16
Result<u32, NonZeroU32>                      8
Result<(), Box<str>>                        16
Option<Vec<u8>>                             24
Option<String> vs String                     0
```

(`Tri` is a three-variant fieldless enum; `Payload` is `enum { Small(u8), Big(Box<[u8; 1024]>) }`; `Shape` is Chapter
2.6's.) Every row follows from counting spare bit patterns:

- **`NonZeroU64` has exactly one niche** (0). `Option<NonZeroU64>` uses it, so `Option<Option<NonZeroU64>>` has nothing
  left and needs a tag: 16 bytes. Niches get **used up**.
- **`bool` has 254 niches**, so three nested `Option`s still fit in one byte. `char` has more than four billion.
- **`Msg` vs `Msg2`.** A `Box` has one niche, null. `Msg2` has one other variant to encode, and its `u32` fits in the
  bytes *beside* the `Box`, so rustc hides the tag in the null pointer: 16 bytes. `Msg` has *two* other variants, and one
  niche value can't name both, so it's tagged: 24.
- **`Payload` is 16, not 8**, even though `Box` has a niche. The `Small(u8)` payload would have to live somewhere other
  than the niche field, but the `Box` *is* the whole variant, so there's no room. Multi-variant niche filling needs space
  around the niche.
- **`Option<Vec<u8>>` and `Option<String>` cost nothing extra.** Their pointers are non-null.

**Enums with an explicit wire encoding** (listing `ch02-09-repr-u8.rs`, verified, and clean under Miri):

```rust
use std::mem::size_of;

/// Wire encoding is part of the contract: explicit discriminants, explicit width.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Bid = 1,
    Ask = 2,
}

#[derive(Debug, PartialEq)]
struct BadSide(u8);

impl TryFrom<u8> for Side {
    type Error = BadSide;
    fn try_from(b: u8) -> Result<Side, BadSide> {
        match b {
            1 => Ok(Side::Bid),
            2 => Ok(Side::Ask),
            other => Err(BadSide(other)),
        }
    }
}

fn main() {
    println!("Side as u8: Bid={} Ask={}", Side::Bid as u8, Side::Ask as u8);
    for b in [1u8, 2, 0, 7] {
        println!("byte {b} -> {:?}", Side::try_from(b));
    }
    println!("size_of Side={} Option<Side>={}", size_of::<Side>(), size_of::<Option<Side>>());
    // What does rustc store for None? One of the byte values Side can never hold.
    let none: Option<Side> = None;
    // SAFETY: Option<Side> is 1 byte with no padding, so reading it as u8 is sound.
    let raw: u8 = unsafe { std::mem::transmute(none) };
    println!("bits of None::<Side> = {raw}");
    println!(
        "discriminant(Bid) == discriminant(Bid)? {}",
        std::mem::discriminant(&Side::Bid) == std::mem::discriminant(&Side::Bid)
    );
}
```

```text
Side as u8: Bid=1 Ask=2
byte 1 -> Ok(Bid)
byte 2 -> Ok(Ask)
byte 0 -> Err(BadSide(0))
byte 7 -> Err(BadSide(7))
size_of Side=1 Option<Side>=1
bits of None::<Side> = 0
discriminant(Bid) == discriminant(Bid)? true
```

`as u8` goes *out* safely. Coming *in* has to be a parse (`TryFrom<u8>`), because most byte values aren't a `Side`. The
compiler also found a niche: `None::<Side>` is stored as 0, a byte `Side` never uses. (Which spare value it picks is
[RUSTC] behavior. Don't depend on it.)

**Packed structs and references** (listing `ch02-04-packed-ref.rs`, verified):

```rust,compile_fail
#[repr(C, packed)]
struct WireHeader {
    kind: u8,
    length: u32, // at offset 1: NOT 4-byte aligned
}

fn main() {
    let h = WireHeader { kind: 1, length: 512 };
    let len_ref: &u32 = &h.length; // a &u32 must be aligned; this one can't be
    println!("{}", len_ref);
}
```

```text
error[E0793]: reference to field of packed struct is unaligned
10 |     let len_ref: &u32 = &h.length; // a &u32 must be aligned; this one can't be
   |                         ^^^^^^^^^
   = note: this struct is 1-byte aligned, but the type of this field may require higher alignment
   = note: creating a misaligned reference is undefined behavior (even if that reference is never dereferenced)
   = help: copy the field contents to a local variable, or replace the reference with a raw pointer and use
           `read_unaligned`/`write_unaligned` (loads and stores via `*p` must be properly aligned even when using raw pointers)
```

A reference promises alignment (Chapter 3.3's `align 4 dereferenceable(4)` in the LLVM IR), and a packed field can't
keep that promise. [VERSION] This was a warning-level lint for years before it became a hard error. The fix the error
suggests (listing `ch02-05-packed-fixed.rs`, verified, and clean under Miri):

```rust,ignore
    let length = h.length; // copy out by value: the compiler emits an unaligned load
    let p = std::ptr::addr_of!(h.length);
    // SAFETY: `p` points to a live, initialized u32 inside `h`; read_unaligned has no alignment requirement.
    let via_ptr = unsafe { p.read_unaligned() };
```

```text
size_of::<WireHeader>() = 5, kind=1, length=512, via_ptr=512
```

---

## Pass 2 · Systems level — *How rustc computes a layout, and what the CPU sees*

### 4. Under the hood

[RUSTC] Layout is computed on demand by the `layout_of` query, which runs once per concrete type, after generics are
substituted. `Order` has one layout, and `Vec<Order>` and `Vec<u8>` have different ones. The result records:

- **`FieldsShape`**: the offset of every field (and for the default repr, the order rustc chose).
- **`Variants`**: `Single` for structs, or `Multiple` for enums, with a **tag encoding**. `Direct` means a separate tag
  field holds the discriminant. `Niche` means one variant (the *untagged* one) is stored as-is, and the others are
  encoded as particular invalid values of one of its fields.
- **The backend representation**: `Scalar` (fits one register, like `u64` or `&T` or `Option<&T>`), `ScalarPair` (two
  registers, like `&[T]` or `Option<u64>`), or memory. This decides how values are passed to functions, and it's
  visible in the assembly in §5.

These names change between compiler versions (this is internals). The concepts don't.

**Niche selection** [RUSTC]: rustc looks for the field with the *largest* range of invalid values among the candidate
variants, checks that all the other variants fit in the bytes that don't overlap it, and picks the encoding (tagged or
niche) that gives the smaller type. When sizes tie it prefers the larger niche left over, so an enclosing `Option` can
still nest. `Msg2` above is the multi-variant case: the `Small(u32)` payload shares bytes with the `u64` field while the
null `Box` pointer says "this is `Small`." rustc gained that ability in 2022. Earlier versions niche-filled only when
every other variant carried no data.

**Guaranteed vs observed.** This distinction matters for FFI and `unsafe` code:

| Layout fact | Status |
|---|---|
| `Option<&T>`, `Option<&mut T>`, `Option<Box<T>>`, `Option<NonNull<T>>`, `Option<NonZeroU*>`, `Option<fn()>` have the size, alignment, and ABI of the inner type; for these, the all-zero bit pattern is `None` | **[LANG] guaranteed** (std's `Option` docs, "Representation"), including through `repr(transparent)` wrappers |
| `repr(C)` field order and offsets; `repr(transparent)` identity; `repr(u8)` tag type | **[LANG] guaranteed** |
| `Option<bool>` = 1, `Option<char>` = 4, `Option<Shape>` = 24, `Msg2` = 16, field reordering | **[RUSTC] observed**: stable in practice, not promised |
| `None::<Side>` stored as 0 | **[RUSTC]**: an implementation choice |

The guaranteed row is what makes `Option<&T>` usable as a nullable pointer in FFI (Part XVI). Nothing else in the table
should cross an FFI boundary or be relied on by `transmute`. To shake out accidental dependence on the default layout,
nightly has `-Z randomize-layout`. To see what rustc decided for every type in a crate, run
`cargo +nightly rustc -- -Zprint-type-sizes` locally. (Not verified here: the Playground can't pass that flag.)

**Invalid bit patterns are undefined behavior, not "just a weird value."** Transmuting a byte that isn't a valid `Side`
tag (listing `ch02-10-invalid-tag.rs`, verified under Miri):

```rust,ignore
    let wire_byte: u8 = 7; // a corrupted or hostile byte from the network
    let side: Side = unsafe { std::mem::transmute(wire_byte) };
```

```text
error: Undefined Behavior: constructing invalid value of type Side: at .<enum-tag>, encountered 0x07, but expected a valid enum tag
13 |     let side: Side = unsafe { std::mem::transmute(wire_byte) };
   |                               ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ Undefined Behavior occurred here
```

This is where niches and safety meet. The compiler *uses* the invalid patterns: 0 already means `None::<Side>`, and a
`match` on `Side` has an `unreachable` `otherwise` edge (Chapter 5.1 §4). A `Side` holding 7 would break both. That's
why `TryFrom<u8>` exists, and why "parse, don't validate" is also a safety rule at byte boundaries.

### 5. Memory

**What a niche looks like in machine code** (listing `ch02-03-niche-asm.rs`, `emit.ps1 -Target asm -Mode release`,
rustc 1.98.1, trimmed):

```rust,ignore
#[inline(never)]
pub fn or_max_niche(x: Option<NonZeroU64>) -> u64 { x.map_or(u64::MAX, NonZeroU64::get) }
#[inline(never)]
pub fn or_max_tagged(x: Option<u64>) -> u64 { x.unwrap_or(u64::MAX) }
#[inline(never)]
pub fn deref_or_max(x: Option<&u64>) -> u64 { x.copied().unwrap_or(u64::MAX) }
```

```text
playground::or_max_niche:              ; ONE register in: rdi = the value, 0 means None
	xor	eax, eax
	cmp	rdi, 1                     ; carry flag set iff rdi == 0 (None)
	sbb	rax, rax                   ; rax = None ? -1 : 0
	or	rax, rdi                   ; None → u64::MAX, Some(v) → v
	ret

playground::or_max_tagged:             ; TWO registers in: dil = tag, rsi = payload
	and	dil, 1                     ; normalize the tag
	xor	eax, eax
	cmp	dil, 1
	sbb	rax, rax
	or	rax, rsi
	ret

playground::deref_or_max:              ; Option<&u64>: the None check IS the null check
	test	rdi, rdi
	je	.LBB0_1
	mov	rax, qword ptr [rdi]
	ret
.LBB0_1:
	mov	rax, -1
	ret
```

Both versions compile to branch-free code for the unwrap. The difference is the **ABI**: `Option<NonZeroU64>` is a
`Scalar` passed in one register, and `Option<u64>` is a `ScalarPair` passed in two. `Option<&u64>` is a plain pointer,
and checking for `None` is the same `test rdi, rdi` a C programmer writes for `NULL`. The difference is that Rust won't
let you skip it.

**In memory, the niche halves the footprint, and the loop changes.** Summing a slice of each (same listing):

```text
playground::niche_in_memory:           ; &[Option<NonZeroU64>]: 8 bytes per element
.LBB3_6:
	movdqu	xmm2, xmmword ptr [rdi + rdx]
	paddq	xmm1, xmm2                 ; just add every word: None is 0, and adding 0 is harmless
	movdqu	xmm2, xmmword ptr [rdi + rdx + 16]
	paddq	xmm0, xmm2
	add	rdx, 32
	cmp	rax, rdx
	jne	.LBB3_6

playground::tagged_in_memory:          ; &[Option<u64>]: 16 bytes per element (tag + pad + value)
.LBB4_6:
	movdqu	xmm2, xmmword ptr [rdi + rdx]        ; load 4 elements = 64 bytes
	movdqu	xmm3, xmmword ptr [rdi + rdx + 16]
	movdqu	xmm4, xmmword ptr [rdi + rdx + 32]
	movdqu	xmm5, xmmword ptr [rdi + rdx + 48]
	...                                          ; shuffle tags and values apart,
	pslld	xmm2, 31                             ; turn each tag into an all-ones/all-zeros mask,
	psrad	xmm2, 31
	pand	xmm2, xmm6                           ; and zero the payloads of None elements
	paddq	xmm1, xmm2
	...
```

LLVM noticed that `None` is the bit pattern 0, and that skipping a `None` means adding 0. So the niche version is a
plain vectorized sum over 8-byte words, with no checks at all. The tagged version reads twice the bytes per element and
spends instructions separating tags from values. Which one is faster in practice depends on memory bandwidth and cache
residency. Predicted from mechanism: the niche version wins on large, uncached slices, by up to 2× from bytes alone.
Exercise 12 (systems) measures it.

**Padding is part of the value's bytes, but not part of its data.** Padding bytes have no defined value. Reading them
as `u8` is undefined behavior, which is why the ecosystem's "view this struct as bytes" crates refuse padded types
([LIB] `bytemuck`, listing `ch02-06-pod-padding.rs`, verified):

```rust,compile_fail
use bytemuck::{Pod, Zeroable};

// A record we want to write to disk "as bytes". It has 7 bytes of padding after `side`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TradeRecord {
    id: u64,
    price_cents: i64,
    side: u8,
}

fn main() {
    let t = TradeRecord { id: 1, price_cents: 1999, side: 1 };
    println!("{:?}", bytemuck::bytes_of(&t));
}
```

```text
error[E0080]: evaluation panicked: derive(Pod) was applied to a type with padding
6 | #[derive(Clone, Copy, Pod, Zeroable)]
  |                       ^^^ evaluation of `_` failed here
```

The derive expands to a compile-time assertion (a `const` evaluation) that the struct's size equals the sum of its
fields' sizes. The fix makes the padding explicit and always zero (listing `ch02-07-pod-explicit.rs`, verified):

```rust,ignore
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct TradeRecord {
    id: u64,
    price_cents: i64,
    side: u8,
    _pad: [u8; 7],
}
```

```text
size = 24, bytes = [1, 0, 0, 0, 0, 0, 0, 0, 207, 7, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]
round trip: TradeRecord { id: 1, price_cents: 1999, side: 1, _pad: [0, 0, 0, 0, 0, 0, 0] }
```

Every byte is now defined. 1999 = 0x07CF, which is `207, 7` in little-endian. Note that this format is still
**endianness-dependent**. Chapter 23.5 covers portable zero-copy formats.

### 6. CPU / OS

**Why alignment exists.** [CPU] Memory moves between RAM and cache in **cache lines**: 64 bytes on current x86-64 and
most ARM server cores, 128 on Apple's M-series. An aligned 8-byte value never straddles two lines. A misaligned one
can, and then one load becomes two cache accesses. x86-64 tolerates misaligned ordinary loads, at a small cost when a
line or page boundary is crossed. **Atomic** operations need natural alignment. A locked instruction that splits a cache
line is either very slow ("split lock", which Linux can detect and penalize) or, on many other architectures, faults.
That's why atomics and references demand alignment, and why `repr(packed)` is for wire formats, never for data
structures that threads share.

**False sharing.** Chapter 3.3 compared the borrow rules to cache coherence: a line is either shared by readers or owned
by one writer. Two *different* counters on the *same* line, each written by a different core, make that line bounce
between cores on every write, even though no data is shared. The fix is layout (listing `ch02-08-cache-aligned.rs`,
verified):

```rust,ignore
/// One value per cache line, so values updated by different cores never share a line.
#[repr(align(64))]
struct CachePadded<T>(T);

/// Eight counters packed together, starting on a line boundary.
#[repr(align(64))]
struct Packed([AtomicU64; 8]);
```

```text
AtomicU64:              size   8, align  8
CachePadded<AtomicU64>: size  64, align 64
Packed (8 counters):    size  64
[CachePadded<_>; 8]:    size 512
packed: counter i lives on line [0, 0, 0, 0, 0, 0, 0, 0] (relative to the array)
padded: counter i lives on line [0, 1, 2, 3, 4, 5, 6, 7]
```

`repr(align(64))` raised the size to 64 as well, so each array element starts a new line. This trades memory (512 bytes
instead of 64) for independence. [LIB] `crossbeam_utils::CachePadded` uses 128 bytes on x86-64 and aarch64, because
Intel's spatial prefetcher pulls cache lines in pairs. How much false sharing costs is measured in Chapter 20.5. Here the
point is that it's a layout decision, and Rust lets you state it in the type.

**Density is a performance feature.** At 24 bytes per order, a 64-byte line holds 2⅔ orders. At 32 bytes, it holds 2.
Scanning ten million orders touches 240 MB instead of 320 MB: fewer lines, fewer TLB entries, and less memory bandwidth.
The speedup is predicted from mechanism and bounded by 1.33×. Measure it before quoting it (Chapter 20.1).

---

## Pass 3 · Architect level — *Layout as a design decision*

### 7. Trade-offs

| Representation | Guarantees | Costs | Use for |
|---|---|---|---|
| default | nothing about order; smallest padding in practice; niches | can't be read as raw bytes or shared with C | everything internal (the default for a reason) |
| `repr(C)` | declaration order, C-compatible | padding you must order fields yourself to avoid | FFI, memory-mapped files, raw wire structs |
| `repr(transparent)` | identical layout and ABI to the one field | exactly one non-ZST field | newtypes crossing FFI; `unsafe` casts between `&T` and `&Wrapper` |
| `repr(u8)` / `repr(i32)` on enums | tag type and values fixed | may block some niche tricks | wire enums, FFI enums, stable discriminants in storage |
| `repr(packed)` | no padding | no references to fields (E0793); unaligned loads | parsing fixed binary headers (prefer explicit byte parsing) |
| `repr(align(N))` | alignment ≥ N | size rounds up to N | per-core counters, lock-free slots, SIMD buffers |

**Enum sizing rules of thumb:**

- **The largest variant sets the size of every value.** Chapter 2.6's `Message::Data([u8; 4096])` made a one-byte
  `Ping` cost 4 KiB. Box the rare large variant (clippy's `large_enum_variant` lint flags it) and pay an allocation only
  on that path.
- **Count niche values before nesting `Option`s.** `Option<Option<NonZeroU64>>` doubles the size (verified). Often the
  outer state can be expressed some other way, as the debugging exercise shows.
- **A field-less enum with fewer than 256 variants is one byte, and `Option` of it is still one byte.** Prefer an enum
  to a `u8` code: same size, and invalid codes are unrepresentable.

**Hot/cold splitting** is the struct-level version of the same idea. If a scan reads only `id`, `price`, and `qty`, the
40 bytes of audit metadata in the same struct ride along into cache for nothing. Split them into a parallel array, or
move them behind a `Box`. This is also the starting point of structure-of-arrays designs (Chapter 20.5).

### 8. Java comparison

| Question | HotSpot JVM (64-bit) | Rust |
|---|---|---|
| Per-object overhead | a 12-byte header with compressed class pointers (16 without); objects 8-byte aligned. JDK 25's compact headers (JEP 519) cut it to 8 bytes, opt-in | none: a struct is its fields |
| Who orders fields | the JVM (groups fields by size) | rustc by default; you, with `repr(C)` |
| Can you pin a layout? | no (off-heap memory via `MemorySegment` + `MemoryLayout` in the FFM API, JDK 22) | yes, `repr(C)` |
| `Order[]` / `Vec<Order>` | an array of *references* to separately allocated objects | the orders themselves, contiguous |
| `Optional<Long>` | two objects (the `Optional`, the `Long`), or null | `Option<NonZeroU64>`: 8 bytes, no allocation |
| False-sharing control | `@jdk.internal.vm.annotation.Contended` (JDK-internal; user code needs `-XX:-RestrictContended`) | `#[repr(align(64))]` |
| Inspect a layout | JOL (Java Object Layout) | `size_of`, `align_of`, `offset_of!`, `-Zprint-type-sizes` |

For a Java engineer, the biggest shift is the `Vec<Order>` row. A Java `ArrayList<Order>` of ten million orders is ten
million headers plus ten million pointers, scattered across the heap, with every scan chasing a reference per element.
The Rust `Vec<Order>` is one contiguous block of 240 MB that hardware prefetchers stream through. Project Valhalla's
value classes aim to let the JVM flatten such arrays (Chapter 5.3).

> **Analogy limit.** "rustc reorders fields like the JVM does" is true, but the consequences differ. The JVM's layout is
> invisible to Java code, so its freedom costs you nothing. Rust code *can* observe layout (through `unsafe`, FFI, and
> `transmute`), so the default layout being unspecified is a rule you must actively respect: anything that depends on
> offsets needs `repr(C)`.

### 9. Production scenario

**Meridian's order-book memory budget.** The Rust matching engine from the Part III review keeps resting orders in an
arena. The capacity plan calls for 10 million resting orders per instance at peak, with headroom. A design review went
through the order type field by field:

- The first draft was `#[repr(C)]` "to be safe," a habit carried over from the C++ team. That's 32 bytes per order with
  10 bytes of padding, 320 MB at peak. There was no FFI and no raw-byte persistence, so it bought nothing. Dropping
  it gave 24 bytes and 240 MB (verified arithmetic, listing `ch02-01-layout-table.rs`).
- `side` went from `u8` with "1 = bid, 2 = ask" to `#[repr(u8)] enum Side { Bid = 1, Ask = 2 }`: still one byte, and
  `Option<Side>` is also one byte. Parsing from the wire goes through `TryFrom<u8>`.
- `parent_order: Option<u64>` (for iceberg child orders) would have cost 16 bytes. IDs start at 1, so the type became
  `Option<OrderId>` with `OrderId(NonZeroU64)`: 8 bytes, a documented guarantee, and a type that can't be confused with
  a quantity (Chapter 5.3).
- The persisted form, the order journal, is **not** the in-memory struct. It's an explicit encoding with a version byte,
  so the in-memory layout can keep changing with rustc and with the code.

The rule the review wrote down: **`repr(C)` marks a boundary.** A reader seeing it should be able to find the FFI call,
file format, or mapped region that requires it. If there isn't one, it's a bug in the making: it blocks optimizations
and invites someone to write the struct to disk.

### 10. Failure scenario

**Nondeterministic checksums in the trade archive.** Meridian's C++ market-data service archived executed trades by
`fwrite`-ing a `struct Trade { uint64_t id; int64_t price; uint8_t side; }` straight to disk: 24 bytes, 7 of them
padding. Each trade record was checksummed and replicated to two sites. For months, a few percent of records had
mismatched checksums between sites, and reconciliation flagged them as corrupted. They weren't. The padding bytes held
whatever the stack held before, which differed by code path and machine. A security review later found worse: in
some records, the padding contained fragments of the *previous* request's data, which the archive exported to a
partner.

The Rust port's first attempt did the same thing with `bytemuck`, and it didn't compile. `derive(Pod)` rejects padded
types at compile time (listing `ch02-06-pod-padding.rs`: "derive(Pod) was applied to a type with padding"). The team
chose explicit, zeroed padding (listing `ch02-07-pod-explicit.rs`) for the in-memory record, and for the archive a
real encoding with fixed little-endian integers and a version byte.

Two lessons:

1. **Padding bytes are uninitialized memory.** Writing them out is an information leak, and checksumming them is
   nondeterministic. In C and C++ it's legal and silent. In safe Rust you can't read them at all: getting at a struct's
   bytes requires `unsafe` or a crate that proves there's no padding.
2. **In-memory layout isn't a file format.** It changes with compilers, flags, and architectures (endianness, alignment
   rules). Persist through an explicit encoding (Chapter 23.5).

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part V).*

1. State the three layout facts every type has, and the two rules relating size, alignment, and field offsets.
2. Why may rustc reorder fields by default? When must you prevent it, and how?
3. Explain why `Option<Option<NonZeroU64>>` is 16 bytes but `Option<Option<Option<bool>>>` is 1.
4. `Msg2 { Big(Box<u64>, u64), Small(u32) }` is 16 bytes, but `Payload { Small(u8), Big(Box<[u8; 1024]>) }` is also 16,
   not 8. Explain both, using the word "niche."
5. Which `Option` niche optimizations are guaranteed, and why does the distinction matter for FFI and `unsafe` code?
6. Why is transmuting 7 into a `#[repr(u8)] enum Side { Bid = 1, Ask = 2 }` undefined behavior even if the program
   never matches on it?
7. Why does creating a reference to a field of a `repr(packed)` struct fail with E0793, and what are the two
   alternatives?
8. What is false sharing, and how does `#[repr(align(64))]` prevent it? What does it cost?

### 12. Exercises

- **Beginner.** Predict the size and alignment of `struct A { a: u8, b: u16, c: u8 }` and of the same struct with
  `repr(C)`. Then verify with `size_of` and `offset_of!`.
- **Intermediate.** Order the fields of a `repr(C)` struct `{ flag: bool, ts: u64, code: u16, qty: u32, kind: u8 }` by
  hand to minimize padding. What size do you reach, and does it match the default repr's size?
- **Advanced.** Predict, then verify: `Option<(NonZeroU32, bool)>`, `Result<&u8, u8>`, `Option<Result<(), bool>>`,
  and `enum E { A(char), B, C, D }`. For each surprise, explain where the niche came from or why none was available.
- **Systems.** Measure `tagged_in_memory` against `niche_in_memory` from listing `ch02-03-niche-asm.rs` on slices of
  1K, 1M, and 100M elements (release build, `std::time::Instant`, 10 repetitions, report the median). Where does the gap
  appear, and does it match the bytes-per-element ratio?
- **Architecture.** Your team shares a struct between a Rust service and a C library through shared memory. List every
  layout property both sides must agree on, and how you'd test the agreement in CI.

### 13. Debugging exercise

A lookup cache in the account service stores negative results so the database isn't asked twice about users without an
account (listing `ch02-11-negative-cache.rs`, verified):

```rust,ignore
/// Version 1: "not looked up yet" = None, "looked up, no account" = Some(None).
type CacheV1 = HashMap<u64, Option<Option<NonZeroU64>>>;

/// Version 2: an explicit enum. Clearer, but it still needs TWO spare bit patterns.
enum Cached {
    Unknown,
    Absent,
    Present(NonZeroU64),
}
```

```text
value sizes: Option<Option<NonZeroU64>>=16 Cached=16 Option<NonZeroU64>=8
entry sizes: v1 (u64, Option<Option<_>>)=24 v3 (u64, Option<_>)=16
```

The cache holds 50 million entries, and its memory is 50% above the estimate.

1. Explain the 16-byte value size by counting values and niches. Why doesn't the explicit enum help?
2. Which of the three states is already represented by something else in the data structure? Read version 3 in the
   listing.
3. Compute the saving for 50 million entries (entry size only, ignoring the hash table's own overhead). Then name
   one more state a production cache would need (hint: time), and design a representation that keeps entries at 16
   bytes.

### 14. Design exercise

**A market-data tick for Meridian's fan-out service.** Each tick has: instrument ID (fits in `u32`), price (`i64`
minor units), quantity (`u32`), side (bid/ask), up to 8 flag bits, an exchange timestamp (`u64` nanoseconds), and a
sequence number (`u64`). The service keeps the last 1M ticks per instrument group in a ring buffer and publishes each
tick over UDP to Rust and C++ subscribers.

1. Design the in-memory type. Target 32 bytes or less, and justify every representation choice (enum vs integer, which
   niches exist, what `repr`).
2. Design the wire format separately. Where do versioning, endianness, and padding get decided? Would you use
   `repr(C, packed)`, `bytemuck`, or explicit encoding functions, and why?
3. The C++ subscribers want to `reinterpret_cast` the UDP payload. What must you guarantee, and how would a test in
   both languages catch a mismatch?
4. The ring buffer's write index is updated by one thread and read by many. Where does `repr(align)` belong, and where
   would it waste memory?
