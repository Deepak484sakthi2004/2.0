// verify: release ok
//! CPU-bound work in an async service: inline, spawn_blocking, or a rayon pool + oneshot.
//! A heartbeat on a 2-worker runtime reports its worst lateness while 8 x ~100 ms of hashing runs.
//! One run on a shared 4-vCPU machine.
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

fn hash_statement(rounds: u32) -> [u8; 32] {
    let mut h = [0u8; 32];
    for _ in 0..rounds {
        h = Sha256::digest(h).into();
    }
    h
}

async fn heartbeat_until(stop: tokio::sync::oneshot::Receiver<()>) -> Duration {
    let mut worst = Duration::ZERO;
    tokio::pin!(stop);
    loop {
        let t = Instant::now();
        tokio::select! {
            _ = &mut stop => return worst,
            _ = tokio::time::sleep(Duration::from_millis(5)) => {}
        }
        worst = worst.max(t.elapsed().saturating_sub(Duration::from_millis(5)));
    }
}

async fn run(label: &str, rounds: u32, how: u8) {
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let hb = tokio::spawn(heartbeat_until(stop_rx));
    tokio::time::sleep(Duration::from_millis(10)).await;
    let t = Instant::now();
    let mut jobs = Vec::new();
    for _ in 0..8 {
        jobs.push(tokio::spawn(async move {
            match how {
                0 => hash_statement(rounds), // inline, on a worker thread
                1 => tokio::task::spawn_blocking(move || hash_statement(rounds)).await.unwrap(),
                _ => {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    rayon::spawn(move || {
                        let _ = tx.send(hash_statement(rounds));
                    });
                    rx.await.unwrap()
                }
            }
        }));
    }
    for j in jobs {
        std::hint::black_box(j.await.unwrap());
    }
    let took = t.elapsed();
    stop_tx.send(()).unwrap();
    println!("{label:<30} 8 jobs took {took:>7.0?}   heartbeat worst lateness {:>8.1?}", hb.await.unwrap());
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    // Calibrate: rounds for ~100 ms of hashing on this machine.
    let t = Instant::now();
    hash_statement(100_000);
    let rounds = (100_000.0 * 0.100 / t.elapsed().as_secs_f64()) as u32;
    run("inline on the async workers", rounds, 0).await;
    run("spawn_blocking", rounds, 1).await;
    run("rayon pool + oneshot", rounds, 2).await;
}
