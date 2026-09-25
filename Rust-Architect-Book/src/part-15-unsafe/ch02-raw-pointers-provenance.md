# Chapter 15.2 — Raw Pointers, Provenance, and Aliasing Models

> **Where this sits:** Part XV · Unsafe Rust · chapter 2 of 6
> **Prerequisites:** Chapter 15.1 (validity vs safety invariants, soundness), Chapter 4.1 (places, loans, `noalias`),
> Chapter 4.6 (Miri's first dangling-reference report), Chapter 14.4 §5 (a lock-free stack that passes Miri only under
> Tree Borrows).
> **After this chapter you can:** explain why a pointer is more than an address; tell in-bounds pointer arithmetic from
> wrapping arithmetic and say what each promises the compiler; use the strict-provenance APIs for tagged pointers; read
> a Stacked Borrows or Tree Borrows report from Miri and fix the code so it passes both; implement `split_at_mut`- and
> `get_disjoint_mut`-shaped APIs soundly; and decide when "zero-copy" parsing needs `unsafe` at all.

---

## Pass 1 · User level — *What a raw pointer is allowed to do*

### 1. Problem

Every unsafe container in this book so far is built on raw pointers. `Vec` keeps one to its buffer. `split_at_mut`
hands out two `&mut` slices made from one. Chapter 15.1's `HeaderBlock` writes into slots through pointers, and
Chapter 14.4's lock-free stack swaps them with compare-and-swap. A Java engineer's first model is "a raw pointer is a
`long` holding an address, and dereferencing it reads whatever is at that address." That model is wrong in two ways,
and both of them produce the kind of bug Chapter 15.1 warned about: code that prints the right answer today and is
undefined behavior anyway.

1. **A pointer is not just an address.** Two pointers can have the same address and still not be interchangeable.
   One may be allowed to read that memory and the other not.
2. **References make promises that outlive their use.** A `&mut T` you created and threw away a line ago can still
   decide whether a raw pointer derived earlier is usable. That's the job of the *aliasing model*, and Rust currently
   has two candidate models that disagree on some programs.

This chapter gives you the rules precisely enough to write pointer code that passes both models, and shows the one
place where they differ in code you're likely to write.

### 2. Mental model

**A pointer value is an address plus provenance.** [LANG] *Provenance* is the permission a pointer carries: which
allocation it may access, and (under the aliasing model) with what rights. You get provenance by deriving a pointer
from something that has it: a reference, `Box::into_raw`, an allocator. Arithmetic keeps it. An integer has none.

```text
   pointer = ( address , provenance )
                 │            │
                 │            └── "may access bytes [start, end) of allocation #214, with these rights"
                 └── a number; the CPU only ever sees this half
```

The CPU has no idea provenance exists (§6). The *compiler* uses it: it's how LLVM knows that a store through a pointer
derived from `a` can't change `b`, even when the two happen to be adjacent in memory.

**The five pointer-ish types and what each one promises** [LANG] [LIB]:

| Type | Non-null | Aligned + dereferenceable | Aliasing promise | Variance in `T` | Niche |
|---|---|---|---|---|---|
| `&T` | yes | yes, whenever it exists | memory doesn't change (outside `UnsafeCell`) while it's live | covariant | yes |
| `&mut T` | yes | yes, whenever it exists | no other pointer accesses it while it's live | invariant | yes |
| `*const T` / `*mut T` | no | only when you dereference | none by itself; inherits the rights of what it came from | covariant / invariant | no |
| `NonNull<T>` | yes | only when you dereference | none | covariant | yes |

The references' promises are checked by the borrow checker when you use references. Raw pointers skip the checker,
not the promises. When you create a reference *from* a raw pointer (`&mut *p`), you are asserting every column of that
reference's row, and the compiler will optimize as if you told the truth.

**Three rules cover most pointer code:**

1. **Arithmetic stays in bounds.** `p.add(n)` / `p.offset(n)` must land inside the same allocation or exactly one past
   its end [LANG]. Leaving it is UB *even if you never dereference the result*. `wrapping_add` has no such
   requirement, but it doesn't grant any new access either.
2. **Access needs provenance, not just the right address.** A pointer can only access the allocation it was derived
   from.
3. **Don't let references invalidate your pointers.** Creating a new `&mut` to memory (even implicitly, through a
   method that takes `&mut self`) can revoke the rights of pointers derived earlier. Exactly *when* is the question
   the aliasing models answer.

> **What actually happens with the aliasing rules?** [VERSION] The Rust Reference lists "breaking the pointer aliasing
> rules" as undefined behavior and says the exact rules aren't settled. There are two candidate formal models, both
> implemented in Miri: **Stacked Borrows** (Jung, Dang, Kang, Dreyer; POPL 2020; Miri's default at the time of
> writing) and **Tree Borrows**
> (Villani, Hostert, Dreyer, Jung; PLDI 2025; opt-in with `-Zmiri-tree-borrows`). Tree Borrows accepts more programs.
> The engineering rule this book uses: **write code that passes both.** When they disagree, the code is in a gray
> area that a future version of the language could resolve either way.

### 3. Rust code

**The basics** (listing `ch02-01-raw-basics.rs`, verified; clean under Miri):

