# Chapter 19.5 — Virtual Memory, Pages, and mmap

> **Where this sits:** Part XIX · Binary, Linker, and OS · chapter 5 of 6
> **Prerequisites:** Chapter 19.4 (how the address space is built), Chapter 3.1 (freed memory isn't necessarily
> returned to the OS), Chapter 11.1 (thread stacks: 2 MiB reserved), the Part IX interlude (guard pages and stack
> probes), Chapter 15.2 (tagged pointers and provenance), Project L1 (`logstat`'s input loop).
> **After this chapter you can:** read `/proc/self/maps` and say what every region is; explain the difference between
> reserved and resident memory with measured page faults; predict when freeing memory lowers RSS and when it can't;
> use transparent huge pages deliberately; decide between `read` and `mmap` for file input, including the failure mode
> that makes `Mmap::map` an `unsafe fn`; and explain the canonical-address rule that pointer-tagging schemes live
> inside.

---

## Pass 1 · User level — *Addresses are promises, pages are memory*

### 1. Problem

Every Java or Rust service dashboard has a "memory" graph, and most arguments about it happen because it's not clear
which number it shows. `VmSize` says 72 MB, RSS says 3 MB, the container limit is 512 MiB, the allocator says 1 MiB is
in use, and the service was just OOM-killed with exit status 137 (Chapter 19.4). All of these can be true at once.

The reason is **virtual memory**. A process sees a private address space of 2^47 bytes (128 TiB) on x86-64. Almost
none of it is backed by anything. Addresses become real memory only when **pages** (4 KiB each) are touched, and they
can become unreal again when the kernel or the program gives them back. The questions an architect has to answer are
all phrased in these terms:

- How much *physical* memory does this service need, at peak and in steady state?
- Why didn't memory go down after we dropped a large structure (Chapter 3.1's route-table refresh)?
- Is memory-mapping our input files faster, and is it safe?
- What counts against the container's memory limit?

### 2. Mental model

```text
 virtual address (48 bits used)          page tables (per process)              physical memory
 ┌─────────────┬──────────────┐          4 levels on x86-64:                    ┌──────────────┐
 │ page number │ offset (12b) │ ───────► PML4 → PDPT → PD → PT ──► frame ──────►│ 4 KiB frame  │
 └─────────────┴──────────────┘          (a 2 MiB huge page stops at PD)        └──────────────┘
        │
        ├─ no mapping at all             → SIGSEGV
        ├─ mapped, never touched         → first access = PAGE FAULT: the kernel allocates a frame
        │                                   (anonymous: zero-filled; file-backed: from the page cache)
        ├─ resident                      → counted in RSS
        └─ mapped PROT_NONE (guard page) → access faults; used to catch stack overflows

 kinds of mappings in /proc/self/maps:
   anonymous private     heap ([heap], malloc arenas), thread stacks, large allocations
   file-backed private   the executable and libraries (code, read-only data, relocated data)
   file-backed shared    mmap of a file you read (or write back)
   special               [stack], [vdso], [vvar], [vsyscall]
```

The two numbers to keep apart: **virtual size** (everything mapped, `VmSize`) is address space, cheap and mostly
meaningless for capacity planning. **Resident set size** (RSS) is pages actually backed by RAM right now, split into
anonymous pages (`RssAnon`: heap, stacks) and file-backed pages (`RssFile`: code, mapped files).

### 3. Rust code

Listing `ch05-01-address-space.rs` spawns one thread, allocates a small `Box` and a 1 MiB `Vec`, and prints its whole
address space. All 36 mappings of a small Rust program, in address order:

