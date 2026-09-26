// verify: release+nightly ok
// std::simd (portable SIMD), still unstable on 1.98: the same newline count, written once for any target.
#![feature(portable_simd)]
use std::hint::black_box;
use std::simd::prelude::*;
use std::time::Instant;

fn count_simd(v: &[u8]) -> usize {
    let (head, body, tail) = v.as_simd::<32>();
    let needle = u8x32::splat(b'\n');
    let mut n = 0usize;
    for chunk in body {
        n += chunk.simd_eq(needle).to_bitmask().count_ones() as usize;
    }
    n + head.iter().chain(tail).filter(|&&b| b == b'\n').count()
}

fn main() {
    let line = b"2026-09-25T10:00:00Z GET /v1/payments 200 3ms\n";
    let text: Vec<u8> = line.iter().copied().cycle().take(16 << 20).collect();
    let expected = text.iter().filter(|&&b| b == b'\n').count();
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        assert_eq!(black_box(count_simd(black_box(&text))), expected);
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
    }
    println!("{expected} newlines; std::simd u8x32: {best:.3} ms ({:.1} GB/s)", (16 << 20) as f64 / 1e9 / (best / 1e3));
}
