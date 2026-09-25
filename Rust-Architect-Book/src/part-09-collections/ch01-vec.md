# Chapter 9.1 — Vec<T>: Pointer, Length, Capacity

> **Where this sits:** Part IX · Collections and Memory · chapter 1 of 5 (plus an interlude)
> **Prerequisites:** Chapter 1.1 (the `std::vector` reallocation diagram), Chapter 3.1 (ownership trees), Chapter 4.1
> (iterator invalidation, `retain`).
> **After this chapter you can:** draw a `Vec<T>` and its buffer from memory; predict how many allocations a sequence of
> pushes costs on today's std, and say which part of that prediction is a guarantee; read the assembly of `push`; choose
> between arrays, slices, `Box<[T]>`, `Vec<T>`, and `SmallVec`; and explain why an `ArrayList<Long>` and a `Vec<u64>`
> have the same Big-O and very different constant factors.

---

## Pass 1 · User level — *Three words on the stack, one buffer on the heap*

### 1. Problem

Chapter 1.1 drew a C++ `std::vector` reallocating under a live reference, and promised two things: that Rust's `Vec`
has the same physical layout and growth, and that the growth policy is deliberately unspecified. This chapter delivers
the details. `Vec` is the collection you'll use for most of your Rust career, and it's the benchmark every other
collection in this Part is measured against. The questions an architect asks about it are:

- What exactly does a push cost, including the rare expensive one?
- When does memory come back, and when does it stay allocated long after the data is gone?
- What does the element type do to all of the above?

### 2. Mental model

A `Vec<T>` is three machine words, owned by whoever owns the `Vec`, plus one heap buffer that the `Vec` owns:

```text
 stack (or inside the parent struct)          heap (one allocation, align_of::<T>())
 ┌──────────────────────────┐
 │ cap = 8                  │                  ┌────┬────┬────┬────┬────┬────┬────┬────┐
 │ ptr ─────────────────────┼─────────────────►│ e0 │ e1 │ e2 │ e3 │ e4 │ ?? │ ?? │ ?? │
 │ len = 5                  │                  └────┴────┴────┴────┴────┴────┴────┴────┘
 └──────────────────────────┘                  ◄──── len: initialized ──►◄─ spare capacity ─►
   24 bytes on x86-64                            (uninitialized memory, never read by safe code)
```

- **`len`** counts initialized elements. Only `[0, len)` may be read.
- **`cap`** counts slots the buffer has room for. `[len, cap)` is uninitialized, and safe code can't observe it.
- **`ptr`** is never null. An empty `Vec` that hasn't allocated holds a *dangling but aligned* pointer, so
  `Option<Vec<T>>` can use null as `None` and stays 24 bytes (verified below).

[LANG] The std docs do guarantee some of this: `Vec` is a (pointer, capacity, length) triple, `Vec::new()` and
`with_capacity(0)` don't allocate, the buffer is always allocated with the global allocator, and the elements are
contiguous so `&v[..]` is a plain slice. The **order** of the three fields is not guaranteed ([RUSTC] picks it; the
assembly below shows capacity first). The **growth policy** isn't guaranteed either: the docs say `push` is amortized
O(1) and leave the factor to the implementation.

The growth rule you should carry, labelled for what it is:

> [LIB] **Today's std:** when a push finds `len == cap`, the new capacity is `max(2 × cap, cap + 1)`, and never below a
> minimum: 8 for 1-byte elements, 4 for elements up to 1 KiB, 1 for larger ones. The old buffer is reallocated: grown in
> place if the allocator can, otherwise copied to a new block and freed.

### 3. Rust code

**The growth sequence, measured with the counting allocator** (listing `ch01-01-vec-growth.rs`, verified in debug and
release on rustc 1.98.1):

```rust,ignore
fn growth<T: Default>(n: usize) -> (Vec<usize>, usize) {
    let mut caps = Vec::with_capacity(64); // allocated BEFORE measuring
    let mut v: Vec<T> = Vec::new();
    caps.push(v.capacity());
    let ((), allocs, _) = measure(|| {
        for _ in 0..n {
            v.push(T::default());
            if v.capacity() != *caps.last().unwrap() {
                caps.push(v.capacity());
            }
        }
    });
    (caps, allocs)
}
```