```text
0x5813b5363000-0x5813b5384000       132 KiB r--p playground
0x5813b5384000-0x5813b53e4000       384 KiB r-xp playground
0x5813b53e4000-0x5813b53e9000        20 KiB r--p playground
0x5813b53e9000-0x5813b53ea000         4 KiB rw-p playground
0x5813b9a5c000-0x5813b9a7d000       132 KiB rw-p [heap]
0x7c8df4000000-0x7c8df4021000       132 KiB rw-p [anonymous]
0x7c8df4021000-0x7c8df8000000     65404 KiB ---p [anonymous]
0x7c8df8a49000-0x7c8df8a4a000         4 KiB ---p [anonymous]
0x7c8df8a4a000-0x7c8df8a4c000         8 KiB rw-p [anonymous]
0x7c8df8a4c000-0x7c8df8a4d000         4 KiB ---p [anonymous]
0x7c8df8a4d000-0x7c8df8c4d000      2048 KiB rw-p [anonymous]
0x7c8df8c4d000-0x7c8df8d50000      1036 KiB rw-p [anonymous]
0x7c8df8d50000-0x7c8df8d78000       160 KiB r--p libc.so.6
0x7c8df8d78000-0x7c8df8f01000      1572 KiB r-xp libc.so.6
   ... (libc's remaining segments, libgcc_s.so.1, small anonymous regions)
0x7c8df8f96000-0x7c8df8f9a000        16 KiB r--p [vvar]
0x7c8df8f9a000-0x7c8df8f9c000         8 KiB r--p [vvar_vclock]
0x7c8df8f9c000-0x7c8df8f9e000         8 KiB r-xp [vdso]
0x7c8df8f9e000-0x7c8df8f9f000         4 KiB r--p ld-linux-x86-64.so.2
   ... (the loader's segments)
0x7ffdafc69000-0x7ffdafc8a000       132 KiB rw-p [stack]
0xffffffffff600000-0xffffffffff601000         4 KiB --xp [vsyscall]
```

(Trimmed in two places, marked.) Reading it top to bottom:

- **The executable**: four mappings, one per `LOAD` segment from Chapter 19.3, with the same permissions (`r--`,
  `r-x`, `r--` after RELRO, `rw-`).
- **`[heap]`**: glibc's main arena, grown with `brk`. The `Box<[u8; 64]>` lives here.
- **132 KiB `rw-p` + 65,404 KiB `---p`**: together exactly 64 MiB, aligned to 64 MiB. That's **glibc's per-thread
  malloc arena** for the spawned thread: it reserves 64 MiB of address space with no access rights and makes only the
  first part usable [LIB]. Listing `ch04-01`'s trace caught it being created on the child thread:
  `mmap(len=134217728 prot=0)` (reserve 128 MiB), two `munmap`s to trim it to an aligned 64 MiB, then
  `mprotect(len=135168 prot=3)`.
- **4 KiB `---p` + 2048 KiB `rw-p`**: the spawned thread's **stack** (2 MiB, Chapter 11.1) with its **guard page**
  below it. The same trace shows `std` asking for it: `mmap(len=2101248 prot=0)` (2 MiB + 4 KiB, no access), then
  `mprotect(len=2097152 prot=3)` on all but the lowest page.
- **1036 KiB `rw-p`**: the 1 MiB `Vec`, in its own mapping. glibc serves allocations above its mmap threshold
  (128 KiB by default, adjusted dynamically [LIB]) with a dedicated `mmap`.
- **Libraries, `[vvar]`/`[vdso]`** (the kernel's code and data for system calls that don't need to enter the kernel,
  Chapter 19.6), **the loader**, the main thread's **`[stack]`**, and **`[vsyscall]`**, a legacy fixed-address page in
  the *kernel* half of the address space, kept for old binaries.

Where the program's values live, and how many address bits each address uses:

```text
fn main                0x5813b5392240  bits used: 47  user half  r-xp playground
static COUNTER         0x5813b53e9c48  bits used: 47  user half  rw-p playground
string literal         0x5813b537203a  bits used: 47  user half  r--p playground
Box<[u8; 64]>          0x5813b9a5cd60  bits used: 47  user half  rw-p [heap]
vec![0; 1 MiB]         0x7c8df8c4d010  bits used: 47  user half  rw-p [anonymous]
local on main stack    0x7ffdafc87410  bits used: 47  user half  rw-p [stack]
local on thread stack  0x7c8df8c4bad8  bits used: 47  user half  rw-p [anonymous]
libc::getpid           0x7c8df8e45b90  bits used: 47  user half  r-xp libc.so.6
vDSO                   0x7c8df8f9c000  bits used: 47  user half  r-xp [vdso]
1 << 47 (limit)        0x800000000000  bits used: 48  NOT user   not mapped
```

and the kernel's summary of the same process:

```text
VmSize   72032 kB
VmRSS    3456 kB
RssAnon  1184 kB
RssFile  2272 kB
```

72 MB of address space, 3.4 MB of memory, and two-thirds of that memory is shared library and executable code.

## Pass 2 · Systems level — *Faults, huge pages, and giving memory back*

### 4. Under the hood

