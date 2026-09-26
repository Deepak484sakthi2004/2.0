# Chapter 20.4 — Allocation and Memory Profiling

> **Where this sits:** Part XX · Performance Engineering · chapter 4 of 7
> **Prerequisites:** Chapter 3.1 (the counting allocator), Chapter 9.1 (Vec growth and capacity policies), Chapter 9.2
> (the access-log allocations), Chapter 9.5 (the fraud feature vector estimate), Chapter 15.3 (zeroed read buffers).
> **After this chapter you can:** build an allocation profile by site, count, and size with nothing but a
> `GlobalAlloc` wrapper; say what an allocation costs at the call site, under threads, and across threads; tell live
> heap from RSS and explain why they diverge; choose between buffer reuse, arenas, and allocator swaps; and verify two
> estimates Part IX left open.

---

## Pass 1 · User level — *Where do the allocations come from, and what do they cost?*

### 1. Problem

Allocation shows up in performance work in three different ways, and each needs its own measurement:

- **CPU at the call site.** Each `malloc`/`free` pair costs ~10 ns for small blocks with glibc (measured below), and
  the code around it pays for cache misses on memory that was just handed out.
- **Tail latency.** Most allocations are fast; some take the slow path (a new page from the kernel, a lock on a shared
  arena, a trim that returns memory). Those land in the p99.
- **Memory utilization.** What the program holds and what the operating system has given it diverge after frees, and
  the container's memory limit is enforced on the second number.

Part IX made two estimates it couldn't verify on the spot: that removing the per-line `format!` calls from the gateway's
access log would help its tail (Chapter 9.2), and that resolving the fraud library's feature names to indices would
save "~3% CPU and 400 allocations per score" (Chapter 9.5). This chapter measures both.

### 2. Mental model

**An allocation profile has four columns: site, count, bytes, and lifetime.** The site tells you where to change the
code; count drives CPU; bytes drive memory and cache footprint; lifetime decides whether a pool, an arena, or plain
reuse is the right fix.

```text
                 short-lived (per request)              long-lived (per process)
 many, small     reuse buffers / arena per request       intern, SmallVec, pack into Vec
 few, large      reuse one buffer with a capacity cap     presize; watch fragmentation and RSS
```

And **memory has at least three sizes**:

```text
 virtual size   everything mapped (heap arenas, stacks, mmaps, reserved but untouched)    rarely what matters
 RSS            pages actually resident in RAM for this process                          what the OOM killer sees
 live heap      bytes your program currently holds through the allocator                 what your code controls
               (RSS − live heap = allocator overhead + freed-but-not-returned + non-heap memory)
```

### 3. Rust code

**An allocation-site profiler in 40 lines** (listing `ch04-01-alloc-site-profiler.rs`). The idea behind dhat and
heaptrack, reduced to a `GlobalAlloc` wrapper: each thread declares its current "site" in a thread-local, and the
allocator attributes every allocation to it and buckets its size by power of two:

```rust,ignore
// excerpt of ch04-01-alloc-site-profiler.rs
unsafe impl GlobalAlloc for Profiling {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let site = SITE.try_with(|s| s.get()).unwrap_or(0);
        COUNT[site].fetch_add(1, Relaxed);
        BYTES[site].fetch_add(layout.size() as u64, Relaxed);
        let bucket = (usize::BITS - layout.size().max(1).leading_zeros()).min(15) as usize;
        SIZE_BUCKETS[bucket].fetch_add(1, Relaxed);
        unsafe { System.alloc(layout) }
    }
    // ...
}
```

The workload is a three-stage order pipeline built from patterns earlier Parts warned about: decode into a dynamic
`serde_json::Value`, "enrich" by cloning a 40-entry config map (Chapter 3.2), and `format!` an access-log line
(Chapter 9.2). 10,000 orders:

```text
site                 allocs        bytes  per order
decode_order         240000     27660000       24.0
enrich_headers       810000     38120000       81.0
access_log            30000       560000        3.0
allocation sizes (all sites, bytes <= bucket):
  <=      4: 80000
  <=      8: 140001
  <=     16: 780080
  ...
```

108 allocations per order, three-quarters of them from one line (`config.clone()`: 40 keys + 40 values + 1 table = 81,
exactly Chapter 3.2's formula), and most of them 16 bytes or smaller. That last fact matters for the fix: tiny, uniform,
short-lived allocations are what arenas and inline storage are best at.

**Chapter 9.2's promise: does removing the access-log allocations change latency?** (listing `ch04-02-access-log-p99.rs`).
The first prototype built each line with one `format!` for the core fields and one per optional field (8 allocations per
line, counting the concatenation); the fix writes into one reused `String`. Batches of 16 lines, 400,000 lines per
thread, on 1 and 4 threads:

```text
ns per line (batches of 16), release; one Playground run, noisy
variant                            p50     p99   p99.9       max  allocs/line
1 thread(s), format! per field     243     252     823      2603         8.00
1 thread(s), reused String         119     121     663      1499         0.00
4 thread(s), format! per field     324     875    9519   1502207         8.00
4 thread(s), reused String         101     119     989   1092607         0.00
```

On one thread, the reused buffer halves the median (243 → 119 ns). On four threads the story is the tail: the
allocating version's p99 is 875 ns and its p99.9 9.5 µs, against 119 ns and 1.0 µs. The median barely noticed the other
threads; the tail did. (The maxima, 1–1.5 ms in both, are the machine taking a vCPU away; they're in both columns, so
they're not the allocator.)

**Chapter 9.5's promise: the fraud feature vector** (listing `ch04-03-fraud-feature-vector.rs`). 400 named features per
score. Before: a `HashMap<String, f64>` built per score, with weights looked up by name. After: names resolved to
indices once at model load, and one reused `Vec<f64>` per worker:

```rust,ignore
// excerpt of ch04-03-fraud-feature-vector.rs
fn score_by_name(m: &Model, event: u64) -> f64 {
    let mut features: HashMap<String, f64> = HashMap::new();
    for (i, name) in m.names.iter().enumerate() {
        features.insert(name.clone(), feature(i, event));
    }
    m.weights_by_name.iter().map(|(name, w)| features[name] * w).sum()
}

fn score_by_index(m: &Model, event: u64, buf: &mut Vec<f64>) -> f64 {
    buf.clear();
    buf.extend((0..FEATURES).map(|i| feature(i, event)));
    buf.iter().zip(&m.weights).map(|(f, w)| f * w).sum()
}
```

```text
score(42): by name -0.283078, by index -0.283078
ns per score; release; one Playground run, noisy
variant                           p50      p99    p99.9       max allocs/score
1 thread(s), HashMap<String,_>    41407    50815    54559     73023        408.0
1 thread(s), index + reused Vec      820      820     6111     28175          0.0
4 thread(s), HashMap<String,_>    51167    65215   300031  14663679        408.0
4 thread(s), index + reused Vec      820      820     7663   6025215          0.0
```

The estimate said "400 allocations per score"; the measurement says 408 (400 key strings plus the table's growth
steps). The estimate said "~3% CPU"; against the ~3 ms of CPU per score Meridian's fraud service spends (Chapter 1.2),
41 µs is **~1.4%**. The estimate was high by 2×, and the section in Chapter 9.5 was right to call it an estimate. The
tail effect under four threads is larger than the median effect: p99.9 went from 300 µs to 7.7 µs.

---

## Pass 2 · Systems level — *What an allocation costs, and where memory goes*

### 4. Under the hood

**glibc malloc in one paragraph.** [LIB] Rust's `System` allocator on Linux is glibc's `malloc`. Small requests are
served from a **per-thread cache** (tcache, since glibc 2.26: a few free chunks per size class, no locking), then from
**arenas** (heaps with their own locks; glibc creates additional arenas as threads contend, up to 8 per core on 64-bit
by default). Large requests (at least 128 KiB by default) are served with `mmap` directly; glibc raises that threshold
dynamically, up to 32 MiB, after it sees an mmapped block freed, so repeated large allocations move back to the heap.
Every chunk carries an 8-byte size field and is rounded up to a multiple of 16 bytes, so a 48-byte object occupies
a 64-byte chunk.

**What that means, measured** (listing `ch04-04-allocator-contention.rs`). Each "request" allocates 32 buffers of 16–512
bytes, writes them, and frees them:

```text
available parallelism: 4
ns per request (32 alloc+free) per thread, best of 3; release; one Playground run, noisy
  1 thread(s): malloc local     347   bump arena    108
  2 thread(s): malloc local     347   bump arena    110
  4 thread(s): malloc local     382   bump arena    217
  1 producer/consumer pair(s): token only (frees local)     770   buffers handed off (cross-thread frees)    4524
  2 producer/consumer pair(s): token only (frees local)     755   buffers handed off (cross-thread frees)    5380
```

Three findings:

- **Local allocation is ~11 ns per allocate-and-free pair, and it scales.** 347 ns for 32 pairs on one thread, and
  roughly the same per thread on four: tcache keeps each thread's small-chunk traffic private. "malloc doesn't scale" is
  folklore for this workload.
- **A bump arena is ~3× cheaper** (108 ns): allocation is a pointer increment and the whole request is freed with one
  `reset`. (The 217 ns on four threads is one noisy run; the arena has no shared state.)
- **Cross-thread frees are the expensive case: ~118 ns per free.** The control (same channel traffic, frees on the
  allocating thread) costs 770 ns per request; handing the 32 buffers to another thread to free costs 4,524 ns. The
  consumer's tcache fills up after a handful of chunks per size, after which its frees go back to the *producer's* arena
  under that arena's lock, while the producer, whose own cache is always empty, allocates from the same arena under the
  same lock [LIB; the mechanism is from glibc's design, and the cost is what this listing measured]. Pipelines that
  allocate on one stage and free on another (Chapter 3.2's ingestion handoff, Chapter 11.5's channels) pay this unless
  they recycle buffers back to the producer or use an allocator built for it.

**Chapter 15.3's promise: zeroing cost vs buffer size** (listing `ch04-06-zeroing-cost.rs`). Each "read" copies `n`
bytes of input into a buffer that is either freshly allocated (`vec![0u8; n]`), reused but zeroed first (`clear` +
`resize`, the pattern Chapter 15.3 found in a profile), or reused and simply overwritten (15.3's `PooledBuf`):

```text
     size      fresh     zeroed     reused      (ns per KiB)
    4 KiB       22.0       18.8        8.9
   16 KiB       20.0       18.4        9.8
   64 KiB       25.8       25.5       17.9
  256 KiB       26.9       26.8       17.9
    1 MiB       32.1       32.0       21.5
    4 MiB       31.7       31.6       21.4
```

Zeroing doubles the cost of a small read (18.8 vs 8.9 ns per KiB: a `memset` plus the copy, instead of just the copy)
and adds ~50% for large ones, where both are limited by memory bandwidth. "Fresh" costs about the same as "zeroed" at
every size here, even at 4 MiB, because of the dynamic mmap threshold: after the first large block is freed, glibc
serves the next one from its heap instead of asking the kernel for fresh zero pages, so there are no page faults to pay.
On a process where that threshold hasn't adapted (or with a different allocator), the "fresh" column for large sizes
includes page faults: ~2 µs per 4 KiB page on this machine (`ch02-03`).

### 5. Memory

**Live heap vs RSS, measured** (listing `ch04-05-live-vs-rss.rs`). A counting allocator tracks live bytes; RSS comes
from `/proc/self/status`. Allocate a million 48-byte objects, free every other one, free the rest, drop the vector that
held them, then call `malloc_trim(0)`:

```text
start                                    live heap     0.0 MiB   RSS     2.1 MiB
1,000,000 x 48-byte objects              live heap    53.4 MiB   RSS    70.8 MiB
freed every other object                 live heap    30.5 MiB   RSS    70.8 MiB
freed all objects (Vec still held)       live heap     7.6 MiB   RSS    70.8 MiB
dropped the Vec too                      live heap     0.0 MiB   RSS    63.2 MiB
after malloc_trim(0) (returned 1)        live heap     0.0 MiB   RSS     2.1 MiB
```

Line by line [LIB][OS]:

- **53.4 MiB live, 70.8 MiB resident.** 1M × 48 B of objects (45.8 MiB) plus the 7.6 MiB vector. The objects occupy
  64-byte chunks (size field plus rounding), so they take 61 MiB of heap: 33% overhead on small objects, invisible to a
  live-heap counter.
- **Freeing half the objects doesn't shrink RSS at all.** The freed chunks are scattered through the heap between live
  ones; no page is entirely free, and glibc returns memory to the kernel only from the top of the heap (or from whole
  free pages when trimming).
- **Freeing all of them still doesn't.** Small freed chunks sit in per-thread caches and fast bins, which glibc doesn't
  coalesce until a consolidation, so the heap's top never becomes one big free block to trim.
- **Dropping the vector returns 7.6 MiB immediately**: it was large enough to be its own `mmap`, and `free` unmaps it.
- **`malloc_trim(0)` returns everything**: it consolidates the free lists and releases free pages back to the kernel
  (`MADV_DONTNEED`), including pages in the middle of the heap.

The architectural lesson: **alert on RSS, but diagnose with live heap next to it.** A widening gap with steady live heap
is fragmentation or retention, not a leak. Allocators designed for long-running services (jemalloc with its
time-based dirty-page decay, mimalloc with its per-page free lists; both reported in their projects' documentation)
return memory more eagerly than glibc; the Meridian gateway ships with mimalloc (Chapter 2.1) partly for that reason.
Neither is on the Playground, so their effect on this listing is not verified here: swapping the global allocator is a
two-line change (`#[global_allocator] static A: mimalloc::MiMalloc = mimalloc::MiMalloc;`) to measure locally.

### 6. CPU / OS

**Allocation rate is CPU rate.** At ~11 ns per small allocate-and-free pair, the fraud example's 408 allocations cost
~4.5 µs of the 41 µs per score. The rest is what the allocations imply: hashing 400 strings, probing a table, touching
fresh cache lines. Removing allocations usually removes the work around them too, which is why the measured speedup
(50×) is far larger than the allocation count alone would predict.

**Measuring allocation in production.** The counting allocator works in production too (the gateway exports
`allocations_total` from one). Beyond that, all not verified here:

```text
heaptrack ./service                               # every allocation with its stack; GUI or heaptrack_print
cargo run --features dhat-heap                    # dhat-rs (Nicholas Nethercote): per-site counts, bytes, lifetimes
bpftrace -e 'uprobe:/lib/x86_64-linux-gnu/libc.so.6:malloc /pid == $1/ { @[ustack] = count(); }' <pid>
MALLOC_CONF=stats_print:true ./service            # jemalloc statistics at exit (tikv-jemallocator reads _RJEM_MALLOC_CONF)
```

**First touch is where the kernel charges you.** `ch02-03` showed 32,768 page faults for 128 MiB, about 2 µs each. A
service that allocates large buffers per request and frees them pays those faults on every request whenever the
allocator gives the memory back to the kernel in between (glibc's mmap threshold and trim settings, or an allocator's
decay timer). Keeping large buffers in a pool (with a capacity cap, Chapter 9.1) avoids that; so does presizing.

---

## Pass 3 · Architect level — *Choosing the fix*

### 7. Trade-offs

| Fix | When | Cost | Evidence in this Part |
|---|---|---|---|
| Reuse a buffer (`clear()` keeps capacity) | Per-request scratch, log lines, response bodies | Capacity must be capped or it grows to the worst case (Chapter 9.1's ingestion incident) | `ch04-02`: 8 → 0 allocs/line, p99.9 9.5 → 1.0 µs on 4 threads |
| Resolve names to indices once | Anything keyed by a fixed vocabulary (features, routes, metric names) | A load-time mapping step; indices must stay in sync with the model | `ch04-03`: 41 µs → 0.8 µs per score |
| Arena per request/batch (bumpalo) | Many small, short-lived allocations with a common lifetime | Lifetimes tie values to the arena; bumpalo doesn't run `Drop` for values it holds by default | `ch04-04`: ~3× cheaper than malloc |
| Inline storage (SmallVec, arrays) | Small collections that are usually under a known size | Larger structs; spills to the heap past the inline size | Chapter 9.1's sizes |
| Share instead of copy (`Arc<[u8]>`, `Bytes`) | Values read by many, written rarely | Refcount traffic; immutability | `ch07-05`: 1.7–2.8× more Ferrite GETs |
| Recycle to the producer | Pipelines that allocate on one thread and free on another | A return channel or a pool | `ch04-04`: cross-thread frees ~118 ns each |
| Swap the global allocator (jemalloc, mimalloc) | Fragmentation, RSS, many-threaded allocation patterns | A dependency; different tuning knobs; measure it | Not measurable on the Playground |

The order of the table is roughly the order to try them: the first three are code changes with local reasoning; the
allocator swap changes the whole process and needs a load test.

### 8. Java comparison

| | Java (HotSpot) | Rust |
|---|---|---|
| Allocation at the site | Pointer bump in the thread's TLAB | `malloc`: tcache hit ~10 ns, slower paths possible |
| Freeing | GC, later, in bulk; young-generation objects that die young cost ~nothing to free | `free` at scope end (`Drop`), one by one |
| Cross-thread frees | Free for the GC | Measurably expensive with glibc (`ch04-04`) |
| Fragmentation | Compacting collectors move objects and remove it | Objects can't move; fragmentation lasts until the holes are freed (`ch04-05`) |
| Allocation profiling | JFR `ObjectAllocationSample`, async-profiler `-e alloc` | Counting allocator, dhat, heaptrack, bpftrace uprobes |
| Memory sizing | `-Xmx`, heap vs RSS vs native memory (NMT) | RSS vs live heap; allocator settings |
| Escape analysis | JIT may scalar-replace non-escaping objects | Values live on the stack unless you `Box` them; no analysis needed |

> **Analogy limit.** "Rust has no GC pressure" is true and incomplete. The Java metric *GC pressure* measures how much
> collector work your allocation causes. Rust's equivalent costs are allocator CPU at the site (`ch04-01`'s 108 per
> order), cross-thread frees, and fragmentation that no collector will ever compact. A Rust service can have a memory
> problem Java's compacting collectors would have hidden (`ch04-05`), and Java can have pause-time problems Rust
> can't.

### 9. Production scenario

**What Part IX's two fixes are actually worth.** The numbers above change how the two changes were justified:

- **The access log** (Chapter 9.2). At 400K req/s with one log line per request, saving ~124 ns per line is
  400,000 × 124 ns ≈ 0.05 cores fleet-wide: nothing. The case for the change is the tail under concurrency (p99.9
  9.5 µs → 1.0 µs on four threads) and the allocations removed from the allocator: ~5 per request in the production
  code Chapter 9.2 counted (~2M per second at peak), 8 in this prototype. The gateway team's design note now says
  exactly that, rather than "saves CPU".
- **The fraud feature vector** (Chapter 9.5). ~1.4% of CPU per score at 50K scores/s, on ~120 cores, is about 1.7 cores.
  Modest. The allocation count (408 → 0 per score, ~20M allocations per second across the fleet) and the four-thread
  p99.9 (300 µs → 7.7 µs) are the bigger wins, and the fraud service's p99 < 5 ms SLO has more headroom as a result.

Both designs were right. The first justification of each was wrong, and only measurement could say so.

### 10. Failure scenario

**The reconciliation job that was OOM-killed with 400 MB live (2026).** payments-core's nightly reconciliation job
(Chapter 8.4's reconciliation for ambiguous charges) processes a day's charges in batches of 500,000. Each batch
allocates millions of small records, matches them against the processor's report, and keeps only the mismatches. The
container limit is 2 GiB.

After a processor changed its report format, records got slightly larger and the job was OOM-killed on the third batch
of the night. Its own metrics said live heap peaked at ~400 MB. RSS said 1.9 GiB. The mechanism is `ch04-05`'s, at scale:
each batch freed most of its small records but kept a few mismatches scattered through the heap, so no page became
free; the next batch's records were a different size class and couldn't reuse the old holes; and glibc returned almost
nothing to the kernel between batches.

The fix had three parts: records for a batch are allocated from a per-batch arena and freed in one go (the mismatches
are copied out first), the job exports live heap *and* RSS so the gap is visible, and the container runs with mimalloc,
measured with the job's real data before rollout. The review rule that came out of it: **any job whose memory limit is
within 3× of its live heap must have an RSS-vs-live dashboard and a batch-level memory test.**

---

## Practice

### 11. Interview & architecture questions

1. What four facts about an allocation site do you need before choosing a fix, and which fix does each point to?
2. Explain the difference between virtual size, RSS, and live heap. Which one does the OOM killer use?
3. Why did freeing half of `ch04-05`'s objects not reduce RSS, and what did `malloc_trim` do differently?
4. In `ch04-04`, local malloc scaled from 1 to 4 threads but cross-thread frees cost ~118 ns each. Explain both.
5. When is a bump arena the right tool, and what do you give up (two things)?
6. The fraud estimate said 3% CPU; the measurement said 1.4%. Why were the allocation count and the tail better
   justifications than CPU?
7. Why does zeroing a read buffer cost ~2× for small buffers but ~1.5× for large ones?
8. Compare fragmentation in a Rust service and in a Java service with a compacting collector.
9. How would you measure allocation rate in production without redeploying with a profiler?
10. What evidence would you require before switching a service's global allocator to mimalloc or jemalloc?

### 12. Exercises

- **Beginner.** In `ch04-01`, replace `config.clone()` with an `Arc<HashMap<..>>` shared across orders. Predict the new
  allocations per order, then measure.
- **Intermediate.** Replace `serde_json::Value` in `ch04-01` with a `#[derive(Deserialize)]` struct that borrows
  `&str` fields (Chapter 22.2 goes further). How many allocations per order remain, and from where?
- **Advanced.** Add a fourth setup to `ch04-04`: producer/consumer pairs where the consumer sends each emptied `Vec`
  back to the producer through a second channel for reuse. Does it beat the cross-thread-free case?
- **Systems.** In `ch04-05`, call `libc::mallopt(libc::M_TRIM_THRESHOLD, 0)` and `libc::M_MMAP_THRESHOLD` variations
  at startup and re-run. Which lines change, and what does each setting cost?
- **Architecture.** Write the memory section of gateway-core's performance contract (Chapter 20.1's design exercise):
  allocation budget per request, RSS vs live-heap alerting, allocator choice, and the tests that enforce them.

### 13. Debugging exercise

A Rust service's dashboard shows: RSS climbs by ~50 MB per hour and never falls; the counting allocator's live-heap gauge
is flat at 300 MB; restarts "fix" it for a day. A teammate proposes a memory-leak hunt with heaptrack. Explain why that
probably won't find a leak, what the two most likely mechanisms are, which one-line experiment distinguishes them, and
what the long-term fix options are.

### 14. Design exercise

Design an **allocation budget system** for Meridian's Rust services: how each hot path declares its budget (allocations
and bytes per operation), how CI enforces it with a counting allocator, how production verifies it (sampling, metrics),
and how an engineer requests an exception. Include how the system handles third-party crates whose allocation behaviour
changes between versions.
