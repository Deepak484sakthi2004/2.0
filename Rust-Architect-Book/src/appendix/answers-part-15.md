# Appendix A — Answer Key: Part XV

> Model answers. Write yours first. Where several answers are defensible, the key says so. Outputs quoted here come
> from the verified listings in `listings/part-15/` (rustc 1.98.1); "predicted" marks reasoning you should check.
>
> This key covers Chapters 15.1–15.3. Chapters 15.4–15.6 and the Part review aren't written yet (see the Part XV
> overview), so their answers aren't here either.

---

## Chapter 15.1 — What `unsafe` Means: Soundness and Invariants

### Interview & architecture questions

**1. The five superpowers.** Dereferencing a raw pointer; calling an `unsafe fn` (including foreign functions and
intrinsics); reading or writing a `static mut`; implementing an `unsafe trait`; reading a `union` field. Everything
else still applies inside an `unsafe` block: type checking, borrow checking of references, privacy, and the bounds
check on `v[i]` (and debug overflow checks). Edition 2024 adds two *declarations* that must be marked unsafe
(`unsafe extern` blocks and unsafe attributes such as `#[unsafe(no_mangle)]`), but they're promises, not new
operations.

**2. `unsafe fn` vs `unsafe { }`.** An `unsafe fn` *defines* an obligation: its `# Safety` section lists what every
caller must guarantee. An `unsafe` block *discharges* one: its `// SAFETY:` comment says why the guarantee holds
here. Before edition 2024, an `unsafe fn` body was one big implicit unsafe block, so a function that exists to state a
precondition could also silently perform unrelated unsafe operations with no justification. The 2024
`unsafe_op_in_unsafe_fn` warning ("an unsafe function restricts its caller, but its body is safe by default") keeps
the two directions separate: each operation inside gets its own block and its own comment.

**3. Soundness.** A safe API is sound if no safe program, using it in any way the type system allows, can cause
undefined behavior. A function with no `unsafe` can be unsound when it breaks an invariant that `unsafe` code
elsewhere in the module relies on: Chapter 15.1's `restore(checkpoint)` set `len` beyond the initialized slots with no
`unsafe` keyword in sight. (Safe code that exploits a compiler soundness bug such as #25860 is a different case: the
*compiler* is unsound there.) A function containing `unsafe` can be sound: `my_split_at_mut` (Chapter 15.2) and
`Vec::push` both are, because every safe input keeps their preconditions true.

**4. Validity vs safety.** A *validity* invariant is defined by the language and must hold whenever a value is
produced or copied, even in private code: a `bool` is 0 or 1, a `&T` is non-null, aligned, and dereferenceable, a
`char` is a Unicode scalar value, an enum has a valid discriminant, an integer is initialized. Violating it is
immediate UB. A *safety* invariant is defined by a type's author and must hold whenever safe code can observe the
value: `Vec`'s `len <= cap` with `[0, len)` initialized, `str`'s UTF-8, `Rc`'s count. The owning module may break it
temporarily; UB happens only when code that relies on it runs.

**5. Null references and uninitialized integers.** Non-null is part of what a reference *is*, and rustc tells LLVM so
(`nonnull`), uses it for niches (`Option<&T>` is 8 bytes), and folds null checks away (Chapter 15.1 §4's
`is_null_ref` compiled to `xor eax, eax`). So the invalid value is UB the moment it exists, used or not. Uninitialized
memory isn't "some bit pattern": to the abstract machine it's no value at all, and LLVM may treat each use of it as a
different arbitrary value. A `u32` made from it can therefore fail `x == x`-style reasoning, and the optimizer is
allowed to assume it never happens.

