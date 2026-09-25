# Appendix A — Answer Key: Part III

> Model answers. Write yours first. Where several answers are defensible, the key says so.

---

## Chapter 3.1 — Ownership

### Interview & architecture questions

**1. The rules.** Each value has exactly one owner at a time. Ownership can be moved, and the old owner loses access. The
value is dropped exactly once, when its owner goes away. "Goes away" means the owning variable leaves scope, the
containing value (struct, collection, `Box`) is dropped, or the owning place is overwritten by assignment. Temporaries
are owned until the end of their statement.

**2. Why a tree.** Each value has one owner, and owners are themselves values (or roots), so following "is owned by"
from any value leads uniquely upward to a root. The roots are locals, parameters, temporaries, and statics. Ways out of
the tree: borrowing (no ownership at all), `Rc`/`Arc` (shared ownership: a count, cycles possible, atomic cost for
`Arc`), arenas with indices (the arena owns everything, and edges are data), and `unsafe` raw pointers (you carry the
proof).

**3. Drop flags.** A hidden boolean per place whose initialized-ness depends on run-time control flow: moved on some paths
but not others, before a common scope end. It's set on initialization, cleared on move, and tested at scope end. In
release builds LLVM usually folds it into the existing branches. The verified `maybe_consume` has no flag at all, just a
direct `__rust_dealloc` on the not-moved path.

**4. Drop glue.** The compiler-generated `drop_in_place::<T>`: call `Drop::drop` if `T` implements it, then drop each
field recursively. Types with no fields needing cleanup (integers, floats, `&T`, arrays and tuples of those) have none,
and `needs_drop::<T>()` is `false`. Generic code skips per-element work for them. `Vec<u64>::clear` is O(1), and dropping
a `Vec<u64>` is one free.

**5. Predicting drop cost.** Count the heap allocations in the ownership tree: one free per owned allocation, plus a
destructor call per value whose type implements `Drop`. A `Vec<String>` is 1 + n allocations, a
`HashMap<String, Vec<u8>>` is roughly 1 + 2n, and a `Vec<u64>` is 1. Add cache-miss cost for walking cold memory.

**6. Latency spikes from drops.** Dropping frees every allocation in the tree synchronously, on the dropping thread: a
million-entry map took about 60 ms in the verified run. Prevention: (a) send the old structure to a **dropper thread**;
(b) use **arenas** or flat layouts (`Vec<u64>` instead of `Vec<Box<T>>`) so there are fewer, bigger frees; (c) control
*which* thread drops the last `Arc` (the refresher keeps it and hands it to the dropper), or drop incrementally in small
batches.

**7. RSS after a drop.** `free` returns memory to the allocator, which keeps it for reuse. Returning pages to the OS
depends on the allocator's policy (glibc trims only in some situations, and `malloc_trim` forces it; jemalloc and
mimalloc use decay timers), and fragmentation can pin pages that are mostly empty. It's usually not a leak.

**8. Who closes a resource.** Java communicates ownership of a `Closeable` through naming and documentation only.
Failure modes: a double close, use-after-close (`Stream closed`), and leaks (nobody closes), all at run time, and a
library can change its convention in a minor release. Rust puts it in the signature: `T` (callee owns and drops it) vs
`&mut T`/`&T` (caller keeps it). Use-after-close and double close are compile errors (E0382), and changing the
convention breaks every call site at compile time.

### Debugging exercise (`close(self)` vs `close(&mut self)`)

1. With `&mut self`, the connection still exists after `close`, so any method can be called on a closed connection:
   use-after-close is back, as a run-time bug.
2. The type must carry state (`closed: bool`, or an `Option` around the socket), and every method must check it and
   return an error. That's a run-time invariant replacing a compile-time one.
3. `close(self)` expresses "closing ends this object's life." `close(&mut self)` is right when the object genuinely
   outlives the close: a reconnecting client, or a pooled wrapper that closes the underlying socket and can be reopened.
   There, "closed" is a real state of a longer-lived object.

### Selected exercises

