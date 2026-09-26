// verify: release ok
// What does reading the clock cost, and what's the smallest interval it can see?
use std::hint::black_box;
use std::time::{Instant, SystemTime};

fn main() {
    let clocksource = std::fs::read_to_string("/sys/devices/system/clocksource/clocksource0/current_clocksource")
        .unwrap_or_else(|_| "unknown".into());
    println!("clocksource: {}", clocksource.trim());

    let n = 5_000_000u32;
    let t = Instant::now();
    for _ in 0..n {
        black_box(Instant::now());
    }
    println!("Instant::now()    {:5.1} ns per call", t.elapsed().as_nanos() as f64 / n as f64);
    let t = Instant::now();
    for _ in 0..n {
        black_box(SystemTime::now());
    }
    println!("SystemTime::now() {:5.1} ns per call", t.elapsed().as_nanos() as f64 / n as f64);

    // Smallest non-zero difference between consecutive readings.
    let mut min_step = u128::MAX;
    let mut zeros = 0u32;
    for _ in 0..1_000_000 {
        let a = Instant::now();
        let b = Instant::now();
        let d = (b - a).as_nanos();
        if d == 0 {
            zeros += 1;
        } else {
            min_step = min_step.min(d);
        }
    }
    println!("back-to-back readings: smallest non-zero step {min_step} ns, {zeros} of 1,000,000 pairs read equal");
}
