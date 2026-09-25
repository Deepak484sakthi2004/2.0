# Chapter 15.3 — `MaybeUninit`, `ManuallyDrop`, and `UnsafeCell`

> **Where this sits:** Part XV · Unsafe Rust · chapter 3 of 6
> **Prerequisites:** Chapter 15.1 (validity vs safety invariants), Chapter 15.2 (`set_len`, provenance), Chapter 3.5
> (drop order, drop glue), Chapter 4.1 (`UnsafeCell` removes `noalias`), Chapter 5.3 (PhantomData's drop-check row).
> **After this chapter you can:** build values piece by piece with `MaybeUninit` without reading or dropping garbage;
> make partial initialization panic-safe; take control of when (and whether) a value is dropped; write a `Cell` from
> `UnsafeCell` and explain why nothing else may mutate behind `&`; and explain drop check, `#[may_dangle]`, and why
> `PhantomData<T>` matters to both.

---

## Pass 1 · User level — *Three wrappers, three switched-off assumptions*

### 1. Problem

Safe Rust makes three assumptions about every value, and they're what makes it safe:

1. **Every value is initialized and valid** from the moment it exists (Chapter 15.1's validity invariant).
2. **Every value is dropped exactly once**, automatically, when its owner goes away (Chapter 3.5).
3. **Memory behind a shared reference doesn't change** while the reference is live (Chapters 3.3 and 4.1).

A container author runs into each one. Chapter 15.1's `HeaderBlock` holds sixteen slots, most of them empty: the
empty ones can't hold a `(String, String)` because no such value exists yet (assumption 1). A type that hands its
buffer to C, or that must close a connection before releasing its pool, needs to decide itself when a field is dropped
(assumption 2). And `Cell`, `RefCell`, `Mutex`, and every atomic mutate through `&self` (assumption 3).

std doesn't let you turn these assumptions off globally. It gives you three **wrapper types, each of which switches
off exactly one of them for one value**. That's the design worth understanding: the unsafety is scoped to a type,
so the obligations it creates can be written down next to that type.

### 2. Mental model

| Wrapper | Assumption it switches off | What you owe instead | Layout [LIB] | Niche |
|---|---|---|---|---|
| `MaybeUninit<T>` | "this is a valid, initialized `T`" (and so also "drop it") | track which bytes are initialized; call `assume_init*` only on valid data; drop what you initialized | same size and alignment as `T` | **hidden** |
| `ManuallyDrop<T>` | "drop this automatically" | drop it exactly once, or deliberately never; never use it after dropping | same as `T` | kept |
| `UnsafeCell<T>` | "memory behind `&` is immutable" | no data races (it's `!Sync`); no reference into the interior may overlap a mutation | same as `T` | **hidden** |

The niche column is measured (§3), and it's a good summary of what each wrapper means to the compiler. A `MaybeUninit<&u8>`
might hold *anything*, including the null bit pattern, so the compiler can't use null to encode `None`. A
`ManuallyDrop<&u8>` is still a valid `&u8`. An `UnsafeCell<NonZeroU32>`'s bytes may change behind a shared
reference, so storing an enum's tag in them would let a tag change under an immutable `&Option<…>` (§4).

**The two methods of `MaybeUninit` that matter most:**

```text
 slot.write(v)          store v; do NOT read or drop what was there        (always correct on an uninit slot)
 *slot.as_mut_ptr() = v store v AFTER dropping "the old value" in place   (UB on an uninit slot: drops garbage)
 slot.assume_init()     "I promise this is a valid T now": the moment the validity invariant is asserted
```

**Drop check in one sentence.** When a type has a `Drop` impl, the compiler assumes `drop` might *use* every value
of every generic parameter, so borrowed data inside must strictly outlive the container. `#[may_dangle]` is an unsafe
promise that `drop` won't, and `PhantomData<T>` tells the compiler that dropping the container drops `T` values (whose
own destructors *might* use their borrows). §3 and §4 show all three cases.

### 3. Rust code

**Building an array slot by slot** (listing `ch03-01-uninit-array.rs`, clean under Miri):

```rust
// Initializing an array element by element: the MaybeUninit way, and the safe way that usually suffices.
use std::mem::MaybeUninit;

fn names_unsafe() -> [String; 4] {
    // An array of uninitialized slots: no String exists yet, so nothing can be dropped by accident.
    let mut slots: [MaybeUninit<String>; 4] = [const { MaybeUninit::uninit() }; 4];
    for (i, slot) in slots.iter_mut().enumerate() {
        slot.write(format!("worker-{i}")); // `write` stores without reading or dropping the old bytes
    }
    // SAFETY: all 4 slots were written above, and [MaybeUninit<String>; 4] has the same size and
    // layout as [String; 4] (MaybeUninit<T> is repr(transparent) over T).
    unsafe { std::mem::transmute::<[MaybeUninit<String>; 4], [String; 4]>(slots) }
}

fn names_safe() -> [String; 4] {
    std::array::from_fn(|i| format!("worker-{i}")) // the same result, no unsafe
}

fn main() {
    println!("{:?}", names_unsafe());
    println!("{:?}", names_safe());
    println!(
        "size_of MaybeUninit<String> = {}, String = {}, Option<String> = {}, MaybeUninit<Option<String>> = {}",
        size_of::<MaybeUninit<String>>(),
        size_of::<String>(),
        size_of::<Option<String>>(),
        size_of::<MaybeUninit<Option<String>>>()
    );
    println!(
        "Option<MaybeUninit<&u8>> = {} B vs Option<&u8> = {} B (MaybeUninit hides the niche)",
        size_of::<Option<MaybeUninit<&u8>>>(),
        size_of::<Option<&u8>>()
    );
}
```

```text
["worker-0", "worker-1", "worker-2", "worker-3"]
["worker-0", "worker-1", "worker-2", "worker-3"]
size_of MaybeUninit<String> = 24, String = 24, Option<String> = 24, MaybeUninit<Option<String>> = 24
Option<MaybeUninit<&u8>> = 16 B vs Option<&u8> = 8 B (MaybeUninit hides the niche)
```

> **Safety invariant (`names_unsafe`).** At the `transmute`, all four slots have been written with `write`, and no
> slot has been read or dropped. The array type change is layout-compatible because `MaybeUninit<T>` has `T`'s size
> and alignment.

Notice `names_safe`. `std::array::from_fn` does the same job with no `unsafe`, and it handles the case the unsafe
version gets wrong: a panic halfway through (below). Reach for `MaybeUninit` when the safe constructors can't express
the initialization pattern, not before.

**The one-character bug: assignment drops the old value** (listing `ch03-02-assign-drops-garbage.rs`):

```rust,ignore
    let mut slot: MaybeUninit<String> = MaybeUninit::uninit();
    let p = slot.as_mut_ptr();
    unsafe { *p = String::from("gateway") }; // BUG: assignment runs drop_in_place on uninitialized memory
```

Assignment to a place of type `String` means "drop the `String` that's there, then move the new one in." There
isn't one. Miri catches the drop reading the "old" `String`'s pointer field, inside `Vec<u8>`'s destructor:

```text
error: Undefined Behavior: reading memory at alloc217[0x8..0x10], but memory is uninitialized at [0x8..0x10], and
this operation requires initialized memory
   --> .../library/alloc/src/raw_vec/mod.rs:635:9
...
Uninitialized memory occurred at alloc217[0x8..0x10], in this allocation:
alloc217 (stack variable, size: 24, align: 8) {
    0x00 │ __ __ __ __ __ __ __ __ __ __ __ __ __ __ __ __ │
    0x10 │ __ __ __ __ __ __ __ __                         │
}
```

Every byte is `__`: uninitialized. Natively, this would pass a garbage pointer to the allocator's `free`. The fix is
the method whose name says what it does: `p.write(...)` or `slot.write(...)`.

**Where `MaybeUninit` came from: `mem::uninitialized`.** [VERSION] Before Rust 1.36 the tool was
`mem::uninitialized::<T>()`, which "returned" a `T` made of uninitialized memory, an invalid value by construction
for most `T`. It's been deprecated since 1.39. What it does today is a mitigation, not a fix (listings `ch03-03` and
`ch03-04`):

```text
release:  u64 = 0x0101010101010101, bool = true
debug:    thread 'main' (14) panicked at .../library/core/src/panicking.rs:225:5:
          attempted to leave type `&u64` uninitialized, which is invalid
          thread caused non-unwinding panic. aborting.
```

[RUSTC] [LIB] Current std fills the memory with `0x01` bytes, which happens to be valid for integers and `bool`, and
panics at run time for types where `0x01…01` is invalid (a reference must be aligned, and `0x0101010101010101` isn't
8-aligned). It's still UB by the language's rules for an integer, since the function's contract is to produce
uninitialized memory. Treat the fill as a seatbelt fitted to old code, not as behavior.

**Panic safety: leak or double drop?** Initializing four connections, where the third one fails (listing
`ch03-05-init-panic-leak.rs`):

```rust,ignore
fn open_all(fail_at: u32) -> [Conn; 4] {
    let mut slots: [MaybeUninit<Conn>; 4] = [const { MaybeUninit::uninit() }; 4];
    for (i, slot) in slots.iter_mut().enumerate() {
        let i = i as u32;
        if i == fail_at {
            panic!("connect failed at {i}");
        }
        slot.write(Conn(i));
    }
    // SAFETY: all slots written (we only get here if no panic happened).
    unsafe { std::mem::transmute::<[MaybeUninit<Conn>; 4], [Conn; 4]>(slots) }
}
```

```text
panicked: true, drops after unwinding: 0
opened: [0, 1, 2, 3]
drops after a successful open + drop: 4
```

On the panic path, connections 0 and 1 were created and never dropped. `MaybeUninit` doesn't drop its contents, so
the unwind drops nothing. That's **safe** (Chapter 15.1: leaks are safe) but it's a resource leak, and §10 shows what
it costs in production. The opposite mistake, dropping slots that were never written, *is* UB. The right answer drops
exactly the initialized prefix, which needs a drop guard (listing `ch03-06-init-guard.rs`, clean under Miri):

```rust,ignore
/// Owns the partially initialized array while it is being filled.
struct Guard<'a, T, const N: usize> {
    slots: &'a mut [MaybeUninit<T>; N],
    /// INVARIANT: slots[..init] are initialized.
    init: usize,
}

impl<T, const N: usize> Drop for Guard<'_, T, N> {
    fn drop(&mut self) {
        for slot in &mut self.slots[..self.init] {
            // SAFETY: slots[..init] are initialized (invariant) and dropped exactly once, here.
            unsafe { slot.assume_init_drop() };
        }
    }
}
```

```text
panicked: true, drops after unwinding: 2
opened: [0, 1, 2, 3]
drops after a successful open + drop: 6
```

Two drops on the panic path (connections 0 and 1), then four more for the successful call. On success, the function
`mem::forget`s the guard, so ownership passes to the returned array and nothing is dropped twice. Note the order in
the loop body: `write`, *then* `init += 1`. The other order would leave an uninitialized slot inside `[..init]` for
the moment between the two lines, and a panic there would be exactly the double-drop-of-garbage case.

> **Safety invariant (`Guard`).** `slots[..init]` are initialized and owned by the guard; `slots[init..]` are not.
> `init` only grows, and only after the slot is written. Either the guard is dropped (unwinding: it drops the prefix)
> or forgotten (success: the caller takes ownership of all `N`), never both.

**`ManuallyDrop`: control over when, and whether** (listing `ch03-07-manuallydrop.rs`, clean under Miri):

```rust,ignore
struct Session {
    // Fields drop in declaration order. We need the connection closed BEFORE the pool handle goes away,
    // whatever order a future refactor puts the fields in, so we drop `conn` explicitly.
    conn: ManuallyDrop<Conn>,
    #[allow(dead_code)] // held only for its Drop
    pool: PoolHandle,
}
```

```rust,ignore
impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: `conn` is dropped exactly once, here; it is never used after this line
        // (the compiler drops `pool` next and never touches a ManuallyDrop field).
        unsafe { ManuallyDrop::drop(&mut self.conn) };
    }
}
```

The same listing takes a `Vec` apart without freeing its buffer and rebuilds it. That's what `Vec::into_raw_parts`
does (stable on Rust 1.98; listing `ch03-20-into-raw-parts.rs` is the same round trip with it, clean under Miri). The
safe half gives the parts away, and the unsafe half, `from_raw_parts`, must get them back exactly once:

```text
drop(Session):
  conn closed
  pool handle released
rebuilt: [1, 2, 3] (len 3, cap 3)
Option<ManuallyDrop<&u8>> = 8 B, Option<&u8> = 8 B (ManuallyDrop keeps the niche)
```

`ManuallyDrop` is `mem::forget` you can take back. The value stays valid and usable, and it's just not dropped
automatically. The danger is the mirror image of the leak: dropping it twice. `ptr::read` makes that easy, because it
creates a second owner of the same heap buffer (listing `ch03-08-double-drop.rs`):

```rust,ignore
    let mut original = ManuallyDrop::new(String::from("token"));
    let copy: String = unsafe { std::ptr::read(&*original) }; // a second owner of the same buffer
    drop(copy); // frees the buffer
    unsafe { ManuallyDrop::drop(&mut original) }; // BUG: frees it again
```

```text
error: Undefined Behavior: constructing invalid value of type &mut [u8]: encountered a dangling reference
(use-after-free)
```

**`UnsafeCell`: the only door to mutation behind `&`** (listing `ch03-09-mycell.rs`, clean under Miri):

```rust
// A minimal Cell<T> over UnsafeCell: the ONLY sanctioned way to mutate behind a shared reference.
use std::cell::UnsafeCell;
use std::num::NonZeroU32;

pub struct MyCell<T> {
    value: UnsafeCell<T>,
}

impl<T: Copy> MyCell<T> {
    pub fn new(v: T) -> Self {
        MyCell { value: UnsafeCell::new(v) }
    }
    pub fn get(&self) -> T {
        // SAFETY: MyCell is !Sync (UnsafeCell is !Sync), so no other thread can access it; and we
        // never hand out references into the cell, so no reference can observe this read racing a write.
        unsafe { *self.value.get() }
    }
    pub fn set(&self, v: T) {
        // SAFETY: as in `get`: single-threaded, and no outstanding reference to the interior exists.
        unsafe { *self.value.get() = v }
    }
}

fn main() {
    let hits = MyCell::new(0u32);
    let r1 = &hits;
    let r2 = &hits; // two shared references, both able to mutate
    r1.set(r1.get() + 1);
    r2.set(r2.get() + 1);
    println!("hits = {}", hits.get());
    println!(
        "Option<NonZeroU32> = {} B, Option<UnsafeCell<NonZeroU32>> = {} B (UnsafeCell hides the niche)",
        size_of::<Option<NonZeroU32>>(),
        size_of::<Option<UnsafeCell<NonZeroU32>>>()
    );
}
```

```text
hits = 2
Option<NonZeroU32> = 4 B, Option<UnsafeCell<NonZeroU32>> = 8 B (UnsafeCell hides the niche)
```

> **Safety invariant (`MyCell<T>`).** No reference to the interior ever escapes (`get` copies out, `set` copies in,
> hence `T: Copy`), and the type is `!Sync`, so all accesses happen on one thread and never overlap. Those two facts
> are what std's `Cell` relies on too [LIB].

The `!Sync` half isn't something you have to remember: it's inherited from the field (listing
`ch03-11-mycell-not-sync.rs`):

```text
error[E0277]: `UnsafeCell<u32>` cannot be shared between threads safely
   = help: within `MyCell<u32>`, the trait `Sync` is not implemented for `UnsafeCell<u32>`
note: required because it appears within the type `MyCell<u32>`
   = note: required for `&MyCell<u32>` to implement `Send`
```

Making a cell `Sync` means taking on the concurrency half of the invariant yourself, with atomics or a lock, and
writing `unsafe impl Sync` with a justification. That's Chapter 11.3's `Mutex` and Part XIV's atomics.

**Drop check, three ways.** A `MyVec<T>` with a plain `impl Drop` (listing `ch03-13-dropck-own-drop.rs`) rejects a
program that std's `Vec` accepts (listing `ch03-14-dropck-std-vec.rs`):

```rust,ignore
fn main() {
    let mut names: MyVec<&String> = MyVec::new();
    let s = String::from("merchant-7"); // declared after `names`, so dropped BEFORE `names`
    names.push(&s);
} // error: `s` dropped here while still borrowed... borrow might be used when `names` is dropped
```

```text
error[E0597]: `s` does not live long enough
37 | } // error: `s` dropped here while still borrowed... borrow might be used when `names` is dropped
   | -
   | `s` dropped here while still borrowed
   | borrow might be used here, when `names` is dropped and runs the `Drop` code for type `MyVec`
```

The compiler can't see inside `drop`, so it assumes the worst: `drop` might read the `&String`s after `s` is gone.
std's `Vec` makes a promise the compiler can't check. Its `Drop` impl is `unsafe impl<#[may_dangle] T, A: Allocator>
Drop for Vec<T, A>` [LIB]: "my destructor doesn't use `T` values except by dropping them." Dropping a `&String` does
nothing, so `Vec<&String>` may outlive the `String` until the moment it's dropped. On nightly (`dropck_eyepatch`,
still unstable since RFC 1327 introduced it [VERSION]) you can make the same promise (listing `ch03-15-may-dangle.rs`, clean under
Miri):

