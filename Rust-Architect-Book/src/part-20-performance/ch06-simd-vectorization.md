# Chapter 20.6 — SIMD and Vectorization

> **Where this sits:** Part XX · Performance Engineering · chapter 6 of 7
> **Prerequisites:** Chapter 2.1 (target triples, the x86-64 baseline, and the `target-cpu=native` SIGILL incident),
> Chapter 7.2 (`sum::<u32>` vectorizes, `sum::<f64>` doesn't), Chapter 10.3 (vectorized iterator chains), Chapter 15.2
> (`noalias` and raw pointers), Chapter 9.2 (memchr measured).
> **After this chapter you can:** read vectorized x86-64 assembly and tell whether a loop was vectorized; list what
> stops LLVM from vectorizing (aliasing, floating-point order, early exits) and how to remove each obstacle without
> `unsafe`; write an explicit SIMD kernel with runtime CPU-feature dispatch that stays correct on every machine;
> explain why a portable binary doesn't use AVX2 by default; and decide between auto-vectorization, intrinsics,
> `std::simd`, and a library.

---

## Pass 1 · User level — *One instruction, many values*

### 1. Problem

A SIMD instruction (single instruction, multiple data) applies one operation to several values packed into a wide
register: 4 `u32`s in a 128-bit SSE register, 8 in a 256-bit AVX2 register, 16 in AVX-512. For loops over contiguous
data, that's a 4–16× reduction in instructions, and Chapter 9.2 measured what it's worth: `memchr` found bytes about 10×
faster than a byte loop.

Three facts make SIMD an architecture topic rather than a micro-optimization:

- **A portable binary doesn't use most of the machine.** Rust's default x86-64 target assumes only the 2003-era
  baseline: SSE2, 128-bit vectors. AVX2 (2013) and AVX-512 aren't used unless you ask. Chapter 2.1's incident was a team
  that asked the wrong way (`-C target-cpu=native` on the build machine) and got `SIGILL` on older production hosts.
- **Auto-vectorization is fragile.** The compiler vectorizes only loops it can *prove* are safe to vectorize. A
  harmless-looking change (a raw pointer instead of a slice, an early `return`, an `f64` instead of a `u32`) turns it off
  silently.
- **Correctness can depend on it.** Vectorizing a floating-point sum changes the order of additions, and therefore the
  result. The compiler won't do it for you, for exactly that reason.

### 2. Mental model

```text
 Four ways to get SIMD, from least to most effort:

 1. A library that already does it       memchr, simdutf8-style validators, hashbrown's group probe (9.3)
 2. Auto-vectorization                    write the loop so LLVM can prove it's safe; check the assembly
 3. Portable SIMD (std::simd, nightly)    write lanes explicitly once, compile for any target
 4. Intrinsics (std::arch)                write the exact instructions per ISA; runtime dispatch; `unsafe`

 What stops auto-vectorization:
   aliasing        could a write through one pointer change what another reads?      slices (noalias) answer "no"
   FP order        vectorizing a float reduction reorders additions                  choose the order yourself (lanes)
   early exits     `return`/`break` inside the loop                                  let a library handle it, or restructure
   unknown bounds  bounds checks with lengths the compiler can't relate              zip, chunks_exact, iterate the slice
```

And the deployment rule: **build for the baseline your fleet guarantees, and dispatch at run time to faster code paths
for the CPUs that have them.**

### 3. Rust code

**What LLVM vectorizes and what it doesn't** (listing `ch06-01-vectorize-asm.rs`, release assembly via
`tools/emit.ps1`; x86-64 baseline, so vectors are 128-bit `xmm` registers). The integer sum:

```text
playground::sum_u32:                       ; v.iter().fold(0u32, |a, &x| a.wrapping_add(x))
.LBB4_6:
	movdqu	xmm2, xmmword ptr [rdi + rdx]
	paddd	xmm1, xmm2                     ; 4 u32 additions per instruction
	movdqu	xmm2, xmmword ptr [rdi + rdx + 16]
	paddd	xmm0, xmm2                     ; two accumulators: 8 u32 per iteration
	add	rdx, 32
	cmp	rax, rdx
	jne	.LBB4_6
```