```rust
use std::ptr::NonNull;

/// A wire struct: `len` sits at offset 1, so it is never 4-aligned.
#[repr(C, packed)]
struct WireHeader {
    kind: u8,
    len: u32,
}

fn main() {
    let xs = [10u32, 20, 30, 40];
    let base: *const u32 = xs.as_ptr();
    // SAFETY: base + 2 stays inside the 4-element array.
    let third = unsafe { base.add(2) }; // arithmetic in units of T: +8 bytes
    let end = base.wrapping_add(xs.len()); // one past the end: may exist, must not be read
    // SAFETY: `third` points to xs[2], initialized and alive for this whole function.
    println!("*third = {}", unsafe { *third });
    // SAFETY: both pointers are derived from `xs` and lie within (or one past) it.
    println!("end - base = {} elements", unsafe { end.offset_from(base) });

    let h = WireHeader { kind: 7, len: 1_000 };
    // `&h.len` would be a misaligned reference (error E0793). A raw pointer may be misaligned:
    let p: *const u32 = &raw const h.len;
    // SAFETY: `p` points to an initialized u32 inside `h`; read_unaligned has no alignment requirement.
    println!("kind = {}, len = {}", h.kind, unsafe { p.read_unaligned() });

    let nn: NonNull<u32> = NonNull::from(&xs[0]);
    println!(
        "NonNull<u32> = {} B, Option<NonNull<u32>> = {} B, *const u32 = {} B, first = {}",
        size_of::<NonNull<u32>>(),
        size_of::<Option<NonNull<u32>>>(),
        size_of::<*const u32>(),
        // SAFETY: `nn` was made from a live reference to xs[0].
        unsafe { *nn.as_ptr() }
    );
}
```

```text
*third = 30
end - base = 4 elements
kind = 7, len = 1000
NonNull<u32> = 8 B, Option<NonNull<u32>> = 8 B, *const u32 = 8 B, first = 10
```

Three details to keep. `add` counts in elements, not bytes. `&raw const` (stable since Rust 1.82 [VERSION]) makes a
raw pointer to a place *without creating a reference first*, which is how you point at a misaligned packed field
(Chapter 5.2's E0793; `ptr::addr_of!` is the older macro spelling of the same thing). And `NonNull` has a niche (`Option<NonNull<u32>>` is still 8 bytes), which is why
`Vec`, `Box`, and `Rc` store `NonNull` rather than `*mut`.

> **Safety invariant (`ch02-01`).** Every dereferenced pointer was derived from a live, initialized allocation and
> stays inside it; `end` is one past the end and is never dereferenced; the packed field is read only with
> `read_unaligned`.

**Out-of-bounds arithmetic is UB before any read** (listing `ch02-02-oob-arithmetic.rs`):

```rust,ignore
    let a = [1u32, 2, 3];
    let p = a.as_ptr();
    let q = unsafe { p.add(10) }; // 40 bytes past the start of a 12-byte allocation
    let back = unsafe { q.sub(10) };
    println!("{}", unsafe { *back });
```

`back` is exactly `p` again, and the read is in bounds. Miri stops the program one step earlier:

```text
error: Undefined Behavior: in-bounds pointer arithmetic failed: attempting to offset pointer by 40 bytes, but got
alloc212 which is only 12 bytes from the end of the allocation
 --> src/main.rs:6:22
6 |     let q = unsafe { p.add(10) }; // 40 bytes past the start of a 12-byte allocation
```

§4 shows why: `add` tells LLVM something `wrapping_add` doesn't.

**Same address, different provenance** (listing `ch02-03-provenance.rs`):

```rust
// Same ADDRESS, different PROVENANCE: a pointer derived from `a` may not access `b`.
fn main() {
    let a = [1u32, 2];
    let b = [3u32, 4];
    let pa = a.as_ptr();
    let pb = b.as_ptr();
    println!("one past a == &b[0]? {}", pa.wrapping_add(2) == pb);
    // Build a pointer with b's address but a's provenance.
    let forged = pa.with_addr(pb.addr());
    println!("forged == pb? {}", forged == pb);
    let v = unsafe { *forged };
    println!("read {v} through a pointer derived from `a`");
}
```

Natively, and then under Miri:

```text
native:  one past a == &b[0]? true
         forged == pb? true
         read 3 through a pointer derived from `a`

Miri:    one past a == &b[0]? false
         forged == pb? true
         error: Undefined Behavior: memory access failed: attempting to access 4 bytes, but got alloc214+0x14 which
         is at or beyond the end of the allocation of size 8 bytes
```

Natively, the compiler happened to place `b` right after `a`, so "one past `a`" *was* `&b[0]`, and the forged read
returned `b`'s value. Miri placed the arrays apart, and it tracks the provenance: the forged pointer compares equal to
`pb` (addresses are equal) but it is still a pointer into `a`, 20 bytes into an 8-byte allocation. Equal addresses
don't make pointers interchangeable. That's the whole idea of provenance in one listing.

**Tagged pointers the strict-provenance way** (listing `ch02-04-tagged-pointer.rs`, clean under both aliasing
models). [VERSION] Rust 1.84 stabilized the *strict provenance* APIs: `addr()` (the address as a plain `usize`, no
provenance), `with_addr(a)` and `map_addr(f)` (a new address, *the same provenance*). They let you do bit tricks on
the address without ever turning a pointer into an integer and back:

```rust,ignore
const TAG_MASK: usize = 0b111;

impl<T> Tagged<T> {
    fn new(p: NonNull<T>, tag: usize) -> Self {
        assert!(align_of::<T>() >= 8 && tag <= TAG_MASK);
        // map_addr keeps p's PROVENANCE and changes only its address (strict provenance, Rust 1.84).
        Tagged { raw: p.map_addr(|a| a | tag) }
    }
    fn tag(self) -> usize {
        self.raw.addr().get() & TAG_MASK
    }
    fn ptr(self) -> NonNull<T> {
        self.raw.map_addr(|a| {
            // SAFETY: the original address was non-zero and 8-aligned, so clearing the
            // three tag bits gives back that same non-zero address.
            unsafe { NonZeroUsize::new_unchecked(a.get() & !TAG_MASK) }
        })
    }
}
```

```text
tag=5 value=7
```

> **Safety invariant (`Tagged<T>`).** `raw`'s address is the address of a live, 8-aligned `T` with its low three bits
> replaced by the tag; its provenance is the original pointer's. `new`'s `assert!` makes the alignment half a checked
> precondition rather than a comment.

The old way is still legal: `p as usize` then `addr as *const T`. Rust calls this **exposed provenance**
(`expose_provenance` / `with_exposed_provenance` are the explicit names). The integer-to-pointer cast "picks up"
provenance from some pointer that was exposed earlier. Listing `ch02-05-int-roundtrip.rs` passes Miri, with a warning
worth reading:

```text
warning: integer-to-pointer cast
  = help: this program is using integer-to-pointer casts or (equivalently) `ptr::with_exposed_provenance`, which
          means that Miri might miss pointer bugs in this program
```

Exposed provenance is necessary sometimes (FFI that hands you addresses as integers, some allocators). It's always a
loss of checking. Prefer the strict APIs in new code.

**Splitting a slice: one pointer, two halves.** This is the shape of std's `split_at_mut` [LIB] (listing
`ch02-07-split-once.rs`, clean under both models):

