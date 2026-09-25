# Chapter 14.3 — Relaxed, Acquire/Release, and SeqCst

> **Where this sits:** Part XIV · Memory Model and Atomics · chapter 3 of 5
> **Prerequisites:** Chapter 14.1 (litmus tests, store buffers), Chapter 14.2 (happens-before, data races, coherence).
> **After this chapter you can:** state what each of the five orderings promises and what it doesn't; choose the
> weakest correct one from a written requirement; explain the "publish a buffer via a counter" bug and fix it; say
> when `SeqCst` is genuinely needed (and when it's a comment that says "I didn't think about it"); and read the LLVM IR
> and x86 assembly each ordering produces.

---

## Pass 1 · User level — *Five orderings, three questions*

### 1. Problem

Every atomic operation in Rust takes an `Ordering`: `Relaxed`, `Release`, `Acquire`, `AcqRel`, or `SeqCst`. Chapter
1.3 used `Relaxed` and promised that one day it would be wrong. Here's that day, as Chapter 1.3's answer key put it: "a
producer fills a buffer and then increments `ready` with `Relaxed`, and a consumer sees `ready == N` and reads the
buffer. Nothing then orders the buffer writes before the buffer reads."

That program prints the right answer on x86 every time, and it's undefined behavior (this chapter runs it under Miri).
The ordering argument is not a performance knob with correctness as a side effect. It's a statement of *what the atomic
operation communicates*: nothing but its own value (`Relaxed`), "everything I did before this is ready" (`Release`),
"show me what the releaser did" (`Acquire`), or "and everyone agrees on the order" (`SeqCst`).

### 2. Mental model

**Three questions pick the ordering:**

```text
 Q1. Does this atomic tell another thread that OTHER memory is ready (publication), or hand over ownership
     (a lock, a refcount reaching zero, a slot in a queue)?
       no  → Relaxed is enough (counters, statistics, IDs, stop flags, self-contained caches)
       yes → the writer needs Release and the reader needs Acquire (both, or it doesn't count)

 Q2. Does one operation both consume an earlier handover and make a new one (a CAS that takes a lock,
     a fetch_sub that may drop the last reference)?
       yes → AcqRel on that read-modify-write (or Release + a separate Acquire fence, Chapter 14.4)

 Q3. Does correctness depend on the relative order of operations on DIFFERENT atomics, as seen by
     different threads (each side writes its flag, then reads the other's; independent writers whose order
     readers must agree on)?
       yes → SeqCst on all participating operations (or SeqCst fences)
```

**What each ordering promises** [LANG] (C++20 [atomics.order], which Rust adopts):

| Ordering | Allowed on | Promise |
|---|---|---|
| `Relaxed` | load, store, RMW | Atomicity, plus coherence on this one location (Chapter 14.2). **Orders nothing else.** |
| `Release` | store, RMW | Everything sequenced before it happens-before anything sequenced after an `Acquire` that reads this value (or a later value in its release sequence). |
| `Acquire` | load, RMW | The other half of the pair: once it reads a value written by a `Release` (or later in its release sequence), the releaser's earlier writes are visible. |
| `AcqRel` | RMW | Both, on one read-modify-write. |
| `SeqCst` | load, store, RMW | Acquire (loads) / Release (stores) / AcqRel (RMWs), **plus** a single total order of all `SeqCst` operations that every thread agrees on. |

Two details that trip up experienced engineers:

- **Release/Acquire is a pair, on the same atomic, and the Acquire must read the released value.** A Release store to
  `a` and an Acquire load of `b` synchronize nothing. An Acquire load that reads the value from *before* the Release
  store synchronizes with nothing either. It simply didn't see the handover yet.
- **Release sequences.** After a Release store to `M`, later **read-modify-writes** on `M`, from any thread and with
  any ordering, extend the release sequence. An Acquire that reads the result of one of those RMWs still synchronizes
  with the original Release [LANG]. That's what makes a reference count work: each `fetch_sub(1, Release)` heads its own
  sequence, the RMWs chain, and the final thread's Acquire synchronizes with all of them (Chapter 14.4's `MiniArc`).

