// verify: release ok
// Contention is wait time, not hold time. Each thread repeatedly takes a lock, does a short piece of work inside it
// (~0.2 us measured) and about 4x as much outside it. Measured per acquisition: time waiting and time holding.
// One Mutex, 1/2/4 threads; then the same work over 16 shards picked by key. One Playground run, noisy.
use hdrhistogram::Histogram;
use std::hint::black_box;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

fn work(rounds: u64, seed: u64) -> u64 {
    (0..rounds).fold(seed, |h, i| (h ^ i).wrapping_mul(0x100_0000_01B3))
}

fn run(threads: usize, shards: usize) -> (Histogram<u64>, Histogram<u64>, f64) {
    let locks: Arc<Vec<Mutex<u64>>> = Arc::new((0..shards).map(|_| Mutex::new(0)).collect());
    let t0 = Instant::now();
    let hs: Vec<_> = (0..threads)
        .map(|t| {
            let locks = locks.clone();
            thread::spawn(move || {
                let mut wait = Histogram::<u64>::new_with_bounds(1, 1_000_000_000, 3).unwrap();
                let mut hold = Histogram::<u64>::new_with_bounds(1, 1_000_000_000, 3).unwrap();
                for i in 0..20_000u64 {
                    let key = (i * 7919 + t as u64 * 104_729) as usize % shards;
                    let a = Instant::now();
                    let mut g = locks[key].lock().unwrap();
                    let b = Instant::now();
                    *g = work(black_box(150), *g); // inside the lock (~0.2 us measured)
                    let c = Instant::now();
                    drop(g);
                    wait.record((b - a).as_nanos().max(1) as u64).unwrap();
                    hold.record((c - b).as_nanos().max(1) as u64).unwrap();
                    black_box(work(black_box(600), i)); // outside it: 4x the rounds
                }
                (wait, hold)
            })
        })
        .collect();
    let mut wait = Histogram::<u64>::new_with_bounds(1, 1_000_000_000, 3).unwrap();
    let mut hold = wait.clone();
    for h in hs {
        let (w, x) = h.join().unwrap();
        wait.add(w).unwrap();
        hold.add(x).unwrap();
    }
    let secs = t0.elapsed().as_secs_f64();
    (wait, hold, (threads * 20_000) as f64 / secs / 1e6)
}

fn main() {
    println!("{:<22} {:>9} {:>9} {:>9} {:>9} {:>10}", "setup", "hold p50", "wait p50", "wait p99", "wait max", "M ops/s");
    for (threads, shards) in [(1, 1), (2, 1), (4, 1), (4, 16)] {
        let (w, h, mops) = run(threads, shards);
        println!(
            "{:<22} {:>9} {:>9} {:>9} {:>9} {:>10.2}",
            format!("{threads} thread(s), {shards} lock(s)"),
            h.value_at_percentile(50.0),
            w.value_at_percentile(50.0),
            w.value_at_percentile(99.0),
            w.max(),
            mops
        );
    }
    println!("(ns; hold/wait measured with Instant, ~20-25 ns per reading on this machine: see ch02-04)");
}