```text
Vec<u8>,   100 pushes: capacities [0, 8, 16, 32, 64, 128], 5 allocator calls
Vec<u64>,  100 pushes: capacities [0, 4, 8, 16, 32, 64, 128], 6 allocator calls
Vec<Big>,   10 pushes: capacities [0, 1, 2, 4, 8, 16], 5 allocator calls  (Big = 2048 bytes)
with_capacity(100) + 100 pushes: 1 allocation
reserve(5): 10 -> 20; reserve_exact(5): 10 -> 15
truncate(10) keeps capacity 1000; shrink_to_fit -> 10 (1 alloc, 1 free)
Vec::new(): 0 allocations
```

Read it line by line:

- **Doubling, with a size-dependent start.** One-byte elements start at 8 (tiny allocations are wasteful, since the
  allocator's minimum chunk is bigger anyway), elements up to 1 KiB start at 4, and a 2 KiB element starts at 1.
- **`with_capacity(n)` is one allocation.** Tell the `Vec` what you know.
- **`reserve(5)` on a full `Vec` of 10 went to 20, not 15.** `reserve` means "at least this much more", and it keeps the
  amortized doubling. `reserve_exact(5)` asks for exactly 15 (the allocator may still hand back more; the `Vec` just
  won't request more).
- **`truncate` never frees.** Capacity stays at 1000 after truncating to 10. `shrink_to_fit` reallocates down. Here that
  shows as "1 alloc, 1 free", because our counting allocator uses `GlobalAlloc`'s default `realloc`, which is
  allocate + copy + free.
- **`Vec::new()` allocates nothing.** Creating empty vectors is free, so there's no reason to share one or keep it in a
  `static`.

**Collecting: when the size is known, you pay once** (listing `ch01-04-collect-sizing.rs`, verified):

```text
range.collect():                 1 allocation(s), cap 10000
range.filter().collect():        11 allocation(s), len 3334 cap 4096
with_capacity + extend(filter):  1 allocation(s), len 3334 cap 3334
vec.into_iter().map().collect(): 0 allocation(s), len 10000
```

`collect` asks the iterator for its `size_hint`. A `Range` knows its exact length, so one allocation fits. `filter`
can't know how many items survive (its lower bound is 0), so the `Vec` grows from 4 and ends with 762 unused slots.
If *you* know an upper bound, `with_capacity` plus `extend` gets back to one allocation. The last line is a [LIB]
specialization: when you consume a `Vec` with `into_iter` and collect elements of the same size and alignment back into
a `Vec`, std reuses the original buffer in place. Part X shows how that specialization is wired.

**Removing many elements: `retain`, not a loop of `remove`** (listing `ch01-03-retain-vs-remove.rs`, release, one run
on a shared machine):

```text
n = 50000, survivors = 25000
remove() in a loop: 56.3438ms
retain():           29.3µs
```

Chapter 4.1 promised the complexity story, and here it is with numbers. Each `remove(i)` shifts the whole tail left by
one with a `memmove`, so removing half the elements one at a time is O(n²): about 25,000 × 37,500 average moves. `retain`
walks once with a read index and a write index, moving each survivor at most once: O(n). The measured ratio is about
1,900×, and it grows linearly with n.

The rest of `Vec`'s operations, by cost:

| Operation | Cost | What moves |
|---|---|---|
| `push`, `pop` | amortized O(1), worst case O(n) at a reallocation | nothing, or the whole buffer |
| `v[i]`, `get(i)` | O(1), one bounds check | nothing |
| `insert(i, x)`, `remove(i)` | O(n − i) | the tail, by `memmove` |
| `swap_remove(i)` | O(1) | the last element into slot i (order not preserved) |
| `retain`, `dedup` | O(n) | each survivor at most once |
| `drain(a..b)` | O(n − a) | removed items are yielded, then the tail shifts once when the `Drain` drops |
| `truncate(k)`, `clear()` | O(len − k) for drop glue, O(1) for `Copy` types | nothing; capacity kept |
| `extend_from_slice` (`T: Copy`) | O(k) | one `memcpy` (after at most one reservation) |
| `split_off(i)` | O(n − i) | the tail into a new allocation |

---

## Pass 2 · Systems level — *What `push` compiles to, and what the allocator does*

### 4. Under the hood

**The push fast path.** Here's `push` for a `Vec<u64>`, compiled in release (listing `ch01-05-push-asm.rs`, fetched with
`tools/emit.ps1 -Target asm -Mode release`, trimmed):

```text
playground::push_one:                         ; rdi = &mut Vec<u64>, rsi = x
	push	r15
	push	r14
	push	rbx
	mov	r14, qword ptr [rdi + 16]             ; r14 = len
	cmp	r14, qword ptr [rdi]                  ; len == cap ?
	je	.LBB0_1                               ; yes: cold path
.LBB0_2:
	mov	rax, qword ptr [rdi + 8]              ; rax = ptr
	mov	qword ptr [rax + 8*r14], rsi          ; ptr[len] = x
	inc	r14
	mov	qword ptr [rdi + 16], r14             ; len += 1
	pop	rbx
	pop	r14
	pop	r15
	ret
.LBB0_1:
	mov	rbx, rdi
	mov	r15, rsi
	call	qword ptr [rip + <alloc::raw_vec::RawVec<u64>>::grow_one@GOTPCREL]
	mov	rsi, r15
	mov	rdi, rbx
	jmp	.LBB0_2
```

Three things to notice:

1. [RUSTC] **The field order is (cap, ptr, len)**: `[rdi]` is compared against the length, `[rdi + 8]` is dereferenced,
   and `[rdi + 16]` is incremented. rustc chose that order; the language doesn't promise it, so never transmute a `Vec`
   or build one from raw parts in any order other than `Vec::from_raw_parts(ptr, len, cap)`.
2. **The common case is a compare, a store, and an increment.** There's no bounds check on the write, because the
   capacity check already proved the slot exists. The expensive path is out of line, and the callee is `grow_one`, not
   the allocator.
3. **The register saves (`push r15`, and so on) happen on the fast path too**, because the function has to preserve
   registers in case the slow path runs. When `push` is inlined into a loop, as it normally is, LLVM usually hoists this
   away. `#[inline(never)]` here exists only so we can look at it.

**The growth policy, in the machine code.** `grow_amortized` is the function that implements the rule stated in the
mental model:

```text
<alloc::raw_vec::RawVecInner>::grow_amortized:
	inc	rsi                     ; required = len + 1
	mov	rax, qword ptr [rdi]    ; rax = cap
	lea	rcx, [rax + rax]        ; rcx = 2 * cap
	cmp	rsi, rcx
	cmovbe	rsi, rcx            ; new_cap = max(required, 2 * cap)
	cmp	rsi, 5
	mov	r14d, 4
	cmovae	r14, rsi            ; new_cap = max(new_cap, 4)   <- the minimum for 8-byte elements
	...
	call	<alloc::raw_vec::RawVecInner>::finish_grow
```

`finish_grow` then checks the byte size for overflow (`shr rcx, 61` tests whether `cap × 8` overflows; the constant
`9223372036854775800` is `isize::MAX` rounded down to the alignment, because [LANG] no allocation may exceed
`isize::MAX` bytes) and calls `__rust_realloc` if a buffer exists or `__rust_alloc` if not. A failed allocation reaches
`handle_error`, which aborts via the allocation-error handler: [RUNTIME] out of memory in `Vec::push` is not a panic you
can catch, it ends the process. (`try_reserve` is the fallible API for when you must survive it; Part XV.)

**Amortized O(1), precisely.** With doubling, building an n-element `Vec` by pushes copies at most `n/2 + n/4 + ... < n`
elements in total across all reallocations, so the average cost per push is O(1). What doubling doesn't give you is a
bound on any *single* push: the push that triggers a reallocation of a 32 MiB buffer copies 32 MiB (unless the allocator
avoids it; see Section 6). That's the latency spike to plan for.

**Why 2× and not 1.5×?** C++ implementations differ (libstdc++ and libc++ double, MSVC grows by 1.5×; Chapter 1.1), and
Java's `ArrayList` grows by 1.5×. The classic argument for a factor below the golden ratio is that the sum of all
previously freed blocks eventually exceeds the next request, so a simple allocator can reuse the freed space. With 2×
that never happens. The counter-argument, and a reasonable reading of Rust std's choice of 2×, is that it means fewer reallocations and fewer copies, and that general-purpose allocators rarely
reuse a vector's freed blocks for that same vector in the way the argument assumes. The reason it isn't a
guarantee is exactly so this trade-off can be revisited.

### 5. Memory

**What lives where** (listing `ch01-06-sizes.rs`, verified):

```text
                    [u64; 4]  32 bytes
                      &[u64]  16 bytes
                  Box<[u64]>  16 bytes
                    Vec<u64>  24 bytes
            Option<Vec<u64>>  24 bytes
          SmallVec<[u64; 4]>  48 bytes
SmallVec<[u64; 4]>, 4 pushes: 0 allocations
SmallVec<[u64; 4]>, 5 pushes: 1 allocation (spilled: true)
Vec (len 10, cap 100) -> Box<[u64]>: 1 alloc, 1 free, len 10
```

That's the SPEC's "stack vs heap" question, answered by type:

| Type | Inline part | Heap part | Resizable | Use it for |
|---|---|---|---|---|
| `[T; N]` | all N elements | none | no, N is part of the type | small, fixed-size data; beware large N on 2 MiB thread stacks (Chapter 2.4's export crash) |
| `&[T]` / `&mut [T]` | (ptr, len), 16 bytes | borrowed from someone else | no | function parameters: accepts arrays, `Vec`s, `Box<[T]>`, and sub-slices |
| `Box<[T]>` | (ptr, len), 16 bytes | exactly len elements | no | frozen data held long-term: saves 8 bytes per handle and any spare capacity |
| `Vec<T>` | (cap, ptr, len), 24 bytes | cap slots | yes | the default |
| `SmallVec<[T; N]>` | N elements plus bookkeeping | only after spilling | yes | many small collections where the common size is known and small |

`SmallVec` (the `smallvec` crate, [LIB]) is 48 bytes here: 32 bytes of inline storage plus the length and a
discriminant. It's a real trade-off, not a free win. Every access checks "inline or spilled?", the handle is twice the
size of a `Vec`, and moving it copies the inline elements. It pays off when most instances stay under N, because each
one saves an allocation and the pointer chase to reach its elements.

**Ownership behavior.** The `Vec` owns its elements. Dropping it runs each element's drop glue in order, index 0 first
[LANG], then frees the buffer. Moving a `Vec` copies 24 bytes, no matter how many elements it holds; the buffer doesn't
move. `clone` allocates a new buffer and clones every element (for `T: Copy`, one `memcpy`). And a `&T` into the buffer
is invalidated by any reallocation, which is exactly why the borrow checker forbids `push` while an element borrow is
alive (Chapter 4.1).

**Memory that doesn't come back.** `clear`, `truncate`, `pop`, `drain`, and `retain` never shrink the buffer. That's
deliberate: a `Vec` reused as a buffer should keep its capacity. It also means a `Vec` that once held a burst of data
keeps its high-water mark forever unless you call `shrink_to_fit` or `shrink_to(n)`. Section 10 is a production failure
built on exactly this.

### 6. CPU / OS

**Contiguity is the whole point.** A `Vec<u64>` of a million elements is 8 MB of consecutive memory. Scanning it reads
consecutive 64-byte cache lines, the hardware prefetcher recognizes the stream and fetches ahead [CPU], and LLVM can
vectorize the loop. Chapter 9.5 measures what happens when you lose that.

**What the allocator does on growth.** [OS] This listing (`ch01-02-realloc-moves.rs`, release, *system* allocator)
pushes 4 million `u64`s and records whether the buffer's address changed at each growth:

```text
21 growth events, buffer address changed in 10 of them
  cap        4 (       32 bytes): moved
  cap        8 (       64 bytes): moved
  cap       16 (      128 bytes): grew in place
  ...                                                  (every step in between: grew in place)
  cap    16384 (   131072 bytes): grew in place
  cap    32768 (   262144 bytes): moved
  cap    65536 (   524288 bytes): moved
  ...                                                  (every step up to 4194304: moved)
```

"Moved" doesn't always mean "copied". The pattern matches glibc malloc's design (this is an inference; verify it
locally with `strace -e trace=mmap,mremap,brk ./target/release/realloc-moves`):

- **Small blocks** come from the heap arena. A block at the end of the heap can often grow in place into free space,
  which is what the long run of "grew in place" shows.
- **Blocks at or above the mmap threshold** (128 KiB by default in glibc, adjusted dynamically) get their own `mmap`.
  Growing one uses `mremap(..., MREMAP_MAYMOVE)`, which lets the kernel move the *virtual address* by rewriting page
  table entries. The bytes aren't copied, so it costs roughly O(pages), not O(bytes).

So on Linux with glibc, a large `Vec`'s growth is cheaper than the "allocate, copy, free" textbook picture. Under
musl, jemalloc, or mimalloc (Meridian's gateway images use musl + mimalloc, Chapter 2.1) the behavior differs. Never
design around it; `with_capacity` is the portable answer.

**Page faults on first touch.** [OS] A fresh large allocation is usually just reserved virtual memory. The first write
to each 4 KiB page faults and the kernel supplies a zeroed page. `Vec::with_capacity(1 << 27)` returns quickly; the
cost arrives as page faults when you fill it. That's worth knowing when you're reading a latency profile and wondering
why the first request after startup was slow.

**Concurrency.** `Vec<T>` has no internal synchronization. It's `Send` if `T: Send` and `Sync` if `T: Sync`, so a
`&Vec<T>` can be shared across threads for reading. For parallel mutation you split it:
`chunks_mut`, `split_at_mut`, or `rayon`'s `par_iter_mut` hand each thread a disjoint `&mut [T]`, which the borrow
checker can prove disjoint (Chapter 4.1, Part XI). Two threads writing adjacent elements can still slow each other down
through false sharing (the same cache line); Part XIV measures that.

---

## Pass 3 · Architect level — *Choosing the container and its capacity policy*

### 7. Trade-offs

**`Vec<T>` vs `LinkedList<T>` in brief** (Chapter 9.4 measures it): the linked list wins only when you need O(1)
splice of whole lists or stable node addresses. It loses on memory (a 24-byte node per element plus allocator overhead,
against 8 bytes), allocation (one per element against about log₂ n), cache (pointer chasing against sequential
scanning), and usually on time, even for "insert in the middle", because finding the middle is a linear walk either
way.

**Capacity policy is an API decision.** A function that returns a `Vec` decides whether the caller inherits spare
capacity. A long-lived structure decides whether to hold `Vec<T>` (growable, 24 bytes, possibly half empty) or
`Box<[T]>` (frozen, 16 bytes, exact). Meridian's route table (Chapter 3.1) is rebuilt every 10 minutes and never
mutated in between, so storing it as `Box<[Route]>` removes up to half of its memory for free.

**The decision matrix** (SPEC comparison row: `Vec<T>` vs `&[T]` vs `Box<[T]>` vs `SmallVec`):

| Criterion | `Vec<T>` | `&[T]` | `Box<[T]>` | `SmallVec<[T; N]>` |
|---|---|---|---|---|
| Memory | 24 + cap·size | 16, borrows | 16 + len·size | inline N·size + bookkeeping, heap after spill |
| CPU | push: compare + store | none of its own | none of its own | a branch on every access |
| Latency | occasional realloc spike | none | none | spill spike, once |
| Throughput | excellent (contiguous) | excellent | excellent | excellent until spilled |
| Contention | none built in | shared reads are free | shared reads are free | none built in |
| Cache | sequential | sequential | sequential | inline: same line as the parent |
| Allocation | ~log₂ n, or 1 presized | none | 1 | 0 until spill |
| Complexity | lowest | lowest | low | medium (choose N) |
| Safety | fully safe | fully safe | fully safe | safe API, `unsafe` inside a crate you audit |
| Maintainability | the default everyone reads | the best parameter type | signals "frozen" | needs a comment justifying N |
| Failure modes | retained capacity; realloc invalidation (compile error) | lifetime tangles | none notable | N too small: always spills, now slower than `Vec` |
| Operational | RSS keeps high-water mark | none | predictable RSS | RSS depends on the size distribution |

### 8. Java comparison

| Java | Rust | Where the analogy holds, and where it breaks |
|---|---|---|
| `ArrayList<E>` | `Vec<T>` | Both: array + size, amortized O(1) add, grow by copying. Java grows by 1.5× (`old + (old >> 1)`); a `new ArrayList<>()` defers allocating its default 10 slots until the first `add`. |
| `ArrayList<Long>` | `Vec<u64>` | **Breaks.** Java stores references to separately allocated `Long` objects. Rust stores the values inline. |
| `long[]` | `Vec<u64>` / `Box<[u64]>` | Close: both contiguous primitives. Java arrays can't grow; `Box<[u64]>` can't either. |
| `ensureCapacity(n)` / `trimToSize()` | `reserve(n)` / `shrink_to_fit()` | Same intent. |
| `Collections.unmodifiableList` | `Box<[T]>`, `&[T]`, or `Arc<[T]>` | Rust's immutability is a type-level fact, not a runtime wrapper. |
| `ConcurrentModificationException` | E0502 at compile time | Chapter 4.1. |

**The boxing gap, as arithmetic.** A `Vec<u64>` with a million elements is 8 MB. An `ArrayList<Long>` with a million
distinct values is a 4 MB reference array (with compressed oops) plus a million `Long` objects. Each object is a 12-byte
header plus an 8-byte `long`, padded to 24 bytes (16 with the compact object headers that became a product option in
JDK 25, hedged: check your JDK's flags). That's roughly 28 MB total, with each element one pointer chase away from the
array. The Big-O is identical; the constant factors aren't.

> **Analogy limit.** It's tempting to say "`Vec<T>` is `ArrayList<T>` without boxing". That holds for `u64`, but for
> `Vec<String>` both designs store pointers to separate heap blocks, and Rust's advantage shrinks to the missing object
> headers. What Rust gives you in every case is the *choice*: `Vec<[u8; 16]>` for fixed-size IDs, `Vec<Box<str>>` for
> frozen strings, one big `String` plus a `Vec<Range<u32>>` of offsets for millions of short strings. Project Valhalla's
> value classes aim to bring flattened layouts to Java; as of this writing they're still a preview/early-access
> feature, so check the status for the JDK you deploy.

> **Why not always `with_capacity`?** Because a wrong guess costs either way. Too small and you pay the growth anyway;
> too large and you've allocated memory you'll never use. Too large also has a subtler cost: a huge `with_capacity` on
> an attacker-controlled length (a length prefix read from the network) is a memory-exhaustion vector. Cap any
> capacity you take from input: `Vec::with_capacity(declared_len.min(MAX_PREALLOC))`.

### 9. Production scenario

**Gateway header vectors.** Meridian's Rust gateway prototype parses request headers into a `Vec<(HeaderName,
HeaderValue)>` per request. A counting-allocator test on recorded traffic (the technique from this chapter) shows the
typical request has 11 to 14 headers, so each request's header `Vec` grows 0 → 4 → 8 → 16: three allocations and two
copies. At the gateway's 400K requests per second peak, that's 1.2 million allocations per second spent on one vector.

The team considered three options:

1. `Vec::with_capacity(16)`: one allocation per request, covering 99% of requests.
2. `SmallVec<[_; 16]>`: zero allocations for 99% of requests. But the handle becomes large (16 inline pairs), and
   moving the request struct between pipeline stages copies all of it.
3. A per-connection `Vec` reused across requests with `clear()`: zero allocations after warm-up, at the cost of
   retaining the largest header count that connection has ever seen.

They shipped option 3, with a cap: after each request, if `capacity() > 64`, the buffer is replaced with a fresh
`Vec::with_capacity(16)`. That combines reuse with a bound on retained memory, and it's a pattern you'll see again in
Section 10 and in Part XIII's buffer pools.

### 10. Failure scenario

**The batch buffer that never shrank.** Meridian's ingestion pipeline (Chapter 3.2) batches events before publishing.
Each worker owns a `Vec<Event>` it `clear()`s after every flush. A partner replayed six hours of backlog, and one batch
reached 2.1 million events (about 400 MB at ~190 bytes per event, spare capacity included). After the replay, traffic
returned to normal batches of a few thousand events, but every worker that had seen a burst kept its 400 MB buffer.
Resident memory stayed high, the pods' memory limits were set for normal operation, and over the following week
Kubernetes OOM-killed workers whenever a second burst hit an already-bloated worker.

- **Detection:** RSS never came back down after the replay, although heap profiling showed live data had.
  `capacity()` exported as a metric per worker made it obvious.
- **Root cause:** `clear()` keeps capacity (documented: it "has no effect on the allocated capacity"), so the buffer's high-water mark became its
  permanent size.
- **Fix:** after a flush, `if buf.capacity() > 4 * TYPICAL_BATCH { buf = Vec::with_capacity(TYPICAL_BATCH) }` (or
  `buf.shrink_to(TYPICAL_BATCH)`), plus a hard cap on batch size so one partner's backlog becomes several batches.
- **Lesson:** every reused buffer needs an explicit capacity policy: what it keeps, what it gives back, and when.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part IX).*

1. Draw a `Vec<u64>` with len 5 and cap 8. What's guaranteed about its layout, and what isn't?
2. What does today's std do when `push` finds `len == cap`? Which parts of your answer are [LANG] and which are [LIB]?
3. Prove that doubling gives amortized O(1) push. What does it *not* bound?
4. Why is `Option<Vec<T>>` the same size as `Vec<T>`?
5. What's the difference between `reserve` and `reserve_exact`? Between `truncate` and `shrink_to_fit`?
6. Why is removing matching elements with `remove` in a loop O(n²), and how does `retain` avoid it?
7. When does `collect` allocate exactly once, and when does it grow? What does `vec.into_iter().map(f).collect()` cost
   when the element size doesn't change?
8. Compare `Vec<T>`, `Box<[T]>`, `&[T]`, `[T; N]`, and `SmallVec<[T; N]>` by where their bytes live.
9. What does a `Vec`'s growth cost on Linux with glibc for small and for large buffers, and why shouldn't you design
   around it?
10. Compare the memory footprint of `Vec<u64>` and `ArrayList<Long>` for a million elements. Where does the analogy
    between them break?

### 12. Exercises

- **Beginner.** Predict the capacity sequence of a `Vec<u16>` over 100 pushes and a `Vec<[u8; 1500]>` over 10 pushes.
  Check both by adapting `ch01-01-vec-growth.rs`.
- **Intermediate.** Write `fn dedup_sorted(v: &mut Vec<u32>)` three ways: with `remove`, with `retain` plus a
  "previous" variable, and with `Vec::dedup`. Measure all three on 100,000 elements with many duplicates.
- **Advanced.** Implement a `Vec`-backed stack with a capacity policy: it shrinks to half when len falls below a quarter
  of capacity. Explain why the thresholds must differ (what goes wrong with "shrink at half"?).
- **Systems.** Run `ch01-02-realloc-moves.rs` locally under `strace -e trace=mmap,mremap,munmap,brk`. Confirm or refute
  the chapter's inference about which growths use `mremap`. Then rerun with `MALLOC_MMAP_THRESHOLD_=1048576` and
  explain the change.
- **Architecture.** List every long-lived `Vec` in a service you know that is reused with `clear()`. For each, state its
  capacity policy today (probably "none") and the one you'd adopt.

### 13. Debugging exercise

```rust,ignore
fn load_ids(bytes: &[u8]) -> Vec<u64> {
    let count = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;
    let mut ids = Vec::with_capacity(count);
    for chunk in bytes[4..].chunks_exact(8) {
        ids.push(u64::from_be_bytes(chunk.try_into().unwrap()));
    }
    ids
}
```

1. This passes every unit test. A fuzzer finds a 4-byte input that aborts the process with
   `memory allocation of 34359738360 bytes failed`. Which input, and why is it an abort rather than a panic?
2. Why doesn't the loop itself help, even though it pushes nothing?
3. Fix it without giving up the pre-allocation for honest inputs.

### 14. Design exercise

**Market-data snapshot buffers.** Meridian's market-data service (the C++ fan-out, now a Rust rewrite candidate)
keeps, per instrument, the last 1,000 trades for late-joining subscribers. There are 40,000 instruments, and trade
rates are very skewed: the top 200 instruments produce 90% of trades, and most instruments trade a few times a day.

Choose the per-instrument container: `Vec<Trade>` with `with_capacity(1000)`, `Vec<Trade>` grown on demand, a
`Box<[Trade; 1000]>` ring buffer, or a `VecDeque<Trade>` (Chapter 9.4). Compute the memory for each design with a
48-byte `Trade`, analyze the latency of the first trade on a quiet instrument, and decide what happens to capacity when
an instrument goes quiet again. Name the metric that would tell you your choice was wrong.