The `f64` sum in source order:

```text
playground::sum_f64:                       ; v.iter().sum()
.LBB3_5:
	addsd	xmm0, qword ptr [rdi + 8*rcx]      ; one scalar addition ("sd" = scalar double)
	addsd	xmm0, qword ptr [rdi + 8*rcx + 8]  ; ... into the SAME register: each waits for the previous
	addsd	xmm0, qword ptr [rdi + 8*rcx + 16]
	addsd	xmm0, qword ptr [rdi + 8*rcx + 24]
	addsd	xmm0, qword ptr [rdi + 8*rcx + 32]
	addsd	xmm0, qword ptr [rdi + 8*rcx + 40]
	addsd	xmm0, qword ptr [rdi + 8*rcx + 48]
	addsd	xmm0, qword ptr [rdi + 8*rcx + 56]
	add	rcx, 8
	cmp	rsi, rcx
	jne	.LBB3_5
```

Unrolled 8×, but still one dependent chain of scalar additions. And the same sum written with 8 independent
accumulators (`sum_f64_lanes`: `chunks_exact(8)`, `acc[i] += c[i]`):

```text
playground::sum_f64_lanes:
.LBB1_5:
	movupd	xmm4, xmmword ptr [rcx]
	addpd	xmm2, xmm4                     ; "pd" = packed double: 2 additions per instruction
	movupd	xmm4, xmmword ptr [rcx + 16]
	addpd	xmm3, xmm4                     ; four independent vector accumulators
	movupd	xmm4, xmmword ptr [rcx + 32]
	addpd	xmm1, xmm4
	movupd	xmm4, xmmword ptr [rcx + 48]
	addpd	xmm0, xmm4
```

Measured (listing `ch06-02-f64-sums.rs`, 32,768 values in L2, best of 7):

```text
ns per element (best of 7):
  u32 wrapping sum          0.035
  f64 sum, source order     0.817
  f64 sum, 8 lanes          0.102
source order 3.173606e9
8 lanes      3.173606e9
sorted |x|   3.173606e9
relative difference, source order vs 8 lanes: 9.02e-16
within the 1e-9 tolerance: yes
```

The lane version is **8× faster** than the source-order sum, and its answer differs by 9 × 10⁻¹⁶ relative: a few units
in the last place, from adding in a different order. That's the trade the compiler refuses to make on its own and the
Meridian fraud library makes deliberately (Chapter 7.2): the explicit lane sum, plus a test that pins the tolerance.

**Aliasing: slices vs raw pointers** (same listing, `saxpy` vs `saxpy_raw`, both computing `y[i] += a * x[i]`). The
slice version goes straight to a vector loop (`mulps`/`addps`, 8 floats per iteration). The raw-pointer version first
checks whether the two ranges overlap:

```text
playground::saxpy_raw:
	lea	rax, [rdi + 4*rdx]             ; end of y
	lea	rcx, [rsi + 4*rdx]             ; end of x
	cmp	rdi, rcx
	setb	cl                             ; y starts before x ends?
	cmp	rsi, rax
	setb	al                             ; x starts before y ends?
	test	cl, al
	je	.LBB5_5                        ; no overlap: jump to the vector loop
	...                                    ; overlap: a scalar loop, unrolled by 2
```

`&mut [f32]` and `&[f32]` can't overlap (Chapter 15.2: `&mut` is `noalias`), so the slice version doesn't need the
check or the fallback. Measured (listing `ch06-06-noalias-bench.rs`, 4,096 floats, best of 7):

```text
ns per element (best of 7):  slices 0.088   raw disjoint 0.086   raw overlapping 4.075
```

The runtime check costs nothing measurable here (0.088 vs 0.086). The overlapping case is **46× slower**, and not only
because it runs the scalar loop: with `y` one element ahead of `x`, every iteration reads the value the previous one
just wrote. That's a true loop-carried dependency through memory, and no compiler can vectorize it. Chapter 15.2's
question ("does `noalias` buy speed in a loop?") gets a precise answer: for simple loops LLVM versions them, so the
direct benefit is small; what `noalias` buys is **the guarantee that the fast path is always taken**, plus code size, and
optimizations that no runtime check can recover (keeping values in registers across writes, Chapter 3.3).

