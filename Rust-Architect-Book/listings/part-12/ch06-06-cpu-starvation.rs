// verify: release ok
//! A CPU-bound task that never awaits can't be interrupted: async scheduling is cooperative.
//! Same heartbeat as ch06-03; this time the other task *computes* for ~150 ms (no blocking syscall).
//! Version 2 yields to the executor every 50,000 iterations. One run on a shared machine: noisy.
use std::time::{Duration, Instant};

async fn heartbeat(beats: u32) -> Duration {
    let mut worst = Duration::ZERO;
    for _ in 0..beats {
        let asked = Instant::now();
        tokio::time::sleep(Duration::from_millis(10)).await;
        worst = worst.max(asked.elapsed().saturating_sub(Duration::from_millis(10)));
    }
    worst
}

/// Busy work: roughly 150 ms of arithmetic on the Playground, calibrated at run time.
fn work_step(i: u64, acc: u64) -> u64 {
    std::hint::black_box(acc.wrapping_mul(6364136223846793005).wrapping_add(i))
}

async fn score_all(iterations: u64, yield_every: Option<u64>) -> (u64, u32) {
    let (mut acc, mut yields) = (0u64, 0u32);
    for i in 0..iterations {
        acc = work_step(i, acc);
        if let Some(n) = yield_every {
            if i % n == n - 1 {
                tokio::task::yield_now().await; // give the executor a chance to run other tasks
                yields += 1;
            }
        }
    }
    (acc, yields)
}

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();

    // Calibrate with the same async function: how many iterations take ~150 ms here?
    let t = Instant::now();
    std::hint::black_box(rt.block_on(score_all(5_000_000, None)));
    let per_iter = t.elapsed().as_nanos() as f64 / 5_000_000.0;
    let iterations = (150_000_000.0 / per_iter) as u64;

    for (label, yield_every) in [("never yields", None), ("yields every 50,000 iterations", Some(50_000))] {
        let (worst, took, yields) = rt.block_on(async {
            let hb = tokio::spawn(heartbeat(30));
            tokio::time::sleep(Duration::from_millis(25)).await;
            let t = Instant::now();
            let (_, yields) = score_all(iterations, yield_every).await;
            let took = t.elapsed();
            (hb.await.unwrap(), took, yields)
        });
        println!(
            "CPU-bound task {label:<31}: took {:>3} ms, {yields:>4} yields; worst heartbeat delay {:>3} ms",
            took.as_millis(),
            worst.as_millis()
        );
    }
}
