// verify: release ok
// One number hides the distribution: time every single Vec::push of 4M u64s.
// The book's per-operation harness: hdrhistogram (on the Playground) records each latency in ns.
use hdrhistogram::Histogram;
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let n = 4_000_000u64;
    let mut hist = Histogram::<u64>::new_with_bounds(1, 10_000_000_000, 3).unwrap();
    let mut v: Vec<u64> = Vec::new();
    let start = Instant::now();
    for i in 0..n {
        let t = Instant::now();
        v.push(black_box(i));
        hist.record(t.elapsed().as_nanos().max(1) as u64).unwrap();
    }
    let total = start.elapsed();
    black_box(&v);

    // The timer itself: an empty timed region, measured the same way.
    let mut empty = Histogram::<u64>::new_with_bounds(1, 1_000_000_000, 3).unwrap();
    for _ in 0..1_000_000 {
        let t = Instant::now();
        empty.record(t.elapsed().as_nanos().max(1) as u64).unwrap();
    }

    println!("{n} pushes, final capacity {}", v.capacity());
    println!("wall time / n          {:>9.1} ns   (the number a 'benchmark' usually reports)", total.as_nanos() as f64 / n as f64);
    for (label, q) in [("p50", 50.0), ("p99", 99.0), ("p99.9", 99.9), ("p99.99", 99.99)] {
        println!("{label:<22} {:>9} ns", hist.value_at_percentile(q));
    }
    println!("max                    {:>9} ns", hist.max());
    println!("mean of samples        {:>9.1} ns", hist.mean());
    println!("empty timed region p50 {:>9} ns   (timer overhead included in every sample above)", empty.value_at_percentile(50.0));
    // How much of the total time did the slowest 0.01% of pushes take?
    let slow: u64 = hist.iter_recorded().filter(|v| v.value_iterated_to() >= hist.value_at_percentile(99.99))
        .map(|v| v.value_iterated_to() * v.count_at_value()).sum();
    println!("slowest 0.01% of pushes = {:.0}% of the summed sample time", 100.0 * slow as f64 / (hist.mean() * hist.len() as f64));
}