**`SeqCst` is for Q3, not for safety.** It's the only ordering that forbids the SB outcome of Chapter 14.1 (verified:
Release/Acquire allowed it 14,901 times in 200,000 rounds, SeqCst 0) and the IRIW outcome below. For plain
publication, it buys nothing over Release/Acquire except a more expensive store on x86. The precise SeqCst rules were
revised in C++20 (paper P0668, "Revising the C++ memory model," 2018) after the old rules turned out to be unsound
with the standard compilation schemes for Power and Arm. That's a hint about how subtle this is, not something you
need to reason about day to day.

### 3. Rust code

**The promised counterexample: publishing a buffer via a `Relaxed` counter** (listing `ch03-01-publish-relaxed.rs`):

```rust
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::thread;

const CAP: usize = 8;

pub struct Batch {
    slots: [UnsafeCell<u64>; CAP],
    published: AtomicUsize, // slots[..published] are "ready"
}

// SAFETY (claimed): slot i is written only before `published` covers it, and read only after.
// That claim needs a happens-before edge from the write to the read, and Relaxed doesn't provide one.
unsafe impl Sync for Batch {}

impl Batch {
    pub fn new() -> Self {
        Batch { slots: std::array::from_fn(|_| UnsafeCell::new(0)), published: AtomicUsize::new(0) }
    }
    /// Single writer: fill slot i, then announce it.
    pub fn push(&self, i: usize, v: u64) {
        unsafe { *self.slots[i].get() = v };
        self.published.store(i + 1, Relaxed); // BUG: should be Release
    }
    /// Any reader: sum the announced prefix.
    pub fn sum_published(&self) -> (usize, u64) {
        let n = self.published.load(Relaxed); // BUG: should be Acquire
        let sum = (0..n).map(|i| unsafe { *self.slots[i].get() }).sum();
        (n, sum)
    }
}

fn main() {
    let batch = Batch::new();
    let (n, sum) = thread::scope(|s| {
        s.spawn(|| (0..CAP).for_each(|i| batch.push(i, 100 + i as u64)));
        loop {
            let (n, sum) = batch.sum_published();
            if n == CAP { break (n, sum); }
            std::hint::spin_loop();
        }
    });
    println!("read {n} slots, sum = {sum} (expected {})", (0..CAP as u64).map(|i| 100 + i).sum::<u64>());
}
```

On x86, natively, release build:

```text
read 8 slots, sum = 828 (expected 828)
```

Under Miri:

```text
error: Undefined Behavior: Data race detected between (1) non-atomic write on thread `unnamed-1` and (2) non-atomic read on thread `main` at alloc320
  --> src/main.rs:32:43
   |
32 |         let sum = (0..n).map(|i| unsafe { *self.slots[i].get() }).sum();
   |                                           ^^^^^^^^^^^^^^^^^^^^ (2) just happened here
   |
help: and (1) occurred earlier here
  --> src/main.rs:26:18
   |
26 |         unsafe { *self.slots[i].get() = v };
   |                  ^^^^^^^^^^^^^^^^^^^^^^^^
```

The native run is right because x86 never reorders the two stores in `push` (TSO) or the two loads in the reader, and
LLVM happened not to either. Neither is a promise the language makes. The fix is two words (listing
`ch03-02-publish-release-acquire.rs`, verified clean under Miri, same output natively):

```rust,ignore
        self.published.store(i + 1, Release); // everything above is published with this store
        // ...
        let n = self.published.load(Acquire); // ...and visible to everything below this load
```

**Miri doesn't only detect races. It also produces weak outcomes.** Miri emulates weak memory: when an atomic load
could legally return an older value, Miri sometimes returns one. Listing `ch03-03-litmus-miri.rs` runs Chapter 14.1's
litmus tests 40 times each inside Miri, with the data itself atomic (so there's no race to report, only outcomes to
count):