```rust
// The shape of std's split_at_mut: ONE raw pointer, both halves derived from it.
fn my_split_at_mut(s: &mut [u32], mid: usize) -> (&mut [u32], &mut [u32]) {
    let len = s.len();
    assert!(mid <= len, "mid > len");
    let p = s.as_mut_ptr();
    // SAFETY: [0, mid) and [mid, len) are in bounds (mid <= len) and do not overlap; both slices
    // derive from the single pointer `p` and borrow `s` for their whole lifetime.
    unsafe {
        (
            std::slice::from_raw_parts_mut(p, mid),
            std::slice::from_raw_parts_mut(p.add(mid), len - mid),
        )
    }
}

fn main() {
    let mut v = [1, 2, 3, 4];
    let (l, r) = my_split_at_mut(&mut v, 2);
    l[0] += 10;
    r[0] += 20;
    println!("{v:?}");
}
```

```text
[11, 2, 23, 4]
```

A one-token variation calls `s.as_mut_ptr()` twice, once per half (listing `ch02-06-stacked-vs-tree.rs`). It prints
the same `[11, 2, 23, 4]` natively and passes under Tree Borrows, but Stacked Borrows rejects it:

```text
error: Undefined Behavior: trying to retag from <454> for Unique permission at alloc213[0x0], but that tag does not
exist in the borrow stack for this location
help: <454> was created by a Unique retag at offsets [0x0..0x8]
 9 |         let left = std::slice::from_raw_parts_mut(s.as_mut_ptr(), mid);
help: <454> was later invalidated at offsets [0x0..0x10] by a Unique function-entry retag inside this call
11 |         let right = std::slice::from_raw_parts_mut(s.as_mut_ptr().add(mid), len - mid);
```

The second `as_mut_ptr()` takes `&mut self`, which is a fresh exclusive reborrow of the *whole* slice. Under Stacked
Borrows that reborrow invalidates everything derived from the previous one, including `left`. §4 explains the
mechanism. The fix is the rule in the listing's name: **derive once, then split the raw pointer.**

**Disjoint `&mut`s: check bounds *and* distinctness.** A helper that hands out two mutable references into one slice
needs two facts, and the classic bug remembers only one (listing `ch02-08-disjoint-duplicate.rs`):

```rust,ignore
// Bounds checked, distinctness forgotten: two `&mut` to the same element.
fn two_mut<T>(s: &mut [T], i: usize, j: usize) -> (&mut T, &mut T) {
    assert!(i < s.len() && j < s.len());
    let p = s.as_mut_ptr();
    unsafe { (&mut *p.add(i), &mut *p.add(j)) }
}
```

Called with `i == j` (a transfer from an account to itself), both models reject it, at different moments. Stacked
Borrows objects when the second `&mut` is created. Tree Borrows lets both exist (a fresh `&mut` starts in a *Reserved*
state) and objects at the first conflicting use:

```text
Tree Borrows:  error: Undefined Behavior: read access through <441> at alloc213[0x0] is forbidden
               help: the accessed tag <441> was created here, in the initial state Reserved
               12 |     let (a, b) = two_mut(&mut balances, 0, 0); // a transfer from an account to itself
               help: the accessed tag <441> later transitioned to Disabled due to a foreign write access at
                     offsets [0x0..0x8]
               13 |     *a -= 30;
```

The sound version is the shape of std's `get_disjoint_mut` [LIB] (stable since 1.86 [VERSION]; listing
`ch02-09-disjoint-fixed.rs`, clean under both models):

