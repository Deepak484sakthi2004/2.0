// verify: release ok
// Three ways the optimizer turns a benchmark into a measurement of nothing.
use std::hint::black_box;
use std::time::Instant;

fn sum_slice(v: &[u64]) -> u64 {
    v.iter().sum()
}

fn sum_to(n: u64) -> u64 {
    (0..n).sum()
}

/// Nanoseconds per call: best of 15 samples of `iters` calls.
fn ns_per_call(iters: u32, mut f: impl FnMut()) -> f64 {
    f(); // one warm-up call
    (0..15)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..iters {
                f();
            }
            t.elapsed().as_nanos() as f64 / iters as f64
        })
        .fold(f64::MAX, f64::min)
}

fn main() {
    let v: Vec<u64> = (0..100_000).collect();

    println!("Summing 100,000 u64 (800 KB), ns per call:");
    let a = ns_per_call(1_000, || {
        sum_slice(&v); // result unused
    });
    let b = ns_per_call(1_000, || {
        black_box(sum_slice(&v)); // result kept, input visible to the optimizer
    });
    let c = ns_per_call(1_000, || {
        black_box(sum_slice(black_box(&v))); // input and result opaque
    });
    println!("  result discarded                     {a:>10.1}");
    println!("  black_box(result)                    {b:>10.1}");
    println!("  black_box(input) and black_box(result) {c:>8.1}");

    println!("Summing the range 0..n, ns per call:");
    for n in [1_000u64, 1_000_000, 1_000_000_000] {
        let d = ns_per_call(1_000, || {
            black_box(sum_to(black_box(n)));
        });
        println!("  n = {n:>13}   {d:>6.2}   (a loop of n additions would take ~n/4 ns or more)");
    }
}