**Demand paging, measured.** Listing `ch05-02-demand-paging.rs` maps 64 MiB of anonymous memory and reports the change
in RSS and in minor page faults (from `getrusage`) after each step:

```text
mmap 64 MiB                                  RSS       +4 KiB   minor faults      +0
write 1 byte in each 4 KiB page              RSS   +65536 KiB   minor faults  +16384
write each page again                        RSS       +0 KiB   minor faults      +0
madvise(MADV_DONTNEED)                       RSS   -65536 KiB   minor faults      +0
read page 0 again (value 0)                  RSS       +0 KiB   minor faults      +1
munmap                                       RSS       +0 KiB   minor faults      +0
```

`mmap` costs nothing but a reservation. The first write to each page takes a **minor fault** (no disk I/O: the kernel
finds a free frame, zeroes it, and installs it): exactly 16,384 faults for 16,384 pages. The second pass is free.
`madvise(MADV_DONTNEED)` gives the frames back while keeping the mapping, and the next read sees **zero**: for private
anonymous memory, "don't need" means "discard the contents" [OS]. (A read of a never-written page maps a shared zero
page, which is why it doesn't show up in RSS.)

**Transparent huge pages.** This kernel's THP mode is `madvise` (only regions that ask get huge pages). The listing asks
for a 2 MiB-aligned region:

```text
mmap 66 MiB + madvise(MADV_HUGEPAGE) = 0     RSS       +0 KiB   minor faults      +0
write 1 byte in each 4 KiB page              RSS   +65536 KiB   minor faults     +32
                                             AnonHugePages 65536 KiB   (defrag: always defer defer+madvise [madvise] never)
```

Thirty-two faults instead of 16,384, and the whole 64 MiB backed by 2 MiB pages. One fault now maps 512 times as much
memory, and each TLB entry covers 2 MiB instead of 4 KiB (§6). **But it isn't guaranteed.** Earlier runs of the same
listing on the Playground got only 2 MiB and 4 MiB of huge pages (and about 15,400–15,900 faults), because at fault
time the kernel must find free, physically contiguous 2 MiB blocks, and on a busy machine it often can't without
compaction. Chapter 20.5's 256 MiB experiment on the same host got 10 MiB. `madvise(MADV_HUGEPAGE)` is a request, not a
reservation.

**Thread stacks: reserved vs used.** The same listing starts a thread and has it call a function with a 1 MiB stack
frame:

```text
thread started: VmSize +67600 KiB
thread started (idle)                        RSS      +16 KiB   minor faults      +4
thread entered a function with a 1 MiB frame RSS    +1024 KiB   minor faults    +256
thread joined (stack unmapped)               RSS     -816 KiB   minor faults      +3
```

Starting a thread added 66 MB of address space (the 2 MiB stack and guard page, and the 64 MiB malloc arena from §3)
and 16 KiB of memory. This is Chapter 11.1's point in numbers: "10,000 threads × 2 MiB" is address space, not RAM. The
1 MiB frame cost exactly 256 faults as soon as the function was *entered*, before it wrote anything interesting. That's
the stack probes from the Part IX interlude: a function whose frame is larger than a page touches each page on entry,
in order, so that an overflow is guaranteed to hit the guard page rather than skip over it. (An earlier version of this
listing put the array directly in the thread's closure and saw the pages become resident at thread start; the probe ran
when the closure was entered.)

**What's inside those stack pages.** Each call pushes a **frame**: the return address, saved callee-saved registers,
and spill slots for values the register allocator couldn't keep in registers (Chapter 17.8 §5 measured them). The ABI
adds two rules that meet the OS here. Frames are laid out so `rsp` is 16-byte aligned at every `call` (Chapter 19.4
shows `_start` establishing it). And a leaf function may use the 128-byte **red zone** below `rsp` without moving
`rsp` (17.8's `opaque` stored its value at `[rsp - 8]`). That's only safe because the kernel cooperates: when it
delivers a signal on the thread's current stack, it skips 128 bytes below `rsp` before writing the signal frame, so
the handler can't overwrite a leaf's scratch data [OS]. And when the stack itself is exhausted, a handler can't run on
it at all, which is why `std` gives every thread an **alternate signal stack** with its own guard page (the
`sigaltstack` and 12 KiB `mmap` in Chapter 19.4's trace): the stack-overflow handler runs there.

**Freed memory and RSS.** Listing `ch05-03-rss-after-free.rs` is Chapter 3.1's promise kept, with glibc's allocator:

```text
step                                                          RSS MiB
start                                                             2.2
one Vec<u8> of 128 MiB (a single mmap'd block)                  130.2
  dropped                                                         2.2
1,000,000 Box<[u8; 64]> (small blocks in the heap)               86.1
  dropped                                                        78.5
  malloc_trim(0) -> 1                                             2.2
again, then free all but every 64th (15625 survive)              86.1
  malloc_trim(0) -> 1                                            86.1
  drop the rest, malloc_trim(0) -> 1                              2.2
```

Three behaviors, three mechanisms [LIB: glibc malloc]:

1. **A large allocation is its own mapping**, so freeing it `munmap`s it: RSS drops immediately.
2. **A million small allocations share the heap.** Freeing them returns the chunks to malloc's free lists, and glibc
   only gives back memory at the *top* of the heap automatically, so RSS stays at 78.5 MiB. `malloc_trim(0)` walks
   the free chunks and releases whole free pages in the middle of the heap with `MADV_DONTNEED`: back to 2.2 MiB.
3. **Fragmentation defeats even `malloc_trim`.** Keeping every 64th block alive (about 1 MB of live data in about
   80 MB of heap chunks) leaves a survivor on nearly every 4 KiB page (each page holds about 50 of these 80-byte chunks), so there's
   almost no *whole* free page to release. RSS stays at 86.1 MiB for 1 MiB of live data.

None of this is a leak. The memory is free *inside the process* and gets reused by the next allocations. It's just
not free *for the machine*. Chapter 20.4 measures the same mechanism with a counting allocator's live-heap bytes next
to RSS (live heap 0.0 MiB, RSS 63.2 MiB, until `malloc_trim`), which is the pair of numbers to graph in production. Allocators differ here: jemalloc and mimalloc return unused pages to the OS in the
background after a decay period [LIB, per their documentation; not measured here], and Chapter 15.5 discusses the
choice.

**`mmap` for file input.** Project L1's `logstat` reads input with a reused buffer; its "omissions" table promised an
`mmap` comparison. Listing `ch05-04-mmap-input.rs` counts the lines of a 44 MB access log three ways (release, best of
3, file already in the page cache, one run on a shared machine: noisy):

```text
file: 43949390 bytes (in the page cache: just written)
read_until, reused buffer  500000 lines  best  13.1 ms  faults/run      6  while in use: RssAnon     +60 KiB  RssFile     +68 KiB
fs::read + memchr          500000 lines  best  28.8 ms  faults/run  10730  while in use: RssAnon  +42980 KiB  RssFile     +68 KiB
mmap + memchr              500000 lines  best   5.6 ms  faults/run    671  while in use: RssAnon     +60 KiB  RssFile  +42988 KiB
```

- **`read_until` with a reused buffer** (logstat's design) copies the file through a 64 KiB buffer: almost no page
  faults, almost no memory.
- **`fs::read`** allocates a 44 MB buffer and copies the whole file into it: 10,730 faults to populate 43 MB of fresh
  anonymous memory, and it's the slowest, because faulting in new memory costs more than the copy.
- **`mmap`** maps the page-cache pages directly: no copy at all, and 671 faults for 10,730 pages because the kernel
  maps up to 16 neighboring pages that are already in the page cache on each fault ("fault-around", 64 KiB by default
  [OS]). The memory shows up as `RssFile`, not `RssAnon`: it's the page cache, shared, and reclaimable.

`mmap` is the fastest here by a factor of two over the streaming reader. One caveat about the `fs::read` row: a scratch
run of the same listing with a 22 MB file made `fs::read` *faster* than the streaming reader, with fewer faults per
run, because glibc raises its mmap threshold after a large mmap'd block is freed (up to 32 MiB by default [LIB]), so
later 22 MB buffers came from the heap and reused already-resident pages. Allocation-heavy measurements depend on the
allocator's state; that's why the listing reports faults alongside time. The last listing shows `mmap`'s price.

**The `SIGBUS` failure mode.** Listing `ch05-05-mmap-truncate.rs` maps a 1 MiB file, then truncates the file to zero
bytes (as log rotation with `copytruncate` does), then reads the mapping again. The child does the dangerous part and
the parent reports how it ended:

```text
child: mapped 1048576 bytes; byte at 512 KiB = 'x'
child: file truncated to 0 bytes; reading offset 512 KiB of the mapping again
parent: child ended with ExitStatus(unix_wait_status(135)), signal Some(7) (SIGBUS = 7)
```

The mapping still covers 1 MiB of addresses, but there's no file data behind them anymore, and touching such a page
raises **`SIGBUS`**. There's no error value to handle: the process dies (status 135, core-dump bit set). This is why
`memmap::Mmap::map` is an `unsafe fn` [LIB]: Rust's guarantee that a `&[u8]` stays valid and unchanged for its
lifetime can't be upheld by the program when another process can change or shrink the file. The `SAFETY` comment in
listing `ch05-04` states the condition the caller must ensure.

**Canonical addresses and pointer tagging.** Every address in §3 used 47 bits. With 4-level paging, x86-64 translates
48 bits, and the hardware requires the upper 16 bits to be copies of bit 47 (**canonical form**); the user half is
`0..2^47` and the kernel half is the top, where `[vsyscall]` lives at `0xffffffffff600000` [CPU]. A load through a
non-canonical address doesn't just miss: it raises a general-protection fault (seen as `SIGSEGV`). With 5-level
paging (57 bits), Linux still hands out addresses below 2^47 unless a program asks for more with an `mmap` hint
[OS]. That unused top is what pointer-tagging schemes borrow: software tags must be stripped before dereferencing
(Chapter 15.2's strict-provenance APIs are the way to do that without losing provenance), while Arm's **Top Byte
Ignore** and Intel's **Linear Address Masking** make the hardware ignore the tag bits (not verifiable on the
Playground: TBI needs an Arm machine, LAM a recent Intel CPU with kernel support).

### 5. Memory

Four accounting facts that decide real deployments [OS]:

1. **RSS double-counts shared pages.** Two processes mapping the same libc both report its resident pages. `PSS`
   (proportional set size, in `/proc/<pid>/smaps_rollup`) divides shared pages among their users and is the honest
   per-process number.
2. **Containers are limited by cgroup memory accounting**, not by RSS. The Playground's cgroup allows 536,870,912
   bytes (512 MiB, from this Part's environment probe). Anonymous memory counts and can't be reclaimed without swap;
   page cache (including your `mmap`'d files) also counts but can be reclaimed under pressure.
3. **Overcommit means allocation rarely fails.** With `vm.overcommit_memory = 0` (the Playground's setting, and the
   usual default), `mmap` and `malloc` succeed far beyond physical memory, and the reckoning comes when pages are
   *touched*. For a Rust service that means the typical out-of-memory event isn't `handle_alloc_error` printing
   "memory allocation of N bytes failed"; it's the kernel's OOM killer sending `SIGKILL`: exit status 137, with no
   message from your process at all (Chapter 19.4).
4. **Freed isn't returned** (§4): steady-state RSS is set by the allocator's retention and fragmentation, not only by
   live data.

### 6. CPU / OS

Every memory access translates a virtual address. The **TLB** caches recent translations; a miss costs a **page walk**
of up to four dependent memory reads (the page-table levels), most of them cached [CPU]. With 4 KiB pages, a 64 MiB
working set needs 16,384 translations, far more than a TLB holds, so a random-access workload over it takes frequent
misses. With 2 MiB pages it needs 32. That's the whole case for huge pages, and it's why they matter for large hash
tables and heaps with random access and hardly at all for sequential streaming.

A minor fault costs the kernel an entry, a zeroed frame, a page-table update, and a return. Chapter 20.4 measured it on
this same Playground machine: about **2 µs per 4 KiB page** on first touch (32,768 faults for 128 MiB). So the 16,384
faults of §4's first pass cost on the order of 30 ms for 64 MiB. That's the cost `fs::read` paid in §4, and the cost a
service pays on its first requests after start, which is why JVMs offer `-XX:+AlwaysPreTouch` and why `MAP_POPULATE`
exists. Chapter 20.5 measured the other side of huge pages: when the kernel tries to compact memory to satisfy
`MADV_HUGEPAGE`, first touch can get *slower* (235 vs 119 ms for 256 MiB there), with only 10 MiB actually backed by
huge pages.

## Pass 3 · Architect level — *Budgets, limits, and input strategies*

### 7. Trade-offs

| Decision | Option | Gains | Costs |
|---|---|---|---|
| File input | buffered `read` (logstat) | works on pipes and sockets; tiny memory; immune to truncation | a copy per byte (13.1 ms here) |
| | read whole file | simplest code | memory = file size; slowest here (28.8 ms) |
| | `mmap` | zero-copy; fastest here (5.6 ms); memory is reclaimable page cache | files only; `SIGBUS` if the file shrinks; `unsafe` contract; I/O errors become signals too |
| Huge pages | THP `madvise` on big, random-access regions | fewer TLB misses and faults | not guaranteed; compaction stalls; memory rounding |
| Allocator | glibc malloc | default; `malloc_trim` available | retains freed small blocks; per-thread 64 MiB arenas |
| | jemalloc / mimalloc | background return of freed pages; less fragmentation (per their docs) | a dependency; tuning knobs; verify with your workload |
| Pre-touching | `MAP_POPULATE` / touch at startup | no faults on the first requests | slower start; memory committed up front |
| Memory limit | set limit to peak RSS plus headroom | predictable OOM kills | headroom must cover allocator retention (§4) |

> **Why not `mmap` everything?** Because the speedup came from skipping a copy that the streaming reader does cheaply,
> and the failure mode (a signal that kills the process when someone else truncates the file) can't be handled as an
> error. For files your process owns and nobody modifies (immutable segments in a storage engine, Chapter 23.1), `mmap`
> is excellent. For log files, uploads, or anything shared with other tools, a buffered reader is the engineering
> choice.

### 8. Java comparison

The JVM reserves its whole heap up front (`-Xmx`) as virtual memory and commits pages as the heap grows, so the
`VmSize`-versus-RSS gap is familiar to anyone who has watched a Java service. The differences are in giving memory
back and in `mmap`:

| | Rust (glibc malloc) | JVM |
|---|---|---|
| Reserve vs use | allocator arenas, thread stacks reserved; pages on touch | heap reserved at `-Xmx`, committed as it grows |
| Pre-touch | `MAP_POPULATE`, manual touching | `-XX:+AlwaysPreTouch` |
| Returning memory | `munmap` for large blocks; `malloc_trim` or a decaying allocator for the rest | collector-dependent: G1 and ZGC can uncommit unused heap (attributed: JEP 346 for G1, ZGC's uncommit) |
| Huge pages | `madvise(MADV_HUGEPAGE)` | `-XX:+UseTransparentHugePages` |
| Memory-mapped files | `Mmap` unmapped deterministically on drop | `MappedByteBuffer` unmapped only when garbage-collected (long-standing, attributed: JDK-4724038); FFM `Arena` makes it deterministic (JDK 22) |
| Truncated mapped file | `SIGBUS` kills the process | the JVM converts the fault into an `InternalError` (attributed) |

> **Analogy limit.** Java's `MappedByteBuffer` turns the `SIGBUS` into an exception because the JVM owns the signal
> handlers and knows which accesses are to mapped buffers. Rust has no such runtime layer: a `&[u8]` from a mapping is
> an ordinary slice, and the compiler assumes reading it can't fail. That's why the Rust API puts the burden on an
> `unsafe` contract, and why "don't map files other processes can shrink" is a design rule rather than an error to
> handle.

### 9. Production scenario

**The session cache that "leaked" every night.** Meridian's session cache (the Rust port from Chapter 9.3: 2 million
sessions, an `IndexMap`, about 490 MB at peak) runs a purge of expired sessions at 03:00 that removes around 1.2 million
entries. After the purge, RSS stayed within a few percent of its peak, and the memory alert (85% of the container
limit) fired most nights. A leak was suspected and a week was spent looking for one.

The team's first measurement was the one listing `ch05-03` makes: the allocator's own statistics showed hundreds of
megabytes *free inside the process* after the purge. The expired sessions were interleaved with live ones in the heap,
the fragmented pattern where even `malloc_trim` can't find whole free pages. The next day's new sessions reused that
memory, which is why RSS never grew beyond the peak: the service didn't leak, it plateaued.

What changed:

1. The alert moved from RSS to **peak RSS versus limit, with the allocator's in-use bytes graphed next to it**. An RSS
   plateau with stable in-use bytes is healthy; in-use bytes that keep growing is a leak.
2. The container limit was set from measured peak plus headroom for allocator retention, instead of from average
   usage.
3. The service moved to the same allocator the gateway already used (mimalloc, Chapter 2.1), after a week-long canary
   comparing RSS after purges and p99 latency. The decision was measured, not assumed.

### 10. Failure scenario

**`logstat` at midnight.** An SRE team forked `logstat` (Project L1) for a log-analysis sidecar and switched its input
to `mmap` after benchmarking a 2× speedup, as in listing `ch05-04`. The sidecar ran on hosts whose logs were rotated
by `logrotate` with the `copytruncate` option: at midnight, the active log is copied aside and then **truncated in
place**, while writers keep writing to it.

Every night at 00:00 the sidecar died with exit status 135 (`SIGBUS`), restarted, and died again whenever it caught
the rotation window. Its own logs said nothing, because nothing in the process ran after the signal. Listing `ch05-05`
is the reduced reproduction. The team's first fix, installing a `SIGBUS` handler, was rejected in review: a signal
handler can't safely unwind or return an error into the middle of `memchr`, and the ways to make it work are fragile
`unsafe` code.

The resolution:

- Input from files that other processes modify goes through the buffered reader (`logstat`'s original design: 13.1 ms
  instead of 5.6 ms for 44 MB, which was never the bottleneck).
- `mmap` is allowed only for files the process owns exclusively and never modifies (the review checklist names
  them), and every `unsafe { Mmap::map(...) }` needs a `SAFETY` comment saying who guarantees that.
- Log rotation moved from `copytruncate` to `create` (rename the old file, start a new one), which is safer for every
  reader, mapped or not.

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XIX).*

1. What's the difference between `VmSize`, RSS, and PSS? Which would you use for capacity planning, and why?
2. Explain a minor page fault. How many did writing one byte per page of a 64 MiB mapping cause, and why didn't the
   second pass cause any?
3. What happens to the contents of anonymous memory after `MADV_DONTNEED`? Why did the listing read back zero?
4. Why did freeing a million small boxes leave RSS at 78.5 MiB, and why did keeping every 64th box alive defeat
   `malloc_trim`?
5. Why did `mmap` beat both `read` strategies in `ch05-04`, and what does its `RssFile` number mean for a container's
   memory limit?
6. What is `SIGBUS` in the context of `mmap`, and why is `Mmap::map` `unsafe`?
7. What does "canonical address" mean on x86-64, and how do TBI and LAM relate to it?
8. Compare Rust's and Java's approaches to memory-mapped files and to returning memory to the OS.

### 12. Exercises

- **Beginner.** Add a `thread_local!` with a large array to listing `ch05-01` and find where its storage lives for the
  main thread and for a spawned thread.
- **Intermediate.** Time listing `ch05-02`'s first-touch loop and its second pass (release, best of N). Compute the cost
  per minor fault on the Playground and compare it with §6's order of magnitude.
- **Advanced.** Rewrite listing `ch05-03` with `bumpalo` (Chapter 15.5) or with a slab of boxes allocated in a
  `Vec<[u8; 64]>`. Show that dropping the arena returns the memory, and explain which Meridian workloads could be
  restructured this way.
- **Systems.** On a Linux machine, run a random-access benchmark over a 1 GiB `HashMap` with and without
  `MADV_HUGEPAGE` on its allocation (use a custom allocator or `mmap`-backed storage), and measure with
  `perf stat -e dTLB-load-misses` (not verifiable on the Playground).
- **Architecture.** Write the memory-sizing method for a new Rust service: which numbers you'd measure (peak RSS, PSS,
  allocator in-use, page cache use), under which load, how you'd set the container request and limit, and which
  alerts you'd configure.

### 13. Debugging exercise

A Rust service shows `VmSize` of 9.4 GB and RSS of 380 MB on a node where each pod has a 1 GiB limit. A colleague
proposes lowering the thread count from 128 to 32 "to cut memory by 6 GB". The service uses glibc's allocator, and
most threads allocate.

1. Using `ch05-01`'s map and `ch05-02`'s measurements, account for the 9.4 GB: which reservations does each thread add?
2. What would lowering the thread count actually change in RSS, and in the risk of an OOM kill?
3. Which glibc setting limits the number of arenas, and how would you decide whether to set it?

### 14. Design exercise

**Input strategy for Ferrite's storage engine (Chapter 23.1 builds it).** Ferrite v3 will store immutable, sorted
segment files and a write-ahead log. Decide, per file type, whether to read with buffered I/O, read whole files, or
`mmap`, and justify it using this chapter's measurements and failure modes: who can modify each file, what happens
during compaction when segments are deleted, how page-cache usage counts against the container limit, and what the
`SAFETY` comment for any `mmap` would say.