```text
MP  Relaxed flag:          data == 0 after flag == 1 in 21 of 40 trials
MP  Release/Acquire flag:  data == 0 after flag == 1 in 0 of 40 trials
SB  Relaxed  r1 == r2 == 0 in 20 of 40 trials
SB  SeqCst   r1 == r2 == 0 in 0 of 40 trials
```

The MP line is the counterexample x86 hardware wouldn't show you in Chapter 14.1 (0 of 200,000). The language allows
it, Miri produces it, and Arm hardware can too. That's why the fix is in the code, not in the choice of CPU.

**IRIW: when even Release/Acquire readers disagree** (listing `ch03-05-iriw-miri.rs`). Two threads write two different
atomics; two readers read them in opposite orders:

```text
 W1: x = 1        W2: y = 1        R1: a = x; b = y        R2: c = y; d = x
 weak outcome: a == 1, b == 0 (R1 saw x first)  and  c == 1, d == 0 (R2 saw y first)
```

```text
IRIW Release/Acquire  readers disagree in 3 of 40 trials
IRIW SeqCst           readers disagree in 0 of 40 trials
```

With Release/Acquire, the two readers may disagree about which write happened first. Nothing in the pairwise
handovers forces a global order. `SeqCst` adds that global order. On real hardware [CPU], x86 and Armv8 are
multi-copy-atomic and won't show IRIW for these loads; POWER can. The language allows it everywhere, so portable code
that needs "everyone agrees on the order of independent events" needs `SeqCst`.

**The compiler rejects nonsense orderings** (listing `ch03-07-invalid-orderings.rs`, verified; the deny-by-default
`invalid_atomic_ordering` lint):

```text
error: atomic loads cannot have `Release` or `AcqRel` ordering
   = help: consider using ordering modes `Acquire`, `SeqCst` or `Relaxed`
error: atomic stores cannot have `Acquire` or `AcqRel` ordering
   = help: consider using ordering modes `Release`, `SeqCst` or `Relaxed`
error: `compare_exchange`'s failure ordering may not be `Release` or `AcqRel`, since a failed `compare_exchange` does not result in a write
   = help: consider using `Acquire` or `Relaxed` failure ordering instead
error: memory fences cannot have `Relaxed` ordering
   = help: consider using ordering modes `Acquire`, `Release`, `AcqRel` or `SeqCst`
```

When the ordering is a run-time value the lint can't see, the same mistake panics instead (listing
`ch03-08-relaxed-fence-dynamic.rs`: `there is no such thing as a relaxed fence`).

---

## Pass 2 · Systems level — *From `Ordering` to instructions*

### 4. Under the hood

**Rust → LLVM** [RUSTC]. The orderings map one-to-one onto LLVM's atomic orderings. From the release LLVM IR of listing
`ch03-04-orderings-asm.rs` (rustc 1.98.1; one instruction per function, comments added, the `cmpxchg` line's `align 8`
trimmed, and the four fence functions shown on one line):

```text
%0 = load atomic i64, ptr %a monotonic, align 8          ; load(Relaxed): LLVM calls Relaxed "monotonic"
%0 = load atomic i64, ptr %a acquire, align 8            ; load(Acquire)
%0 = load atomic i64, ptr %a seq_cst, align 8            ; load(SeqCst)
store atomic i64 %v, ptr %a monotonic, align 8           ; store(Relaxed)
store atomic i64 %v, ptr %a release, align 8             ; store(Release)
store atomic i64 %v, ptr %a seq_cst, align 8             ; store(SeqCst)
%_0 = atomicrmw add ptr %a, i64 1 monotonic, align 8     ; fetch_add(1, Relaxed)
%_0 = atomicrmw add ptr %a, i64 1 seq_cst, align 8       ; fetch_add(1, SeqCst)
%0 = cmpxchg ptr %a, i64 %old, i64 %new acq_rel acquire   ; compare_exchange(old, new, AcqRel, Acquire)
fence acquire / fence release / fence acq_rel / fence seq_cst
fence syncscope("singlethread") seq_cst                  ; compiler_fence(SeqCst)
```

