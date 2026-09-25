// verify: release ok
//! 64 CPU-bound jobs on a 4-core machine: one thread per job vs a pool of 4 threads.
//! Total time is about the same; WHEN each job finishes is not. One run, noisy.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const JOBS: usize = 64;

/// About `ms` milliseconds of pure CPU work (calibrated once).
fn burn(iters: u64) -> u64 {
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    for _ in 0..iters {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
    }
    x
}

fn calibrate(target: Duration) -> u64 {
    let iters = 1_000_000;
    let t = Instant::now();
    std::hint::black_box(burn(iters));
    let per_iter = t.elapsed().as_secs_f64() / iters as f64;
    (target.as_secs_f64() / per_iter) as u64
}

fn report(label: &str, start: Instant, mut done: Vec<Duration>) {
    let total = start.elapsed();
    done.sort();
    let mean = done.iter().sum::<Duration>() / done.len() as u32;
    println!(
        "{label:<22} total {:>7.1?}   first job done {:>7.1?}   mean completion {:>7.1?}   last {:>7.1?}",
        total, done[0], mean, done[done.len() - 1]
    );
}

fn main() {
    let cores = thread::available_parallelism().unwrap().get();
    let iters = calibrate(Duration::from_millis(5));
    println!("cores = {cores}, {JOBS} jobs of ~5 ms CPU each");

    // One OS thread per job: the kernel time-slices 64 runnable threads over 4 cores.
    let start = Instant::now();
    let handles: Vec<_> = (0..JOBS)
        .map(|_| {
            thread::spawn(move || {
                std::hint::black_box(burn(iters));
                start.elapsed()
            })
        })
        .collect();
    let done = handles.into_iter().map(|h| h.join().unwrap()).collect();
    report("thread per job (64):", start, done);

    // A pool sized to the cores: jobs run to completion one after another on each worker.
    let start = Instant::now();
    let next = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(Mutex::new(Vec::with_capacity(JOBS)));
    let workers: Vec<_> = (0..cores)
        .map(|_| {
            let (next, done) = (Arc::clone(&next), Arc::clone(&done));
            thread::spawn(move || {
                while next.fetch_add(1, Ordering::Relaxed) < JOBS {
                    std::hint::black_box(burn(iters));
                    done.lock().unwrap().push(start.elapsed());
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    let done = Arc::try_unwrap(done).unwrap().into_inner().unwrap();
    report(&format!("pool of {cores}:"), start, done);
}