```rust,ignore
fn get_disjoint<T, const N: usize>(s: &mut [T], idx: [usize; N]) -> Result<[&mut T; N], DisjointError> {
    for (k, &i) in idx.iter().enumerate() {
        if i >= s.len() {
            return Err(DisjointError::IndexOutOfBounds);
        }
        if idx[..k].contains(&i) {
            return Err(DisjointError::OverlappingIndices); // O(N^2): fine for small N
        }
    }
    let p = s.as_mut_ptr();
    // SAFETY: every index is in bounds and all are pairwise distinct (checked above), so the N
    // references point to N different elements. All derive from the one pointer `p`, and their
    // lifetime is tied to the exclusive borrow of `s`.
    Ok(idx.map(|i| unsafe { &mut *p.add(i) }))
}
```

```text
[70, 80, 70]
Err(OverlappingIndices)
Err(IndexOutOfBounds)
[70, 81, 70]
```

> **Safety invariant (`get_disjoint`).** Every index is `< s.len()`, the indices are pairwise distinct, and all
> returned references are derived from one pointer obtained from `s`, whose exclusive borrow lasts as long as they do.

**`set_len`: initialize first, then publish** (listing `ch02-10-set-len-ok.rs`, clean under Miri):

```rust
// Initialize first, THEN publish the new length.
fn fill_squares(n: usize) -> Vec<u64> {
    let mut v: Vec<u64> = Vec::with_capacity(n);
    for (i, slot) in v.spare_capacity_mut()[..n].iter_mut().enumerate() {
        slot.write((i as u64) * (i as u64)); // MaybeUninit::write: no read of the old contents
    }
    // SAFETY: n <= capacity (with_capacity(n)), and every slot in [0, n) was written above.
    unsafe { v.set_len(n) };
    v
}

fn main() {
    println!("{:?}", fill_squares(6));
}
```

```text
[0, 1, 4, 9, 16, 25]
```

`set_len` is the most direct way to break `Vec`'s safety invariant from outside: its whole contract is "`new_len <=
capacity` and the first `new_len` elements are initialized." `spare_capacity_mut` gives you the uninitialized tail as
`&mut [MaybeUninit<T>]` (Chapter 15.3), so writing it needs no `unsafe` at all. The only unsafe line is the one that
makes the promise. The debugging exercise (§13) is the version that publishes first.

**The lint that catches the obvious case** (listing `ch02-12-reference-casting-lint.rs`). Writing through a pointer
cast from a shared reference is deny-by-default:

```text
error: assigning to `&T` is undefined behavior, consider using an `UnsafeCell`
6 |     let p = r as *const u32 as *mut u32;
  |             --------------------------- casting happened here
7 |     unsafe { *p = 2 };
  = note: `#[deny(invalid_reference_casting)]` on by default
```

The lint sees only the same-function case. Chapter 15.3's listing `ch03-10` hides the cast in a helper function, and
only Miri catches it.

---

## Pass 2 · Systems level — *What rustc tells LLVM, and how Miri checks it*

### 4. Under the hood

**`add` vs `wrapping_add` in IR.** [RUSTC] Listing `ch02-15-codegen.rs` defines the same function twice. The debug LLVM
IR (via `tools/emit.ps1`, rustc 1.98.1) shows the difference that §3's out-of-bounds rule comes from:

```text
; playground::nth            (p.add(i))
  %_0.i = getelementptr inbounds nuw i32, ptr %p, i64 %i

; playground::nth_wrapping   (p.wrapping_add(i))
  %_0.i = getelementptr i32, ptr %p, i64 %i
```

`inbounds` tells LLVM that the result stays inside the object `%p` points into. If it doesn't, the result is *poison*
(LLVM's "this value is the product of UB"). `nuw` adds that the offset doesn't wrap around as an unsigned number,
which rustc can promise because `add` takes a `usize`. Those facts let LLVM fold comparisons, prove pointers don't
alias, and reason about loop bounds. In release, both functions compile to the same instruction, and LLVM merges them:

```text
playground::nth_wrapping:
	lea	rax, [rdi + 4*rsi]
	ret
playground::nth = playground::nth_wrapping
```

Same machine code, different contracts. Nothing about the instruction makes `add(10)` on a 3-element array fail. The
UB is in what the *surrounding* code is allowed to assume after it, which is why §3's program with `add(10)` then
`sub(10)` is UB even though every address it touches is fine.

**Why aliasing rules exist: the optimizer's side of `&mut`.** Chapter 4.1 showed rustc marking `&mut T` parameters
`noalias`. Listing `ch02-19-alias-assumption.rs` shows what that assumption buys, and what it costs when unsafe code
breaks it:

```rust,ignore
#[inline(never)]
fn transfer(from: &mut i64, to: &mut i64, amount: i64) -> i64 {
    *from -= amount;
    *to += amount;
    *from // `from` and `to` are both `&mut` (noalias): the compiler may reuse the value it just computed
}
```

Fed two `&mut` to the same `i64` by the buggy `two_mut` from §3:

```text
debug:    reported balance = 100, stored balance = 100
release:  reported balance = 70, stored balance = 100
```

The release assembly shows the assumption directly:

```text
playground::transfer:
	mov	rax, qword ptr [rdi]
	add	rax, -30
	mov	qword ptr [rdi], rax     ; *from = from - 30        (rax = 70)
	add	qword ptr [rsi], 30      ; *to += 30                 (same address: memory is 100 again)
	ret                          ; return rax: 70, never reloaded from [rdi]