LLVM also has an ordering Rust doesn't expose: `unordered`, which the LLVM Language Reference describes as "intended
to provide a guarantee strong enough to model Java's non-volatile shared variables." That's a useful fact for Chapter
14.5: Java's plain fields sit *between* Rust's non-atomic accesses and `Relaxed`.

**LLVM → x86-64** [CPU], from the release assembly of the same listing:

```text
playground::load_relaxed:   mov  rax, qword ptr [rdi]
playground::load_acquire:   mov  rax, qword ptr [rdi]
playground::load_seqcst:    mov  rax, qword ptr [rdi]          ; all three loads: identical
playground::store_relaxed:  mov  qword ptr [rdi], rsi
playground::store_release:  mov  qword ptr [rdi], rsi          ; Relaxed and Release stores: identical
playground::store_seqcst:   xchg qword ptr [rdi], rsi          ; SeqCst store: xchg (implicitly locked)
playground::add_relaxed:    mov eax, 1 / lock xadd qword ptr [rdi], rax
playground::add_seqcst:     mov eax, 1 / lock xadd qword ptr [rdi], rax    ; RMWs: identical
playground::add_unused:     lock inc qword ptr [rdi]           ; result unused → lock inc
playground::cas:            lock cmpxchg qword ptr [rdi], rdx
```

(One line per function; `ret`s and register shuffles trimmed.) On x86, **every ordering choice for loads, and every
choice except SeqCst for stores, compiles to the same instruction**. The x86 hardware already provides
Acquire/Release (TSO), and every locked RMW is already a full barrier. The SeqCst store is the one place the
hardware needs help: `xchg` drains the store buffer, forbidding the SB outcome.

So on x86 the ordering you choose mostly constrains the **compiler**, which is exactly why getting it wrong "works" on
x86. The mistake surfaces on a weaker CPU, under a smarter compiler, or under Miri.

**LLVM → AArch64** (*not verified here*: the Playground emits x86-64 only; check locally with
`rustc --target aarch64-unknown-linux-gnu -O --emit asm` after `rustup target add aarch64-unknown-linux-gnu`). The
standard C/C++11-to-Armv8 mappings, which LLVM follows:

| Operation | AArch64 |
|---|---|
| load `Relaxed` / `Acquire` / `SeqCst` | `ldr` / `ldar` (or `ldapr` on Armv8.3+ for Acquire) / `ldar` |
| store `Relaxed` / `Release` / `SeqCst` | `str` / `stlr` / `stlr` |
| RMW | an `ldxr`/`stxr` retry loop, or a single LSE instruction (`ldadd`, `cas`, with `a`/`l`/`al` suffixes) on Armv8.1+ |
| fence `Acquire` / `Release`, `AcqRel`, `SeqCst` | `dmb ishld` / `dmb ish` |

On Arm, Acquire and Release are real instructions with real (if modest) costs, and Relaxed is genuinely cheaper.

### 5. Memory

**Publication, drawn.** What Release/Acquire guarantees in the fixed batch:

```text
 writer thread                                           reader thread
 slots[0] = 100  ┐
 ...             │ all sequenced before
 slots[7] = 107  ┘
 published.store(8, Release) ─────── synchronizes-with ───► published.load(Acquire) == 8
                                                              │ sequenced before
                                                              ▼
                                                           read slots[0..8] → sees 100..107
```

With `Relaxed` on either side, the arrow is missing. The reader may see `published == 8` and still read old slot
values. That's allowed by the language, observed under Miri, and possible on Arm hardware. The *direction* matters
too: the Release store has to come **after** the writes it publishes, and the Acquire load **before** the reads it
protects. A Release store at the top of `push` would publish nothing.

