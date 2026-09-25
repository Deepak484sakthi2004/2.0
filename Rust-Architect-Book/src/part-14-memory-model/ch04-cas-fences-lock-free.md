# Chapter 14.4 — Compare-and-Swap, Fences, and Lock-Free Building Blocks

> **Where this sits:** Part XIV · Memory Model and Atomics · chapter 4 of 5
> **Prerequisites:** Chapters 14.2 (happens-before, data races) and 14.3 (orderings, release sequences).
> **After this chapter you can:** write and review CAS loops; build a spinlock, a seqlock, and a reference count with
> the right orderings (and recognize the wrong ones, which Miri rejects); explain ABA and memory reclamation, and when
> tags, epochs, or hazard pointers solve them; use fences deliberately; and measure false sharing.

---

## Pass 1 · User level — *Read-modify-write, and what you build with it*

### 1. Problem

Loads and stores can publish data, but they can't *decide* anything between threads. Two threads that each load a
value, compute a new one, and store it will overwrite each other (Chapter 14.2's lost updates). Every concurrent
structure, from a counter to a mutex to a lock-free queue, needs an operation that reads and writes **atomically**, so
that a thread can act on the value it saw only if nobody changed it in between.

That operation is **compare-and-swap** (CAS): "if the value is still `old`, replace it with `new`; either way, tell me
what it was." Maurice Herlihy showed in 1991 ("Wait-free synchronization") that CAS is *universal*: any concurrent
object can be built from it without locks. This chapter builds the common ones, and shows the three things that make
lock-free code much harder than its size suggests: orderings, **ABA**, and **memory reclamation**.

### 2. Mental model

**The RMW family** [LANG]. Each is one indivisible operation on one location, and reads the latest value in its
modification order:

| Operation | Does | x86-64 (verified in Chapter 14.3 / §4) |
|---|---|---|
| `fetch_add`, `fetch_sub` | add, return old | `lock xadd` (or `lock inc`/`dec` if the result is unused) |
| `swap` | store new, return old | `xchg` |
| `fetch_or`, `fetch_and`, `fetch_xor` | bit op, return old | `lock or` etc. if unused; a `lock cmpxchg` loop if the old value is used |
| `fetch_max`, `fetch_min` | max/min, return old | a `lock cmpxchg` loop |
| `compare_exchange(old, new, success, failure)` | CAS | `lock cmpxchg` |
| `compare_exchange_weak` | CAS that may fail spuriously | same as strong on x86; a cheaper LL/SC form on some Arm targets |
| `try_update(set, fetch, f)` / `update(set, fetch, f)` | a CAS loop around `f` | `lock cmpxchg` loop |

**The CAS loop** is the universal pattern: read the current value, compute the new one, CAS, and on failure retry with
the value the CAS returned. It's **lock-free**: some thread always makes progress, because a CAS only fails when
another thread's succeeded. It isn't **wait-free**: one unlucky thread can keep losing.

**Three hazards** that the rest of the chapter makes concrete:

```text
 ORDERINGS    the CAS decides; the ORDERING says what the winner may assume about memory the previous winner wrote
              (a lock's CAS must be Acquire; its unlock Release — §3)
 ABA          "the value is still A" doesn't mean "nothing happened": A → B → A in between, and the CAS succeeds
              on a stale decision (§5)
 RECLAMATION  after you unlink a node, another thread may still be reading it. When can you free it? (§5)
```

**Fences** are orderings without an access: `fence(Release)` makes the *next* store act as a Release store for
synchronization purposes, and `fence(Acquire)` makes the *previous* load act as an Acquire load. They let you pay for
ordering only on the path that needs it (Arc's drop, §4).

### 3. Rust code

**A CAS loop, three ways** (listing `ch04-01-cas-max.rs`): a "max latency since the last scrape" gauge, updated by
four threads with interleaved, increasing values so their CASes collide:

```rust,ignore
/// The classic CAS loop. Returns how many times the CAS lost a race and had to retry.
fn record_max_cas(max: &AtomicU64, v: u64) -> u64 {
    let mut retries = 0;
    let mut cur = max.load(Relaxed);
    while v > cur {
        match max.compare_exchange_weak(cur, v, Relaxed, Relaxed) {
            Ok(_) => break,
            Err(actual) => {
                retries += 1; // another thread changed it (or, on LL/SC CPUs, a spurious failure)
                cur = actual; // retry against the value we just learned
            }
        }
    }
    retries
}

fn record_max_update(max: &AtomicU64, v: u64) {
    // try_update (called fetch_update before it was renamed) is the same loop, written by std.
    // `None` means "no change needed".
    let _ = max.try_update(Relaxed, Relaxed, |cur| (v > cur).then_some(v));
}
```

```text
CAS loop     max = 3999999  failed CAS attempts = 1205236  (35.6 ms)
try_update   max = 3999999  failed CAS attempts =       0  (35.5 ms)
fetch_max    max = 3999999  failed CAS attempts =       0  (27.8 ms)
scrape -> 42, after scrape -> 0
```

(Release build, one run; the other two variants don't report their retries, but they happen inside.) About 1.2
million of 4 million CASes lost a race. `Relaxed` is right here because the gauge publishes nothing but its own value.
The scrape uses `swap(0, Relaxed)`, reading and resetting in *one* atomic step, so no maximum recorded between "read"
and "reset" is lost.

> **[VERSION]** `try_update` and the infallible `update` are stable on 1.98.1. The older name, `fetch_update`, still
> compiles on stable, but on the Playground's nightly (1.100.0) it's deprecated: "renamed to `try_update` for
> consistency" (listing `ch04-12-fetch-update-renamed.rs` checks both channels). Code built with warnings denied, as
> Meridian's CI is (Chapter 8.1), will need the rename when that reaches stable.

**A spinlock** (listing `ch04-02-spinlock.rs`, verified clean under Miri and exact natively): a lock is a CAS that
decides who enters, plus orderings that make each critical section happen-before the next:

```rust,ignore
    pub fn lock(&self) -> Guard<'_, T> {
        let mut backoff = 1u32;
        loop {
            // Acquire on success: the previous holder's writes (before its Release) are visible to us.
            if self.locked.compare_exchange_weak(false, true, Acquire, Relaxed).is_ok() {
                return Guard { lock: self };
            }
            // "Test" before "test-and-set": spin on a plain load, which keeps the cache line Shared
            // instead of bouncing it between cores with failed read-modify-writes.
            while self.locked.load(Relaxed) {
                for _ in 0..backoff {
                    spin_loop(); // x86 `pause`: tells the core this is a spin-wait
                }
                backoff = (backoff * 2).min(64);
            }
        }
    }
// ...
impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        // Release: our writes inside the critical section happen-before the next Acquire that sees `false`.
        self.lock.locked.store(false, Release);
    }
}
```

```text
under Miri:     counter = 100 (expected 100)
release build:  counter = 800000 (expected 800000)
```

Change both orderings to `Relaxed` (listing `ch04-03-spinlock-relaxed.rs`). Mutual exclusion still holds, since the CAS
is still atomic and only one thread at a time sees `false`. Natively on x86 it still prints `800000`. Miri rejects it:

```text
error: Undefined Behavior: Data race detected between (1) non-atomic write on thread `unnamed-1` and (2) retag write of type `u64` on thread `unnamed-2` at alloc219
  --> src/main.rs:56:18
   |
56 |         unsafe { &mut *self.lock.value.get() } // SAFETY: we hold the lock, exclusively
   |                  ^^^^^^^^^^^^^^^^^^^^^^^^^^^ (2) just happened here
   |
help: and (1) occurred earlier here
  --> src/main.rs:75:21
   |
75 |                     *counter.lock() += 1;
   |                     ^^^^^^^^^^^^^^^^^^^^
   = help: retags occur on all (re)borrows and as well as when references are copied or moved
   = help: retags permit optimizations that insert speculative reads or writes
   = help: therefore from the perspective of data races, a retag has the same implications as a read or write
```

Exclusion without ordering isn't a lock. Thread 2 entered *after* thread 1 left, but nothing made thread 1's write
visible to thread 2. Notice *where* Miri caught it: creating the `&mut` counts as an access, because the compiler may
insert reads and writes through a reference.

**A seqlock: readers that never write** (listing `ch04-04-seqlock.rs`). For a small record that's read constantly and
written rarely, a sequence lock lets readers proceed without writing any shared memory, so they don't contend with
each other at all. The writer makes the sequence number odd, writes, and makes it even again. Readers retry if they
saw an odd number or if it changed during their read. The correct Rust version reads the data through **atomics**
(`Relaxed`) and uses fences, following Hans Boehm's analysis ("Can Seqlocks Get Along With Programming Language Memory
Models?", MSPC 2012):

```rust,ignore
    pub fn write(&self, limit: u64, used: u64) {
        let s = self.seq.load(Relaxed);
        self.seq.store(s + 1, Relaxed); // odd: readers that overlap us will retry
        fence(Release); // the odd seq is ordered before the data stores below (pairs with the reader's fence)
        self.limit.store(limit, Relaxed);
        self.used.store(used, Relaxed);
        self.seq.store(s + 2, Release); // even again: publishes the data stores above
    }

    pub fn read(&self) -> ((u64, u64), u32) {
        let mut retries = 0;
        loop {
            let s1 = self.seq.load(Acquire); // sees an even value ⇒ the data of that write is visible
            if s1 & 1 == 0 {
                let limit = self.limit.load(Relaxed);
                let used = self.used.load(Relaxed);
                fence(Acquire); // if we read ANY value of a newer write, the load below sees its odd seq
                if self.seq.load(Relaxed) == s1 {
                    return ((limit, used), retries);
                }
            }
            retries += 1;
            spin_loop();
        }
    }
```

```text
under Miri:     4 consistent reads, 19 retries, 0 torn reads
release build:  1992439 consistent reads, 190666 retries, 0 torn reads
```

Two readers checked the invariant `used * 10 == limit * 3` on every read while one writer made 2 million updates.
The "obvious" seqlock, with the record in a plain `UnsafeCell<(u64, u64)>` and the same sequence protocol (listing
`ch04-05-seqlock-naive.rs`), also never tore natively (`1904141 reads, none torn (on this machine, this time)`), and
Miri rejects it:

```text
error: Undefined Behavior: Data race detected between (1) non-atomic write on thread `unnamed-1` and (2) non-atomic read on thread `unnamed-2` at alloc217
  --> src/main.rs:32:34
   |
32 |                 let v = unsafe { *self.data.get() }; // non-atomic read, possibly racing with write()
   |                                  ^^^^^^^^^^^^^^^^ (2) just happened here
```

"We throw torn values away" doesn't help. A racing non-atomic read is UB whether or not you use the result, because
the compiler reasons about the read itself. Making the data words atomic costs nothing on x86 (`Relaxed` loads are
plain `mov`s), and it's the difference between a correct program and one that works.

---

## Pass 2 · Systems level — *Instructions, fences, and memory that won't stay put*

### 4. Under the hood

**RMWs on x86** [CPU], from the release assembly of Chapter 14.3's listing (comments added):

```text
playground::add_unused:              ; fetch_add with the result unused
	lock		inc	qword ptr [rdi]
	ret

playground::or_relaxed:              ; fetch_or with the result used: x86 has no "or and return old"
	mov	rax, qword ptr [rdi]
.LBB2_1:
	mov	rcx, rax
	or	rcx, 1
	lock		cmpxchg	qword ptr [rdi], rcx    ; a CAS loop, generated for you
	jne	.LBB2_1
	ret
```

A `lock`-prefixed instruction owns the cache line for the whole read-modify-write. That's what makes it atomic, and
it's also what makes it slow under contention: the line has to travel to whichever core wants it next (§6).

**Fences on x86** (same listing; one line per function, `ret`s trimmed):

```text
playground::fence_acquire:   #MEMBARRIER            ; compiler-only: no instruction (TSO already orders it)
playground::fence_release:   #MEMBARRIER
playground::fence_acqrel:    #MEMBARRIER
playground::fence_seqcst:    lock or dword ptr [rsp - 64], 0   ; a locked no-op on the stack: a full barrier
playground::compiler_only:   #MEMBARRIER            ; compiler_fence(SeqCst)
```

The `SeqCst` fence isn't `mfence`. LLVM emits a locked `or` of zero into a stack slot below the stack pointer, a no-op
whose `lock` prefix drains the store buffer, which is typically cheaper than `mfence` [RUSTC][CPU]. (HotSpot has
historically used the same trick, a `lock addl` to the stack, after Java volatile stores. *Not verified here*: requires
a JDK with the hsdis disassembler and `-XX:+PrintAssembly`.)

**The fence rules** [LANG]. A `fence(Release)` followed (in program order) by any atomic store `S` synchronizes with a
`fence(Acquire)` preceded by any atomic load that reads `S`'s value (or later in its release sequence). There are also
mixed forms (fence–atomic, atomic–fence). In other words, a fence lends its ordering to the access next to it, whatever
that access's own ordering is. That's how the seqlock above works: the reader's data loads are `Relaxed`, and the
`fence(Acquire)` after them is what makes "I read a newer write's data" imply "I'll see its odd sequence number."

**Why fences exist: Arc's drop.** `Arc::drop` does `fetch_sub(1, Release)`, and only if that was the last reference,
`fence(Acquire)` before freeing. Every drop pays for a Release, and only the last one pays for an Acquire. std's
version shows up in any program that uses scoped threads, for example in the release assembly of Chapter 14.2's
`ch02-02` listing (comments added):

```text
	lock		dec	qword ptr [r12]         ; strong-count fetch_sub(1, Release)
	jne	.LBB6_11                        ; not the last reference: done
	#MEMBARRIER                             ; fence(Acquire): on x86, compiler-only
	mov	rdi, r14
	call	<alloc::sync::Arc<std::thread::scoped::ScopeData>>::drop_slow
```

Listing `ch04-09-mini-arc.rs` builds the same thing from scratch, `MiniArc<T>`, and Miri accepts it:

```rust,ignore
impl<T> Drop for MiniArc<T> {
    fn drop(&mut self) {
        // Release: this thread's uses of `data` happen-before whoever frees it.
        if unsafe { self.ptr.as_ref() }.refs.fetch_sub(1, Release) != 1 {
            return;
        }
        // Acquire fence: synchronizes with every earlier Release decrement (they form a release sequence
        // on `refs`), so ALL other threads' uses happen-before the free below.
        fence(Acquire);
        drop(unsafe { Box::from_raw(self.ptr.as_ptr()) }); // SAFETY: we were the last reference
    }
}
```

With `Relaxed` and no fence (listing `ch04-10-mini-arc-relaxed.rs`), the output natively is still `sums = [410, 410,
410]`, and Miri reports a race between another thread's use of the data and the deallocation:

```text
error: Undefined Behavior: Data race detected between (1) retag read on thread `main` and (2) retag write of type `Inner<std::vec::Vec<u64>>` on thread `unnamed-3` at alloc395
```

The free is a write to the whole allocation. Without the Release/Acquire chain, the thread that frees has no
happens-before relationship with the threads that last read, so it may free memory another thread is still using. On a
weakly ordered CPU that's a use-after-free waiting for the right timing.

### 5. Memory

**ABA, replayed deterministically** (listing `ch04-06-aba.rs`). Object pools often keep free slots in a lock-free
stack of *indices* (a Treiber stack, after R. K. Treiber's 1986 IBM report). Pop reads `head` and `next[head]`, then
CASes `head` from the old head to `next`. The listing splits thread A's pop into those two steps, so we can "preempt"
A between them and run other threads' operations:

```text
 free list: 0 → 1 → 2

 A: pop_begin()            reads head = 0, next = 1          … A is preempted
 B: pop() → 0              free list: 1 → 2
 B: pop() → 1              free list: 2          (B now owns slots 0 and 1)
 B: push(0)                free list: 0 → 2      head is 0 AGAIN
 A: pop_commit(0 → 1)      CAS succeeds: "head is still 0" … but next[0] is no longer 1
 C: pop() → 1              C gets slot 1, which B still owns
```

```text
naive:  A's stale CAS succeeded = true; A got slot 0; C got slot 1; B still owns slot 1
tagged: A's stale CAS succeeded = false; A retried and got slot 0; C got slot 2; B owns slot 1
```

A's CAS compared the head *value*, and the value had come back. The decision A made ("next is 1") was stale. The fix
in the listing packs a **version tag** next to the index in one `AtomicU64` (`(tag << 32) | index`). Every successful
CAS increments the tag, so "same index, different history" no longer compares equal. Java has the same fix as a class,
`AtomicStampedReference`. Two limits: a 32-bit tag can in principle wrap around (after 4 billion operations between
A's two steps, astronomically unlikely but not impossible), and tagging a full 64-bit *pointer* would need a 128-bit
CAS, which std doesn't offer for the baseline x86-64 target (listing `ch04-11-no-atomic-u128.rs`: `error[E0432]:
unresolved import std::sync::atomic::AtomicU128`; `lock cmpxchg16b` isn't in that target's feature set).

**Memory reclamation.** With pointers instead of indices, ABA gets worse, because the "A" that came back may be a
*freed and reallocated* node (allocators reuse addresses, Chapter 1.1). And there's a second problem even without ABA:
after thread A reads `head` and before it reads `head.next`, thread B may pop that node and free it. A then reads freed
memory. In Java, the garbage collector solves this: a node isn't freed while any thread holds a reference. Rust has no
GC, so lock-free structures need a **reclamation scheme**:

| Scheme | Idea | Cost | Used by |
|---|---|---|---|
| Epoch-based (EBR) | threads "pin" the current epoch while reading; unlinked nodes are freed two epochs later, when no pinned thread can see them | cheap pins; memory can pile up if a thread stays pinned | `crossbeam-epoch` |
| Hazard pointers | each thread publishes the pointers it's about to dereference; a node is freed only when no hazard pointer names it | a store + fence per protected pointer; bounded garbage | `haphazard` crate; C++26 `<hazard_pointer>` |
| Reference counting | per-node counts | an RMW per access; contention | `Arc`-based structures |
| Don't free (arena, generational indices) | reuse slots, detect staleness with generations | memory is never returned | Chapter 3.6, the tagged free list above |

Listing `ch04-07-epoch-stack.rs` is a Treiber stack with epoch reclamation (after `crossbeam-epoch`'s own example):
`pop` unlinks the node with a CAS, then `guard.defer_destroy(head)`, freeing it only after every thread that might have
seen it has unpinned. Four threads each push and pop 200,000 values:

```text
pushed 800000 values, popped sum = 319999600000 (expected 319999600000)
```

Miri accepts it under **Tree Borrows** (`debug+tree miri-ok`, 80 values). Under the default Stacked Borrows, Miri
reports UB, but inside `crossbeam-epoch` 0.9.20 itself, not in the stack: `Local::element_of`, a `container_of`-style
cast from a pointer to a field back to its containing struct, which Stacked Borrows forbids and Tree Borrows allows.
The aliasing models are still experimental, and Chapter 15.2 covers the difference. The listing also gives the stack
its own `Collector`, so dropping the stack runs every deferred free. With the global default collector, garbage that
is still pending when the process exits shows up as leaks in Miri's report.

### 6. CPU / OS

**False sharing, measured** (listing `ch04-08-false-sharing.rs`; Parts V and IX promised this). Four threads each
increment a counter 5 million times with `fetch_add(Relaxed)`. Only the counters' placement changes:

```text
size_of: AtomicU64 = 8, Pad64 = 64, CachePadded<AtomicU64> = 128
one shared counter (true sharing)                   13.64 ns per increment per thread
own counter, adjacent in one line (false sharing)   14.96 ns per increment per thread
own counter, 64-byte aligned                         4.62 ns per increment per thread
own counter, CachePadded (128 bytes)                 5.76 ns per increment per thread
thread-local count, one fetch_add at the end         1.20 ns per increment per thread
```

(One run, release, 4 vCPUs, noisy. An earlier run of the same experiment gave 17.8 / 19.1 / 3.3 / 3.4 / 0.6 ns. The
ratios are stable, the absolute numbers aren't.)

Four threads updating **four different variables** that share a 64-byte line were as slow as four threads updating
**one** variable. Coherence works per line, so each `lock xadd` had to pull the whole line into its core's cache in
Modified state, invalidating the other three copies. Padding each counter to its own line removed the traffic and made
each increment 3-4× cheaper. Counting locally and publishing once removed atomics from the loop entirely.

On this machine (AMD Zen 4), 64-byte and 128-byte padding performed the same within noise. `crossbeam`'s
`CachePadded` uses 128 bytes on x86-64 and aarch64 because some x86 CPUs' adjacent-line prefetchers fetch lines in
pairs and some Arm cores use 128-byte lines (Chapter 5.2), so 128 is the portable choice. Chapter 20.5 goes further
into cache effects.

**Spinning and the OS** [OS]. A spinlock is only as good as the assumption that the holder is *running*. If the
lock holder is descheduled (preempted, or throttled by a container CPU quota), every waiter burns its time slice
spinning on a lock that can't be released. That's why `std::sync::Mutex` spins briefly and then sleeps in the kernel
(a futex wait on Linux; Chapter 11.3). The `spin_loop()` hint compiles to x86's `pause`, which reduces power and
pipeline pressure while spinning. Its latency varies a lot between microarchitectures (Intel documented a large
increase with Skylake), so backoff tuned on one CPU can misbehave on another.

---

## Pass 3 · Architect level — *When to build lock-free, and when not to*

### 7. Trade-offs

| Approach | Progress | Complexity | Reclamation | Tail latency | Verify with |
|---|---|---|---|---|---|
| `Mutex` / `RwLock` | blocking | low | not an issue | holder preemption → waiter stalls | ordinary tests |
| Spinlock | blocking (busy) | low | not an issue | terrible if the holder is descheduled | avoid in user space unless critical sections are tiny and threads are pinned |
| Seqlock | readers retry, writer never blocks | medium | none (data is inline) | reader starvation under constant writes | Miri, loom |
| `ArcSwap` / RCU-style snapshot | readers wait-free-ish | low (library) | handled by library | writer pays | library's tests |
| CAS loop on one word | lock-free | low | none | retries under contention | Miri |
| Lock-free linked structures | lock-free | **high** | **required** (epochs, hazard pointers) | good, if done right | Miri, loom, stress on Arm |
| Sharding / striping | no contention by design | low | none | best | ordinary tests |

The honest summary: **most "we need lock-free" problems are contention problems, and sharding solves contention more
cheaply than lock-freedom.** The false-sharing numbers above show the cost that matters, cache-line traffic. A mutex
per shard with little contention is fast. Lock-free structures earn their complexity when threads must not block (a
signal handler, a real-time thread, a tail-latency budget that can't absorb a descheduled lock holder).

### 8. Java comparison

| Java | Rust |
|---|---|
| `AtomicLong.compareAndSet(old, new)` | `compare_exchange(old, new, SeqCst, SeqCst)`, or weaker orderings |
| `weakCompareAndSetPlain` / `...Volatile` (JDK 9) | `compare_exchange_weak` with `Relaxed` / `SeqCst` |
| `getAndUpdate(f)`, `accumulateAndGet` | `update(..)`, `try_update(..)` (formerly `fetch_update`) |
| `AtomicStampedReference` (ABA fix) | a tag packed next to an index (`ch04-06`), or epochs |
| `ConcurrentLinkedQueue`, `ConcurrentLinkedDeque` | `crossbeam` queues (`SegQueue`, `ArrayQueue`) |
| `LongAdder` | striped, cache-padded counters (Chapter 14.5) |
| `@Contended` (`jdk.internal.vm.annotation`) padding | `CachePadded<T>`, `#[repr(align(128))]` |
| the GC reclaims unlinked nodes | epochs, hazard pointers, or refcounts, explicitly |

> **Analogy limit.** "Lock-free code in Rust is like lock-free code in Java" breaks on reclamation. In Java, a node
> that any thread still references *can't* be freed, so a lock-free stack's pop is memory-safe even when it's logically
> wrong. The GC also makes classic pointer-ABA much rarer, since a node's address can't be reused while someone holds
> it. In Rust, both are your problem, and getting them wrong is a use-after-free, not a lost update. That's why Rust
> code reaches for `crossbeam-epoch` or a library structure, where Java code might hand-roll a CAS on a reference.

### 9. Production scenario

**Meridian's gateway metrics, per worker.** Chapter 14.2 left the gateway's process-wide counters as shared
`Relaxed` atomics. A profile of the Graviton pods under load showed `requests_total.fetch_add` among the top hot
spots. It wasn't the instruction that cost, it was the cache line: every worker thread in the pod (one per vCPU)
incrementing one counter. The `ch04-08` numbers explain it. A shared counter costs roughly as much as a contended line transfer, and
false sharing costs the same even when the counters are logically separate. That second case had bitten the first
redesign, which gave each worker its own counter in a `Vec<AtomicU64>`, adjacent in memory.

The shipped design: each worker has a `CachePadded` block of its own counters (requests, bytes, errors by class), and
the scrape handler sums across workers every 15 seconds. It's Chapter 14.5's striped counter with one stripe per
worker. The scrape reads each worker's block with `Relaxed` loads. The totals aren't a snapshot of one instant (§9 of
Chapter 14.2 explains why that's acceptable for rates). Hot-path cost went from a contended line transfer to an
uncontended `lock xadd` on a line the worker owns, 3-4× cheaper in the Playground measurement, and no longer growing
with the worker count.

### 10. Failure scenario

**Double-booked reservation slots in risk limits.** The risk-limits service (100K ops/s, a no-breach invariant) holds
in-flight exposure reservations in a fixed pool of slots, and a lock-free free list of slot *indices* hands them out.
It's exactly the naive list in `ch04-06`. In a load test at 3× normal traffic, two concurrent payments occasionally
received the **same** slot, and one reservation silently overwrote the other. The merchant's exposure was
under-counted by one payment, which is exactly the kind of "breach by construction" the invariant exists to prevent.

The investigation took a week because the bug needed a specific interleaving: a thread preempted between reading
`head`/`next` and its CAS, while other threads popped two slots and pushed one back. It reproduced only under
oversubscription (more runnable threads than cores), when preemption in that window became likely. The deterministic
replay in `ch04-06` was written during the postmortem, and it turned a one-in-millions interleaving into a unit test.

The fixes, in the order the team weighed them:

1. **Tag the head** (`TaggedFreeList` in `ch04-06`): correct, small, and the stale CAS now fails.
2. **Replace the free list with a `Mutex<Vec<u32>>`**: at 100K ops/s spread over sharded pools, an uncontended mutex
   costs tens of nanoseconds per reservation, invisible against a 1 ms p99.
3. **Pre-assign slot ranges per worker thread**, which removes sharing entirely.

They shipped option 2 immediately and option 3 the next quarter. The tagged list stayed in the codebase as a teaching
example, next to the postmortem. The review rule that came out of it: **a hand-written lock-free structure needs a
written argument for ABA and for reclamation, a Miri test, and a benchmark showing that a lock was too slow.** Without
the benchmark, use the lock.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XIV).*

1. What's the difference between `compare_exchange` and `compare_exchange_weak`? When should you use each?
2. Why does a spinlock's `lock` need `Acquire` and its `unlock` `Release`, if the CAS already guarantees mutual
   exclusion?
3. Explain ABA with a concrete interleaving. Name three fixes and their costs.
4. What is the memory reclamation problem, and why doesn't Java have it? Compare epochs and hazard pointers.
5. How does a seqlock work? Why must its data fields be atomics in Rust, even though torn reads are discarded?
6. What does a `fence(Acquire)` do that an `Acquire` load doesn't? Use `Arc::drop` to explain.
7. What does `fence(SeqCst)` compile to on x86-64, and why isn't it `mfence`?
8. What is false sharing? How would you detect it in production, and how do you fix it?

### 12. Exercises

- **Beginner.** Rewrite `record_max_cas` with `fetch_max`, and explain why the result is the same and why no ordering
  stronger than `Relaxed` is needed.
- **Intermediate.** Add a `try_lock` to `ch04-02`'s spinlock. Which orderings does it need on success and on failure?
  Verify it with Miri.
- **Advanced.** Implement `MiniWeak` for `ch04-09`'s `MiniArc` (a weak count, `upgrade` via a CAS loop that refuses to
  go from 0 to 1). Write down each ordering and the edge it creates, then verify with Miri.
- **Systems.** Change `ch04-08` to use 8 threads on the 4-vCPU Playground. Predict what happens to each row, especially
  the spinning-free ones, then run it. What does oversubscription do to a spinlock (try it with `ch04-02`)?
- **Architecture.** For the risk-limits reservation pool, estimate the cost of option 2 (a sharded mutex) at 100K
  ops/s, and write the benchmark you'd use to decide between options 2 and 3.

### 13. Debugging exercise

A lock-free "latest value" cell lets one writer publish readings and many readers take the most recent one:

```rust,ignore
pub struct Latest { ptr: AtomicPtr<Reading> }

impl Latest {
    pub fn publish(&self, r: Reading) {
        let new = Box::into_raw(Box::new(r));
        let old = self.ptr.swap(new, AcqRel);
        if !old.is_null() {
            drop(unsafe { Box::from_raw(old) });
        }
    }
    pub fn read(&self) -> Reading {
        let p = self.ptr.load(Acquire);
        unsafe { (*p).clone() }
    }
}
```

1. The orderings look right: Release on publish, Acquire on read. Find the memory-safety bug. (Hint: when does the
   writer free `old`, and what might a reader be doing at that moment?)
2. Would `SeqCst` everywhere fix it? Why not?
3. Fix it three ways: with `arc_swap`, with `crossbeam-epoch`, and with a `Mutex<Arc<Reading>>`. Compare their read-path
   costs.

### 14. Design exercise

**A bounded MPMC work queue for Meridian's settlement batcher** (Chapter 9.4's EDF scheduler feeds it). Producers are
the scheduler threads, and consumers are 16 settlement workers. Requirements: bounded memory, blocking when empty
(workers must sleep, not spin), p99 enqueue under 10 µs, and no job ever lost or run twice. Compare `Mutex<VecDeque>`
+ `Condvar`, `crossbeam::channel::bounded`, and a hand-written lock-free ring. For each, name the reclamation story,
the blocking story, and how you'd test it. Pick one and write the ADR paragraph.