**6. The helper that returned 2.** Some unsafe code produced a `bool` holding the byte 2 (a `transmute`, a bad read
from a buffer, an FFI value). Because a `bool` is 0 or 1, `if b { 1 } else { 0 }` is the identity on its byte, and
rustc compiled it to `mov eax, edi`. The function faithfully returned its input. No compiler bug: the UB happened
earlier, where the invalid `bool` was created, and that's exactly where Miri stops (`constructing invalid value of
type bool: encountered 0x02`).

**7. Private fields vs a caller's `Ord`.** A module's private fields can only be written by the module's own code, so
the module is one proof, and its unsafe code may rely on them. `Ord`, `Hash`, `Eq`, `ExactSizeIterator::len`, and
`Deref` are *safe* traits: any safe code may implement them wrongly, and a wrong answer must be at worst a logic
error, never UB. So unsafe code must stay sound whatever they return. When std genuinely needs to trust a length, it
asks for an *unsafe* trait, `TrustedLen` (unstable), whose implementers promise an exact `size_hint` as a safety
obligation.

**8. `#[no_mangle]` and `unsafe extern`.** An unmangled exported symbol can collide with another symbol of the same
name (`malloc`, `open`) and replace it at link time, which is UB for every caller of the original. Edition 2024
therefore requires `#[unsafe(no_mangle)]`. A foreign declaration is a promise about a signature the compiler can't
see: wrong argument types or ABI make every call UB, so the declaring block is `unsafe extern`, and items with no
preconditions can be marked `safe`.

**9. #25860.** Implied bounds (a `&'a &'b ()` implies `'b: 'a`) combined with a coercion to a higher-ranked
function pointer let safe code turn a short lifetime into `'static`, producing a dangling reference with no `unsafe`.
On rustc 1.98.1 the classic snippet is rejected (`lifetime may not live long enough`), but a higher-ranked variant
still compiles, and Miri reports the dangling reference. It means "safe Rust can't cause UB" holds relative to a
correct compiler. The hole needs contrived code, it's tracked as `I-unsound`, and the practical lesson is the same as
for `unsafe`: keep the trusted base small, and test with Miri.

### Debugging exercise (`collect_exact`)

1. **Not sound.** `collect_exact` lets a *safe* trait's answer decide which memory it writes. The listing's `Liar`
   implements `ExactSizeIterator::len` as `2` and yields four items, with no `unsafe` anywhere. The loop writes items
   3 and 4 past the end of a 16-byte allocation.
2. **Debug build:** the out-of-bounds writes aren't checked (`ptr::write` has no bounds check), so the heap is
   corrupted silently. Then `set_len(4)` hits std's debug precondition check:
   ```text
   unsafe precondition(s) violated: Vec::set_len requires that new_len <= capacity()
   This indicates a bug in the program. This Undefined Behavior check is optional, and cannot be relied on for safety.
   ```
   and the process aborts, *after* the damage. **Miri** stops at the first bad write, in the loop:
   ```text
   error: Undefined Behavior: memory access failed: attempting to access 8 bytes, but got alloc317+0x10 which is at
   or beyond the end of the allocation of size 16 bytes
   ```
3. **Fix** (listing `ch01-16-robust-to-safe-trait.rs`, clean under Miri): use `len()` only as a capacity *hint* and
   `push` each item. In the common case (an honest `len`), that's still one allocation; a lying `len` costs a
   reallocation, never UB. If profiling ever shows the push path matters, keep a check against `out.capacity()` inside
   the loop. `ExactSizeIterator` is safe because implementing it carries no unsafe obligation; std's unstable
   `unsafe trait TrustedLen` is the version whose answer unsafe code may trust.

### Selected exercises

- **Beginner.** `3u8 → bool`: UB (validity). `0x110000 → char`: UB (above the last scalar value). A `&[u8]` of length
  0 with a null pointer: UB, because references and slices must be non-null even when empty (use
  `NonNull::dangling()`, as `Vec` does). A `String` with non-UTF-8 bytes that's never read: not immediate UB, since it
  breaks a *safety* invariant, but any std method may turn it into UB. `MaybeUninit::<[u8; 4]>::uninit().assume_init()`:
  UB (uninitialized integers).
- **Advanced (`if b { 10 } else { 20 }`)** (listing `ch01-19-bool-ten-twenty.rs`): release prints `pick(b) = 10`.
  The assembly:
  ```text
  playground::pick:
  	test	edi, edi
  	mov	ecx, 10
  	mov	eax, 20
  	cmovne	eax, ecx
  	ret
  ```
  For a valid `bool`, "not zero" and "equal to 1" are the same test, and LLVM picked `test` + `cmovne`, so any
  non-zero byte behaves as `true`. With `if b { 1 } else { 0 }` it picked an instruction sequence that simply returns
  the byte. Same UB, different visible outcome, decided by an instruction-selection choice. That's why "what does this
  UB do?" has no stable answer.