- **Beginner:** with `notes: Some(Noisy("note"))`, the output gains `drop note` after the lines (fields drop in
  declaration order, and `notes` is last). With `None`, there's nothing extra: `Option<Noisy>` drops its payload only if
  one is present.
- **Advanced:** the loop-with-break version produces a flag that's cleared inside the loop on the moving iteration and
  tested after the loop exits, at the end of the scope.

---

## Chapter 3.2 — Move, Copy, and Clone

### Interview & architecture questions

**1. `let b = a;` for a `String`.** Language: ownership transfers, `a` is statically uninitialized, and only `b` will be
dropped. MIR: the borrow-checked MIR has `move _a`, and after optimization it may print as `copy` (the verified
`pass_along` shows `copy _2`), because at the machine level they're the same. Machine code: a 24-byte copy (pointer,
capacity, length). The verified asm is one 16-byte `movups` and one 8-byte `mov`, often eliminated entirely. The heap
buffer isn't touched.

**2. Why the source dies.** Two live owners of one buffer means a **double free** at scope end. A live source after the
destination frees means a **use-after-free**. Two writable paths to one buffer means **aliasing** mutation, which becomes
a data race across threads. Killing the source statically removes all three at zero run-time cost.

**3. `Copy` vs `Clone`.** `Copy` is an implicit bitwise duplicate, where the source stays usable. `Clone` is an explicit
`.clone()` with arbitrary cost. A type with a destructor can't be `Copy`: if bitwise copies were full duplicates, the
destructor would run once per copy and free the same resource several times. E0184 enforces it.

**4. `&T` vs `&mut T`.** Copying a shared reference makes another reader, which the rules allow. Copying an exclusive
reference would make two exclusive references, which is a contradiction. (`&mut` is *reborrowed*, not copied.)

**5. Move costs.** A `Vec` of any length: 24 bytes (the header), since the elements stay where they are. A
`[u8; 4096]`: 4,096 bytes, since the array *is* the value. Move cost is `size_of::<T>()`, so large inline data should
live behind a `Box` if it moves often.

**6. `let first = names[0];`.** It would leave a hole in the `Vec`. The vector still owns that slot, so it would drop the
moved-out `String` again: a double free. Alternatives: `&names[0]` (borrow, free); `names[0].clone()` (an allocation);
`mem::take(&mut names[0])` (moves out and leaves an empty `String`, no allocation); `names.swap_remove(0)` (O(1),
reorders); `names.remove(0)` (O(n) shift); `into_iter()` (consumes the vector).

**7. When to derive `Copy`.** Small, plain values whose identity doesn't matter: IDs, coordinates, money, timestamps,
enums without payload. Avoid it for large structs (silent copies everywhere) and for types that might someday own a
resource. Adding `Copy` is easy, but removing it is a breaking change.

**8. Defensive copies.** Java references are shared and mutable, so a callee storing a list must copy it in case the
caller mutates it later, and a getter must copy out in case the caller mutates the internals. A Rust owned parameter
proves the caller gave the value up, and a borrowed parameter can't be stored without an explicit clone. The signature
carries the guarantee a defensive copy would otherwise provide.

### Debugging exercise (move in a loop)

1. The first iteration moves `payload` into `consume`, and `consume` drops it. Iteration two has nothing left to move.
2. Fixes: (a) `fn consume(s: &str)`, 0 allocations, the best fix when `consume` only reads; (b) `consume(payload.clone())`,
   1 allocation per iteration; (c) move `payload` in on the *last* iteration only, or restructure so the loop borrows and
   one final call consumes. Measure with the counting allocator: 0, 3, and 0 respectively.
3. When `consume` genuinely needs its own copy. For example, it sends each copy to a different thread or stores it in
   several independent records. Then a clone per iteration is the honest cost.

### Selected exercises