```

Because `from` and `to` can't alias, the write through `to` can't change `*from`, so there's no reason to reload it.
The function is correct for every input its signature allows. The UB was manufacturing an input the signature
forbids, and Miri flags that at the moment the second `&mut` is made.

**Stacked Borrows: a stack of permissions per location.** [RUSTC: Miri's model, not rustc's] Every reference and
every raw pointer derived from one gets a *tag*. Each byte of memory keeps a stack of the tags allowed to access it.

```text
 creating a reference = "retag": push a new tag for the bytes it covers
 using a pointer      = find its tag in the stack; pop everything ABOVE it (they lose their rights)
 tag not in the stack = UB ("that tag does not exist in the borrow stack for this location")
```

Replay `split_twice` (§3, `ch02-06`) on the bytes of `v[0..2]`:

```text
 after `&mut v` passed in           [ v | s(Unique) ]
 left = from_raw_parts_mut(s.as_mut_ptr(), mid)
   as_mut_ptr(&mut *s): reborrow    [ v | s | s1(Unique) ]           ── left derives from s1
   left is a new &mut [u32]         [ v | s | s1 | left(Unique) ]
 right = ... s.as_mut_ptr() ...
   second reborrow of *s: uses s    [ v | s | s2(Unique) ]           ── s1 and left POPPED
 return (left, right): retag left   left's tag is gone → UB
```

The error message names every step: `<454>` was *created by a Unique retag* at the first `as_mut_ptr`, and *later
invalidated by a Unique function-entry retag* inside the second call. In `my_split_at_mut`, both halves come from
one `p`, so nothing ever reborrows `s` again, and nothing is popped.

**Tree Borrows: a tree of permissions, and lazy `&mut`.** The newer model keeps, per location, a *tree* of tags
(children are pointers derived from their parent) and a permission for each tag. The key states for `&mut`:

```text
 Reserved  ── first write through it ──►  Active
    │                                        │
    └── foreign write (by a non-descendant) ─┴──►  Disabled   (any later use: UB)
 foreign read of an Active tag ──► Frozen (read-only from then on)
```

A fresh `&mut` starts **Reserved**: it doesn't claim exclusivity until it actually writes. That's why `split_twice` is
fine under Tree Borrows: `left` was never invalidated, because creating the second reborrow isn't an *access* to
`left`'s bytes, just a new sibling. `two_mut(…, 0, 0)` still fails, because both references are then *used*, and the
write through `a` is a foreign write for `b`, which becomes Disabled. The Tree Borrows report said exactly that.

**The difference you're most likely to hit: `container_of`.** Intrusive data structures store a link *inside* a
struct and recover the struct from a pointer to the link (the Linux kernel's `container_of`). Chapter 14.4's
Treiber stack passed Miri only under Tree Borrows because `crossbeam-epoch` 0.9.20's `Local::element_of` does this.
Listing `ch02-17-container-of.rs` is the minimal case:

```rust,ignore
fn entry_of(link: *const Link) -> *const Entry {
    link.wrapping_byte_sub(offset_of!(Entry, link)).cast::<Entry>()
}

fn main() {
    let e = Entry { key: 7, link: Link { next: std::ptr::null() } };
    let lp: *const Link = &e.link; // a reference to the FIELD, then a raw pointer
    let ep = entry_of(lp);
    println!("key = {}", unsafe { (*ep).key }); // reads bytes outside `link`
}
```

```text
native, Tree Borrows:  key = 7
Stacked Borrows:       error: Undefined Behavior: attempting a read access using <442> at alloc215[0x0], but that
                       tag does not exist in the borrow stack for this location
                       help: <442> was created by a SharedReadOnly retag at offsets [0x8..0x10]
                       26 |     let lp: *const Link = &e.link; // a reference to the FIELD, then a raw pointer
```

`&e.link` was retagged for the link's bytes only (`[0x8..0x10]`), so under Stacked Borrows the pointer derived from
it has no rights to `key` at offset 0. Tree Borrows lets a pointer read outside its reference's range as long as
nothing conflicting has happened there. The version that passes **both** models (listing `ch02-18-container-of-raw.rs`)
never creates a reference to the field: it takes a raw pointer to the whole struct and projects with `&raw const`:

```rust,ignore
    let whole: *const Entry = &raw const e;
    // SAFETY: `whole` points to a live Entry; `&raw const` projects to the field without creating a reference.
    let lp: *const Link = unsafe { &raw const (*whole).link };
```

> **Safety invariant (`container_of`).** The link pointer was derived from a pointer with rights to the *entire*
> `Entry` (never narrowed by a reference to the field), the `Entry` is live, and `offset_of!` is the field's real
> offset (`#[repr(C)]` isn't even required for `offset_of!`, but it keeps the layout reviewable).

### 5. Memory

The two arrays of listing `ch02-03`, as the native run laid them out:

```text
 address:   ...f0        ...f4        ...f8        ...fc
            ┌────────────┬────────────┬────────────┬────────────┐
            │ a[0] = 1   │ a[1] = 2   │ b[0] = 3   │ b[1] = 4   │
            └────────────┴────────────┴────────────┴────────────┘
            ◄── allocation a (8 B) ──►◄── allocation b (8 B) ──►
 pa.wrapping_add(2) ─────────────────►│  address ...f8, provenance = a  → may NOT be read
 pb ──────────────────────────────────►│  address ...f8, provenance = b  → may be read
```

A pointer's provenance also carries its *aliasing* rights, which is what the Stacked and Tree Borrows diagrams in §4
track per byte. Two practical consequences:

- **Narrowing is sticky under Stacked Borrows.** A pointer derived from `&x.field` or `&v[i]` only has rights to those
  bytes. Pointer code that walks outside the element it started from must start from a pointer to the whole thing
  (`as_mut_ptr()` on the slice, `&raw mut` on the struct).
- **One-past-the-end is a valid pointer, not a valid place.** `end` in `ch02-01` can be compared and subtracted, never
  read. Iterators over slices use exactly this `(ptr, end)` pair [LIB].

### 6. CPU / OS

- **Hardware doesn't track provenance.** [CPU] On x86-64 and Arm64, a pointer is a 64-bit integer, and `lea rax,
  [rdi + 4*rsi]` has no idea which allocation it's in. Provenance is a rule for the *compiler* and for tools like Miri.
  The exception is **CHERI** (the Arm Morello prototype, CHERIoT): hardware *capabilities* carry bounds and
  permissions in the pointer itself, and an out-of-bounds or forged access traps. Strict provenance is roughly the
  subset of Rust that maps onto such hardware. Rust on CHERI is experimental [VERSION].
- **Where tag bits can live.** Low bits: an 8-aligned pointer has three zero bits (guaranteed by the type's alignment
  [LANG]), which is what `Tagged<T>` uses. High bits: x86-64 addresses are *canonical* (the top 16 bits copy bit 47 with
  4-level paging), so stuffing data there faults when dereferenced unless the CPU masks it. Arm's Top Byte Ignore and
  Intel's Linear Address Masking make the top bits ignorable, but they depend on the CPU and the OS enabling them [CPU]
  [OS]. Low-bit tags are portable, and high-bit tags are a platform decision.
- **Misalignment.** [CPU] x86 reads the misaligned `u32` in `ch02-13` without complaint (next section), and the language
  still calls it UB [LANG] (Chapter 15.1 §6: aligned SIMD instructions fault on misaligned addresses, and the compiler
  may choose them because the type says it may).
- **Where pointers come from matters to the OS too.** [OS] Memory from `mmap` or an FFI call has no Rust allocation
  behind it. Miri can't check it, and code receiving such pointers relies on exposed provenance or on the FFI
  contract (Part XVI).

---

## Pass 3 · Architect level — *When do you need a raw pointer at all?*

### 7. Trade-offs

| Need | First choice (safe) | If that's not enough | Raw pointers only when |
|---|---|---|---|
| Two disjoint `&mut` into a slice | `split_at_mut`, `chunks_mut`, `get_disjoint_mut` (1.86), iterators | an index-based API | building a new std-like primitive; then copy std's shape (§3) |
| Parse a wire header | `from_be_bytes` on sub-slices (§9: compiles to one load + `bswap`) | `zerocopy` / `bytemuck` derives that *check* layout | never a raw cast to `&Struct` |
| Pack a tag into a pointer | an enum next to a `Box` (16 B) | `Tagged<T>` with `map_addr` | memory per entry matters and you'll test it under Miri |
| Recover a struct from a field pointer | an index or handle into an arena (Chapter 3.6) | `container_of` from a whole-object pointer | intrusive structures (Chapter 15.6's design exercise) |
| Uninitialized tail of a `Vec` | `extend`, `resize`, `collect` | `spare_capacity_mut` + `set_len` | never `set_len` before initialization |

**Rules for pointer code that passes both models:**

1. Derive every pointer you'll use from **one** raw pointer taken at the start (`let p = s.as_mut_ptr();`), then do
   arithmetic on `p`. Don't call `&mut self` methods on the parent while children are live.
2. Don't create references in between. Use `&raw const` / `&raw mut` for field projections, `ptr::read` / `ptr::write`
   for access, and create a reference only at the end, when you hand it out.
3. Use `add` when you know you're in bounds and `wrapping_add` when you're computing something that might not be
   (a tag, an end marker for a loop that may not run). Never rely on out-of-bounds pointers coming back in bounds.
4. Prefer strict provenance (`with_addr`, `map_addr`) to integer round trips. Treat every `as usize` on a pointer as a
   review question.
5. Run Miri with **both** models in CI (Chapter 15.6): `cargo miri test`, then again with
   `MIRIFLAGS=-Zmiri-tree-borrows`.

> **Why not just use `usize` indices instead of pointers everywhere?** Often you should: indices carry no provenance,
> can't dangle into freed memory (only into the wrong element), and are checked. Pointers win when the data structure
> must not move elements, when the pointer comes from outside (FFI, intrusive links), or when the bounds check really
> blocks vectorization. That last case must be measured, not assumed (Chapter 15.1's systems exercise).

### 8. Java comparison

| Java | Rust |
|---|---|
| No pointers in the language; references are opaque, and the JVM may move objects (compacting GC) | Raw pointers are addresses plus provenance; nothing moves unless your code moves it |
| `Unsafe.getInt(Object base, long offset)`: access is *relative to an object*, so the GC can move `base` and the access still works | Provenance: access is relative to the allocation the pointer came from. Same idea, enforced by the compiler's assumptions instead of by the GC |
| `Unsafe.getInt(long address)`: off-heap, no base object, no checking | Exposed provenance: an integer address, and tools can't check it |
| FFM `MemorySegment` (JDK 22): every access is checked against the segment's size (spatial) and its arena's lifetime (temporal); `MemorySegment.ofAddress(addr)` gives a zero-length segment that must be `reinterpret`ed, a *restricted* method | Strict provenance plus Miri: the same idea checked in testing, not at run time; `with_exposed_provenance` is the escape hatch |
| The JIT's alias analysis uses types and escape analysis; Java code can't create aliasing the JIT doesn't know about (short of `Unsafe`) | rustc emits `noalias` for `&mut` and `&T` parameters; unsafe code *can* create aliasing that breaks those facts, and §4 showed the result |

> **Analogy limit.** A `MemorySegment` out-of-bounds access throws `IndexOutOfBoundsException`, every time, at run
> time. A Rust provenance or aliasing violation isn't checked by anything at run time. It's undefined behavior, and the
> symptom may be a wrong value computed from a promise the compiler relied on (70 instead of 100), in a release build
> only. The only checker is Miri, during testing, on the paths your tests execute.

### 9. Production scenario

**Meridian's market-data frame header, parsed three ways.** The market-data feed (Chapters 2.4–2.5: magic `0xCAFE`,
version, flags, big-endian `u32` body length) runs through the Rust ingest service at about 2M messages/s at peak
(Chapter 6.5). During the rewrite, an engineer from the C++ team proposed "real zero-copy": cast the read buffer to a
`#[repr(C)]` struct, as the C++ fan-out did. Listing `ch02-13-frame-header-cast.rs`:

```rust,ignore
fn parse_cast(buf: &[u8]) -> FrameHeader {
    assert!(buf.len() >= size_of::<FrameHeader>());
    unsafe { *(buf.as_ptr() as *const FrameHeader) } // BUG 1: alignment. BUG 2: byte order.
}
```

The review ran it in all three configurations. The header starts at offset 1 of an 8-aligned pooled buffer (byte 0 is
a channel tag):

```text
release, native:  magic=0xfeca version=1 body_len=704643072
debug, native:    misaligned pointer dereference: address must be a multiple of 0x4 but is 0x7ffc25655d41
Miri:             error: Undefined Behavior: accessing memory based on pointer with alignment 1, but alignment 4
                  is required
```

The release build "works" and returns garbage: x86 performed the misaligned load, and the fields came out in the
host's little-endian order (`0xfeca`, and 42 read as `704643072` = `0x2A000000`). Two bugs, and only one of them is
UB. The byte-order bug would have survived any amount of Miri.

The safe version (listing `ch02-14-frame-header-safe.rs`) reads each field with `from_be_bytes` from a checked
8-byte sub-slice, and prints `magic=0xcafe version=1 flags=0 body_len=42`. It has no `unsafe` at all, and it's
already zero-copy. The release assembly of its `body_len` accessor (listing `ch02-15-codegen.rs`):

```text
playground::body_len:
	cmp	rsi, 8                    ; buf.len() >= 8 ?
	jb	.LBB1_1
	mov	edx, dword ptr [rdi + 4]  ; one (unaligned) 4-byte load
	bswap	edx                       ; big-endian → host order
	mov	eax, 1                    ; Some
	ret
```

One compare, one load, one byte swap. The team adopted the safe parser for the eight-byte header and set a rule for
larger structures: use `zerocopy`'s derives, which **check** the layout instead of trusting it. Listing
`ch02-16-frame-header-zerocopy.rs` declares the fields as `U16<BigEndian>` / `U32<BigEndian>` (alignment 1, byte
order in the type), and `FrameHeader::ref_from_prefix(buf)` borrows the buffer without copying, failing on a short
buffer instead of reading past it:

```text
magic=0xcafe version=1 flags=0 body_len=42 (align_of FrameHeader = 1)
7-byte buffer: true
```

The review's conclusion went into the service's `unsafe` policy: **"zero-copy" is a property of the design (borrow,
don't copy), not a license to cast.** Raw casts from bytes to structs are banned; `zerocopy`/`bytemuck` derives are
the approved path.

### 10. Failure scenario

**The self-transfer in the risk-limits service.** Meridian's risk-limits service (Chapter 1.2: about 100K ops/s,
p99 under 1 ms, and a "no breach" invariant) keeps per-merchant exposure buckets in a `Vec<i64>`. Re-routing a
payment between two of a merchant's sub-accounts moves exposure from one bucket to another. The Rust port predates
`get_disjoint_mut` (stable in 1.86), so it had a hand-written `two_mut` helper with the §3 bug: bounds checked,
distinctness forgotten. Code review passed it: "it asserts the indices."

Then a merchant configured a re-route rule whose source and destination were the same sub-account. `two_mut(i, i)`
produced two `&mut` to one bucket, and `transfer` (listing `ch02-19-alias-assumption.rs`) returned the headroom it
had computed, not the one it had stored:

```text
release:  reported balance = 70, stored balance = 100
```

The decision path used the reported value. Every self-re-route was checked against a limit 30 units tighter than the
stored state, so some payments were declined when they shouldn't have been. The daily reconciliation flagged it,
because recorded headroom didn't match the stored buckets. The debug build and every unit test agreed with memory
(`100, 100`), so the bug was invisible until release. Nothing about the direction of the error is guaranteed: a
different inlining decision or compiler version could make the same UB loosen the limit instead of tightening it.

The fix had three layers:

1. **Code:** `slice::get_disjoint_mut([i, j])`, with `i == j` handled explicitly as "nothing to move" before the call
   (the self-route is a no-op, not an error).
2. **Tests:** the module's unit tests run under Miri in both models. The Stacked Borrows run fails on the old helper
   at the moment the second `&mut` is created, with no special test input beyond `i == j`.