- **Systems (predicted).** A loop whose index comes from data, such as `total += prices[idx[i]]`, keeps one bounds
  check per element, and that check usually blocks vectorization. `get_unchecked` is sound under exactly one fact:
  every `idx[i] < prices.len()`. An `assert!` over all indices before the loop is a pre-pass the optimizer generally
  can't carry into the loop, so it doesn't produce the same code. Validating the indices where they're *created*
  (and storing them as a newtype that guarantees the bound) is the design fix. Measure before and after, several runs.

### Design exercise (`AsciiStr`)

- (a) Safe validating constructor: the invariant is enforced at construction by a run-time check. Only the type's own
  module needs review. Sound.
- (b) (a) plus `unsafe fn new_unchecked`: the invariant becomes the parser's documented obligation at each call site.
  Sound, and the parser's call sites join the proof.
- (c) A *safe* `new_unchecked` guarded by `debug_assert!`: **unsound**. In release, `AsciiStr::new_unchecked(&[0xFF])`
  is a safe call that creates a non-ASCII `AsciiStr`. If the type offers `as_str()` implemented with
  `from_utf8_unchecked` (the natural implementation, since ASCII is valid UTF-8), that safe program ends in UB.
- (d) `&str` plus a comment: no guarantee, but nothing unsafe relies on it, so nothing is unsound, just unchecked.

Recommendation: (a), unless a measurement says otherwise. Checking ASCII is a byte scan that std vectorizes, and at
400K requests/s with a dozen short header names per request the validation volume is on the order of 100 MB/s (an
estimate), far below what a vectorized scan handles (predicted from mechanism). The measurement that would change the
decision: `is_ascii` cost as a share of parser CPU in a profile of the real parser.

---

## Chapter 15.2 — Raw Pointers, Provenance, and Aliasing Models

### Interview & architecture questions

**1. Provenance.** The permission a pointer carries: which allocation it may access, and with which aliasing rights.
Two pointers can have equal addresses and different provenance. In listing `ch02-03-provenance.rs`,
`pa.with_addr(pb.addr())` compares equal to `pb`, but it's a pointer into `a`, and Miri reports the read as an access
"at or beyond the end of the allocation of size 8 bytes."

**2. `add` vs `wrapping_add`.** `add` lowers to `getelementptr inbounds nuw`, a promise to LLVM that the result stays
inside the object (the result is *poison* otherwise), and the optimizer may use that promise for comparisons and
alias analysis whether or not you dereference. So leaving the allocation is UB at the arithmetic, as Miri reported
("in-bounds pointer arithmetic failed"). `wrapping_add` lowers to a plain `getelementptr`: no promise, and no new
access rights. In release, both compile to `lea rax, [rdi + 4*rsi]` and LLVM merged the two functions. Same machine
code, different contracts.

**3. Strict provenance.** `addr()` gives the address as a plain `usize` without provenance; `with_addr(a)` and
`map_addr(f)` return a pointer with a new address and the *original* provenance. They let you do bit tricks (tags,
alignment masks) without turning a pointer into an integer and back, which is what used to lose provenance
information. Miri warns on integer-to-pointer casts that it "might miss pointer bugs in this program" and suggests the
strict APIs (and `-Zmiri-strict-provenance` to enforce them).

**4. Stacked Borrows in five sentences.** Every reference, and every raw pointer derived from one, gets a tag.
Creating a reference is a *retag*: it pushes a new tag onto a per-location stack for the bytes the reference covers.
Each location's stack lists the tags currently allowed to access it. Using a pointer finds its tag in the stack and
pops the tags above it (a write pops everything above; a read pops the unique ones), revoking their rights. "Tag does
not exist in the borrow stack" means the pointer you used was revoked by a later use of a parent or sibling.

