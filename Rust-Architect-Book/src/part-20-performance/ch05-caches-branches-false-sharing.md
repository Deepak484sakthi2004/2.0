# Chapter 20.5 — Caches, Branch Prediction, and False Sharing

> **Where this sits:** Part XX · Performance Engineering · chapter 5 of 7
> **Prerequisites:** Chapter 9.5 (the three rules of the memory hierarchy, AoS vs SoA, shuffled boxes), Chapter 5.2
> (niches and hot/cold splitting), Chapter 14.4 and Chapter 11.7 (false sharing measured), Chapter 6.5 (dispatch
> benchmark).
> **After this chapter you can:** quote this machine's memory-hierarchy latencies from your own measurement and
> explain each step; measure what a branch misprediction costs and recognize code where the compiler removed the branch
> for you; choose between array-of-structs, hot/cold splitting, and structure-of-arrays by access pattern; explain why
> false sharing slows down threads that only *read*; and decide when huge pages help and when they hurt.

---

## Pass 1 · User level — *The machine under the code*

### 1. Problem

Chapter 9.5 stated the three rules that decide how fast data-heavy code runs: **bytes touched, dependent loads, and
predictability.** It measured them with collections: structure-of-arrays 12× faster than array-of-structs for a column
scan, shuffled boxes 37× slower than a flat vector. This chapter goes one level down, to the hardware mechanisms behind
those rules, and pays off the layout promises earlier Parts left open:

- Chapter 5.2's systems exercise: `Option<u64>` (16 bytes) vs `Option<NonZeroU64>` (8 bytes), at three sizes.
- Chapter 5.2 and 9.5: hot/cold splitting, and why structure-of-arrays slowed the matcher down (9.5's incident).
- Chapter 9.4: the order-book redesign, benchmarked.
- Part IX: transparent huge pages.
- Chapters 5.2, 9.5, 11.7, 14.4: false sharing, with a case the earlier chapters didn't show.

### 2. Mental model

```text
 core ──► L1 (32 KiB, ~1.6 ns) ──► L2 (1 MiB, ~5 ns) ──► L3 (16 MiB shared, ~16 ns) ──► DRAM (~130–170 ns)
           the unit of transfer at every level is a 64-byte cache line
           the TLB caches virtual→physical page translations; a miss adds a page-table walk
           prefetchers fetch ahead for sequential and constant-stride access
           branch predictor guesses every branch ~15–20 cycles before it resolves; a wrong guess discards that work
           coherence: a line can be written by one core at a time; a write invalidates every other copy
```

(Sizes are this machine's, read from `/sys` by `ch05-01`; latencies are `ch05-01`'s measurements.)

Every performance effect in this chapter is one of four costs: **a miss** (the data wasn't in the nearest cache), **a
dependency** (the next load can't start until this one finishes), **a misprediction** (the CPU guessed a branch wrong),
or **a transfer** (another core owns the line). Good layouts reduce misses, break dependencies, make branches
predictable or remove them, and keep each written line on one core.

### 3. Rust code

**The latency ladder** (listing `ch05-01-latency-ladder.rs`). Pointer chasing through a random cycle (Sattolo's
algorithm guarantees one cycle through every element), one element per 64-byte line, so every load depends on the
previous one and the prefetcher can't guess the next address:

```rust,ignore
// excerpt of ch05-01-latency-ladder.rs
#[repr(align(64))]
#[derive(Clone, Copy)]
struct Line {
    next: usize,
}
// ...
let t = Instant::now();
for _ in 0..loads {
    p = lines[p].next;
}
```

```text
caches (cpu0): L1 Data 32K, L1 Instruction 32K, L2 Unified 1024K, L3 Unified 16384K
working set  ns per load
    16 KiB          1.6
    32 KiB          1.7
   128 KiB          4.4
   512 KiB          5.4
     1 MiB          9.2
     4 MiB         16.3
    16 MiB        128.7
    64 MiB        151.3
   256 MiB        171.0
```

Four plateaus: L1 (~1.6 ns, about 5 cycles), L2 (~4–5 ns), L3 (~16 ns), and DRAM (~130–170 ns). Each step is where the
working set outgrows a level. Two details matter. At 1 MiB the set exactly fills L2 and shares it with page tables and
everything else, so it's already paying some L3 trips (9.2 ns). At 16 MiB, nominally the L3's size, it's almost all
DRAM: the L3 is shared with the other cores and with other tenants, and a random walk over 16 MiB also misses in the
TLB. From 16 MiB up, latency keeps climbing (~130 → ~170 ns) as more loads also need a page-table walk whose own entries
miss the caches (predicted from mechanism; `perf stat -e dTLB-load-misses` would confirm it locally).

**Branch prediction** (listing `ch05-02-branch-prediction.rs`). 4 million bytes; for each byte ≥ 128, call a small
non-inlined function (so the compiler must keep a real branch):

```rust,ignore
// excerpt of ch05-02-branch-prediction.rs
#[inline(never)]
fn branchy(v: &[u8]) -> u64 {
    let mut acc = 0u64;
    for &x in v {
        if x >= 128 {
            acc = on_big(acc, x); // taken ~50% of the time on random data
        }
    }
    acc
}
```

```text
ns per element (best of 5):
  branch + call, random order      3.75
  branch + call, sorted            0.82
  branch + call, repeating 2-on-2-off  0.89
  count (branch-free), random      0.36
  count (branch-free), sorted      0.36
taken fraction, random: 0.501
```

Same data, same code: random order costs **4.6×** sorted order. On random data the branch is taken with probability
0.501, so the predictor is wrong about half the time; sorted, it's wrong about once. A repeating pattern
(taken, taken, not, not, …) is learned perfectly (0.89 ns). So one misprediction costs about
(3.75 − 0.82) / 0.5 ≈ **5.9 ns**, roughly 20 cycles on this machine: the depth of the pipeline the CPU throws away.
The last two rows are the same question asked without a call in the body (`filter(|&&x| x >= 128).count()`): the
compiler removed the branch entirely (§4), and order no longer matters.

**A lexer's inner loop: comparison chain vs class table** (listing `ch05-08-byte-class-table.rs`, Chapter 17.2's
Systems exercise in miniature). Classify every byte as whitespace, identifier, digit, punctuation, or other, either with
a chain of `if`s or with one load from a 256-entry table built at compile time (the technique Chapters 9.2 and 17.2
recommend). Three 1 MiB inputs: source-like text (a Rust snippet, repeated), a perfectly periodic `a b c …`, and a
random sequence with the same byte frequencies as the source-like text:

```text
ns per byte (best of 7):          chain   table
  source-like text              1.913   1.094
  periodic "a b c ..."          0.896   0.402
  random, same byte mix         7.047   1.073
```

The chain's cost depends on how predictable the *sequence* of classes is: 0.9 ns per byte when it alternates perfectly,
1.9 ns on the repeated snippet (a ~170-byte pattern the predictor partly learns), and 7.0 ns on the same bytes in random
order. Real source code doesn't repeat every 170 bytes, so the random row is closer to the truth for a real lexer. The
table costs about 1.1 ns per byte whatever the order: one load, no data-dependent branch. (The table's periodic row is
faster because two alternating counters don't wait on each other; with one class repeating, every increment waits for
the previous store to the same counter.) Chapter 17.2's claim, measured: **on unpredictable input, a table beats a
comparison chain by 6–7×.**

**Chapter 5.2's exercise: niche vs tag in memory** (listing `ch05-03-niche-scan.rs`). Sum the present values of
`Option<u64>` (16 bytes each) and `Option<NonZeroU64>` (8 bytes each), 10% `None`, median of 11:

```text
  elements    Option<u64> Option<NonZero>   ratio   (ns per element, median of 11)
      1000          0.337          0.075    4.47   (0 MB vs 0 MB)
   1000000          0.347          0.079    4.39   (16 MB vs 8 MB)
  16000000          0.514          0.230    2.23   (256 MB vs 128 MB)
```

The exercise asked where the gap appears and whether it matches the bytes-per-element ratio (2×). **In cache, the gap
is 4.4×, more than the bytes**: the tagged loop is compute-bound, because Chapter 5.2's assembly showed it shuffling tags
and values apart and masking every element, while the niche loop just adds (`None` is 0). **From DRAM, the gap is 2.2×,
almost exactly the bytes ratio**: both loops now wait on memory bandwidth, and the tagged one streams twice as much. The
size of a type is a bandwidth cost once data leaves the cache, and an instruction cost before that.

**Hot/cold splitting vs structure-of-arrays, by access pattern** (listing `ch05-04-hot-cold.rs`). 500,000 accounts:
16 bytes of hot fields (balance, limit) and 112 bytes of cold ones (KYC reference, address). Two workloads: a *report*
that scans every balance, and *payments* that each touch one random account's hot fields and one byte of its cold
record:

```text
bytes per account: AoS 128, hot 16, cold 112
report scan (500K balances), ms:       AoS   2.62   hot/cold   0.10   SoA   0.04
payments (1M random accounts), ms:     AoS  11.13   hot/cold  12.93   SoA  13.45
```

The scan loves the split layouts: AoS drags 64 MB of mostly cold bytes through the caches (2.6 ms); hot/cold reads 8 MB
(0.10 ms); SoA reads just the 4 MB balance column (0.04 ms, and it vectorizes). The random payments reverse the order:
AoS is fastest because one account's hot and cold fields are in the same two cache lines, while hot/cold needs two
unrelated lines and SoA three (balance, limit, cold). **The layout that wins depends on the access pattern, and a system
usually has both.** That's Chapter 9.5's matcher incident, measured: the SoA arena made the risk report 6× faster and
the matcher's per-order path slower. The differences on the random path are smaller than on the scan (1.2×, not 26×)
because independent random loads overlap in the memory system (memory-level parallelism, Chapter 9.5).

**False sharing hurts readers too** (listing `ch05-06-read-mostly-neighbor.rs`). Chapters 11.7 and 14.4 measured false
sharing between *writers*: four threads incrementing counters that share a line (14.96 vs 5.76 ns per increment in
14.4). Here three threads only *read* a config value, while one thread increments a counter that happens to sit in the
same cache line:

```rust,ignore
// excerpt of ch05-06-read-mostly-neighbor.rs
struct SameLine {
    config_version: AtomicU64,
    counter: AtomicU64,
}

struct Apart {
    config_version: CachePadded<AtomicU64>,
    counter: CachePadded<AtomicU64>,
}
```

```text
same line: fields 8 bytes apart; padded: 128 bytes apart
ns per config read, mean of 3 reader threads (1 writer incrementing the counter):
  config and counter in one line     1.41
  config and counter padded apart    0.35
```

Reads got **4× slower**, and no reader wrote anything. Every increment by the writer invalidates the line in the three
readers' caches, so their next read misses and has to fetch the line back. The config value never changed; its
*neighbour* did. Read-mostly data (configuration, feature flags, routing tables' headers) that sits next to a hot counter
pays this on every read.

---

## Pass 2 · Systems level — *The mechanisms*

### 4. Under the hood

**Caches.** [CPU] A cache is organized in *sets* of *ways*: an address maps to one set by some of its bits and can live
in any way of that set. A 32 KiB, 8-way L1 has 64 sets of 8 lines each. Consequences worth knowing: data larger than a
cache evicts itself (capacity misses, `ch05-01`'s steps); strides that are large powers of two map many addresses to the
same set and evict each other early (conflict misses: walking a matrix column whose row length is 4,096 bytes is the
classic case); and every miss moves a whole 64-byte line, so touching one byte of a line costs as much as touching all of
it (`ch05-04`'s AoS scan).

**Prefetchers and memory-level parallelism.** [CPU] Hardware prefetchers detect sequential and constant-stride access
and fetch lines ahead of use; that's why the scans above run near memory bandwidth. A core can also have many
independent misses outstanding at once (on the order of 10–20 L1 miss buffers per core on current server cores), so
loads that don't depend on each other overlap. `ch05-01` defeats both on purpose (random order, each address depends on
the previous load); `ch05-05`'s random reads don't depend on each other, and cost ~10 ns each rather than ~170 ns. Same
DRAM, 17× difference: that's MLP.

**Branch prediction.** [CPU] Modern predictors combine the branch's own history with the recent history of other
branches (TAGE-like designs), which is how `ch05-02`'s 2-on-2-off pattern is learned perfectly. A misprediction is
discovered when the branch executes, many pipeline stages after the guess; everything fetched and executed since is
discarded. `ch05-02` measured ~20 cycles per misprediction. The compiler can avoid the question entirely by computing
both outcomes and selecting one (`cmov`), or, for simple loops, by vectorizing. Here is what LLVM did with the
branch-free `count_big` (release, `tools/emit.ps1 -CrateType bin`; excerpt):

```text
playground::count_big:
.LBB12_6:
	movzx	edx, word ptr [rdi + rax]      ; two bytes
	movd	xmm4, edx
	...
	psrlw	xmm4, 7                        ; shift each byte's top bit (x >= 128) down to bit 0
	pand	xmm4, xmm3                     ; keep just that bit: 1 if x >= 128, else 0
	...
	paddq	xmm2, xmm4                     ; add the 0/1 values: no branch at all
```

and the `branchy` loop, where the call forces a real conditional jump:

```text
playground::branchy:
.LBB11_4:
	movzx	ecx, byte ptr [r14 + r15]
	test	cl, cl                         ; sign bit set <=> x >= 128
	jns	.LBB11_5                       ; the branch the predictor has to guess
	movzx	esi, cl
	mov	rdi, rax
	call	playground::on_big
	jmp	.LBB11_5
```

Note the trick in both: `x >= 128` for a byte is "the top bit is set", so the compiler tests the sign bit (`test` +
`jns`) or shifts it down (`psrlw 7`) instead of comparing with 128. The same data-dependent branch appears as an
*indirect* call in Chapter 6.5's mixed `dyn` dispatch; `perf stat -e branch-misses` (Chapter 20.3) is how you'd confirm
that it mispredicts about half the time there too.

**Coherence.** [CPU] Caches keep one line consistent with a protocol from the MESI family: a line is Modified (one
core has written it), Exclusive, Shared (read-only copies), or Invalid. To write, a core needs the line exclusively, so
it invalidates every other copy; the next reader on another core misses and gets the line transferred from the writer's
cache. `ch07-04` measures that transfer directly (a round trip between two cores costs 56–70 ns on this machine).
False sharing is this protocol working exactly as designed, on data you didn't intend to share.

In production you find it with `perf c2c` (not verified here; it needs `perf` and, on Intel, load-latency sampling
support): `perf c2c record -p <pid> -- sleep 10` then `perf c2c report`. It lists the cache lines with the most HITM
events (a load that found the line Modified in another core's cache), the byte offsets within each line that were read
and written, and the code that touched them. Two different offsets in one hot line, one written and one read by other
threads, is `ch05-06`'s signature.

### 5. Memory

**TLBs and huge pages** (listing `ch05-05-huge-pages.rs`). Every memory access translates a virtual address; the TLB
caches recent translations, and with 4 KiB pages a TLB of a few thousand entries covers only a few MiB. Transparent huge
pages (THP) let the kernel back memory with 2 MiB pages instead, so one TLB entry covers 512 times as much. The listing
maps 256 MiB twice, once with `MADV_NOHUGEPAGE` and once with `MADV_HUGEPAGE`, touches every page, then does 4 million
random reads:

```text
THP enabled: always [madvise] never
THP defrag: always defer defer+madvise [madvise] never
4 KiB pages      madvise rc 0: first touch  118.8 ms,  65539 minor faults, AnonHugePages      0 MiB; random read  10.1 ns
huge pages       madvise rc 0: first touch  235.3 ms,  62981 minor faults, AnonHugePages     10 MiB; random read   9.7 ns
```

This is an honest negative result, and a useful one. THP is enabled in `madvise` mode, the call succeeded, and the
kernel still backed only **10 MiB** of the 256 MiB with huge pages (`AnonHugePages` in `/proc/self/smaps_rollup`): it
couldn't find enough free, contiguous 2 MiB physical blocks on this busy shared host. With `defrag=madvise`, it tried
to compact memory synchronously to get them, which **doubled the first-touch time** (235 vs 119 ms) for almost no huge
pages. The random-read time didn't change because almost nothing changed. On a machine with free contiguous memory the
same request would typically cut page faults by ~512× and remove most TLB misses from `ch05-01`'s DRAM rows; that effect
is predicted from mechanism and not verified here. **A huge-page request is a request, not a guarantee: verify it with
`AnonHugePages`, and measure what the attempt costs.**

**Allocation layout is data layout** (listing `ch05-07-order-book.rs`, Chapter 9.4's promise). The same stream of one
million order-book operations (60% limit orders, 30% cancels, 10% market orders; both books FIFO per price level with
lazy cancel) against the Part III review's Java-shaped book (every order an `Rc<RefCell<Order>>`, levels holding `Rc`
clones) and Chapter 9.4's arena (orders in a `Slab`, levels holding indices):

```text
Java-shaped    total  100.5 ms   ns/op p50   89 p99   246 p99.9    468   allocs/op 0.82   filled 15053091
arena          total   71.2 ms   ns/op p50   63 p99   120 p99.9    286   allocs/op 0.22   filled 15053091
```

Identical fills (the logic is the same), 1.4× the throughput, half the p99, and a quarter of the allocations. The
arena's orders are packed in one contiguous slab whose freed slots are reused, so recently touched orders share cache
lines; the Java-shaped book allocates every order separately and reaches it through a pointer (and a `RefCell` borrow
flag). The remaining 0.22 allocations per operation are new price levels (`VecDeque`s) and table growth, which a
production book would preallocate for its price band.

### 6. CPU / OS

**The instruction cache and code layout.** [CPU] Code is data too: the L1 instruction cache is 32 KiB here
(`ch05-01`'s first line), and the front end fetches and decodes instructions in aligned blocks. Chapter 20.2's layout
bias (`ch02-11`: the same loop, 1.7× apart at different addresses) is a front-end effect. Chapter 7.3's warning about
duplicated hot code is the capacity version: monomorphizing a hot generic function for many types puts several copies
in the i-cache, and if they're all hot they compete. That cost is **predicted from mechanism and not measured here**;
the exercise below measures it, and `perf stat -e L1-icache-load-misses` measures it locally. BOLT (Chapter 20.3)
attacks exactly this, by reordering functions and blocks so hot code is contiguous.

**SMT.** [CPU] When two hardware threads share a core, they share its L1 and L2 caches and its execution units. The
Playground's 4 vCPUs each report themselves as their own SMT sibling (`ch07-04`), so this machine can't show SMT effects;
on your own hardware, a cache-bound benchmark pinned to two siblings typically runs slower per thread than on two
separate cores.

---

## Pass 3 · Architect level — *Layout decisions*

### 7. Trade-offs

| Access pattern | Layout | Why | Evidence |
|---|---|---|---|
| Scans over one or two fields of many records | SoA or hot/cold split | Bytes touched; vectorization | `ch05-04` scan: 26× (hot/cold), 65× (SoA) vs AoS |
| Random access to whole records | AoS | One or two lines per record | `ch05-04` payments: AoS fastest |
| Both (the usual case) | Hot/cold split, or AoS plus a columnar snapshot for the scans | Keeps records together for the hot path; the report reads a copy | 9.5's matcher fix |
| Optional values in bulk | Niche types (`NonZero*`, `Option<&T>`, enums with spare values) | Half the bytes; simpler loops | `ch05-03`: 4.4× in cache, 2.2× from DRAM |
| Many small objects with shared lifetime | Arena/slab with indices | Contiguity, slot reuse, no per-object allocation | `ch05-07`: 1.4× throughput, 2× p99 |
| Data-dependent branches in hot loops | Sort or group by the condition; branch-free formulations | Mispredictions ~6 ns each | `ch05-02`; 6.5's grouped dispatch |
| Written counters near read-mostly data | Pad (`CachePadded`) or separate by thread | Coherence traffic hits readers | `ch05-06`: reads 4× slower |
| Very large, randomly accessed heaps | Huge pages, explicitly requested and verified | TLB reach | `ch05-05`: not granted here |

**Padding costs memory.** `CachePadded<AtomicU64>` is 128 bytes for 8 bytes of data. Pad the few hot, shared,
written variables; don't pad arrays of millions of values (Chapter 14.4's per-worker blocks are the pattern: pad per
worker, not per counter).

### 8. Java comparison

| | Java | Rust |
|---|---|---|
| Record layout | Objects on the heap with a 12-byte header (compressed class pointers), fields reordered by the JVM | Structs inline, fields reordered by rustc (Chapter 5.2), no header |
| Array of records | `Order[]` is an array of *references*: every element is a pointer to a separate object (Chapter 9.5's `Vec<Box<T>>`) | `Vec<Order>` stores the records inline |
| Value types | Project Valhalla's value classes (JEP 401) aim to flatten them; hedged, not in a final release at the time of writing | Every struct is a value type |
| Padding against false sharing | `@Contended` (JDK internal; needs `-XX:-RestrictContended` for application classes) | `CachePadded<T>`, `#[repr(align(128))]` |
| Branch-free idioms | The JIT may emit `cmov` from profiles | LLVM decides at build time; PGO (20.3) supplies profiles |
| GC and locality | Compacting collectors can improve locality by moving objects together (allocation order ≈ layout) | Objects never move; layout is your decision |

> **Analogy limit.** A Java engineer's intuition "the GC compacts, so fragmentation and locality take care of
> themselves" doesn't transfer. In Rust, what you allocate together and store together is what stays together, which is
> a burden (you have to design it) and a guarantee (it won't change under you).

### 9. Production scenario

**The order-book decision, with numbers.** Meridian's matching engine (the Part III review artifact, redesigned in
Chapter 9.4) had to justify its move from the Java-shaped `Rc<RefCell>` prototype to the slab arena. `ch05-07` is the
shape of the benchmark the team used, with their production order stream instead of a synthetic one:

- **Throughput and tail.** The arena book handled the same stream 1.4× faster, with a p99 per operation of 120 ns vs
  246 ns. For a matching engine the p99 is the number that matters: it bounds how long a burst takes to drain.
- **Allocation.** 0.82 → 0.22 allocations per operation, and the rest was eliminated by preallocating price levels
  for the instrument's price band.
- **Layout.** Orders live in one slab; levels hold `usize` indices (8 bytes) instead of `Rc`s (also 8 bytes, but each
  pointing to a separately allocated `RcBox` that adds two reference counts and the `RefCell` borrow flag: 24 bytes of
  overhead per order, plus the allocator's own). Chapter 5.2's memory budget (10M resting orders at 24 bytes each) holds
  only in the arena design.

The report generator reads a **columnar snapshot** taken once per second, not the live arena (Chapter 9.5's fix),
so neither workload's layout fights the other's.

### 10. Failure scenario

**THP `always` on the session cache (2026).** The session cache's Rust port (Chapter 9.3: 2M sessions, ~490 MB with
`IndexMap`) is randomly accessed and larger than any TLB's reach, so an engineer switched its hosts to
`transparent_hugepage=always` after reading that huge pages cut TLB misses. Median lookup latency improved slightly.
The p99.9 got worse, with spikes of several milliseconds a few times an hour.

Two mechanisms, both visible in miniature in `ch05-05`: with `defrag` set to allow synchronous compaction, a page fault
that wants a huge page can stall while the kernel compacts memory (the doubled first-touch time); and the kernel's
background `khugepaged` merges small pages into huge ones, which briefly holds locks the service's page faults need.
Database vendors document the same trade-off: Redis's documentation warns that THP causes latency and memory problems
and recommends disabling it, while other vendors' advice differs and has changed over time (MongoDB reversed its
long-standing "disable THP" recommendation in version 8.0). The lesson is the same as `ch05-05`'s: measure your
workload.

The fix: THP back to `madvise`, `defrag=defer` (compaction in the background, never in the fault path), and an explicit
`madvise(MADV_HUGEPAGE)` only on the session table's large, long-lived allocation, verified with `AnonHugePages` after
startup and exported as a metric.

---

## Practice

### 11. Interview & architecture questions

1. Quote the latency of an L1 hit, an L3 hit, and a DRAM access on a current server, and explain why pointer chasing
   exposes the full DRAM latency while a scan doesn't.
2. Why does `ch05-01` jump from 16 ns to 129 ns at 16 MiB when the L3 is 16 MiB?
3. How much does a branch misprediction cost, and how did `ch05-02` measure it? When does the compiler remove the
   branch for you?
4. `Option<u64>` vs `Option<NonZeroU64>`: why is the gap 4.4× in cache and 2.2× from DRAM?
5. When is array-of-structs the right layout? When is structure-of-arrays? What do you do when a system needs both?
6. Explain false sharing to a Java engineer, and explain why `ch05-06`'s *readers* slowed down.
7. What is memory-level parallelism, and why did `ch05-05`'s random reads cost 10 ns rather than 170 ns?
8. What do huge pages improve, what can they make worse, and how do you verify you actually got them?
9. Why did the arena order book beat the `Rc<RefCell>` one? Name three separate effects.
10. How would you find out whether duplicated monomorphized code is hurting your service's instruction cache?

### 12. Exercises

- **Beginner.** Add 8 KiB and 64 KiB rows to `ch05-01`. Where exactly does L1 end on this machine?
- **Intermediate.** In `ch05-02`, replace the `on_big` call with `acc += x as u64` and check the assembly. Does the
  branch survive? How does the random-order time change?
- **Advanced.** Add a fourth layout to `ch05-04`: AoS with the cold part behind a `Box`. Predict both rows before
  measuring.
- **Systems.** Measure i-cache pressure from monomorphization: generate 1, 8, and 64 copies of a hot generic function
  (distinct types), call them round-robin in a loop, and compare ns per call. At what point does it change, and does
  that match 32 KiB of L1i?
- **Architecture.** For the risk-limits service (3M merchants, per-payment counter updates plus a per-minute scan of
  80% of them, Chapter 9.5's design exercise), choose a layout and a concurrency design, and list the three
  measurements that would prove your choice.

### 13. Debugging exercise

A metrics library's registry is
`struct Registry { schema_version: AtomicU64, requests_total: AtomicU64, counters: Vec<AtomicU64> }`. Every metric
operation reads `schema_version` and the vector's header (pointer and length), then calls `counters[i].fetch_add(1)`;
the ~200 counters are contiguous, and a dozen of them are incremented on every request. A release added the
`requests_total` field, incremented by every request. Afterwards, the latency of *every* metric operation doubled on
16-core hosts, but not on 2-core hosts. Explain the mechanism, name the two separate false-sharing problems (one new,
one that the release made worse), and give a fix for each that doesn't pad every counter.

### 14. Design exercise

Design the in-memory layout for Meridian's gateway **route table**: ~1,000 routes, looked up on every request (method +
path prefix), read by 16 worker threads, replaced atomically every 10 minutes (Chapter 3.1's route-table refresh), and
scanned once a minute for metrics export. Specify the data structures, where hot and cold fields live, how updates avoid
false sharing with readers, and which of this chapter's listings you'd adapt to measure each decision.