**Release sequences, drawn** (why an RMW in the middle doesn't break the chain):

```text
 modification order of M:   A: store(1, Release)  →  B: fetch_add(1, Relaxed)  →  C: fetch_add(1, Relaxed)
                            └──────────── release sequence headed by A (A, then RMWs) ─────────────┘
 a load(Acquire) that reads C's result still synchronizes-with A
```

### 6. CPU / OS

**What each ordering costs on x86, measured** (listing `ch03-06-ordering-cost.rs`, release, one thread, cache line
already local, 20 million operations each; one run on a shared machine):

```text
store(Relaxed)   mov           0.14 ns/op
store(Release)   mov           0.14 ns/op
store(SeqCst)    xchg          2.08 ns/op
load(Relaxed)    mov           0.27 ns/op
load(SeqCst)     mov           0.27 ns/op
fetch_add(Relaxed) lock xadd   2.07 ns/op
fetch_add(SeqCst)  lock xadd   2.07 ns/op
store(Relaxed) + fence(SeqCst)   1.98 ns/op
```

These match the instructions. Plain `mov` stores retire into the store buffer at more than one per cycle. `xchg`, `lock
xadd`, and a SeqCst fence each cost roughly 2 ns (about 7-8 cycles at this CPU's clock, order of magnitude), because
each waits for the store buffer to drain. The **ordering** choice for loads and RMWs costs nothing measurable on x86.
Only SeqCst stores do.

Two cautions about this table. First, it's the *uncontended* cost. When other cores write the same line, every one of
these operations pays for a cache-line transfer instead (Chapter 14.4 measures 13-19 ns per increment under
contention). Contention, not ordering, is what makes shared atomics slow. Second, on Arm the rows differ: `ldar`/`stlr`
and `dmb` have real costs, which is where choosing Relaxed or Acquire/Release over SeqCst pays off. Measure on your
target (the Systems exercise).

---

## Pass 3 · Architect level — *Choosing, documenting, reviewing*

### 7. Trade-offs

| Requirement | Weakest correct ordering | Why not weaker | Typical example |
|---|---|---|---|
| Count events; read the total after join or for monitoring | `Relaxed` | — | request counters, `ch02-02` |
| Generate unique IDs | `Relaxed` `fetch_add` | — | request IDs (14.2's debugging exercise) |
| Stop flag that publishes nothing | `Relaxed` | — | Chapter 14.1's reconciliation worker |
| Publish data (buffer, snapshot, initialized object) | writer `Release`, reader `Acquire` | Relaxed: reader may see the flag and stale data (`ch03-01`) | `ch03-02`, `OnceLock`, `ArcSwap` |
| Take / release a lock | lock: `Acquire` (CAS success); unlock: `Release` | Relaxed: critical sections not ordered (Chapter 14.4) | spinlock, mutex |
| Drop the last reference | `fetch_sub(Release)` + `fence(Acquire)` if last | Relaxed: free races with other threads' reads (Chapter 14.4) | `Arc` |
| Each side writes its flag, then reads the other's | `SeqCst` on all four, or `SeqCst` fences | Release/Acquire allows both to read stale (`ch01-01`) | drain handshake (14.1), Dekker |
| Readers must agree on the order of independent writes | `SeqCst` | Release/Acquire allows disagreement (IRIW) | rare; global event ordering |

**"Just use SeqCst everywhere"?** It's a defensible team default for code where performance doesn't matter and
reviewers aren't experts, and it's how Java's `volatile` and `AtomicInteger` behave. The objections are about
*communication* as much as cost. A `SeqCst` in the code doesn't say which requirement the author had in mind, so a
reviewer can't check it, and it doesn't make incorrect lock-free algorithms correct: the ABA bug in Chapter 14.4 is
wrong under any ordering. Mara Bos's *Rust Atomics and Locks* (O'Reilly, 2023) makes the point that SeqCst is rarely
the ordering an algorithm actually requires. Meridian's rule is a compromise: **any ordering is fine, but every
atomic operation carries a comment naming what it publishes or synchronizes with, or says "publishes nothing."** The
comment is the part reviewers check.

**Prefer not to write any of this.** `Mutex`, channels, `OnceLock`, `ArcSwap`, and crossbeam's structures encapsulate
correct orderings. Hand-written orderings belong in small, well-tested, Miri-checked building blocks, not scattered
through business logic.

### 8. Java comparison

| Java | Rust | Note |
|---|---|---|
| `volatile` read / write | `load(SeqCst)` / `store(SeqCst)` | on x86 both compile to the same instructions as Rust's (`mov`; locked instruction or `xchg` for the store) |
| `AtomicInteger.get()` / `set()` | `load(SeqCst)` / `store(SeqCst)` | |
| `AtomicInteger.lazySet()` (JDK 6), `setRelease()` (JDK 9) | `store(Release)` | "lazySet" was Java's first Release store |
| `getAcquire()` / `setRelease()` (JDK 9 `VarHandle`) | `load(Acquire)` / `store(Release)` | |
| `getOpaque()` / `setOpaque()` | ≈ `Relaxed` | Chapter 14.5 gives the analogy limit |
| `incrementAndGet()` | `fetch_add(1, SeqCst) + 1` | same `lock xadd` as Rust's `Relaxed` on x86 |

The biggest difference is the default. Java's atomics default to the strongest mode, and weaker modes arrived only in
JDK 9. Rust makes you choose on every call. Chapter 14.5 covers the whole mapping, and where it stops being exact.

> **Analogy limit.** "A Java `volatile` is a Rust `SeqCst` atomic" is right for the *variable*. It's not right for the
> *program around it*. In Java, a plain field read racing with a write is still a defined (if surprising) read, so a
> missing `volatile` produces stale values. In Rust, the non-atomic data a missing Release/Acquire fails to protect is
> a data race, which is UB, as `ch03-01` showed. The same mistake has a stronger penalty.

### 9. Production scenario

**Meridian's risk-limits snapshots.** The risk-limits service (Chapter 1.2: 100K operations/s, p99 under 1 ms, a
no-breach invariant) checks each payment against per-merchant limits. Limits change a few times an hour. The first
design protected the table with an `RwLock`, and profiling showed readers contending on the lock's reader count, one
atomic cache line written by every reader on every request (Chapter 14.4 measures what that costs).

The redesign treats the table as an **immutable snapshot**: build a complete new `Limits`, then publish it. Readers
take the current snapshot and use it for the whole request. The publication requirement is exactly Q1 in §2: a
Release on publish, an Acquire on read. It also has a second requirement that plain `AtomicPtr` doesn't solve: **the
old snapshot must stay alive** until every reader that loaded it is done. That's memory reclamation (Chapter 14.4).
The team used `arc_swap::ArcSwap`, which handles both (listing `ch03-09-arc-swap.rs`, verified natively and under
Miri):

```rust,ignore
    let current = ArcSwap::from_pointee(build(0, MERCHANTS));
    // writer:
    current.store(Arc::new(build(v, MERCHANTS))); // publish a fully built snapshot
    // reader, per request:
    let limits = current.load(); // a guard: the snapshot stays alive while we use it
```

```text
checked 1606 snapshots, last version 200, none half-built
```

The reader checks an invariant on every entry of every snapshot it sees. It never saw a half-built table, and versions
never went backwards. The ordering reasoning lives inside a library with its own tests. Meridian's code states only
the requirement: "readers see complete snapshots."

### 10. Failure scenario

**The access-log shipper on Graviton.** Each gateway worker hands finished access-log lines (Chapter 9.2) to a shipper
thread through a fixed per-worker batch: the worker writes a slot, then bumps `len`, and the shipper copies
`slots[..len]` to the tenant's log export. It's exactly `ch03-01`, with 256 slots of log-line buffers instead of eight
`u64`s. The contractor who wrote it used `Relaxed` for `len`, with the comment "x86 is TSO, stores stay in order."

On the x86 fleet it was correct in practice for eight months, for the reason the comment gave. Then Meridian moved a
third of the gateway pool to Graviton (aarch64; Chapter 2.1's multi-arch build). Within a week, a tenant reported an
access-log line whose path wasn't theirs. The Arm core had made the new `len` visible before the slot's new contents,
and the shipper copied the slot's *previous* contents: another request's line, sometimes another tenant's. Across the
affected window, the investigation found stale lines in 0.002% of shipped batches and two cross-tenant exposures, both
disclosed to the tenants.

What the team changed, in order:

1. **The fix:** `Release` on the `len` store, `Acquire` on the load (`ch03-02`), and the same for the shipper's
   "consumed" counter going the other way, since a slot must not be overwritten while it's being copied (the Part XIV
   review's ring buffer has the same two-directional handover).
2. **A test that fails on x86:** the batch now has a Miri test (`cfg!(miri)` sizes), which reports the data race on the
   x86 CI runners, where the bug was otherwise invisible.
3. **aarch64 CI runners** for the gateway's concurrency tests.
4. **A review rule:** every `Relaxed` needs a comment saying what it publishes (nothing, or else it's the wrong
   ordering), and "x86 is TSO" is never a justification. The language model is the contract. The CPU is an
   implementation detail.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XIV).*

1. What does `Relaxed` guarantee? Give three correct uses and one incorrect one.
2. Explain Release/Acquire as a pair. What happens if the Acquire load reads a value from *before* the Release store?
3. What is a release sequence, and why does a reference count depend on it?
4. What does `SeqCst` add to Acquire/Release? Give the two litmus tests where it matters.
5. On x86-64, which orderings compile to different instructions, and why? What does that imply about testing on x86?
6. Why does `compare_exchange` take two orderings, and why can't the failure ordering be `Release`?
7. "Our code uses SeqCst everywhere, so it's correct." Respond.
8. How would you *document* an ordering choice so a reviewer can check it?

### 12. Exercises

- **Beginner.** For each operation in the Trade-offs table, write a one-line comment in the style of Meridian's rule
  ("publishes X to Y" or "publishes nothing").
- **Intermediate.** Modify `ch03-01` so that only the store is `Release` and the load stays `Relaxed`. Predict Miri's
  verdict, then run it. Repeat with only the load fixed.
- **Advanced.** Replace the `Release` store and `Acquire` load in `ch03-02` with `Relaxed` accesses plus fences:
  `fence(Release)` before the store, `fence(Acquire)` after the load. Verify it with Miri, and explain which
  synchronizes-with rule makes it correct (Chapter 14.4 names it).
- **Systems.** Cross-compile `ch03-04-orderings-asm.rs` for `aarch64-unknown-linux-gnu` locally, and compare every
  function with the x86 table in §4. Which functions differ, and which of the differences are LSE vs LL/SC?
- **Architecture.** Take a lock-free component you know (a Java `ConcurrentLinkedQueue`, a Disruptor ring, Netty's
  `MpscQueue`) and list each atomic access with the ordering it uses and the requirement (Q1-Q3) it serves.

### 13. Debugging exercise

A configuration hot-reload uses a generation counter:

```rust,ignore
static GENERATION: AtomicU64 = AtomicU64::new(0);
static mut CONFIG: Option<Config> = None;

fn reload(new: Config) {
    unsafe { CONFIG = Some(new) };
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

fn current() -> &'static Config {
    let _g = GENERATION.load(Ordering::SeqCst);
    unsafe { (*std::ptr::addr_of!(CONFIG)).as_ref().unwrap() }
}
```

1. Every atomic operation here is `SeqCst`. Is the program data-race-free? Identify the conflicting accesses.
2. Suppose reloads could never overlap with reads. Is it correct then? What about the `&'static Config` returned to a
   request that's still running when the next reload happens?
3. Redesign it with `ArcSwap` or `OnceLock` + `ArcSwap`, and state the happens-before edges of your design.

### 14. Design exercise

**Choose orderings for a metrics histogram.** The gateway records request latencies into a histogram of 64
`AtomicU64` buckets (one `fetch_add` per request) plus a `count` and a `sum`. A scraper reads all 66 values every 15
seconds and exports them. Requirements from the SRE team: no increments lost; the exported `count` must equal the sum
of the buckets "at some point in time"; export cost is irrelevant. Decide whether the second requirement is achievable
with `Relaxed` counters, with SeqCst counters, or only with a different design (a seqlock, per-thread histograms merged
on scrape, a double-buffered swap). Justify the design with the three questions in §2.