**Early exits aren't vectorized.** `position_of` (`v.iter().position(|&x| x == needle)`) compiles to a scalar loop, one
comparison per element:

```text
playground::position_of:
.LBB0_3:
	cmp	dword ptr [rdi + 4*rdx], ecx
	je	.LBB0_4
	inc	rdx
	add	rsi, -4
	jne	.LBB0_3
```

This is where libraries earn their place: `memchr` searches with explicit SIMD and handles the early exit itself.

**Explicit SIMD with runtime dispatch** (listing `ch06-03-runtime-dispatch.rs`). Count newline bytes in 16 MiB of
log-like text three ways: a plain loop, an AVX2 kernel chosen at run time, and `memchr`:

```rust,ignore
// excerpt of ch06-03-runtime-dispatch.rs
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn count_avx2(v: &[u8]) -> usize {
    use std::arch::x86_64::*;
    let chunks = v.chunks_exact(32);
    let tail = chunks.remainder();
    let needle = _mm256_set1_epi8(b'\n' as i8);
    let mut n = 0usize;
    for c in chunks {
        // SAFETY: `c` is exactly 32 readable bytes; loadu has no alignment requirement.
        let x = unsafe { _mm256_loadu_si256(c.as_ptr().cast()) };
        let eq = _mm256_cmpeq_epi8(x, needle);
        n += (_mm256_movemask_epi8(eq) as u32).count_ones() as usize;
    }
    n + count_scalar(tail)
}

fn count_dispatch(v: &[u8]) -> usize {
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("avx2") {
        // SAFETY: we just checked that this CPU supports AVX2, the only precondition of `count_avx2`.
        return unsafe { count_avx2(v) };
    }
    count_scalar(v)
}
```

```text
this CPU has: sse2 sse4.2 popcnt avx avx2 bmi2 fma avx512f avx512bw
364722 newlines in 16777216 bytes
  plain loop              5.627 ms     3.0 GB/s
  AVX2 via dispatch       0.783 ms    21.4 GB/s
  memchr::memchr_iter     0.371 ms    45.2 GB/s
```

The binary was built for the baseline and still ran AVX2 code on this AVX-512-capable CPU: **7× faster** than the plain
loop. LLVM did vectorize the plain loop, but for SSE2 and awkwardly: its inner loop (inlined into `main`; release
assembly) compares two bytes at a time and widens each match to a 64-bit counter:

```text
.LBB6_72:
	movzx	edi, word ptr [rcx + rsi]      ; load 2 bytes
	movd	xmm4, edi
	...
	pcmpeqb	xmm4, xmm1                     ; compare with '\n'
	punpcklbw	xmm4, xmm4                 ; widen the 0x00/0xFF results ...
	pshuflw	xmm4, xmm4, 212
	pshufd	xmm4, xmm4, 212
	pand	xmm4, xmm3                     ; ... to 0/1 in 64-bit lanes
	paddq	xmm2, xmm4                     ; and add them to 64-bit counters
```

The AVX2 kernel compares 32 bytes per `vpcmpeqb`. `memchr` is 2× faster again: it unrolls
further, uses its own dispatch, and was tuned by people who do nothing else. The same listing also runs under **Miri**
(`debug miri-ok`), where feature detection reports only `sse2`, so Miri checks the scalar path and the dispatch logic;
the `unsafe` in the AVX2 path is one unaligned load with a stated precondition.

Since Rust 1.86, a `#[target_feature]` function can be a safe `fn`, but calling it from code without that feature
still requires `unsafe`, because the caller has to prove the CPU supports it (listing
`ch06-04-target-feature-call.rs`):

```rust,compile_fail
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn fast_path(v: &[u8]) -> usize {
    v.len()
}

fn main() {
    let v = [1u8, 2, 3];
    println!("{}", fast_path(&v)); // no runtime check, no `unsafe`: rejected
}
```

