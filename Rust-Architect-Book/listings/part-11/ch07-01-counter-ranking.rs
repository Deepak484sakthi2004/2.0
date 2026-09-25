// verify: release ok
//! Chapter 1.3 predicted "no sharing < atomic < mutex" from mechanism. Measured here, plus the two cases in
//! between: per-thread atomics that share a cache line (false sharing) and per-thread atomics padded apart.
//! 4 threads x 5M increments each; one run on a shared machine, noisy.
use crossbeam::utils::CachePadded;
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Instant;

const PER_THREAD: u64 = 5_000_000;
const THREADS: usize = 4;

fn run(label: &str, body: impl Fn(usize) + Sync) {
    let t = Instant::now();
    thread::scope(|s| {
        for id in 0..THREADS {
            let body = &body;
            s.spawn(move || body(id));
        }
    });
    let ns = t.elapsed().as_nanos() as f64 / PER_THREAD as f64;
    println!("{label:<52} {ns:>7.2} ns per increment");
}

fn main() {
    // 1. No sharing: a local counter; black_box keeps every increment (otherwise LLVM folds the loop away).
    let totals: Vec<AtomicU64> = (0..THREADS).map(|_| AtomicU64::new(0)).collect();
    run("no sharing (local counter, merged at the end)", |id| {
        let mut local = 0u64;
        for _ in 0..PER_THREAD {
            local = black_box(local) + 1;
        }
        totals[id].store(local, Ordering::Relaxed);
    });

    // 2. One atomic per thread, padded to separate cache lines: atomic RMW, but no line is shared.
    let padded: Vec<CachePadded<AtomicU64>> = (0..THREADS).map(|_| CachePadded::new(AtomicU64::new(0))).collect();
    run("per-thread atomics, padded apart (CachePadded)", |id| {
        for _ in 0..PER_THREAD {
            padded[id].fetch_add(1, Ordering::Relaxed);
        }
    });

    // 3. One atomic per thread, adjacent in memory: logically unshared, physically one cache line.
    let adjacent: [AtomicU64; THREADS] = std::array::from_fn(|_| AtomicU64::new(0));
    run("per-thread atomics, adjacent (false sharing)", |id| {
        for _ in 0..PER_THREAD {
            adjacent[id].fetch_add(1, Ordering::Relaxed);
        }
    });

    // 4. One shared atomic.
    let shared = AtomicU64::new(0);
    run("one shared AtomicU64 (fetch_add, Relaxed)", |_| {
        for _ in 0..PER_THREAD {
            shared.fetch_add(1, Ordering::Relaxed);
        }
    });

    // 5. One shared std Mutex.
    let locked = Mutex::new(0u64);
    run("one shared std::sync::Mutex<u64>", |_| {
        for _ in 0..PER_THREAD {
            *locked.lock().unwrap() += 1;
        }
    });

    // 6. One shared parking_lot Mutex (spins briefly before parking).
    let pl = parking_lot::Mutex::new(0u64);
    run("one shared parking_lot::Mutex<u64>", |_| {
        for _ in 0..PER_THREAD {
            *pl.lock() += 1;
        }
    });

    let expected = PER_THREAD * THREADS as u64;
    let all_correct = totals.iter().map(|t| t.load(Ordering::Relaxed)).sum::<u64>() == expected
        && padded.iter().map(|a| a.load(Ordering::Relaxed)).sum::<u64>() == expected
        && adjacent.iter().map(|a| a.load(Ordering::Relaxed)).sum::<u64>() == expected
        && shared.load(Ordering::Relaxed) == expected
        && *locked.lock().unwrap() == expected
        && *pl.lock() == expected;
    println!("({THREADS} threads, wall time / increments per thread; all six totals = {expected}: {all_correct})");
    println!("size_of CachePadded<AtomicU64> = {} bytes", std::mem::size_of::<CachePadded<AtomicU64>>());
}
