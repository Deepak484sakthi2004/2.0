// verify: release ok
// verify: debug miri-ok
// Runtime CPU-feature dispatch: count '\n' bytes with (a) a plain loop, (b) AVX2 intrinsics chosen at run time,
// (c) the memchr crate (which does its own dispatch). One binary, built for the x86-64 baseline, still uses AVX2
// when the machine has it: the answer to Chapter 2.1's `target-cpu=native` SIGILL incident.
use std::hint::black_box;
use std::time::Instant;

fn count_scalar(v: &[u8]) -> usize {
    v.iter().filter(|&&b| b == b'\n').count()
}

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

fn ms(mut f: impl FnMut() -> usize) -> (f64, usize) {
    let mut best = f64::MAX;
    let mut r = 0;
    for _ in 0..5 {
        let t = Instant::now();
        r = black_box(f());
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
    }
    (best, r)
}

fn main() {
    #[cfg(target_arch = "x86_64")]
    {
        let feats = ["sse2", "sse4.2", "popcnt", "avx", "avx2", "bmi2", "fma", "avx512f", "avx512bw"];
        let have: Vec<&str> = feats.iter().copied().filter(|f| match *f {
            "sse2" => is_x86_feature_detected!("sse2"),
            "sse4.2" => is_x86_feature_detected!("sse4.2"),
            "popcnt" => is_x86_feature_detected!("popcnt"),
            "avx" => is_x86_feature_detected!("avx"),
            "avx2" => is_x86_feature_detected!("avx2"),
            "bmi2" => is_x86_feature_detected!("bmi2"),
            "fma" => is_x86_feature_detected!("fma"),
            "avx512f" => is_x86_feature_detected!("avx512f"),
            _ => is_x86_feature_detected!("avx512bw"),
        }).collect();
        println!("this CPU has: {}", have.join(" "));
    }
    // 16 MiB of log-like text (Miri gets 4 KiB: it interprets every instruction).
    let size = if cfg!(miri) { 4096 } else { 16 << 20 };
    let line = b"2026-09-25T10:00:00Z GET /v1/payments 200 3ms\n";
    let text: Vec<u8> = line.iter().copied().cycle().take(size).collect();
    let (a, na) = ms(|| count_scalar(black_box(&text)));
    let (b, nb) = ms(|| count_dispatch(black_box(&text)));
    let (c, nc) = ms(|| memchr::memchr_iter(b'\n', black_box(&text)).count());
    assert!(na == nb && nb == nc);
    let gbs = |ms: f64| size as f64 / 1e9 / (ms / 1e3);
    println!("{} newlines in {} bytes", na, size);
    if !cfg!(miri) {
        println!("  plain loop            {a:7.3} ms  {:6.1} GB/s", gbs(a));
        println!("  AVX2 via dispatch     {b:7.3} ms  {:6.1} GB/s", gbs(b));
        println!("  memchr::memchr_iter   {c:7.3} ms  {:6.1} GB/s", gbs(c));
    }
}
