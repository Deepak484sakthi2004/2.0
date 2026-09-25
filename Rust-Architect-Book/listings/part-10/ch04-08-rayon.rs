// verify: release ok
// parallelStream() vs Rayon's par_iter(): same shape, work-stealing underneath (Chapter 11.6 goes deep).
// Best of 5 runs each, after warming up the pool; one Playground run, noisy. The core count bounds the speedup.
use rayon::prelude::*;
use std::hint::black_box;
use std::time::Instant;

/// Stand-in for real per-item work: 16 rounds of a splitmix64-style mixer. The xor-shifts make it
/// non-linear, so the optimizer can't collapse the loop into a closed form.
fn score(x: u64) -> u64 {
    let mut h = x;
    for _ in 0..16 {
        h ^= h >> 30;
        h = h.wrapping_mul(0xbf58476d1ce4e5b9);
        h ^= h >> 27;
    }
    h >> 60
}

fn best_ms(mut f: impl FnMut() -> u64) -> (f64, u64) {
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
    let data: Vec<u64> = (0..2_000_000).collect();
    let d = black_box(&data);
    let _ = d.par_iter().map(|&x| x).sum::<u64>(); // warm-up: the global pool's threads start on first use

    let (seq_ms, seq) = best_ms(|| d.iter().map(|&x| score(x)).sum());
    let (par_ms, par) = best_ms(|| d.par_iter().map(|&x| score(x)).sum());
    assert_eq!(seq, par); // integer sum: associative, so the parallel reduction gives the same answer
    println!("threads in Rayon's global pool: {}", rayon::current_num_threads());
    println!("sequential {seq_ms:>6.2} ms   parallel {par_ms:>6.2} ms   speedup {:.1}x", seq_ms / par_ms);

    // Tiny per-item work: the parallel version pays splitting and joining for little gain.
    let (seq_small, _) = best_ms(|| d.iter().map(|&x| x & 1).sum());
    let (par_small, _) = best_ms(|| d.par_iter().map(|&x| x & 1).sum());
    println!("trivial work: sequential {seq_small:>5.2} ms   parallel {par_small:>5.2} ms");

    // Floating point is NOT associative: a parallel sum may differ in the last bits.
    let xs: Vec<f64> = (1..=1_000_000).map(|i| 1.0 / i as f64).collect();
    let s_seq: f64 = xs.iter().sum();
    let s_par: f64 = xs.par_iter().sum();
    println!("f64 harmonic sum: seq {s_seq:.17}  par {s_par:.17}  equal: {}", s_seq == s_par);
}