3. **Policy:** hand-written disjointness code is banned where std has the primitive. The review checklist for any
   function returning two `&mut` from one source asks two questions: *bounds?* and *distinct?*

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XV).*

1. What is pointer provenance? Give a program where two pointers compare equal but only one may be dereferenced.
2. Why is `p.add(10)` UB on a three-element array even if the result is never dereferenced? What does `wrapping_add`
   promise instead, and what do the two compile to?
3. What do `addr()`, `with_addr()`, and `map_addr()` do, and why were they added (Rust 1.84) when `as usize` already
   existed? What does Miri warn about integer-to-pointer casts?
4. Explain Stacked Borrows in five sentences: tags, retags, the per-location stack, what a use does, and what the
   error "tag does not exist in the borrow stack" means.
5. How does Tree Borrows treat a newly created `&mut` differently, and why does that accept `split_twice` but still
   reject `two_mut(s, 0, 0)`?
6. Why does std's `split_at_mut` call `as_mut_ptr()` once? What goes wrong, and under which model, if it's called
   twice?
7. A function returns two `&mut T` from one `&mut [T]`. What two conditions make it sound? What's the cost of checking
   distinctness for N indices, and why is that acceptable in `get_disjoint_mut`?
8. `transfer(from: &mut i64, to: &mut i64)` returned 70 while memory held 100. Explain it from the assembly. Is
   `transfer` buggy?
9. "Zero-copy parsing needs `unsafe`." Argue against it with the frame-header example, and say when `zerocopy` or
   `bytemuck` is still the right tool.

### 12. Exercises

- **Beginner.** For each line, say whether it is UB and why: (a) `p.add(len)` where `p` is a slice's start pointer;
  (b) `*p.add(len)`; (c) `p.add(len + 1)`; (d) `p.wrapping_add(len + 1)`; (e) `p.wrapping_add(len + 1).wrapping_sub(1)`
  then dereferencing it. Check (b) and (c) under Miri.
- **Intermediate.** Rewrite `split_twice` (`ch02-06`) so it passes Stacked Borrows without changing its signature, then
  write `split_three(s, a, b) -> (&mut [T], &mut [T], &mut [T])` with the same technique, and test it under both
  models.
- **Advanced.** Implement `fn swap_elems<T>(s: &mut [T], i: usize, j: usize)` with raw pointers and `ptr::swap`, and
  make it correct for `i == j`. Is `ptr::swap` or `ptr::swap_nonoverlapping` the right primitive? Test under both
  models, then compare with std's `slice::swap`.
- **Systems.** Emit the release assembly of `transfer` with parameters `*mut i64` instead of `&mut i64` (using
  `unsafe` inside). Find the reload that appears. Then measure a loop of a million calls to each (one run is noisy;
  do several) and decide whether `noalias` is worth anything in this function.
- **Architecture.** Search a Rust codebase you know for `as usize` on pointers and `as *const`/`as *mut` from integers.
  Classify each: strict-provenance candidate (`addr`, `map_addr`), genuine exposure (FFI, allocator), or bug. Propose
  a lint policy (`clippy::as_conversions`, `fuzzy_provenance_casts` on nightly) and estimate the migration cost.

### 13. Debugging exercise

A frame checksum helper in a pooled-buffer code path (listing `ch02-11-set-len-uninit.rs`):

```rust,ignore
fn checksum_frame(src: &mut impl Read, n: usize) -> u32 {
    let mut buf: Vec<u8> = Vec::with_capacity(n);
    unsafe { buf.set_len(n) }; // BUG: claims n initialized bytes before any are written
    let _got = src.read(&mut buf).unwrap(); // a short read initializes only `_got` of them
    buf.iter().map(|&b| b as u32).sum() // reads the rest: uninitialized
}
```

1. The author's argument was "`read` overwrites the buffer anyway." Give two reasons it's wrong: one about `set_len`'s
   contract, one about `Read::read`'s.
2. With `src` = the three bytes `[1, 2, 3]` and `n = 8`, what does Miri report, at which byte, and what does its
   allocation dump look like?
3. Fix it three ways: (a) no `unsafe` at all, zeroing; (b) no `unsafe`, without zeroing the buffer on every call
   (hint: Chapter 15.3 §9); (c) with `spare_capacity_mut`, if `Read` could write into uninitialized memory. Why is (c)
   not possible with stable std on Rust 1.98?

### 14. Design exercise

**Packing connection state into a pointer.** The gateway's connection table holds about 1M live connections per pod
at peak, each a `Box<Conn>` plus a 2-bit state (`Idle`, `Reading`, `Writing`, `Closing`). Today an entry is
`(ConnState, Box<Conn>)`, 16 bytes. A proposal packs the state into the low bits of the box pointer (8 bytes).

- Write the safety invariant for the packed entry, including what must be true of `Conn`'s alignment, and how you
  enforce it at compile time (hint: a `const` assertion) rather than with a comment.
- Which APIs create, read, and update the tag without losing provenance? Where exactly is `Box::from_raw` called, and
  what makes calling it exactly once part of the invariant?
- What is the memory saving at 1M connections, and what else in the entry (or in the `HashMap` holding it) dominates
  memory? Is the saving worth a module of `unsafe` under the gateway policy of Chapter 15.1 §9?
- Write the Miri test plan: which operations, which models, and which bug you'd plant to check that the tests would
  catch it.
