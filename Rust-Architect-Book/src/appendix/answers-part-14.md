# Appendix A — Answer Key: Part XIV

> Model answers. Write yours first. Where several answers are defensible, the key says so. Measurements quoted here
> are the chapters' Playground runs (one run each, noisy); the orderings and happens-before arguments are
> [LANG] facts about the C++20 model Rust adopts.

---

## Chapter 14.1 — Why Memory Ordering Exists

### Interview & architecture questions

**1. Sequential consistency.** The result of any execution is as if all threads' operations ran in *some* single total
order that respects each thread's program order (Lamport, 1979). The SB test (A: `x = 1; r1 = y`, B: `y = 1; r2 = x`)
has three SC outcomes: `(0, 1)`, `(1, 0)`, and `(1, 1)`. The outcome `(0, 0)` is outside that set, and the Playground's
x86 CPU produced it 5,321 times in 200,000 rounds with `Relaxed` atomics (`ch01-01`).

**2. Coherence vs consistency.** Coherence is **per location**: all cores agree on one order of writes to each address.
Consistency is about the order in which accesses to *different* addresses become visible to other cores. MESI (and
its variants) provides coherence only. Consistency comes from the core (store buffer, load ordering) and the model the
architecture promises, which is why SB's outcome breaks no coherence rule.

**3. Store buffer.** A per-core queue of retired stores waiting to be written into the L1 cache, which needs the line in
the Modified state (a read-for-ownership that can take tens to hundreds of nanoseconds, order of magnitude). It exists
so the core doesn't stall on every store miss. It produces the SB outcome on x86: each core's load reads the cache
while its own store is still buffered. The core sees its own store immediately through store-to-load forwarding,
which is why x86 is "total store order" rather than SC.

**4. MP on Arm vs x86.** The weak MP outcome ("saw the flag, missed the data") needs either the writer's two stores or
the reader's two loads to be reordered. x86-TSO forbids both, so `ch01-02` saw it 0 times in 200,000. Armv8 allows
both [CPU]. A test suite that runs only on x86 therefore can't detect a missing Release/Acquire pair. You need Miri
(which produced the MP outcome 21 times in 40 trials, `ch03-03`) or Arm hardware, and the Chapter 14.3 failure
scenario is what happens without either.

**5. Compiler transformations.** Hoisting a load out of a loop (`wait_plain`), merging or sinking stores (the progress
counter stored once), replacing a loop by `memcpy`, eliminating redundant loads and stores, and reordering independent
accesses. All are licensed by the data-race-freedom assumption: non-atomic memory can't be modified concurrently
without a data race, which is UB, so reasoning as if the thread were alone is sound.

**6. `wait_plain`.** The loop body is empty: no calls, no writes, no atomics. A `&bool` also promises the pointee
doesn't change while the reference is live (it's `noalias readonly` in LLVM, Chapter 4.1). So `*flag` is loop-invariant.
LLVM loads it once, and if it's false, the loop becomes `jmp` to itself. It's not a compiler bug. The only way
another thread could change the flag is a data race, and a program with one has no defined behavior to preserve.

**7. `compiler_fence`.** It stops the *compiler* from moving memory accesses across it, in the directions its ordering
names. It emits no CPU instruction: on x86 you see only a `#MEMBARRIER` marker in the assembly. It's the right tool
when the "other party" runs on the same thread asynchronously, such as a signal handler (or an interrupt handler on
bare metal). It's never a tool for ordering between threads.

**8. Two cores at once.** Weak outcomes come from per-core buffering and out-of-order execution overlapping with another
core's accesses within a window of nanoseconds. Time-slicing on one core goes through the kernel, which serializes,
and one core always sees its own accesses in program order. "We never saw it in testing" is weak evidence: tests rarely
align threads that tightly (the Advanced exercise), x86 hides most reorderings, and debug builds hide compiler
reorderings (Chapter 14.1's reconciliation worker).

### Debugging exercise (`READY` + `yield_now`)

1. **Yes, in practice, and it's still broken.** Verified with listing `answers-ch01-yield-now.rs` (release asm via
   `tools/emit.ps1`, trimmed, comments added):

   ```text
   playground::wait_until_ready_static_mut:
   	mov	rbx, qword ptr [rip + playground::READY@GOTPCREL]
   	cmp	byte ptr [rbx], 0
   	jne	.LBB2_3
   	mov	r14, qword ptr [rip + std::thread::functions::yield_now@GOTPCREL]
   .LBB2_2:
   	call	r14                           ; yield_now(): an opaque call
   	cmp	byte ptr [rbx], 1             ; READY is re-read after every call
   	jne	.LBB2_2
   ```

   `yield_now` is an out-of-line function in std, and LLVM must assume an opaque call may write any global whose
   address has escaped, including `READY`, so the load can't be hoisted. But that's a property of today's codegen,
   not of the language. A concurrent write to `READY` while another thread reads it is still a data race, and so UB.