```rust,ignore
// SAFETY: drop only DROPS the T values (through Vec's own drop); it never reads them otherwise.
unsafe impl<#[may_dangle] T> Drop for MyVec<T> {
    fn drop(&mut self) {
        // SAFETY: ptr/len/cap describe a buffer this MyVec owns; rebuilt exactly once, here.
        unsafe { drop(Vec::from_raw_parts(self.ptr, self.len, self.cap)) };
    }
}
```

The promise is about *your* destructor. The elements' own destructors are still dangerous, and that's what
`PhantomData<T>` is for. An element type whose `Drop` reads its reference (listing `ch03-16-may-dangle-inspector.rs`)
is still rejected with `E0597`, because `_owns: PhantomData<T>` tells drop check "dropping `MyVec<T>` drops `T`
values," and `T`'s drop glue is checked normally. Remove the `PhantomData` (listing `ch03-17-may-dangle-no-phantom.rs`)
and the program **compiles**, then reads freed memory:

```text
error: Undefined Behavior: constructing invalid value of type &std::string::String: encountered a dangling
reference (use-after-free)
    --> .../library/core/src/fmt/mod.rs:2872:71
```

That's Chapter 5.3's PhantomData table, row "drop check," made concrete: with `#[may_dangle]`, `PhantomData<T>` is
not decoration. It's the other half of the soundness argument.