```text
error[E0133]: call to function `fast_path` with `#[target_feature]` is unsafe and requires unsafe block
  --> src/main.rs:12:20
   |
12 |     println!("{}", fast_path(&v)); // no runtime check, no `unsafe`: rejected
   |                    ^^^^^^^^^^^^^ call to function with `#[target_feature]`
   |
   = help: in order for the call to be safe, the context requires the following additional target feature: avx2
```

**Portable SIMD** (listing `ch06-05-portable-simd.rs`, nightly: `std::simd` is still unstable on 1.98 [VERSION]):

```rust,ignore
// excerpt of ch06-05-portable-simd.rs
fn count_simd(v: &[u8]) -> usize {
    let (head, body, tail) = v.as_simd::<32>();
    let needle = u8x32::splat(b'\n');
    let mut n = 0usize;
    for chunk in body {
        n += chunk.simd_eq(needle).to_bitmask().count_ones() as usize;
    }
    n + head.iter().chain(tail).filter(|&&b| b == b'\n').count()
}
```

```text
364722 newlines; std::simd u8x32: 1.019 ms (16.5 GB/s)
```

Written once with 32 lanes and compiled for the baseline, it runs 5.5× faster than the plain loop without any `unsafe`
or dispatch. The nightly release assembly shows how: with only SSE2 available, LLVM splits each 256-bit operation into
two 128-bit ones and joins the masks:

```text
.LBB5_28:
	movdqa	xmm0, xmmword ptr [rdi + r10]
	pcmpeqb	xmm0, xmm1                     ; bytes 0..15 == '\n'
	pmovmskb	r11d, xmm0                 ; 16-bit mask
	movdqa	xmm0, xmmword ptr [rdi + r10 + 16]
	pcmpeqb	xmm0, xmm1                     ; bytes 16..31
	pmovmskb	r14d, xmm0
	shl	r14d, 16
	or	r14d, r11d                         ; the u8x32 mask as one 32-bit value
	mov	r11d, r14d
	shr	r11d
	and	r11d, 1431655765                   ; 0x55555555: count_ones() done with bit tricks,
	...                                        ; because the baseline has no popcnt instruction
```

Compiled with AVX2 enabled (a `#[target_feature]` wrapper, or `-C target-cpu=x86-64-v3`), the same source would use
256-bit registers.

---

## Pass 2 · Systems level — *How the compiler vectorizes, and what stops it*

### 4. Under the hood

**LLVM has two vectorizers.** [RUSTC] The *loop vectorizer* turns a loop into one that processes several iterations
per instruction, with a scalar epilogue for the remainder (the `.LBB*_4` loops after each vector loop above). The *SLP
vectorizer* combines independent scalar operations in straight-line code into vector operations (it's what turned
`sum_f64_lanes`'s eight `acc[i] += c[i]` into four `addpd`). Both run a **legality check** and then a **cost model**.

**Legality: the three obstacles.** [RUSTC][LANG]

- **Aliasing.** Vectorizing `y[i] += a * x[i]` loads several `x[i]` before storing any `y[i]`, which is only correct if
  the stores can't change the loaded values. rustc marks `&mut` and (freeze-able) `&` parameters `noalias` (Chapter 3.3,
  15.2), which answers the question statically. Raw pointers don't carry that promise, so LLVM emits the overlap check
  you saw and keeps both versions.
- **Floating-point order.** [LANG] Rust's `+` on `f64` is IEEE 754 addition, which isn't associative. Vectorizing a sum
  reorders it, so LLVM may only do it when allowed to reassociate, and stable Rust never allows that implicitly. (Nightly
  has "algebraic" floating-point operations that permit reassociation per operation [VERSION]; stable code chooses the
  order explicitly, as `sum_f64_lanes` does.) One more detail from the assembly: `sum_f64` starts from the constant
  `0x8000000000000000`, which is **−0.0**, not 0.0. Negative zero is the true identity for IEEE addition (−0.0 + x = x for
  every x, including −0.0; 0.0 + (−0.0) would give +0.0), and recent std versions start float sums from it [LIB][VERSION].
