// verify: release ok
//! Parallelism has a fixed cost per call: splitting, waking workers, stealing, joining.
//! Below some input size the sequential loop wins. One run, noisy.
use rayon::prelude::*;
use std::time::Instant;

fn work(x: u64) -> u64 {
    ((x as f64).sqrt() * 1.000_1) as u64
}

fn time_per_call(reps: u32, f: impl Fn() -> u64) -> f64 {
    let t = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(f());
    }
    t.elapsed().as_nanos() as f64 / reps as f64 / 1_000.0
}

fn main() {
    println!("{:>10} {:>14} {:>14} {:>9}", "n", "sequential", "par_iter", "speedup");
    for (n, reps) in [(100u64, 20_000u32), (1_000, 5_000), (10_000, 1_000), (100_000, 200), (10_000_000, 3)] {
        let data: Vec<u64> = (0..n).collect();
        let seq = time_per_call(reps, || data.iter().map(|&x| work(x)).sum());
        let par = time_per_call(reps, || data.par_iter().map(|&x| work(x)).sum());
        println!("{n:>10} {:>11.1} µs {:>11.1} µs {:>8.2}x", seq, par, seq / par);
    }
}