- **Beginner:** `Point` moved twice compiles (it's `Copy`). A `String` moved twice doesn't. After moving a
  `(u32, String)` whole, `.0` is unusable (the whole tuple moved). After moving a `(u32, u32)`, `.0` is fine (`Copy`).
- **Intermediate:** `names.sort_unstable(); names.dedup(); names` performs zero allocations (`sort_unstable` is in-place,
  and `dedup` drops duplicates in place). Stable `sort` allocates a scratch buffer. To preserve first-seen order, keep a
  `HashSet<&str>` of seen values in a *first pass* over indices, then `retain` using a precomputed `Vec<bool>`. The set's
  allocation is O(n), but no string is cloned.
- **Advanced:** after `let customer = order.customer;`, `order` is **partially moved**: other fields are still usable
  individually, but `order` as a whole isn't (no `&self` method calls, no passing `order`). If `Order` implements
  `Drop`, the partial move is rejected outright (E0509), because `Drop::drop` needs the whole value. Otherwise, drop
  elaboration drops the remaining fields individually.

---

## Chapter 3.3 — Borrowing

### Interview & architecture questions

**1. The two rules.** Aliasing XOR mutation, and references never outlive their referent. Relax the first and you get
iterator invalidation, data races, and no `noalias` optimizations. Relax the second and you get dangling references
(use-after-free).

**2. `Copy` for `&T` but not `&mut T`:** see Chapter 3.2, question 4.

**3. Run-time cost.** A reference is a pointer (8 bytes, 16 when fat). Borrow checking costs **nothing** at run time:
it's done on MIR and erased.

**4. `add_twice`.** With `&mut i32`/`&i32`, the parameters are `noalias`, so LLVM knows the store to `*a` can't change
`*b`. It loads `*b` once, computes `2*b`, and does one read-modify-write on `*a`. With raw pointers, `a == b` is possible
(the correct answer for `a == b == 1` is 4), so it must reload `*b` after the first store.

**5. E0505.** A reference is an address. Moving the value relocates its bytes (and for a `Vec`, the moved `Vec` may be
dropped or reallocated by its new owner), leaving the reference pointing at abandoned memory. So while a loan is live,
the value must stay put.

**6. MESI parallel.** Coherence keeps each cache line either shared by many readers or exclusively owned by one writer
(SWMR). Rust enforces the same shape per value, at compile time. Since safe code can't create "two paths, one writing"
without synchronization, and `Send`/`Sync` extend this across threads, safe Rust has no data races.

**7. Two-phase borrows.** For autoref'd method calls, the `&mut v` is first *reserved* (it acts like a shared borrow),
the arguments are evaluated (`v.len()` reads `v`), and then the borrow is *activated*. The shared read has ended by then,
so nothing conflicts.

**8. `self.rate()` vs `self.rate_bps`.** Borrows are tracked per place. `self.accounts.iter_mut()` mutably borrows the
place `self.accounts`. Reading `self.rate_bps` is a different place, so there's no conflict. `self.rate()` takes `&self`,
a shared borrow of **all of `*self`** per its signature (the checker doesn't look inside `rate`), which overlaps the live
mutable borrow of `self.accounts`.

### Debugging exercise (split borrows)

1. Loan A: `&mut self.accounts` (live for the loop). Loan B: `&*self` for the `rate()` call, which covers all of `*self`
   per the signature.
2. Soundness in general: a future `rate()` could read `self.accounts` (for example, average the balances), which would
   observe a vector mid-mutation, possibly mid-reallocation. The checker must be correct for *any* body matching the
   signature.
3. The three fixes: access the field directly; copy `rate` into a local before the loop; restructure into
   `struct Bank { accounts: Vec<Account>, policy: Policy }` and pass `&self.policy` explicitly, so the disjointness is
   visible in types. The third scales best, because it makes the independence of the data part of the design.

### Selected exercises

- **Beginner:** word count `&str`; append to a log `&mut String` (or `&mut Vec<String>`); store in a registry an owned
  `String`; median without mutation `&[f64]` (the function clones into a scratch buffer internally, or the caller
  passes one).
- **Advanced:** `let (left, right) = xs.split_at_mut(j);` (for `i < j`) gives two non-overlapping `&mut` slices, and
  `mem::swap(&mut left[i], &mut right[0])` swaps them. `&mut xs[i]` and `&mut xs[j]` together are rejected because the
  checker can't prove `i != j`. `split_at_mut` is a safe API whose `unsafe` inside proves it once.

---

## Chapter 3.4 — Slices, String, and &str

### Interview & architecture questions

**1. Sizes.** A `String` is pointer + capacity + length (24 bytes). A `&str` is pointer + length (16). A fat pointer is a
pointer carrying extra metadata: a length for slices and `str`, a vtable pointer for `dyn Trait` (Part VI). A DST is a
type whose size isn't known at compile time (`str`, `[T]`, `dyn Trait`), so it's only ever used behind a fat pointer.

**2. `s[0]`.** "The first character" could mean the first *byte* (not a character for non-ASCII text), the first
*`char`* (a Unicode scalar value, O(n) to reach the nth), or the first *grapheme cluster* (what a user sees; not in std).
Rust refuses to pick silently.

**3. The UTF-8 invariant.** Every `str` is valid UTF-8. `from_utf8` validates, slicing checks boundaries, and every
safe mutating API preserves validity. `from_utf8_unchecked` is `unsafe`. Violating the invariant is undefined behavior,
because other code (including `chars()` and the optimizer) is allowed to assume it holds.

**4. `&String` parameters.** They accept only `String`, while `&str` accepts `String` (by deref coercion), literals, and
sub-slices. `&String` also gives the function nothing extra, since you can't grow a `String` through `&`. Take `&str`.

**5. ASCII search on UTF-8.** ASCII bytes (0x00–0x7F) never appear inside multi-byte sequences: leading bytes are
0xC0 and up, and continuation bytes are 0x80–0xBF. So an ASCII delimiter found by byte search is a real character, and
its position is a char boundary.

**6. `Box<str>` / `Arc<str>`.** `Box<str>` for many immutable owned strings: 16 bytes instead of 24, and no spare
capacity (map keys, stored names). `Arc<str>` when the same immutable string is shared by many structures or threads,
where clone is a refcount bump instead of a copy.

**7. `Cow`.** `Borrowed(&'a str)` or `Owned(String)`. It pays off when most inputs pass through unchanged
(normalization, escaping, redaction). The cost when it doesn't: an enum discriminant check on access, and a lifetime tied
to the input.

**8. The `substring` story.** Java ≤ 7u5 `substring` shared the parent's `char[]`, so a tiny substring could keep a huge
string alive (a leak). 7u6 made it copy. Rust offers both explicitly: `&s[a..b]` borrows (fast, and it keeps the
original alive *visibly*, via a lifetime), and `.to_owned()` copies (detached).

### Debugging exercise (`s[0]`)

1. `.bytes().nth(0)` for byte-oriented protocols (ASCII headers); `.chars().nth(0)` for text. For "é…", the first byte is
   `0xC3` and the first char is `'é'`.
2. `&s[0..1]` compiles always, and **panics at run time** when the first character is multi-byte. A compile-time
   refusal is better than a panic that only non-ASCII input triggers.
3. `fn first_char(s: &str) -> Option<char> { s.chars().next() }`. It's O(1) because decoding the *first* character
   reads at most 4 bytes. `nth(k)` is O(k) because it must decode k characters to find the k-th.

### Selected exercises

- **Intermediate:** scan the bytes for `<>&"`, return `Cow::Borrowed(s)` if none are found, otherwise build one `String`
  with capacity `s.len() + extra` and push the escapes. That gives 0 allocations for clean inputs and 1 for dirty ones.
- **Advanced:** unquoted fields and quoted fields without `""` can be `&str` slices. A quoted field containing `""` must
  be unescaped, so return `Cow<'_, str>` per field: `Borrowed` for most, `Owned` only for fields with escaped quotes.

---

## Chapter 3.5 — Drop and RAII

### Interview & architecture questions

**1. When destructors run, and when they don't.** They run at scope end, when the containing value drops, on overwrite,
on explicit `drop`, at the end of a temporary's statement, and during unwinding. They don't run for moved-from places,
`mem::forget`, `Box::leak`, `Rc` cycles, `process::exit`, `panic = "abort"`, fatal signals, or crashes.

**2. Order.** Locals in reverse declaration order (later locals may borrow earlier ones, so borrowers die first). Fields
in declaration order after the struct's own `drop`. Tuple, array, and `Vec` elements front to back.

**3. Scrutinee temporaries.** Temporaries in a `match` scrutinee live until the end of the whole `match`, because arms
may bind references into them. [VERSION] Edition 2024: `if let` scrutinee temporaries are dropped before `else`, and a
block's tail-expression temporaries are dropped before the block's locals. The verified listings show `STILL LOCKED` vs
`free`, and E0597 under 2021.

**4. `two_names` unwind.** If the second `to_string` panics, `bb1`'s unwind edge goes to `bb9`, which drops only `_3`
(first), then `resume`. If `String::len` panicked (it can't in practice, but MIR models every call as able to unwind),
the edge goes to `bb8`, which drops `_4`, then `bb9` drops `_3`: reverse order, as on the normal path.

**5. A panic in a destructor during unwinding.** The cleanup edge is `unwind terminate(cleanup)`: the process **aborts**.
A double panic doesn't unwind.

**6. Why `Drop` can't return errors.** It runs implicitly on paths (including unwinding) where there's nobody to hand an
error to. `File` ignores close errors on drop, `BufWriter` ignores flush errors on drop, and a transaction guard can only
roll back or log. So durability-critical operations need explicit `flush()`, `sync_all()`, and `commit()` calls returning
`Result`, with `Drop` as the backstop.

**7. `Copy` and `Drop`:** see Chapter 3.2, question 3.

**8. `process::exit`.** It terminates without running destructors on any thread, so unflushed buffers, temp-file
cleanup, and graceful closes are lost. Return `ExitCode` from `main` (everything drops normally), or flush and clean up
explicitly before calling `exit`.

### Debugging exercise (the self-deadlock)

1. The `MutexGuard` from `queue.lock().unwrap()` in the `match` scrutinee lives until the end of the `match`. Inside the
   `Some` arm, `queue.lock()` tries to take the same mutex on the same thread.
2. `Mutex::lock` documents that locking a mutex the current thread already holds is **unspecified**: it may deadlock or
   panic, depending on the platform's implementation. It's not guaranteed to be a clean error.
3. `let next = queue.lock().unwrap().pop();` ends the guard at the end of the `let` statement. Or scope it explicitly:
   `let next = { let mut q = queue.lock().unwrap(); q.pop() };`. The explicit block makes the lock scope obvious to the
   next reader.
4. With `if let Some(job) = queue.lock().unwrap().pop() { ... }`, the guard still lives through the **body**. Edition
   2024 only releases it before the `else`. The re-lock is in the body, so it would still deadlock.

### Selected exercises

- **Intermediate (`TempDir::persist`):** `mem::forget(self)` after extracting the path leaks the other fields.
  `ManuallyDrop` is precise but awkward. A `persisted: bool` flag checked in `Drop` is the clearest: set it, then return
  a clone of the path.
- **Advanced:** if commit fails, the transaction's state is unknown (the server may or may not have committed). Model
  that as `Result<Committed, CommitError>` where the error *consumes* the guard and says "outcome unknown: reconcile",
  rather than silently rolling back.

---

## Chapter 3.6 — Graphs

### Interview & architecture questions

**1. `parent: &Node`.** The parent owns the child (a `Vec<Node>` field) and may mutate it, so it holds or takes `&mut`
of its children. A child holding `&parent` would be a shared borrow of something that's also mutably accessible: aliasing
plus mutation. It's also self-referential (the parent's own data would borrow the parent), and a lifetime can't express
that.

**2. `Rc::clone`.** It increments the strong count and returns a new pointer to the same allocation, with no deep copy.
`Weak` keeps the **allocation** alive (so its counts can be checked) but not the **value**. The value is dropped when the
strong count hits zero, and the block is freed when the weak count also hits zero.

**3. `Rc` vs `Arc`.** `Rc` updates its counts with non-atomic operations, so concurrent clones or drops could lose
updates, leading to a premature free or a leak. `Arc` uses atomic RMWs. That's roughly the cost of an uncontended atomic,
more under contention, since every clone and drop writes a shared cache line.

**4. `RefCell`.** It tracks active borrows in a counter, checks aliasing XOR mutation on each `borrow()` or
`borrow_mut()`, and panics on a violation ("RefCell already borrowed"). It's worse than a compile error because failures
surface in production, on specific paths, typically reentrant callbacks. It's worth it when sharing and mutation are
genuinely dynamic and can't be restructured, and the borrow scopes are short and obvious.

**5. ABA and generations.** An index to a slot that was freed and reused now refers to a different value, and the holder
can't tell. A generation counter per slot, bumped on removal and stored in each key, turns that into a detectable
mismatch (`None`). The cost is one integer compare per access and 4 bytes per key.

**6. Index-linked LRU in safe Rust.** All nodes are owned by one `Vec`, and links are integers. Every mutation goes
through `&mut self` on that one owner, so the ordinary borrow rules apply and there's never aliasing between node
references. A pointer-linked doubly linked list has two pointers to each node (from `prev` and `next`) that both need
mutation rights, which safe references can't express. It needs `Rc<RefCell>` plus `Weak`, or `unsafe`.

**7. Cache behavior.** An `Rc` graph traversal is a chain of dependent loads to scattered allocations, each a likely cache
miss. An arena keeps nodes contiguous, so traversals touch neighboring lines, and the prefetcher helps. Links are also
half the size (`u32` instead of a pointer).

**8. `Rc<RefCell<T>>` as Java's object model.** Shared references (`Rc`) to mutable objects (`RefCell`) with run-time
checks: exactly Java's model, but without the GC's cycle collection. It's right for small, dynamic, genuinely shared
mutable state where the borrow scopes are short: UI trees, some interpreters, prototypes.

### Debugging exercise (event bus)

1. `publish` holds `handlers.borrow()` (shared) for the whole loop. A handler calls `subscribe`, which requests
   `handlers.borrow_mut()` (exclusive) on the same `RefCell` while the shared borrow is live. The check is at run time
   because `RefCell` exists precisely to defer it there.
2. The cycle: the bus owns the `Vec<Handler>`, which owns the closure, which owns `bus_for_handler: Rc<EventBus>`, which
   points back to the bus. The strong count never reaches zero, because `Rc` can't detect cycles.
3. Keep `pending: RefCell<Vec<Handler>>` for subscriptions made during `publish`, and drain it into `handlers` after the
   loop (or have `subscribe` push to `pending` when `try_borrow_mut` fails). Have the closure capture
   `let weak = Rc::downgrade(&bus);` and `if let Some(bus) = weak.upgrade() { ... }`. A `Drop` impl printing on
   `EventBus` confirms it's freed at the end of `main`.

### Selected exercises

- **Advanced:** DFS with a `Vec<Key>` stack and a color map (white/gray/black). A back-edge to a gray node is a cycle.
  Recursion depth equals path length, and a 1M-node chain would overflow a 2 MiB thread stack (Chapter 2.4). An explicit
  stack lives on the heap.

---

## Project Level 2 — `redact` architecture review

**1. Other allocations.** The `BufWriter`'s buffer (once), the `StdoutLock` (none), `from_utf8_lossy` for invalid
UTF-8 lines only (one per such line), the line buffer growing on longer lines (amortized, rare), and the final stats
formatting. None is per clean line.

**2. Worst case.** Every position is examined a bounded number of times. Digit runs and email local parts are only
scanned from their *start* (the "previous byte isn't a word character" checks), and a failed domain scan can restart at
most once, at the domain's first character. That's linear, with no super-linear input. The slowest inputs per byte are
long runs that start many candidate scans that fail late, such as many short `x@y` fragments. Still linear, just a
larger constant.

**3. Split secrets.** Across lines or writes, `redact` never sees the whole secret. Solve it at the *source*: structured
logging with typed fields and redaction in the logger (a `tracing` layer), where values are whole. A downstream filter
can't reliably reassemble arbitrary splits.

**4. Unbounded lines.** Cap the buffer (say 1 MB): stop reading into it at the cap, pass the prefix through `redact`,
then either drop the rest of the line or process it in overlapping chunks (overlap ≥ the longest pattern, 19 digits for
cards). Count `truncated_lines` or `chunked_lines` in stats and report it, because it's a visibility and completeness
trade-off.

**5. `Cow` vs `String`.** `Cow` lets clean lines skip allocation. If output had to be queued to another thread, the
borrowed variant couldn't outlive the line buffer, so you'd need `into_owned()` (an allocation per line) or a pool of
reusable buffers handed off by ownership. That's the Chapter 1.4 boundary again: zero-copy within a scope, owned at
queue boundaries.

**6. Parallelism.** Split the file into chunks at newline boundaries, and give each worker its own chunk, its own buffers,
its own output buffer, and its **own `Stats`**. Write outputs in chunk order (keep results indexed by chunk ID). Merge
`Stats` by summing fields: no sharing, merge at the end.

**7. `unsafe`.** Only in the test-only counting allocator, forwarding to `System` with its safety argument documented.
`redact` could use `from_utf8_unchecked` when building output (every pushed piece is valid UTF-8), but `String::push_str`
of `&str` pieces already keeps validity with no validation cost. There's no benefit, so no `unsafe`.

**8. Priorities.** *Recall first:* broaden patterns (cards with separators, IDN emails), accept more false positives,
and add a second-stage validator. *Latency first:* flush per line when interactive (`IsTerminal`, or an explicit flag),
and keep the zero-allocation path. *Velocity first:* use the `regex` crate with a `RegexSet`, accepting its compile cost
and heavier dependency, and keep the `Cow` wrapper so clean lines still don't allocate.

---

## Part III Review — the order book

### Part 1: problems

| # | Location | Problem | Consequence | Fix |
|---|---|---|---|---|
| 1 | `Order.book` back-reference | Strong cycle: book → orders → `Rc<RefCell<OrderBook>>` → book | **The whole book leaks**, forever | No back-reference: operations take `&mut OrderBook` |
| 2 | `history: Vec<Order>` of clones | Every clone also clones the `book` `Rc`: more cycle edges, and **unbounded growth** | Memory grows without limit; can never be freed | An append-only event log of small `Copy` events, or an external journal |
| 3 | `order.clone()` into `shared` | Two copies of each order: the live one and the history one | Divergent state; double memory | One owner per order |
| 4 | `cancel` removes only from `by_id` | `bids`/`asks` still hold the `Rc` | **Cancelled orders still match**, a correctness bug in a matching engine | One owner (arena) plus indices; cancel removes from every index |
| 5 | Three `Rc<RefCell<Order>>` owners per order | No single source of truth | Consistency depends on remembering every index | The arena owns; indices hold keys |
| 6 | `RefCell` everywhere | Aliasing checks at run time | Panics under reentrancy (a fill callback that places an order) | `&mut OrderBook` operations, compile-time checks |
| 7 | `Rc` (`!Send`) | Can't move the book to another thread | Blocks the multi-threaded engine (Part XI) | Owned data plus `Send` types; one engine thread owning the book |
| 8 | `sort_by` on every `place` | O(n log n) per insert, plus `RefCell` borrows in the comparator | Latency grows with book depth | Price levels in a `BTreeMap<Price, VecDeque<Key>>` |
| 9 | `is_bid: bool` | Stringly or boolean-typed side | Easy to pass the wrong side | `enum Side { Bid, Ask }` |
| 10 | `pub` fields | Anyone can mutate `bids`/`by_id` directly | Invariants unenforceable (Chapter 2.7) | Private fields plus methods |
| 11 | `Rc` per order | Scattered allocations, 16 B of counts each | Cache misses on every level walk | Contiguous arena |
| 12 | `place` sorts only `bids` | `asks` are never sorted | Wrong matching priority on the ask side | Price-level structure for both sides |

### Part 2: redesign (model answer)

```rust,ignore
#[derive(Clone, Copy, PartialEq, Eq, Hash)] pub struct OrderId(u64);
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)] pub struct Price(i64);   // cents
#[derive(Clone, Copy)] pub enum Side { Bid, Ask }

struct Order { id: OrderId, side: Side, price: Price, qty: u32 }        // plain data, owned by the arena

pub struct OrderBook {
    orders: Arena<Order>,                               // the ONE owner (generational keys, Chapter 3.6)
    bids: BTreeMap<Price, VecDeque<Key>>,               // price levels; FIFO within a level = time priority
    asks: BTreeMap<Price, VecDeque<Key>>,
    by_id: HashMap<OrderId, Key>,                       // lookup for cancel/modify
    events: Vec<BookEvent>,                             // append-only, small Copy events; drained to a journal
}

impl OrderBook {
    pub fn place(&mut self, order: Order) -> Vec<Fill> { ... }   // &mut self: compile-time exclusivity
    pub fn cancel(&mut self, id: OrderId) -> Option<Order> { ... } // removes from arena; stale keys in levels are skipped (or removed eagerly)
}
```

Justification: one owner (the arena) means one source of truth and no cycles. Keys instead of references means no
lifetimes tying the indexes to the arena, and stale keys are detected. `&mut self` operations make exclusivity a
compile-time property. `BTreeMap` price levels make inserting O(log P + 1) with correct price-time priority, with no
re-sorting. The event log records history without cloning live state. The whole structure is `Send`, so one engine
thread can own it (Part XI and the Part XXIV message-broker patterns).

---

## Part III Review — Interview mode

1. **Why ownership:** it removes double frees, use-after-free, and dangling pointers (and, with borrowing and
   `Send`/`Sync`, data races) with no run-time collector. It costs expressiveness for graph-shaped data and front-loaded
   design effort.
2. **`Copy` vs `Clone`:** Chapter 3.2, questions 3 and 4.
3. **What a lifetime describes:** the region of the program during which a loan is live, from the borrow to its last
   use. The compiler proves the loan is shorter than the owner's validity and doesn't overlap a conflicting loan. It
   isn't a duration in time and doesn't exist at run time.
4. **Non-null references:** a reference must point to a valid value, so null isn't a possible value. The nullable
   equivalent is `Option<&T>`, the same size as `&T` thanks to the niche.
5. **Exactly once with conditional moves:** static move tracking per path, plus drop flags where paths merge. The flags
   are elided in release when control flow already encodes the answer (Chapter 3.1's verified asm).
6. **Drop glue:** Chapter 3.1, question 4.
7. **`noalias`:** Chapter 3.3, question 4.
8. **Unwind MIR:** normal-path drops in reverse order, plus `(cleanup)` blocks reached by each call's unwind edge,
   dropping exactly the initialized values in reverse and ending in `resume`. A panic inside cleanup means
   `unwind terminate`, an abort.
9. **Clone cost:** a `Vec<String>` of 1,000 is 1,001 allocations (measured). It becomes expensive when the value owns
   many allocations, sits in a hot path, or is large.
10. **Move cost:** `size_of::<T>()` bytes, often elided. Significant for large inline values (arrays, big enums, structs
    embedding buffers) that move often.
11. **Drop latency:** Chapter 3.1, question 6.
12. **`Cow`:** Chapter 3.4, question 7.
13. **Graph with deletions and outside handles:** generational arena (safe stale handles, contiguous, one owner);
    `Rc`/`Weak` (independent lifetimes, scattered, run-time borrow checks if mutable); a plain index arena with a free
    list (fast, but stale indices are silently wrong). Prefer generational keys when handles escape.
14. **`Drop` and network calls:** `Drop` can't fail or `.await`, doesn't run on abort, `SIGKILL`, or crash, and blocking
    in it stalls the thread. Use an explicit async `release()` for promptness, `Drop` as a best-effort backstop, and
    **server-side expiry** for correctness (Chapter 3.5's design exercise).
15. **`Rc<RefCell<_>>` and `.clone()` everywhere:** a Java-shaped design translated rather than redesigned. Ownership was
    never decided, so the code pays at run time (borrow panics, clone costs, cycles). Recommend drawing the ownership
    tree, choosing one owner per entity (often an arena), passing `&`/`&mut`, and reserving `Rc`/`Arc` for genuinely
    shared, immutable data.