- **Early exits.** A loop that can stop at any iteration can't be run 8 iterations at a time without care: the vector
  version might read past the end or run side effects the scalar loop wouldn't. LLVM's support for vectorizing early-exit
  loops is recent and limited; on 1.98.1, `position` stayed scalar.

**The cost model, and register pressure.** [RUSTC] Even a legal vectorization is skipped if LLVM estimates it won't pay
off: short trip counts, gathers from non-contiguous memory, or operations the target has no vector instruction for. The
number of registers is part of that estimate. Unrolling and multiple accumulators (the four `addpd` accumulators in
`sum_f64_lanes`) hide instruction latency, but each needs a register; x86-64 has 16 vector registers without AVX-512
(32 with it), and when a loop needs more live values than that, the register allocator **spills** some to the stack and
reloads them, adding a load and a store per iteration (Chapter 17.8 §6 implements a linear-scan allocator and counts its
spills; Chapter 6.4 showed a real spill: an `f64` accumulator saved around every `dyn` call because the SysV ABI makes
all vector registers caller-saved). A kernel that got slower when you unrolled it further is often a spill: look for
`[rsp + …]` operands inside the loop in the assembly. The baseline target matters
here: without AVX2 there are no 256-bit integer operations and no gathers, and `count_scalar`'s byte comparison was
vectorized only with an expensive widening sequence (3.0 GB/s).

**Runtime detection.** [LIB] `is_x86_feature_detected!("avx2")` runs the `cpuid` instruction on first use and caches the
result in a static, so each check afterwards is a load and a test. `#[target_feature(enable = "avx2")]` compiles *that
function* as if AVX2 were available, **and nothing more**. `count_avx2`'s assembly shows the catch: its
`count_ones()` compiled to the same bit-trick sequence as above (`and r10d, 1431655765`, …) instead of one `popcnt`
instruction, because AVX2 doesn't imply the separate POPCNT feature. `#[target_feature(enable = "avx2,popcnt")]` (and a
matching check) fixes it; every CPU with AVX2 has POPCNT. A second consequence: functions with different target features
generally can't be inlined into each other, so put the whole hot loop inside the feature-enabled function (as
`count_avx2` does) rather than calling a small feature-enabled helper per element.

### 5. Memory

**SIMD moves the bottleneck to memory.** `memchr` counted newlines at 45 GB/s over a 16 MiB buffer, which on this
machine lives partly in L3 and partly in DRAM. Much faster kernels wouldn't help: the data can't arrive faster. The
speed-of-light check from Chapter 20.2 applies: bytes per second of the kernel vs the bandwidth of the level the data
lives in. Once a kernel is at bandwidth, the next gains come from touching fewer bytes (Chapter 20.5: smaller types,
hot/cold splitting, SoA), not from wider vectors.

