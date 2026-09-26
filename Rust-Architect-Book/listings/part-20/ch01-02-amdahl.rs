// verify: release ok
// Amdahl's law, measured on 1, 2, 4 rayon threads, for two jobs with the same shape
// (a parallel part followed by a serial part):
//   compute-bound: ~64 dependent multiply-rotate rounds per element, data fits in cache
//   memory-bound:  one cheap operation per element over 192 MB, data streams from DRAM
use rayon::prelude::*;
use std::hint::black_box;
use std::time::Instant;

fn mix(mut x: u64, rounds: u32) -> u64 {
    for i in 0..rounds {
        x = (x ^ i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(17);
    }
    x
}

fn parallel_part(data: &[u64], rounds: u32) -> u64 {
    data.par_chunks(8192).map(|c| c.iter().fold(0u64, |a, &x| a ^ mix(x, rounds))).reduce(|| 0, |a, b| a ^ b)
}

fn serial_part(data: &[u64]) -> u64 {
    // A dependent chain: each step needs the previous result, so no thread can help.
    data.iter().fold(0u64, |h, &x| (h ^ x).wrapping_mul(0x100_0000_01B3))
}

fn run(label: &str, par_data: &[u64], rounds: u32, ser_data: &[u64]) {
    let (mut t1, mut f) = (0.0, 0.0);
    println!("{label}");
    for threads in [1usize, 2, 4] {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
        let (mut best, mut par_ms, mut ser_ms) = (f64::MAX, 0.0, 0.0);
        for _ in 0..5 {
            let t = Instant::now();
            let a = pool.install(|| parallel_part(black_box(par_data), rounds));
            let mid = t.elapsed().as_secs_f64() * 1e3;
            let b = serial_part(black_box(ser_data));
            let total = t.elapsed().as_secs_f64() * 1e3;
            black_box((a, b));
            if total < best {
                (best, par_ms, ser_ms) = (total, mid, total - mid);
            }
        }
        if threads == 1 {
            (t1, f) = (best, ser_ms / best);
        }
        let predicted = 1.0 / (f + (1.0 - f) / threads as f64);
        println!(
            "  {threads} thread(s): {best:5.1} ms = parallel {par_ms:5.1} + serial {ser_ms:4.1}   speedup {:.2}x (Amdahl predicts {predicted:.2}x)",
            t1 / best
        );
    }
    println!("  serial fraction f = {f:.2} on 1 thread; Amdahl's ceiling 1/f = {:.1}x", 1.0 / f);
}

fn main() {
    let small: Vec<u64> = (0..400_000u64).collect(); // 3.2 MB: fits in L2 + L3
    let serial: Vec<u64> = (0..3_000_000u64).collect();
    run("compute-bound parallel part (64 rounds per element, 3.2 MB):", &small, 64, &serial);
    let big: Vec<u64> = (0..24_000_000u64).collect(); // 192 MB: streams from DRAM
    run("memory-bound parallel part (1 round per element, 192 MB):", &big, 1, &serial);
}