2. **The publication bug.** `load_config()`'s writes and `READY = true` are unordered non-atomic stores. A reader that
   sees `READY == true` has no happens-before edge to the config writes, so it may see the flag and a stale or partly
   written config: the MP pattern. On x86 it happens to work (TSO keeps the stores in order, if the compiler does).
   On Arm, or after the compiler sinks a config store below the flag store (it's allowed to), it doesn't.
3. **Fix:** `static READY: AtomicBool`, `READY.store(true, Release)` after `load_config()`, and
   `while !READY.load(Acquire) { yield_now() }`. Release/Acquire rather than Relaxed, because the flag *publishes*
   the config (Chapter 14.3's Q1). Relaxed would fix the hoisting and not the publication. The same listing's
   `wait_until_ready_atomic` compiles to the same loop shape (`call r14` then `movzx eax, byte ptr [rbx]`). Better
   still: make the config itself the signal, `static CONFIG: OnceLock<Config>`, where `get()` returning `Some` is
   the Acquire.

### Selected exercises

- **Beginner:** SC: `(0,1)`, `(1,0)`, `(1,1)`. x86-TSO: those plus `(0,0)`. Rust model with `Relaxed`: all four.
  With a `SeqCst` fence on only one side, expect `(0,0)` to remain possible and to keep appearing. The fence orders
  only its own thread's store before its load, while the other thread's load can still pass its own buffered store.
  SeqCst fences forbid SB only when both sides have one.
- **Intermediate:** the LB weak outcome is `r1 == 1 && r2 == 1` (each load sees the other thread's *later* store). The
  Rust model allows it with `Relaxed`. x86-TSO never reorders a load with a later store, so expect zero on the
  Playground. Armv8 permits it architecturally, though it's rarely observed on real cores [CPU].
- **Advanced:** without the per-round barrier, the threads drift apart, and one thread's stores have long drained by
  the time the other reads. Predict a weak-outcome count near zero. The lesson is the one litmus harnesses (`litmus7`,
  jcstress) encode: fresh locations per round and a tight start barrier, or you're measuring scheduling, not the
  memory model.
- **Systems:** two plain `mov` stores and a plain `mov` load, and no fence instruction. The hardware may still complete
  the load before either store drains (store→load reordering). The two stores stay in order with each other (TSO).

### Design exercise (multi-architecture test policy): model answer

Three tiers. (1) **All code:** ordinary tests on x86. (2) **Any crate with `unsafe impl Send/Sync`, `UnsafeCell`
outside std types, `static mut`, or atomics other than counters:** a Miri job over its concurrency tests with
`cfg!(miri)` sizes, on every PR. Miri checks the *model*, so it runs fine on x86 runners. (3) **Hand-written
synchronization** (the gateway's drain handshake, any lock-free structure): loom tests for the protocol, plus
litmus-style stress tests on x86 *and* aarch64 runners, nightly. Review rules: a CI grep that fails on new
`static mut`, `unsafe impl Sync`, `read_volatile`, and `Ordering::Relaxed` without an ordering comment on the same
line, all routed to a concurrency CODEOWNER. Cost: Miri is slow (an interpreter), so it runs minutes on small tests;
aarch64 runners are the main cost. If you could keep only one check, keep **Miri on the concurrency tests**. It's the
one that caught Chapter 14.2's hash cache and Chapter 14.5's `volatile` flag on x86 runners, where the hardware
hides the bug.

---

## Chapter 14.2 — Happens-Before

### Interview & architecture questions

**1. Happens-before.** The transitive closure of *sequenced-before* (program order within a thread) and
*synchronizes-with* (a Release operation on an atomic and an Acquire operation on the same atomic that reads the
value it wrote, or a later value in its release sequence). std edges: `spawn` → start of the child; end of a thread →
`join` returning; `Mutex` unlock → the next `lock`; `send` → the matching `recv`; `OnceLock` initialization → a `get`
that returns `Some`; `Barrier::wait` all-to-all.

**2. Data race vs race condition.** A data race is two accesses to the same location from different threads, at least
one a write, at least one non-atomic, and neither happening-before the other. It's UB. A race condition is a logic
bug whose outcome depends on timing, like Chapter 1.3's TOCTOU (−700). It can be written entirely with atomics or
locks, in safe Rust, with no UB. Safe Rust rules out the first and not the second.

**3. Why UB.** Declaring races undefined lets the compiler optimize every non-atomic access as if the thread were alone:
keep values in registers, hoist, merge, vectorize, replace loops with `memcpy`. Defining racy reads (as Java does)
would constrain those optimizations everywhere, or require knowing which memory is shared, and would have to be
preserved on hardware where racy accesses can tear.

**4. The `Relaxed` counter proof.** *Atomicity:* each `fetch_add` is an RMW, and an RMW always reads the latest value in
the location's modification order and writes its successor. So the modification order is 0, 1, …, 4,000,000, with no
increment lost, whatever the ordering argument. *Visibility:* each increment is sequenced-before its thread's end,
each thread's end synchronizes-with the scope's join, and the join is sequenced-before the final load. So every
increment happens-before the load, and write-read coherence forbids the load from returning a value older than one
that happens-before it: it reads 4,000,000. The ordering (`Relaxed`) contributes atomicity. The threading library
contributes the ordering.

**5. Coherence.** Every atomic location has a single modification order that all threads agree on, whatever the
orderings used. A thread's successive reads never go backwards in it (`ch02-07`: 0 backwards reads in 61 million),
and an RMW reads the latest value. Coherence does *not* relate different locations (MP, SB), and it doesn't promise
that a read returns the latest value in real time. The model only asks that writes become visible "in a reasonable
amount of time."

**6. The racy release build.** LLVM, assuming no data race, collapsed each thread's million increments into one
`add qword ptr [rdi], 1000000`, a non-atomic RMW without `lock`. The race window shrank to one instruction per thread,
and the four adds didn't overlap in that run. That's no comfort: it's still UB, an overlap would now lose a million
increments at once, and a different compiler version or input (the Systems exercise) produces different code.

**7. Miri's race detector.** It keeps a vector clock per thread, and the clocks of the last accesses per location.
Release/Acquire (and the std edges built from them) merge clocks. An access whose location was last accessed
conflictingly by another thread, at a time the current thread's clock doesn't cover, is a race, reported with both
source locations. It also counts reference creation (retags) as accesses. It misses races on paths and schedules it
didn't run (one schedule per run; vary it with Miri's seed flags), in code it can't execute (unsupported FFI and
syscalls), and in input sizes too large to interpret.

**8. Java vs Rust.** Java: racy programs stay memory-safe and fail gracefully (stale values), which matters for a
platform that runs untrusted or careless code. Rust: the compiler never has to preserve racy semantics, and the type
system keeps races out of safe code entirely, so "defined but surprising" behaviors never need reasoning about.

### Debugging exercise (`IdGen`)

1. The read `*self.next.get()` in one thread and the write `*self.next.get() = id + 1` in another (and write-write
   between two threads' stores) touch the same location, are non-atomic, include a write, and have no happens-before
   edge. There's no lock and no atomic, and the `unsafe impl Sync` claims an edge that doesn't exist.
2. In both builds, `next()` is a non-atomic load, add, and store. Two threads can load the same value, both return it,
   and both store `id + 1`. So yes, two requests can get the **same ID**, and an increment is lost. (And since it's UB,
   the language promises nothing at all.)
3. `next: AtomicU64` and `self.next.fetch_add(1, Relaxed)`, which returns the old value. Uniqueness comes from RMW
   atomicity on the modification order, not from ordering. `Relaxed` is enough because the ID publishes nothing else.

### Selected exercises

- **Beginner:** each graph is "write → (sb) → release-side operation → (sw) → acquire-side operation → (sb) → read."
  Spawn: `write(1)` → spawn → child start; child's write → child end → join. Mutex: `write(10)` → guard drop (unlock) →
  the `lock()` that sees `true`. Channel: `write(20)` → `send` → `recv`. OnceLock: `write(30)` → `set` → the `get()`
  that returns `Some`. Barrier: `write(40)` → `wait` → the other thread's `wait` returning. Atomics: `write(50)` →
  `store(true, Release)` → the `load(Acquire)` that reads `true`.
- **Intermediate:** Miri reports a data race. The write now comes *after* the unlock, so it isn't sequenced-before the
  Release that the reader's lock synchronizes with. The reader's read and the writer's write are unordered in every
  execution: whichever happens first in time, the vector clocks don't cover it.
- **Advanced:** Miri reports the race only for the odd input, because it checks the executed path. For CI, choose Miri
  inputs by coverage of the concurrent paths (the same thinking as branch coverage), keep them small, and vary the
  schedule with multiple seeds.
- **Systems** (predicted from mechanism; emit it to check): with a run-time count, LLVM can still promote `*p` to a
  register and turn "increment n times" into "add n", so expect `add qword ptr [rdi], <reg>` rather than a loop. The
  race looks the same: one non-atomic read-modify-write per thread.

### Design exercise (config snapshot): model answer

| Option | Publishing edge | Worker hot path | Old config freed |
|---|---|---|---|
| `Mutex<Arc<Config>>` | unlock → lock | lock RMW + `Arc` clone RMW + unlock, all on shared lines (64 workers contend) | when the last `Arc` clone drops, often on a worker thread |
| `RwLock<Arc<Config>>` | write-unlock → read-lock | reader-count RMW on one shared line (Chapter 14.3's risk-limits finding) + `Arc` clone | same |
| `AtomicPtr<Config>` (own) | `store(Release)` → `load(Acquire)` | one load | **unsolved**: freeing while workers read is a use-after-free; leaking a 1M-entry table every 10 minutes isn't acceptable |
| `ArcSwap<Config>` | `store` → `load` (Release/Acquire inside the library) [LIB] | a guard load, designed to avoid a shared-line RMW in the common case | when the last guard or `Arc` drops |

Pick `ArcSwap`. The justification: "Workers read the routing config on every request. `ArcSwap` publishes each new
config with a release/acquire edge and keeps the old one alive until the last reader's guard drops, so readers never
see a partial table and never touch freed memory. Its read path avoids the shared reference-count line that `Mutex`
and `RwLock` would make every worker write." One refinement from Chapter 3.1: make the reloader, not a request
thread, pay for dropping the large old table. For example, the reloader keeps the previous `Arc` and drops it on its
next cycle.

---

## Chapter 14.3 — Relaxed, Acquire/Release, and SeqCst

### Interview & architecture questions

**1. `Relaxed`.** It gives atomicity (no tearing, no lost RMW updates) and coherence on its own location, and orders
nothing else. Correct uses: event counters read after join or for monitoring, `fetch_add` ID generation, a stop flag
that publishes nothing, the self-contained hash cache (14.2), and a max gauge (14.4). An incorrect use: announcing that
a buffer is ready (`ch03-01`). Also incorrect: a spinlock's CAS and unlock (`ch04-03`), a refcount decrement before
freeing (`ch04-10`), and a DCL pointer (`ch05-02`).

**2. Release/Acquire as a pair.** When an Acquire load of `M` reads the value written by a Release store to `M` (or a
later value in its release sequence), everything sequenced-before the store happens-before everything
sequenced-after the load. If the Acquire load reads a value from *before* the Release store, nothing synchronizes.
The reader simply hasn't observed the handover yet, and it must not act as if it had. That's why readers check the
value, or loop until they see it.

**3. Release sequence.** It's headed by a Release operation on `M`, and continues through the contiguous run of RMWs
that follow it in `M`'s modification order, from any thread and with any ordering. An Acquire that reads any value in
the sequence synchronizes with its head. Reference counting depends on it. Every decrement is an RMW, so the chain of
`fetch_sub(Release)` operations is unbroken, and the final thread's `fence(Acquire)` synchronizes with every earlier
decrement. So all other threads' uses of the data happen-before the free (`Arc`, and `MiniArc` in `ch04-09`).

**4. What `SeqCst` adds.** A single total order over all `SeqCst` operations, consistent with happens-before, that every
thread agrees on. It matters for **SB/Dekker** (each side writes its flag then reads the other's: Release/Acquire
allowed `(0,0)` 14,901 times, SeqCst 0) and **IRIW** (readers must agree on the order of independent writes: 3 of 40
Miri trials disagreed with Release/Acquire, 0 with SeqCst).

**5. x86-64.** Loads: all orderings compile to `mov`. Stores: `Relaxed` and `Release` are `mov`, and `SeqCst` is
`xchg`. RMWs: all `lock`-prefixed, identical across orderings. Fences: Acquire, Release, and AcqRel are compiler-only,
and SeqCst is `lock or dword ptr [rsp - 64], 0`. That's because TSO already provides acquire loads and release stores,
and every locked instruction is a full barrier. Only store→load ordering needs an instruction. The implication:
testing on x86 almost never reveals a missing Acquire or Release. Only compiler reorderings could, and rarely. Test
the model with Miri.

**6. Two orderings on `compare_exchange`.** The success ordering applies to the read-modify-write when the CAS writes.
The failure ordering applies to the load when it doesn't. A failed CAS performs no store, so there's nothing for
Release semantics to attach to: failure orderings of `Release` or `AcqRel` are rejected (`ch03-07`: "a failed
`compare_exchange` does not result in a write").

**7. "SeqCst everywhere, so it's correct."** It isn't a correctness argument. SeqCst doesn't fix non-atomic data races
(the debugging exercise below is all SeqCst and still races), ABA (wrong under any ordering, 14.4), or reclamation
(14.4's debugging exercise). It also hides intent: a reviewer can't tell which requirement each operation serves. It
costs an `xchg` per store on x86 and more on Arm. Ask for the requirement behind each operation, and for a Miri test.

**8. Documenting orderings.** Put a comment on every atomic operation naming its partner and what it publishes
("Release: publishes `slots[..n]` to the `Acquire` in `sum_published`") or saying "publishes nothing." For `SeqCst`,
name the pattern (SB/Dekker, IRIW). On each `unsafe impl Sync`, add a SAFETY comment listing the happens-before
edges, and link the Miri test.

### Debugging exercise (generation counter + `static mut CONFIG`)

1. **Not race-free.** `CONFIG = Some(new)` is a non-atomic write that also drops the old `Config` in place. The read in
   `current()` races with it whenever the reader's `GENERATION` load didn't read *this* reload's increment, and the
   reader never checks which generation it saw. Two overlapping reloads also race write-write. `SeqCst` on
   `GENERATION` creates an edge only for a reader whose load happens to read the new value, which covers none of the
   concurrent cases.
2. **Still wrong without overlap.** The `&'static Config` returned to a request is a lie that `unsafe` made possible.
   The next reload drops the `Config` it points to (and its heap parts) while the request may still be using it: a
   use-after-free. Even without freeing, the request would see a different config mid-request.
3. **Redesign:** `static CONFIG: LazyLock<ArcSwap<Config>>` (initialized from the startup config),
   `reload(new) { CONFIG.store(Arc::new(new)) }`, and `current() -> Arc<Config> { CONFIG.load_full() }` (or a
   request-scoped guard). Edges: building the new config is sequenced-before the store (Release inside `ArcSwap`),
   which synchronizes-with a load (Acquire) that sees it, which is sequenced-before the request's reads. The old
   config is freed only when the last `Arc` or guard drops. The generation counter is unnecessary. If it's still
   wanted for logging, put it inside `Config`.

### Selected exercises

- **Beginner:** "`Relaxed`: counts requests; read after join or by the scraper; publishes nothing." "`Release`:
  publishes `slots[..n]`; pairs with the `Acquire` in `sum_published`." "`fetch_sub(Release)` + `fence(Acquire)` on
  the last reference: every holder's uses happen-before the free." "`SeqCst` (all four): SB/Dekker drain handshake;
  Release/Acquire would allow both sides to read stale."
- **Intermediate:** both half-fixes still race. With a `Release` store but a `Relaxed` load, there's no Acquire to
  synchronize. With an `Acquire` load but a `Relaxed` store, there's no Release to synchronize with. Either way the
  vector clocks never merge, and Miri reports the data race on the slots exactly as for `ch03-01`.
- **Advanced:** correct, by the fence–fence rule (Chapter 14.4 §4): a `fence(Release)` sequenced-before an atomic
  store `S` synchronizes-with a `fence(Acquire)` sequenced-after an atomic load that reads `S`. Expect `miri-ok`.
  It's the mechanism `ch04-04`'s seqlock uses, which Miri accepts.
- **Systems** (not verifiable on the Playground): loads `ldr` / `ldar` (or `ldapr`) / `ldar`, stores `str` / `stlr` /
  `stlr`. RMWs on rustc's default aarch64 Linux target typically appear as calls to `__aarch64_*` outline-atomic
  helpers, which pick LSE instructions or an LL/SC loop at run time [RUSTC]. With `-C target-feature=+lse` you see
  `ldadd`/`casal` inline. Check locally.

### Design exercise (latency histogram): model answer

The requirement "`count` equals the sum of the buckets at some point in time" is a **multi-location snapshot**. No
ordering provides one: the scraper reads 66 locations at 66 different moments, and increments interleave with those
reads under `Relaxed` and `SeqCst` alike. So change the design. The simplest fix is to **stop storing `count`**:
export `count = Σ buckets`, computed from the same 64 reads, and the requirement holds by construction. Buckets and
`sum` stay `Relaxed` (Q1: they publish nothing; Q3 doesn't apply). If `sum` must also be consistent with the buckets,
use a double-buffered phase swap (the scraper flips the active histogram and waits until writers have left the old
one: HdrHistogram's recorder/`WriterReaderPhaser` pattern). Another option is per-thread histograms, each behind
its own single-writer seqlock, merged on scrape. Both avoid contention on the hot path, and both are more code than
dropping the redundant counter.

---

## Chapter 14.4 — CAS, Fences, and Lock-Free Building Blocks

### Interview & architecture questions

**1. Strong vs weak CAS.** `compare_exchange` fails only if the value differs. `compare_exchange_weak` may also fail
spuriously (on LL/SC architectures, a lost reservation). Use the weak form inside a retry loop, where a spurious failure
just means another iteration and the weak form can be cheaper. Use the strong form for single attempts whose failure
must mean "the value really differs," such as a `try_lock` that reports "busy."

**2. Spinlock orderings.** The CAS decides *who* enters. The orderings decide *what the next holder sees*. Without
Acquire on lock and Release on unlock, consecutive critical sections aren't ordered by happens-before, so the next
holder may not see the previous holder's writes, and the compiler and CPU may move accesses out of the critical
section. `ch04-03` keeps mutual exclusion, prints the right total on x86, and Miri reports the race on the retag of
the protected value.

**3. ABA.** From `ch04-06`: A reads `head = 0`, `next[0] = 1`, and is preempted. B pops 0, pops 1, and pushes 0 back:
`head` is 0 again, and `next[0]` is now 2. A's CAS (0 → 1) succeeds on a stale decision, and C then pops slot 1,
which B still owns. Fixes: a **version tag** in the same word (cheap; needs spare bits or a double-width CAS for
pointers; can wrap in principle); **reclamation schemes** that stop a node's address being reused while referenced
(epochs, hazard pointers; memory and read-path overhead); or **a design without the race** (a mutex, per-thread slot
ranges: Chapter 14.4's failure scenario shipped both).

**4. Reclamation.** After a node is unlinked, other threads may still hold pointers they read before the unlink.
Freeing it is a use-after-free, and address reuse makes pointer ABA likely. Java's GC keeps any reachable node alive, so
Java code never faces this. **Epochs:** cheap pins (a thread-local announcement), and garbage is freed two epochs
later, but a thread that stays pinned stalls reclamation and memory grows. **Hazard pointers:** each protected pointer
costs a store and a fence, plus validation, but garbage is bounded and a stalled thread holds back only the nodes it
protects.

**5. Seqlock.** The writer makes `seq` odd, writes the data, and makes `seq` even. A reader reads an even `seq`, reads
the data, fences, re-reads `seq`, and retries if it changed. The data reads race with the writer *by design*.
Non-atomic racy reads are UB even if the result is discarded, because the compiler reasons about the read itself, so
the data must be atomics. `Relaxed` loads cost nothing on x86 (`mov`). `ch04-05`'s plain-data version never tore
natively and Miri rejected it.

**6. `fence(Acquire)` vs an Acquire load.** A fence lends Acquire semantics to *preceding* atomic loads. It lets you pay
for Acquire only on the path that needs it. `Arc::drop` does `fetch_sub(1, Release)` on every drop (each holder's uses
must happen-before the free). Only the drop that takes the count to zero executes `fence(Acquire)` before freeing. So
non-final drops don't pay for an Acquire, which matters on Arm. On x86 the fence is compiler-only (`#MEMBARRIER`,
verified in `ch02-02`'s release asm).

**7. `fence(SeqCst)` on x86-64.** `lock or dword ptr [rsp - 64], 0`: a locked no-op RMW on a stack slot below the
stack pointer. Any locked instruction is a full barrier. It's typically cheaper than `mfence`, which also orders
weakly-ordered memory types and non-temporal stores that normal code doesn't use [CPU][RUSTC]. The stack line is
almost certainly already in L1, in the Modified state, so there's no coherence traffic.

**8. False sharing.** Independent variables written by different cores share a cache line, so every write invalidates
the other cores' copies: coherence traffic identical to true sharing. In `ch04-08`, four adjacent counters cost 14.96
ns per increment, against 13.64 ns for one shared counter and 4.62 ns padded (one run). Detect it with `perf c2c`
(HITM events) on Linux, or with profiles showing hot atomic instructions that scale badly with thread count. Fix it by
padding (`CachePadded`, 128 bytes), keeping per-thread state merged on read, or grouping fields by the thread that
writes them.

### Debugging exercise (`Latest`)

1. **Use-after-free.** A reader can `load` the old pointer just before `publish` swaps it out. The writer then frees
   `old` immediately, while the reader is still inside `(*p).clone()`. Three more defects: `read` before the first
   `publish` dereferences null; the last `Reading` leaks because there's no `Drop`; and `AtomicPtr<Reading>` makes
   `Latest` `Sync` for *any* `Reading`, including one that isn't `Send` (an `Rc` inside it would then be cloned from
   several threads).
2. **No.** The orderings already publish the contents correctly. The bug is lifetime: nothing tells the writer when
   readers are done with `old`. No ordering can.
3. **`arc_swap`:** `ArcSwap<Reading>`, `store(Arc::new(r))` / `load().as_ref().clone()`. The read path is a guard
   load designed to avoid a shared RMW in the common case [LIB]. **`crossbeam-epoch`:** `Atomic<Reading>`, publish
   with `swap(Owned::new(r), AcqRel, &guard)` then `guard.defer_destroy(old)`; read under `epoch::pin()`. The read
   path is a pin (a thread-local announcement plus a fence [LIB]) and a load. **`Mutex<Arc<Reading>>`:** lock, clone
   the `Arc`, unlock. The read path is two to three atomic RMWs on shared lines, and it contends as readers are added.
   Predicted ranking for many readers, from mechanism: `arc_swap` ≈ epoch < `Mutex`. Measure it before choosing.

### Selected exercises

- **Beginner:** `max.fetch_max(v, Relaxed)` performs the same compare-and-retry internally (on x86, a `lock cmpxchg`
  loop). The gauge publishes nothing but its own value, so `Relaxed` is enough, and RMW atomicity guarantees no
  maximum is lost.
- **Intermediate:** `self.locked.compare_exchange(false, true, Acquire, Relaxed).is_ok()`. Use Acquire on success
  (entering the critical section must see the previous holder's writes) and `Relaxed` on failure (we act on nothing
  but "busy"). Use the strong CAS, so that `try_lock` doesn't report "busy" spuriously.
- **Advanced:** `upgrade` is a CAS loop that loads the strong count, returns `None` at 0, and otherwise CASes `n → n +
  1`. std's `Weak::upgrade` uses `Acquire` on success (its source comment explains: to synchronize with
  `Arc::new_cyclic`, where the value can be initialized after `Weak`s exist) and `Relaxed` on failure [LIB]. The weak count
  is decremented with `Release`, and the thread that takes it to zero fences with `Acquire` before deallocating, like
  the strong count.
- **Systems** (predicted from mechanism; run it to check): with 8 threads on 4 vCPUs, the shared and falsely shared rows
  get no better and may get worse. The padded rows keep their per-increment cost, but each thread's wall time roughly
  doubles from time-slicing. The spinlock collapses: a descheduled holder leaves waiters spinning away whole time
  slices.
- **Architecture:** an uncontended lock and unlock is a few atomic operations (about 2 ns each uncontended, Chapter
  14.3) plus call overhead: tens of nanoseconds. At 100K reservations/s that's a few milliseconds of CPU per second,
  well under 1% of one core, and sharding keeps contention rare. The deciding benchmark: reserve and release in a
  multi-threaded loop at 1×, 3×, and 10× production rate, with production thread counts, on the production CPU type.
  Record p50/p99/p99.9 of `reserve()` with `hdrhistogram`, and choose option 3 only if option 2 threatens the 1 ms p99.

### Design exercise (settlement work queue): model answer

Choose `crossbeam::channel::bounded`. It's bounded, MPMC, blocks when empty (workers park rather than spin), and
there's no reclamation problem in your code. `Mutex<VecDeque>` + `Condvar` is equally correct and easy to review. At
settlement rates either meets a 10 µs p99 enqueue, and the choice between them is taste. A hand-written lock-free ring
fails Chapter 14.4's review rule. There's no benchmark showing a lock is too slow, it needs an ABA and reclamation
argument, and it has no blocking story without adding parking. "Never lost or run twice" isn't a queue property alone:
the queue guarantees each job is popped once, but a worker can crash after popping. So jobs carry idempotency keys
(Chapter 8.4) and a lease or outbox marks completion. The ADR: "Bounded crossbeam channel between the EDF scheduler
and 16 workers; capacity sized to 2× the largest batch; producers block on full (backpressure to the scheduler);
jobs are idempotent and completion is recorded in the settlement table, so a crash between pop and commit re-runs the
job safely."

---

## Chapter 14.5 — Rust Atomics vs Java `volatile` and `VarHandle`

### Interview & architecture questions

**1. VarHandle modes.** Plain → non-atomic (only for unshared data). Opaque → `Relaxed`. Acquire/Release →
`Acquire`/`Release`. Volatile → `SeqCst`. The inexact spots: Java's **plain** mode is defined under races (no tearing
of `int`s or references, no values out of thin air), and Rust has nothing between non-atomic accesses (UB under races)
and `Relaxed`. LLVM's `unordered` is exactly that gap, and Rust doesn't expose it. Opaque ≈ Relaxed is close but
defined by different texts. And Java's `loadLoadFence`/`storeStoreFence` have no Rust counterparts (use the stronger
Acquire or Release fence).

**2. `read_volatile`.** It's for memory-mapped I/O. The compiler won't elide, merge, or reorder the access relative to
other volatile accesses. It's not atomic, creates no happens-before edge, and racing it is a data race (`ch05-03`: Miri
reports the race on the flag itself). Use it for device registers, and for memory shared with hardware (DMA), together
with the right fences. For threads, use atomics.

**3. Double-checked locking.** Before Java 5, the store of the reference could become visible before the constructor's
field stores (compiler or CPU reordering), and `volatile` didn't order the surrounding non-volatile accesses. Readers
could see a non-null reference to an unconstructed object. JSR-133 gave `volatile` release/acquire semantics, which
made DCL with a `volatile` field correct. The Rust equivalent of the fix is `AtomicPtr` with a `Release` store and
`Acquire` loads (`ch05-01`). What you should use instead is `OnceLock` or `LazyLock`.

**4. No final-field semantics.** Java lets you publish a reference through a data race, and `final` fields guarantee
their constructed values even then. In Rust, a reference reaches another thread only through `Send`/`Sync` types and a
synchronizing operation. A value can't be referenced before it's fully constructed. The racy publication that final
fields protect against can't be written in safe Rust, and with `unsafe` it's UB anyway.

**5. Racy reads.** Java gains memory safety and graceful failure for racy code. It pays in optimization constraints
(the JIT must never tear or invent values) and in a notoriously complex causality specification. Rust gains full
optimization freedom and a simpler compiler contract, and keeps races out of safe code. It pays in having no floor at
all when `unsafe` code gets it wrong.

**6. `LongAdder`.** A base value plus an array of padded cells (`@Contended`). A thread hashes to a cell and adds to
it with a CAS. When CASes fail (contention), it re-hashes its probe and the array grows, up to the number of CPUs.
`sum()` adds the base and the cells without locking, so it's not an atomic snapshot: concurrent updates may or may not
be included. In Rust: `ch05-04`'s `StripedCounter`, a `Box<[CachePadded<AtomicU64>]>` with a per-thread index,
`fetch_add(Relaxed)`, and a `sum()` of `Relaxed` loads (8× faster than one `AtomicU64` with four threads, one run).

**7. Tools.** jcstress runs litmus-style tests on the real JVM and hardware, many iterations, and classifies the
observed outcomes. loom exhaustively explores the interleavings and weak-memory outcomes of a small Rust test, up to a
bound. Miri interprets real executions, detecting data races and other UB, and emulates some weak outcomes. For a new
lock-free queue: in Java, jcstress tests plus a review. In Rust, loom tests for the protocol, Miri for the `unsafe`
code, and stress tests on x86 and aarch64 hardware.

**8. Reentrancy.** Java monitors are reentrant: `a()` holds the monitor and `b()` re-enters it. A naive port with one
`std::sync::Mutex` locks in `a()` and locks again in `b()` on the same thread, which std documents as a deadlock or
a panic. Restructure: lock once in the public method, and pass the guarded data (`&mut Inner`) to private helpers
that don't lock. (`parking_lot::ReentrantMutex` exists but hands out only shared references, so mutation needs a
`RefCell` inside. It's rarely the better design.)

### Debugging exercise (`Interner`)

1. **Yes.** Increments in `intern` are ordered with each other by the mutex, but `hits()` reads `*self.hits.get()`
   without taking it. That read and another thread's increment are unordered, non-atomic, and one is a write: a data
   race.
2. In Java, a plain `long` read without synchronization is defined. It returns some value that was written (possibly
   stale, and on a 32-bit JVM possibly torn, JLS §17.7). In Rust it's UB. The compiler may, for example, keep
   `hits()`'s value in a register across a monitoring loop. And because the type claims `Sync`, the whole type's
   soundness rests on it.
3. **Inside the mutex:** `map: Mutex<Inner>` with `struct Inner { map: HashMap<..>, hits: u64 }`, and `hits()` locks.
   **As an atomic:** `hits: AtomicU64`, `fetch_add(1, Relaxed)` (it can even move outside the lock) and
   `load(Relaxed)` in `hits()`. `Relaxed`, because a statistic publishes nothing. Either way, delete the
   `unsafe impl Sync`: `Mutex` and `AtomicU64` are `Sync` on their own.
4. **Mostly not.** Chapter 4.3's objection was leaking *per request*, which is unbounded. Here a leak happens once per
   *distinct* string, which is what an interner means (Java's intern pool behaves the same, except that the GC can
   collect it). It does apply if the key space is unbounded or attacker-controlled, such as interning user input. Then
   growth is unbounded, and you need a cap, or an arena whose lifetime ends.

### Selected exercises

- **Intermediate:** `static MODEL: LazyLock<Model> = LazyLock::new(load_model);` The hand-rolled `model()` in `ch05-01`
  has 3 `unsafe` blocks, 3 `Ordering` arguments (`Acquire`, `Acquire`, `Release`) and a 12-line body. The `LazyLock`
  version has none of either, and one line.
- **Advanced:** growing the cell array means replacing it while other threads may be indexing the old one: a
  reclamation problem (freeing the old array is a use-after-free for a thread still reading it). `LongAdder` avoids it
  because the GC keeps the old array alive while any thread references it, and the cells are separate objects that the
  new array shares, so no count is lost. In Rust: allocate the maximum number of cells up front (the number of CPUs),
  or publish the array through `ArcSwap` or an epoch.
- **Architecture:** see the design exercise.

### Design exercise (Meridian's porting guide, concurrency section): model answer

**Default mappings, no review needed:** `volatile` flag → `AtomicBool` (Release/Acquire if it announces data,
`Relaxed` otherwise); `AtomicLong` counters → `AtomicU64` with `Relaxed`; `LongAdder` → per-worker `CachePadded`
counters; `ConcurrentHashMap` → a sharded `Mutex<HashMap>`; `synchronized` → `Mutex` (restructure reentrant calls);
static initializers and DCL → `LazyLock` / `OnceLock`; a reloadable `volatile` reference → `ArcSwap`; executors and
queues → crossbeam channels. **Design review required:** any `Ordering` other than a `Relaxed` counter, any `Condvar`,
any lock held across I/O. **Banned:** `static mut`, `read_volatile`/`write_volatile` outside drivers,
`UnsafeCell` + `unsafe impl Sync` in application code, and hand-written lock-free structures without a benchmark.
**Mandatory tools:** Miri for every crate with `unsafe impl Sync/Send`; loom plus aarch64 stress tests for any
hand-written synchronization. If the guide is too heavy, drop the design-review requirement for non-`Relaxed`
orderings *inside* crates that already have loom tests: the tests carry the evidence the review would ask for.

---

## Part XIV Review — Capstone: the SPSC ring PR

The defects, by class (the fixed version is `review-spsc-fixed.rs`):

1. **Memory model: the producer→consumer handover** (`tail`, both sides `Relaxed`). The slot write isn't
   happens-before the consumer's `assume_init_read`, and this is the race Miri reports. On Arm, or after a compiler
   reordering of the slot store past the `tail` store (allowed with `Relaxed`), the consumer reads a slot before its
   contents arrive: `assume_init_read` of uninitialized memory, a garbage `String` pointer, and a crash or double
   free. **Fix:** `tail.store(.., Release)` in `push` and `tail.load(Acquire)` in `pop`.
2. **Memory model: the consumer→producer handover** (`head`, both sides `Relaxed`). The consumer's read of slot `i`
   isn't happens-before the producer's later overwrite of slot `i` after the ring wraps. The producer can overwrite a
   slot the consumer is still moving out of: a torn `(u64, String)`, a double free, or a leak. It shows when the ring
   is full, meaning under load, when the fan-out falls behind. It's allowed by the model and by Arm (a load may be
   reordered with a later store), and Miri stopped at the first race before reaching it. **Fix:**
   `head.store(.., Release)` in `pop` and `head.load(Acquire)` in `push`.
3. **API soundness: `unsafe impl<T> Sync` without `T: Send`.** It lets safe code share a `Ring<Rc<..>>` across threads
   and move `Rc`s between them. **Fix:** `unsafe impl<T: Send> Sync`.
4. **API soundness: SPSC isn't enforced.** `push` and `pop` take `&self`, so two threads can push at once: both read the
   same `tail`, write the same slot (a data race), and one element is lost. That's UB reachable from safe code, so the
   type is unsound. **Fix:** `spsc(capacity) -> (Producer<T>, Consumer<T>)`, neither `Clone`, with `push`/`pop` on
   `&mut self`.
5. **Resources: no `Drop`.** `MaybeUninit` never drops its contents, so every unconsumed quote's `String` leaks when
   the ring is dropped. **Fix:** `Drop for Ring<T>` that drops the slots in `[head, tail)` (the fixed listing's test
   counts 5 drops).
6. **Resources: index arithmetic.** `tail - head` and `+ 1` overflow after `usize::MAX` operations. That's a debug
   panic, or a broken full/empty check in release. On 64-bit it's centuries away at 2M messages/s (Chapter 6.5's
   market-data rate). On a 32-bit target, or after someone "optimizes" the indices to `u32`, 2³² / 2M/s is about 36
   minutes. **Fix:** `wrapping_sub`/`wrapping_add`, with a power-of-two capacity so the slot mapping stays consistent
   across the wrap (with `%` and a non-power-of-two length, a wrapping index jumps slots).
7. **Performance: `%` by a run-time length.** It compiles to a division instruction (tens of cycles, order of
   magnitude) on every push and pop. **Fix:** power-of-two capacity (asserted at construction) and `& mask`.
8. **Performance: false sharing between the indices.** `head` (written by the consumer) and `tail` (written by the
   producer) share a cache line, so every operation bounces it (Chapter 14.4 measured 3-4×). **Fix:** `CachePadded`
   for each. A common extension, not in the fixed listing: each side caches the last value it saw of the other's
   index and re-reads it only when the ring looks full or empty.
9. **Process: the justification.** "x86 is TSO, so Relaxed is fine" is exactly what Chapter 14.3's review rule bans,
   and the PR had no Miri test. **Fix:** a Miri test with `cfg!(miri)` sizes, and an ordering comment on each atomic.
10. **Minor: `with_capacity(0)`.** Every `push` returns `Err` forever and the producer spins. The fixed version's
    power-of-two assertion rejects 0 at construction.

**Design questions.**

- **The other handover** is defect 2: `head`, from the consumer back to the producer. It corrupts a slot being read
  while the producer reuses it, and only when the ring is full, which is why a CI run with a fast consumer never
  exercises it.
- **Nothing** stops two pushers in the PR. The *types* should say it: one `Producer` and one `Consumer` from a
  constructor, neither `Clone`, with `&mut self` methods. Then `Sync` is needed only for the shared inner ring, and
  the SAFETY comment can name each handover (as the fixed listing's does).
- **Why x86 CI passes.** *Compiler:* LLVM happened not to move the non-atomic slot accesses across the `Relaxed` index
  accesses (the model allows it; today's optimizer doesn't do it here). *Core:* TSO keeps store→store and
  load→load order, and never lets a load be reordered with a later store, so both handovers happen to hold in
  hardware. *Memory system:* x86 is multi-copy atomic. All three preserve program order for this pattern. Only the
  language, the contract that matters, doesn't promise it.
- **Evidence.** Chapter 14.4's rule: a benchmark showing the `crossbeam` channel was too slow for the fan-out's
  budget at production rates, a written argument for each handover's ordering, a Miri test, a loom test for the
  index protocol, and stress runs on aarch64. Without the benchmark, keep the `crossbeam` channel (or `ArrayQueue`),
  or adopt a vetted SPSC ring crate (for example `rtrb`; not verified here, since it's not on the Playground) rather
  than a hand-written one.

---

## Part XIV Review — Interview mode

1. **Data race:** Chapter 14.2, questions 2 and 3.
2. **Happens-before:** Chapter 14.2, question 1.
3. **Five orderings:** Chapter 14.3 §2. Request counter: `Relaxed`. Published config: `Release`/`Acquire` (or
   `ArcSwap`). Lock: `Acquire` on the successful CAS, `Release` on unlock. Reference count: `Relaxed` increments,
   `Release` decrements, and `fence(Acquire)` on the last one. Dekker handshake: `SeqCst` on all four accesses.
4. **Release sequence:** Chapter 14.3, question 3. `Arc` depends on it.
5. **Compiler optimizations on non-atomic memory:** the flag load hoisted into `jmp .LBB0_1` (Chapter 14.1), the copy
   loop turned into `memcpy` with one progress store (14.1), and a million racy increments collapsed into one `add`
   (14.2). All are legal under the data-race-freedom assumption.
6. **LLVM orderings:** `monotonic` (Relaxed), `acquire`, `release`, `acq_rel`, `seq_cst`. `unordered` has no Rust
   counterpart. It exists to model Java's plain shared variables (no tearing, no ordering).
7. **Fences:** `fence` constrains the compiler and, through the fence synchronization rules, the CPU. `compiler_fence`
   constrains only the compiler. On x86-64, Acquire/Release/AcqRel fences and `compiler_fence` emit no instruction,
   and a `SeqCst` fence is `lock or dword ptr [rsp - 64], 0` (Chapter 14.4, question 7).
8. **Store buffer:** Chapter 14.1, questions 3 and 4. Arm adds MP, LB, and load→load reordering. POWER adds IRIW
   (it isn't multi-copy atomic).
9. **Coherence vs consistency:** Chapter 14.1, question 2.
10. **`xchg`:** it's implicitly locked, so it drains the store buffer before later loads execute, which forbids the SB
    outcome. Measured on the Playground: 2.08 ns per `SeqCst` store vs 0.14 ns for a `mov` store (one run,
    uncontended).
11. **Cache line, not ordering:** one shared counter 13.64 ns per increment, falsely shared 14.96, 64-byte aligned
    4.62, `CachePadded` 5.76, thread-local with one final add 1.20 (Chapter 14.4, one run). The fixes are padding,
    per-thread state, and counting locally.
12. **Seqlock vs `RwLock`:** a seqlock suits small, copyable data read constantly and written rarely, because readers
    write no shared memory, whereas `RwLock` readers all write the reader count. The writer pays two extra stores and
    a fence. Readers retry during writes and can starve under constant writes, and the data must be atomics
    (Chapter 14.4, question 5).
13. **Hand-written lock-free:** Chapter 14.4 §7 and §10. It's justified when threads must not block, or when a
    benchmark proves a lock too slow. The PR contains an ABA argument, a reclamation argument, an ordering comment
    per operation, a Miri test, loom tests where feasible, and aarch64 stress runs.
14. **x86 vs Arm:** Miri in CI (tests the model on any runner), aarch64 runners for stress tests, loom for small
    protocols, and a review rule that "x86 is TSO" is never a justification (Chapter 14.3 §10).
15. **The Java port:** `volatile` → an atomic with the ordering its role needs; `AtomicLong` → `AtomicU64`/`AtomicI64`;
    `LongAdder` → striped `CachePadded` counters; `synchronized` → `Mutex` (restructure reentrancy); DCL →
    `OnceLock`/`LazyLock`. The habit that becomes UB is the "benign" race: a plain field read and written by
    several threads, such as a racy hash cache or unsafe publication (Chapter 14.5 §8).