**5. Tree Borrows and a fresh `&mut`.** Tree Borrows keeps a tree of tags and a permission per tag. A new `&mut`
starts **Reserved**: it doesn't claim exclusivity until its first write. Creating the second half in `split_twice` is
a new sibling, not an access to `left`'s bytes, so `left` is never invalidated. In `two_mut(s, 0, 0)` both references
are *used*: the write through `a` is a foreign write for `b`, which transitions to Disabled, and the next access
through `b` is UB (the report said exactly that).

**6. `split_at_mut` and one `as_mut_ptr()`.** Deriving both halves from one raw pointer means nothing reborrows the
parent slice again. Calling `as_mut_ptr()` twice creates a second exclusive reborrow of the *whole* slice, which under
Stacked Borrows invalidates the first half ("invalidated … by a Unique function-entry retag"). Tree Borrows accepts it.
Write the version that passes both.

**7. Two `&mut` from one slice.** Every index in bounds, and all indices pairwise distinct; then derive all
references from one pointer. Checking distinctness is O(N²) comparisons for N indices. `get_disjoint_mut` takes a
fixed-size array (a small constant N), so that's a handful of comparisons. For large N, you'd sort first.

**8. 70 vs 100.** `transfer` computed `*from - 30` into `rax`, stored it, added 30 through `to`, and returned `rax`.
It didn't reload `*from`, because both parameters are `&mut` and rustc told LLVM they're `noalias`, so the store
through `to` can't change `*from`. `transfer` isn't buggy: it's correct for every input its signature allows. The bug
is `two_mut` manufacturing two `&mut` to one location. Debug (no such optimization) printed 100 and 100.

**9. Zero-copy without `unsafe`.** `from_be_bytes` on a checked 8-byte sub-slice borrows the buffer (no copy of the
frame) and compiles to one compare, one unaligned load, and one `bswap`. The raw cast was UB (alignment) *and* wrong
(host byte order). `zerocopy` or `bytemuck` are right when you want a typed view of large structures or arrays of
records, or need to write them back: their derives check size, alignment, and validity at compile time and the length
at the cast.

### Debugging exercise (`checksum_frame`)