> **Safety invariant (`#[may_dangle] T` on `MyVec<T>`).** `MyVec::drop` never reads, writes, or calls methods on a
> `T` value except by dropping it in place; and the type contains `PhantomData<T>` so that drop check still verifies
> `T`'s own destructor against the borrows it holds.

---

## Pass 2 · Systems level — *What each wrapper is, and what the compiler does with it*

### 4. Under the hood

**`MaybeUninit<T>` is a union.** [LIB] Its definition is essentially

```rust,ignore
#[repr(transparent)]
pub union MaybeUninit<T> {
    uninit: (),
    value: ManuallyDrop<T>,
}
```

(an excerpt of std, not a listing). A union has no validity invariant of its own beyond its size, which is what makes
"no valid `T` here yet" representable. The `ManuallyDrop` field is why a `MaybeUninit<T>` going out of scope never
drops anything. And a union field can be any bit pattern, so the compiler can't use a niche inside it (§3's 16 bytes).
`assume_init` is a plain read of the `value` field, and that read is where the validity invariant gets asserted: Miri
reports an invalid `bool` or an uninitialized integer *there* (Chapter 15.1's table), not where the bytes were written.

**`ManuallyDrop<T>` is a transparent struct the drop glue skips.** [LIB] [RUSTC] It's `#[repr(transparent)]` over `T`
and marked as a lang item, so drop elaboration (Chapter 3.1's drop flags) simply never schedules a drop for it. It
doesn't change validity, which is why its niche survives.

**`UnsafeCell<T>` is the one type the compiler treats specially for aliasing.** [LANG] [RUSTC] Chapter 4.1 showed the
IR: a `&i32` parameter is `noalias readonly`, and a `&Cell<i32>` gets neither, so the compiler reloads through it after
any opaque call. The rule underneath is language-level: *all* mutation through a shared reference must go through an
`UnsafeCell`, and the compiler may assume that memory reachable through a `&T` but not inside an `UnsafeCell` doesn't
change. Listing `ch03-10-shared-write-ub.rs` breaks that rule while hiding the cast from the lint of Chapter 15.2:

```rust,ignore
fn as_mut_ptr_from_shared<T>(r: &T) -> *mut T {
    r as *const T as *mut T
}

impl Counter {
    fn bump(&self) {
        let p = as_mut_ptr_from_shared(&self.hits);
        unsafe { *p += 1 }; // UB: this memory is behind a shared reference and not inside an UnsafeCell
    }
}
```

```text
error: Undefined Behavior: attempting a write access using <444> at alloc212[0x0], but that tag only grants
SharedReadOnly permission for this location
help: <444> was created by a SharedReadOnly retag at offsets [0x0..0x4]
10 |     r as *const T as *mut T
```

In Stacked Borrows terms (Chapter 15.2 §4): a pointer derived from a `&T` outside an `UnsafeCell` carries the
*SharedReadOnly* permission, and no cast can upgrade it. Inside an `UnsafeCell`, a shared reference gets
*SharedReadWrite* instead. That's the whole mechanism.

**Why `UnsafeCell` hides niches.** [RUSTC] If `Option<UnsafeCell<NonZeroU32>>` stored `None` as `0` inside the cell's
bytes, then code holding a plain `&Option<UnsafeCell<…>>` could watch the enum's discriminant change when someone
wrote through the cell. That would be a shared, non-`UnsafeCell` value (the `Option`'s tag) changing behind a shared
reference, which breaks the rule above. So rustc doesn't place niches inside `UnsafeCell`, and the `Option` gets its
own tag (8 bytes instead of 4, measured).

**Drop check in the compiler.** [LANG] [RUSTC] For a type with a `Drop` impl, the borrow checker treats the implicit
drop at scope end as a *use* of every generic parameter, so every lifetime in them must strictly outlive the value.
`#[may_dangle]` on a parameter removes that use for your destructor. Then drop check looks at the type's *fields* to
find what else gets dropped. A raw pointer `*mut T` owns nothing as far as the compiler knows, and `PhantomData<T>`
says "treat me as owning a `T`." That's why `ch03-17` compiled: with `#[may_dangle]` and no `PhantomData`, drop check
saw nothing that would drop a `T`. [VERSION] The rule has changed over the years. As the Rustonomicon's PhantomData
chapter describes it today, and as listings `ch03-13`, `ch03-16`, and `ch03-17` show on 1.98: without
`#[may_dangle]`, `PhantomData<T>` isn't what makes drop check strict (the `Drop` impl already counts as a use), and
with `#[may_dangle]`, it is.

### 5. Memory

`HeaderBlock<4>` (Chapter 15.1's module) mid-life, with the three states a slot can be in:

```text
 slots: [MaybeUninit<(String, String)>; 4]                                   len = 2
 ┌──────────────────────┬──────────────────────┬──────────────────────┬──────────────────────┐
 │ ("host", "api.m…")   │ ("x-req", "r-42")    │ ??????? (never        │ ??????? (was written, │
 │ initialized, owned   │ initialized, owned   │ written)              │ dropped by restore)   │
 └──────────────────────┴──────────────────────┴──────────────────────┴──────────────────────┘
   ◄──── [..len]: assume_init_ref OK, Drop drops these ────►◄──── [len..]: never read, never dropped ──►
```

Slot 3 is the one people forget: after the fixed `restore` (Chapter 15.1 §10) drops a value in place, the bytes
still look like a `(String, String)`
(stale pointers to freed buffers). To the abstract machine it's back to "not a valid value," and only `len` says so.

The pooled read buffer from §9, which avoids uninitialized memory altogether:

```text
 PooledBuf { buf: Vec<u8> (len == cap == 16 KiB, zeroed ONCE at creation), filled }
 ┌──────────── this read: buf[..filled] ────────────┬──────── stale bytes from earlier reads ───────┐
 │ current frame                                     │ initialized (so `read` may take &mut [u8]),  │
 │                                                   │ never exposed: only buf[..filled] is returned │
 └───────────────────────────────────────────────────┴───────────────────────────────────────────────┘
```

Stale bytes are *initialized* memory, so there's no UB. But they're other requests' data. Returning `&buf[..filled]`
and never `&buf` is a confidentiality rule, the same class of bug as Chapter 5.2's padding leak.

### 6. CPU / OS

- **Zeroing costs memory bandwidth, and it's measurable.** [CPU] The release assembly of listing
  `ch03-18-zeroing-codegen.rs` (`buf.clear(); buf.resize(n, 0); r.read(buf)`):

  ```text
  playground::fill:
  	mov	qword ptr [rdx + 16], 0          ; clear(): len = 0
  	...
  	call	qword ptr [rip + memset@GOTPCREL] ; resize(n, 0): zero n-1 bytes...
  	...
  	mov	byte ptr [rax], 0                  ; ...and the last one
  	...
  	mov	qword ptr [rdx + 16], rbx        ; len = n
  	mov	rax, qword ptr [rsi + 24]        ; Read::read from the vtable
  	jmp	rax                              ; tail call
  ```

  (trimmed; register saves omitted.) Listing `ch03-19-pooled-buffer.rs` measures the `memset` against a buffer that's
  zeroed once and kept initialized, for a 16 KiB read from an in-memory reader (release, three runs on the
  Playground, noisy): **166–175 ns per read** kept-initialized vs **287–293 ns** with clear-and-resize. So about
  115 ns per 16 KiB, cache-hot.
- **The OS already zeroes fresh pages.** [OS] A new allocation served by fresh `mmap`ed pages reads as zeros, and the
  kernel paid for that when it faulted the page in (Chapter 19.5). Recycled heap memory holds whatever was there.
  Neither fact makes uninitialized memory readable in Rust: the rule is about the abstract machine, not the bytes.
- **Why `Read::read` can't take uninitialized memory on stable.** [LIB] [VERSION] `read(&mut self, buf: &mut [u8])`
  receives a `&mut [u8]`, and a `u8` reference to uninitialized memory is an invalid value. std's answer,
  `BorrowedBuf` / `Read::read_buf`, tracks "filled" and "initialized" separately. On Rust 1.98 it's still unstable
  (listing `ch03-12-borrowed-buf-unstable.rs`):

  ```text
  error[E0658]: use of unstable library feature `core_io_borrowed_buf`
    = note: see issue #117693 <https://github.com/rust-lang/rust/issues/117693> for more information
  ```

- **`UnsafeCell` and the CPU.** [CPU] Nothing changes in hardware: `UnsafeCell` affects what the *compiler* may cache
  in registers. Across threads, visibility and ordering are the atomics' job (Part XIV), which is exactly why
  `UnsafeCell` alone is `!Sync`.

---

## Pass 3 · Architect level — *Which wrapper, if any?*

### 7. Trade-offs

| You need | First choice (safe) | Wrapper, when the safe tool can't express it | Cost of the wrapper |
|---|---|---|---|
| Fill an array element by element | `array::from_fn`, `Vec` + `try_into()` | `MaybeUninit` array + drop guard | a guard type, Miri tests, panic-safety review |
| A fixed-capacity inline container | `ArrayVec`-style crates, `[Option<T>; N]` | `[MaybeUninit<T>; N]` + `len` (15.1's `HeaderBlock`) | a module-level invariant (Chapter 15.1 §10) |
| Avoid zeroing a read buffer | keep the buffer initialized and reuse it (§9) | `BorrowedBuf` (unstable), or `set_len` after a trusted writer | UB if the writer is short (Chapter 15.2 §13) |
| Control drop order | declare fields in the order you want them dropped, with a comment | `ManuallyDrop` + explicit `drop` in `Drop` | one `unsafe` call, "exactly once" review |
| Hand ownership to C / rebuild later | `Box::into_raw` / `from_raw`, `Vec::into_raw_parts` (stable on 1.98, listing `ch03-20`) | `ManuallyDrop` + `from_raw_parts` | the pointer, length, and capacity must round-trip exactly, once |
| Mutate through `&self` | `Cell`, `RefCell`, `OnceCell`, `Mutex`, atomics | `UnsafeCell` | you're writing a synchronization primitive now |
| A container of borrowed data that outlives its referents | `Vec` / `HashMap` (they already use `#[may_dangle]`) | `#[may_dangle]` + `PhantomData<T>` (nightly) | an unstable feature and an easy-to-forget half |

> **Why not `[Option<T>; N]` instead of `[MaybeUninit<T>; N]`?** Often you should. It's safe, drops correctly, and
> costs nothing when `T` has a niche (`Option<String>` is 24 bytes, §3). It costs space when `T` has none, and a
> discriminant check per access. The `MaybeUninit` version is for when those costs are measured and matter, or when
> the layout must be exactly `[T; N]` (FFI).

> **Why not `mem::forget` instead of `ManuallyDrop`?** `forget` consumes the value, so you can't use it afterwards,
> and it can't be applied to a *field* of a struct you're still using. `ManuallyDrop` keeps the value usable in place
> and makes "not dropped automatically" part of the type, where a reviewer sees it.

### 8. Java comparison

| Java | Rust |
|---|---|
| No uninitialized memory is ever observable: fields and array elements start at `0`/`false`/`null` (JLS §4.12.5), and the JVM zeroes every allocation | Uninitialized memory exists; safe code can't observe it; `MaybeUninit` lets unsafe code *hold* it without observing it |
| HotSpot can skip zeroing when it proves an array is fully overwritten immediately (for example by an array copy) [LIB: HotSpot optimization, not a guarantee] | Skipping the zeroing is your explicit, `unsafe` decision, with the proof obligation that goes with it |
| No destructors; resources close via `try`-with-resources or `Cleaner`; finalization is deprecated for removal (JEP 421, JDK 18) | Destructors run deterministically; `ManuallyDrop` opts one value out |
| Every non-`final` field is mutable through any reference; the JIT can't assume a field is unchanged across unknown code | Memory behind `&T` is immutable unless it's in an `UnsafeCell`; that's what lets rustc emit `noalias readonly` (Chapter 4.1) |
| `final` fields: safe publication after construction (JLS §17.5), though HotSpot doesn't treat most instance `final`s as truly constant, since reflection can change them | `&T` immutability is a compile-time rule the optimizer relies on; breaking it via a cast is UB (§4), not a reflection trick |

> **Analogy limit.** It's tempting to say "`UnsafeCell` is Rust's `volatile`." It isn't. Java's `volatile` is about
> *inter-thread visibility and ordering*. `UnsafeCell` is about *the compiler's aliasing assumptions*, and by itself
> it's `!Sync`: it gives you no cross-thread guarantees at all. The Java construct closest to a Rust type built on
> `UnsafeCell` is a class whose every method is `synchronized`, which is what `Mutex<T>` gives you (Chapter 11.3), with
> the lock inside the type.

### 9. Production scenario

**Meridian's gateway read buffers: the `memset` that wasn't worth `unsafe`.** The gateway (Chapter 1.2: about 400K
requests/s at peak, roughly 0.5 ms of CPU per request) reads upstream responses into pooled 16 KiB buffers. A
profiling session showed `memset` in the read path, from `clear()` + `resize(16 * 1024, 0)` before each `read`
(listing `ch03-18`). A PR proposed replacing the `resize` with `set_len` "because `read` overwrites the buffer anyway."

The review rejected it on two grounds. The first is Chapter 15.2's debugging exercise: `read` may return fewer bytes,
and then the tail is uninitialized memory behind a `&mut [u8]`, which is UB with no short-read input needed to make
it so. The second was the number. Listing `ch03-19` measured the `memset` at about 115 ns per 16 KiB read, cache-hot.
Even if every request did one such read, that's 115 ns out of about 500 µs of CPU, **roughly 0.02%**. The saving
was real but tiny, and `unsafe` would have made the gateway's policy (Chapter 15.1 §9) route the module to the unsafe
owners forever.

What shipped instead gets the same saving with no `unsafe` at all (listing `ch03-19-pooled-buffer.rs`):

```rust,ignore
/// INVARIANT: `buf.len() == buf.capacity() == SIZE` for the whole life of the buffer (all bytes initialized);
/// only `buf[..filled]` holds data from the current read. Bytes after `filled` are stale and never exposed.
struct PooledBuf {
    buf: Vec<u8>,
    filled: usize,
}

impl PooledBuf {
    fn new(size: usize) -> Self {
        PooledBuf { buf: vec![0; size], filled: 0 } // the only zeroing, once per pooled buffer
    }
    fn read_from(&mut self, r: &mut dyn Read) -> std::io::Result<&[u8]> {
        self.filled = r.read(&mut self.buf)?; // `read` gets an initialized &mut [u8]: nothing to zero
        Ok(&self.buf[..self.filled]) // expose only what this read produced
    }
}
```

The buffer is zeroed once when the pool creates it and stays initialized forever, so every later `read` gets a valid
`&mut [u8]`. The invariant that matters is a *confidentiality* invariant, not a memory-safety one: only `buf[..filled]`
is ever exposed. The team added a test that reads a long frame and then a short one, and asserts the second result
contains none of the first frame's bytes.

### 10. Failure scenario

**The warm-up that leaked connections.** Meridian's payments-core service (Chapter 8.4) keeps a small fixed set of
connections to the card processor per worker, built at start-up by a function shaped like listing `ch03-05`: an array
of `MaybeUninit<Conn>` slots, filled in a loop, `transmute`d at the end. It passed code review and Miri, because it
contains no UB.

During a processor maintenance window, the third connection of each warm-up failed, and the code panicked on it (an
`expect` on the connect result). The task supervisor caught the panic (Chapter 8.3's per-task containment) and
retried warm-up every few seconds. Each failed attempt had already opened two connections, and the panic dropped
neither: `drops after unwinding: 0`. The sockets stayed open with no owner. By the time maintenance ended, the
processor's per-client connection limit was full of connections payments-core had forgotten, and new connections were
refused. The outage outlasted the maintenance window, until a rolling restart closed the leaked sockets.

The fix had three parts:

1. **Code:** the guard from listing `ch03-06`. A failed warm-up now drops exactly the connections it opened
   (`drops after unwinding: 2`).
2. **Better code:** on review, the team replaced the `MaybeUninit` array with a safe version: collect into a
   `Vec<Conn>` with `?` (no panic on connect failure, Chapter 8.1) and convert with `try_into()` to `[Conn; 4]`. `Vec`'s
   own `Drop` handles the partial case, and the module lost its `unsafe` entirely.
3. **Lesson for reviews:** "no UB" isn't the same as "correct." For `MaybeUninit` code, the review question is *what
   happens to the initialized prefix if anything between the first `write` and the `assume_init` panics?* Leaks are
   safe, and they can still take a service down.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XV).*

1. Which assumption does each of `MaybeUninit`, `ManuallyDrop`, and `UnsafeCell` switch off, and what obligation does
   each create?
2. Why is `*slot.as_mut_ptr() = value` a bug on an uninitialized slot while `slot.write(value)` isn't? What did Miri
   report, and in which function?
3. `Option<MaybeUninit<&u8>>` is 16 bytes and `Option<ManuallyDrop<&u8>>` is 8. Explain both. Why must
   `UnsafeCell` hide niches?
4. What does `mem::uninitialized` do on Rust 1.98, why was it deprecated, and why is its `0x01` fill not a fix?
5. A function initializes an array of `N` values and panics at element `k`. Describe the leak outcome and the
   double-drop outcome. Which is UB? How does a drop guard avoid both?
6. Compare `ManuallyDrop`, `mem::forget`, and `Option::take` for controlling when a value is dropped. When is each the
   right tool?
7. Why is `UnsafeCell` the only legal way to mutate through `&T`? What does rustc emit differently for `&UnsafeCell<T>`
   than for `&T`, and what permission does Miri give each?
8. Why does a `MyVec<&String>` with a plain `Drop` impl fail to compile where `Vec<&String>` succeeds? What does
   `#[may_dangle]` promise, and what goes wrong if `PhantomData<T>` is missing?
9. Why can't a stable `Read::read` write into uninitialized memory, and what are your options when zeroing a buffer
   shows up in a profile?

### 12. Exercises

- **Beginner.** Which of these are UB, and why? (a) `MaybeUninit::<u8>::uninit().assume_init()`;
  (b) `MaybeUninit::<[u8; 4]>::zeroed().assume_init()`; (c) `MaybeUninit::<&u8>::zeroed().assume_init()`;
  (d) `MaybeUninit::<bool>::zeroed().assume_init()`; (e) `MaybeUninit::<String>::zeroed().assume_init()`. Check your
  answers under Miri.
- **Intermediate.** Fix listing `ch03-02-assign-drops-garbage.rs` two ways and run both under Miri. Then write
  `fn replace_slot(slot: &mut MaybeUninit<String>, initialized: bool, v: String)` that's correct in both states.
- **Advanced.** Implement `fn try_array_from_fn<T, E, const N: usize>(f: impl FnMut(usize) -> Result<T, E>) ->
  Result<[T; N], E>` with `MaybeUninit` and a drop guard. Test it under Miri with an `f` that fails at every index and
  one that panics. Then write the safe version with `Vec` + `try_into()` and compare the release assembly.
- **Systems.** Run listing `ch03-19-pooled-buffer.rs` several times, then change `SIZE` to 256 KiB and 4 MiB. Explain
  how the gap changes when the buffer no longer fits in the caches. At what size does zeroing start to matter against
  a 1 µs syscall?
- **Architecture.** Find every place in a codebase you know where correctness depends on struct field declaration
  order for drop order. For each, choose: reorder with a comment, `ManuallyDrop` + explicit drop, or an explicit
  `close()` method. Justify the choice by who will edit the struct next.

### 13. Debugging exercise

A request counter in a small internal library (listing `ch03-10-shared-write-ub.rs`):

```rust,ignore
struct Counter {
    hits: u32,
}

fn as_mut_ptr_from_shared<T>(r: &T) -> *mut T {
    r as *const T as *mut T
}

impl Counter {
    fn bump(&self) {
        let p = as_mut_ptr_from_shared(&self.hits);
        unsafe { *p += 1 }; // UB: this memory is behind a shared reference and not inside an UnsafeCell
    }
}
```

1. Chapter 15.2 showed a deny-by-default lint for this pattern. Why doesn't it fire here?
2. The program prints `1` natively in the author's tests. What does Miri report, and what does "SharedReadOnly" mean?
3. Describe a caller of `bump` for which the optimizer may make the program print a stale count, using what rustc
   tells LLVM about `&self`.
4. Fix it for single-threaded use, then for use from several threads. Which fix makes `Counter` `Sync`, and why is
   the first one not?

### 14. Design exercise

**An inline ring of recent trades.** The market-data service keeps, per instrument, the most recent 64 trades
(Chapter 9.1's design exercise used 40,000 instruments and a 48-byte `Trade`). A proposal: `InlineRing<T, const N:
usize>` backed by `[MaybeUninit<T>; N]`, with `head` and `len`, and no heap allocation per instrument.

- Write the invariant precisely, including wrap-around: which slots are initialized for a given `head` and `len`?
- `push` on a full ring must drop the oldest element and store the new one. In which order, and what happens if
  `T::drop` panics halfway? Write the `// SAFETY:` comments for `push`, `iter`, and `Drop`.
- Is `InlineRing<T, N>` `Send`/`Sync` automatically? Should it be? What does `MaybeUninit<T>` inherit from `T`?
- Compare against `VecDeque::with_capacity(64)` per instrument and `[Option<Trade>; 64]` in total memory for 40,000
  instruments, allocation count, and code you must audit.
- Write the Miri test plan: which operations, which panic injections, and which planted bug proves the tests work.