**Layout for SIMD.** Vector loads take contiguous lanes, so SIMD wants structure-of-arrays: summing `price` across a
`Vec<Order>` needs a gather or a shuffle per element, while summing a `Vec<i64>` of prices is `paddq` on consecutive
memory (Chapter 9.5's 12×). Alignment matters less than it used to: `loadu` (unaligned) is as fast as aligned loads on
current cores except when a load crosses a cache line or page boundary, which is why the kernels above use `loadu` and
don't bother aligning.

### 6. CPU / OS

**Microarchitecture levels.** [CPU][VERSION] Since 2020 the x86-64 psABI names four levels, which rustc accepts as
`-C target-cpu` values:

| Level | Adds (roughly) | Hardware since about |
|---|---|---|
| `x86-64` (v1, Rust's default) | SSE, SSE2 | 2003 |
| `x86-64-v2` | SSE3, SSSE3, SSE4.1/4.2, POPCNT | 2009 |
| `x86-64-v3` | AVX, AVX2, BMI1/2, FMA, MOVBE | 2013–2015 |
| `x86-64-v4` | AVX-512 (F, BW, CD, DQ, VL) | 2017 (servers) |

Build for the level your whole fleet guarantees, and dispatch at run time above it. `target-cpu=native` means "whatever
the build machine has", which is only safe when the binary runs where it was built (Chapter 2.1's SIGILL).

**Arm.** [CPU] On aarch64, 128-bit NEON is part of the baseline, so Meridian's Graviton pool gets SIMD without any
flags; SVE (scalable vectors) is optional and needs the same dispatch discipline as AVX2. `std::arch::aarch64` has NEON
intrinsics, and `std::simd` code compiles for both. Aarch64 isn't available on the Playground, so none of this chapter's
numbers are for Arm.

**Frequency.** [CPU] On some older Intel server cores, heavy AVX-512 (and to a lesser degree AVX2) use lowered the
core's clock frequency for a while, which could make a mixed workload slower overall (widely documented for Skylake-SP;
reported as much smaller on newer Intel and on AMD Zen 4). Measure end-to-end throughput, not just the kernel, when you
add wide vectors to a service that does other work on the same cores.

---

## Pass 3 · Architect level — *Where SIMD belongs in a system*

### 7. Trade-offs

| Approach | Portability | Safety | Speed | Maintenance | Use when |
|---|---|---|---|---|---|
| Library (memchr, hashbrown, …) | Built in | Safe API | Excellent | Someone else's | Always first |
| Auto-vectorization | Every target | Safe | Good, fragile | Check the asm in CI (Chapter 20.2's artifact check) | Simple loops over slices |
| `std::simd` (nightly) | Every target | Safe | Good | Nightly toolchain | Kernels you own, on nightly-tolerant crates |
| Intrinsics + dispatch | Per ISA | `unsafe` at the edges | Best | One path per ISA, scalar fallback, differential tests | Proven hot kernels, after the above |
| `-C target-cpu=x86-64-v3` for the whole binary | Fleet must be v3 | Safe | Good everywhere | Build-matrix policy | Fleets you control completely |

### 8. Java comparison

| | Java (HotSpot) | Rust |
|---|---|---|
| Target CPU | The JIT compiles for the CPU it's running on, automatically | AOT for a baseline; runtime dispatch or build flags for more |
| Auto-vectorization | C2's SuperWord vectorizes simple loops | LLVM's loop and SLP vectorizers |
| FP reductions | Not reordered either (strict IEEE semantics since Java 17, JEP 306) | Not reordered; choose lanes explicitly |
| Explicit SIMD | Vector API (`jdk.incubator.vector`), incubating across many JDK releases | `std::arch` (stable), `std::simd` (nightly) |
| Aliasing | Arrays can alias; the JIT adds runtime checks or skips | `&mut`/`&` are `noalias`; raw pointers get checks |

> **Analogy limit.** Java gets "target-cpu=native, but safe" for free, because the JIT compiles on the machine that runs
> the code. A Rust binary has to decide at build time, and the burden of runtime dispatch is yours. In exchange, the
> Rust code is vectorized before it ever runs, with no warm-up, and you can read the result in the assembly.

### 9. Production scenario

**The fleet's SIMD policy after Chapter 2.1's incident.** Meridian's Rust build policy now has three rules:

1. **Binaries target the fleet baseline**: `x86-64` for the general pool (it includes older instance types), and
   `aarch64` for the Graviton pool, where NEON comes free. `target-cpu=native` is banned in CI configuration.
2. **Hot kernels use libraries first** (memchr for the log shipper's scanning, hashbrown's group probing via
   `HashMap`), then auto-vectorization with an assembly check, then explicit kernels with runtime dispatch, in that order.
3. **Every explicit kernel ships with a scalar reference and a differential test**: random lengths (0 to a few
   thousand), random alignments (start at every offset 0–63), and random contents, comparing the SIMD path with the
   scalar one on every CI run, on both x86-64 and aarch64 runners. Miri runs the scalar path and the dispatch logic.

The fraud feature library's `f64` sums follow Chapter 7.2's rule: explicit 8-lane sums where order doesn't matter to the
model, with the tolerance written down (`ch06-02` shows the difference is ~10⁻¹⁵ relative for realistic values, against a
1e-9 bound), and source-order sums where results must be bit-for-bit reproducible across versions.

### 10. Failure scenario

**The log shipper's AVX2 line counter (2026).** The access-log shipper (Chapter 11.5) counted lines per batch to
reconcile with the collector's count. An engineer replaced the counter with a hand-written AVX2 kernel that processed
32-byte chunks and **forgot the remainder**: for a buffer whose length wasn't a multiple of 32, up to 31 trailing bytes
went uncounted. Most batches ended mid-chunk, so the shipper under-reported by about one line in a few thousand
batches, the reconciliation job raised a "lines lost" alert every night, and the on-call spent two weeks looking for
lost data that was never lost.

A differential test would have caught it on the first run: for any length not divisible by 32, the SIMD and scalar
counts differ. `ch06-03`'s kernel has the line that was missing (`n + count_scalar(tail)`), and its `main` asserts that
all three methods agree. The review rule that came out of it is rule 3 in §9. The incident's second lesson: the
engineer's benchmark had used a 16 MiB buffer, an exact multiple of 32, so it was fast *and* correct on the only input
anyone measured.

---

## Practice

### 11. Interview & architecture questions

1. What does the "d" in `addsd` and `addpd` mean, and what does "s" vs "p" tell you about a loop?
2. Why won't LLVM vectorize `v.iter().sum::<f64>()`, and how do you get a vectorized sum without `unsafe`? What do you
   give up?
3. Explain why the slice version of `saxpy` needs no overlap check and the raw-pointer version does. Why was the
   overlapping case 46× slower?
4. Why does a Rust binary built with default settings not use AVX2 on an AVX2 machine? Give two correct ways to use it.
5. What does `is_x86_feature_detected!` cost per call, and where should the check go relative to the hot loop?
6. Why does calling a `#[target_feature]` function require `unsafe` from ordinary code, even if the function is a safe
   `fn`?
7. Why is `std::simd` attractive, and what keeps many production crates from using it today?
8. `memchr` ran at 45 GB/s. How would you know whether a faster kernel could help?
9. Why do float sums start from −0.0 in recent std versions?
10. Compare how Java and Rust get CPU-specific vector code, and the risks each approach carries.

### 12. Exercises

- **Beginner.** Emit the assembly of `sum_u32` in `ch06-01` with the `wrapping_add` replaced by `+`. Does the release
  build still vectorize (overflow checks are off in release)? What changes in a debug build?
- **Intermediate.** Write `count_avx2` without `chunks_exact`, using index arithmetic and `get_unchecked`, and compare
  the assembly and the speed. Was the `unsafe` worth anything?
- **Advanced.** Add an AVX-512 path (`avx512bw`: `_mm512_cmpeq_epi8_mask`) to `ch06-03` with a three-way dispatch.
  Measure it on the Playground's CPU (it has `avx512bw`). Is it faster than AVX2, and than memchr?
- **Systems.** Rewrite `position_of` so that it vectorizes: compare 8 elements per step with `chunks_exact(8)` and
  `iter().any()`, then find the exact index in the chunk that matched. Check the assembly and measure it against the
  scalar version on a 1M-element slice with the needle at the end.
- **Architecture.** Write Meridian's differential-testing harness for SIMD kernels as a reusable test helper: its API,
  the input generator (lengths, alignments, contents), and how it runs on x86-64 and aarch64 CI.

### 13. Debugging exercise

A PR adds `#[target_feature(enable = "avx2")]` to a small helper `fn lanes_eq(a: __m256i, b: __m256i) -> u32` and calls
it once per 32-byte chunk from an ordinary loop inside `unsafe` after an `is_x86_feature_detected!` check at the top of
the function. It's correct, and it's slower than the plain loop it replaced. Explain why (two reasons, one about
inlining and one about where the feature is enabled), and restructure it.

### 14. Design exercise

Design the SIMD strategy for a new Meridian component: a PII redaction filter (Project L2's `redact`, productionized)
that scans every log line for card numbers and e-mail addresses at 2 GB/s per core. Decide which parts use libraries,
which use auto-vectorized loops, and which justify explicit kernels; how you dispatch across x86-64 levels and aarch64;
what the correctness tests are; and what you measure to know you're at the memory bandwidth limit.