1. `set_len`'s contract is about the moment it's called: the first `new_len` elements must *already* be initialized.
   And `Read::read` may return fewer bytes than asked (a short read), and its implementations aren't forbidden from
   reading `buf` (std's docs tell *callers* not to rely on how `buf` is used). A `&mut [u8]` over uninitialized bytes
   is the wrong type for that memory whatever `read` does.
2. Miri, with three input bytes and `n = 8`, stops at the first uninitialized byte the sum reads, index 3:
   ```text
   error: Undefined Behavior: reading memory at alloc318[0x3..0x4], but memory is uninitialized at [0x3..0x4], and
   this operation requires initialized memory
   alloc318 (Rust heap, size: 8, align: 1) {
       01 02 03 __ __ __ __ __
   }
   ```
3. (a) `let mut buf = vec![0u8; n];`, then sum only `buf[..got]` (the original also had a logic bug: it summed `n`
   bytes when only `got` were read). (b) A pooled buffer that stays initialized and is reused (Chapter 15.3 §9's
   `PooledBuf`), exposing only `[..got]`. (c) `spare_capacity_mut` gives `&mut [MaybeUninit<u8>]`, but stable
   `Read::read` only accepts `&mut [u8]`. The API for reading into uninitialized memory, `BorrowedBuf` /
   `Read::read_buf`, is unstable on Rust 1.98 (`E0658`, issue #117693), so (c) needs nightly.

### Selected exercises

- **Beginner.** (a) `p.add(len)`: fine, one past the end. (b) `*p.add(len)`: UB, one-past-the-end isn't a readable
  place. (c) `p.add(len + 1)`: UB at the arithmetic. (d) `p.wrapping_add(len + 1)`: fine, no promise and no access.
  (e) Wrapping out and back in, then dereferencing: fine. Wrapping arithmetic keeps the pointer attached to the same
  allocation, and std's `wrapping_offset` docs explicitly allow going out of bounds and coming back.
- **Intermediate.** Take `let p = s.as_mut_ptr();` once and build both halves from `p` and `p.add(mid)` (that's
  `ch02-07`). `split_three` asserts `a <= b <= len` and derives three slices from the same `p`.
- **Advanced.** `ptr::swap` allows its two pointers to be equal or overlapping, so `swap_elems(s, i, i)` is fine with
  it; `ptr::swap_nonoverlapping` with `i == j` is UB. std's `slice::swap` checks both indices, then uses `ptr::swap`.
  Handling `i == j` as an early return is also correct, and cheaper to reason about.
- **Systems (predicted).** With `*mut i64` parameters the compiler can't assume the pointers differ, so you should see
  `*from` reloaded after the write through `to` (a `mov rax, qword ptr [rdi]` after `add qword ptr [rsi], 30`). Verify
  it with `tools/emit.ps1`. In a loop of a million calls the difference is one load that hits L1, so expect it to be
  in the noise. `noalias` pays off when it enables vectorization or hoisting in loops, not in a function like this.

### Design exercise (tagged connection pointer)

- **Invariant:** the entry's pointer is a `Box<Conn>` pointer, from `Box::into_raw`, with its low two bits replaced by
  the state, and its provenance is the box's. It's turned back into a `Box` exactly once, in `Drop` (or in a
  `take()` that consumes the entry), after clearing the tag bits. Enforce the alignment at compile time with
  `const _: () = assert!(align_of::<Conn>() >= 4);` next to the type.
- **APIs:** `NonNull::from(Box::leak(b))` or `Box::into_raw`, then `map_addr(|a| a | tag)` to set, `addr() & 0b11` to
  read, `map_addr(|a| (a & !0b11) | new)` to update, and `map_addr(|a| a & !0b11)` before `Box::from_raw`. No
  `Clone`, or a deep clone that allocates a new `Conn`.
- **Memory:** 8 bytes × 1M = 8 MB per pod (arithmetic). The `Conn` allocation itself, its buffers, and the map's
  own overhead (keys, control bytes, load factor) are each far larger, so the saving is under a few percent of the
  table (an estimate; measure with the counting allocator). The cheaper alternative with no `unsafe`: store the state
  *inside* `Conn`, where it likely fits in existing padding. Under the gateway's policy (Chapter 15.1 §9), the packed
  pointer isn't worth a new unsafe module.
- **Miri plan:** create, retag the state several times, move entries between maps, drop, under both models. Planted
  bug: skip clearing the tag before `Box::from_raw`. Expect Miri to report the deallocation through a pointer that
  isn't the start of the allocation (predicted; running it is part of the exercise).

---

## Chapter 15.3 — `MaybeUninit`, `ManuallyDrop`, and `UnsafeCell`

### Interview & architecture questions

**1. What each switches off.** `MaybeUninit<T>` switches off "this is a valid, initialized `T`" (and so automatic
drop): you owe tracking which slots are initialized, calling `assume_init*` only on valid data, and dropping what you
initialized. `ManuallyDrop<T>` switches off automatic drop: you owe dropping exactly once, or deliberately never, and
never using the value afterwards. `UnsafeCell<T>` switches off "memory behind `&` doesn't change": you owe freedom from
data races (it's `!Sync`) and no reference into the interior overlapping a mutation.

**2. `*p = v` vs `write`.** Assignment drops the value already in the place, then moves the new one in. On an
uninitialized `MaybeUninit<String>` there's no value, so the drop reads a garbage `String`. Miri caught it reading the
uninitialized pointer field (`alloc217[0x8..0x10]`) inside `alloc::raw_vec`, called from `Vec<u8>`'s `Drop`.
`p.write(v)` / `slot.write(v)` store without reading or dropping.

**3. Niches.** A `MaybeUninit<&u8>` may hold any bytes, including zero, so the null niche can't encode `None`, and
`Option` adds a tag (16 bytes). A `ManuallyDrop<&u8>` is still a valid `&u8`, so the niche survives (8 bytes). An
`UnsafeCell`'s bytes can change behind a shared reference. If `Option<UnsafeCell<NonZeroU32>>` kept its tag in those
bytes, a write through the cell could change the `Option`'s discriminant while someone holds a plain `&Option<…>`,
which breaks the immutability of memory outside `UnsafeCell`. So rustc hides niches inside `UnsafeCell` (8 bytes,
measured, instead of 4).

**4. `mem::uninitialized`.** It was deprecated in Rust 1.39 because its contract (return a `T` made of uninitialized
memory) produces an invalid value for almost every `T`, which is UB by construction. On 1.98 it fills the memory with
`0x01` bytes (a `u64` reads `0x0101010101010101`, a `bool` reads `true`), and panics at run time for types where that
pattern is invalid (`attempted to leave type &u64 uninitialized, which is invalid`, since the pattern isn't
8-aligned). The fill is a mitigation for old code, not a definition of behavior: the contract is still UB.

**5. Panic during initialization.** *Leak:* the initialized prefix is never dropped (listing `ch03-05`: 0 drops after
unwinding). Safe, but a resource leak. *Double drop / drop of garbage:* dropping slots that were never written, or
dropping a value twice. UB. A drop guard owns the prefix while it's being built: it records `init` after each `write`,
drops exactly `[..init]` if unwinding reaches it (listing `ch03-06`: 2 drops), and is `mem::forget`-ed on success so
the finished array owns everything.

**6. `ManuallyDrop` vs `mem::forget` vs `Option::take`.** `Option::take` is the safe choice when a `None` state is
acceptable: `self.conn.take()` in a `Drop` impl drops the connection early, and the type pays for the `Option` (often
nothing, thanks to niches). `ManuallyDrop` keeps a field as a plain `T` and makes "dropped manually" part of the type:
use it for drop order in `Drop` impls, for FFI handoff, and inside unsafe containers. `mem::forget` consumes a whole
value and never drops it: use it when ownership has moved somewhere the compiler can't see (a guard on success, a
buffer handed to C).

**7. `UnsafeCell` and `&T`.** The language rule is that all mutation through a shared reference goes through an
`UnsafeCell`; everywhere else, the compiler may assume memory behind `&T` doesn't change. rustc emits `noalias
readonly` for `&T` parameters and neither for types containing an `UnsafeCell` (Chapter 4.1's IR). In Miri's Stacked
Borrows, a `&T` outside an `UnsafeCell` gets *SharedReadOnly* permission (listing `ch03-10`'s error), and inside one,
*SharedReadWrite*. `MyCell` is `!Sync` because `UnsafeCell` is. Making it `Sync` requires atomics or a lock inside and
an `unsafe impl Sync` whose comment explains why concurrent access is race-free.

**8. Drop check.** With a plain `Drop` impl, the compiler treats dropping `MyVec<&String>` as a possible use of every
`&String`, so `s` must strictly outlive the vector (`E0597`, "borrow might be used here, when `names` is dropped").
std's `Vec` uses `unsafe impl<#[may_dangle] T, A: Allocator> Drop`, promising its destructor only drops `T` values.
`PhantomData<T>` then tells drop check that `T` values *are* dropped, so `T`'s own destructor is still checked
(listing `ch03-16` stays `E0597`). Without it, the program in listing `ch03-17` compiles and reads a freed `String`.

**9. Reading into uninitialized memory.** `Read::read` takes `&mut [u8]`, which must refer to initialized bytes, and
implementations may read `buf`. Options on stable: keep a reused buffer initialized (zero once, `PooledBuf`); use
`read_to_end` into a `Vec` (std manages the initialized part of the spare capacity itself [LIB]); or use
`BorrowedBuf`/`read_buf` on nightly. First, measure: zeroing 16 KiB cost about 115 ns per read on the Playground,
against a request's roughly 500 µs of CPU at the gateway.

### Debugging exercise (`Counter::bump`)

1. `invalid_reference_casting` recognizes a cast from `&T` to `*mut T` followed by a write *in the same function*.
   Here the cast is inside a generic helper, `as_mut_ptr_from_shared`, and the write is in `bump`. The lint can't
   connect them.
2. Miri: `attempting a write access using <444> at alloc212[0x0], but that tag only grants SharedReadOnly permission
   for this location`, with `<444>` created by a *SharedReadOnly retag* at the cast. *SharedReadOnly* is the
   permission any pointer derived from a `&T` outside an `UnsafeCell` gets: read, never write. No cast upgrades it.
3. (Predicted from mechanism.) A function `fn report(c: &Counter) -> (u32, u32) { let a = c.hits; c.bump(); (a,
   c.hits) }`. `c` is `&Counter` with no `UnsafeCell`, so rustc marks it `noalias readonly`, and LLVM may reuse the
   first load of `c.hits` for the second, returning `(0, 0)` after an increment (the same effect as Chapter 4.1's
   `add eax, eax`).
4. Single-threaded: `hits: Cell<u32>` and `self.hits.set(self.hits.get() + 1)`. Multi-threaded:
   `hits: AtomicU32` and `self.hits.fetch_add(1, Ordering::Relaxed)` (a statistics counter needs no ordering, Part XIV).
   The atomic version makes `Counter` `Sync`. `Cell` is `!Sync` by design, because its `get` + `set` is a
   non-atomic read-modify-write that two threads would race on.

### Selected exercises

- **Beginner.** (a) UB (uninitialized integer). (b) Fine: all-zero bytes are a valid `[u8; 4]`. (c) UB: a null
  reference. (d) Fine: 0 is `false`. (e) UB: a `String` holds a `Vec`, whose buffer pointer is a `NonNull`, and zero is
  invalid for it.
- **Intermediate.** `unsafe { p.write(String::from("gateway")) }` or `slot.write(String::from("gateway"))`.
  `replace_slot`: `if initialized { unsafe { slot.assume_init_drop() } } slot.write(v);`, with a `// SAFETY:` that
  `initialized` is true exactly when the slot holds a value, and a note on who updates that flag.
- **Advanced.** The `MaybeUninit` version is the `ch03-06` guard with an early `return Err(e)` path, where the guard's
  `Drop` handles both errors and panics. The safe version, `(0..N).map(f).collect::<Result<Vec<T>, E>>()?.try_into()`,
  is shorter, allocates once, and gets partial-failure cleanup from `Vec`'s `Drop` for free. Choose it unless the
  allocation is measurably the problem.
- **Systems (predicted, order of magnitude).** At 16 KiB both buffers stay in L1/L2, and the gap is one cache-hot
  `memset` (~115 ns measured). At 256 KiB and 4 MiB the `memset` and the copy start streaming to and from L3 or DRAM.
  The gap grows roughly linearly with size: at ~10–20 GB/s, zeroing 4 MiB is a few hundred microseconds, far more than
  a 1 µs syscall. Zeroing starts to matter when buffers are large and reads are small, which is also the case where
  keeping the buffer initialized helps most.

### Design exercise (`InlineRing<T, N>`)

- **Invariant:** `len <= N`, and the initialized slots are exactly `(head + k) % N` for `k in 0..len`. All others are
  uninitialized.
- **`push` on a full ring:** move the oldest value *out* first (`assume_init_read`), `write` the new value into that
  slot, advance `head`, and only then drop the old value. If `T::drop` panics, the ring is already consistent: every
  slot the invariant calls initialized holds a live value. The reverse order (drop, then write) leaves an initialized-
  looking slot holding a dropped value if the drop panics, which `Drop` would drop again.
- **Auto traits:** `MaybeUninit<T>` is `Send`/`Sync` exactly when `T` is, so `InlineRing<T, N>` inherits the same, which
  is correct: it owns its `T`s like an array does. No `unsafe impl` needed.
- **Memory (arithmetic):** `InlineRing<Trade, 64>` is 64 × 48 = 3,072 bytes plus two `usize`s, inline. For 40,000
  instruments that's about 123 MB, with no allocation per instrument. `VecDeque::with_capacity(64)` needs a 24-byte
  header plus one heap block of the same 3 KiB per instrument: 40,000 allocations plus allocator overhead.
  `[Option<Trade>; 64]` is 3 KiB if `Trade` has a niche, and 64 × 56 = 3.5 KiB if it doesn't, with no unsafe at all.
  The safe array is usually the right answer, and the `MaybeUninit` version needs a measured reason.
- **Miri plan:** push past capacity several times, drop a partly full ring, drop after wrap-around, clone, and push
  with a `T` whose `Drop` panics on a flag. Planted bug: an off-by-one in `Drop`'s wrap-around loop. Expect Miri to
  report an uninitialized read or a double drop (predicted).

---

*Chapters 15.4–15.6, the Part XV review capstone, and interview mode continue below.*
