// verify: release ok
//! What a task costs on each runtime flavor: spawn + join, and a ping-pong round trip between two tasks.
//! Three runs of each measurement in one process; one Playground execution, shared machine: noisy.
use std::time::{Duration, Instant};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;

const SPAWNS: u32 = 100_000;
const TRIPS: u32 = 100_000;

async fn spawn_join() -> Duration {
    let t = Instant::now();
    let handles: Vec<_> = (0..SPAWNS).map(|i| tokio::spawn(async move { i })).collect();
    let mut sum = 0u64;
    for h in handles {
        sum += h.await.unwrap() as u64;
    }
    assert_eq!(sum, (SPAWNS as u64 - 1) * SPAWNS as u64 / 2);
    t.elapsed() / SPAWNS
}

async fn ping_pong() -> Duration {
    let (to_b, mut b_rx) = mpsc::channel::<u32>(1);
    let (to_a, mut a_rx) = mpsc::channel::<u32>(1);
    let b = tokio::spawn(async move {
        while let Some(x) = b_rx.recv().await {
            if to_a.send(x + 1).await.is_err() {
                break;
            }
        }
    });
    let t = Instant::now();
    let mut x = 0;
    for _ in 0..TRIPS {
        to_b.send(x).await.unwrap();
        x = a_rx.recv().await.unwrap();
    }
    let per_trip = t.elapsed() / TRIPS;
    drop(to_b);
    b.await.unwrap();
    assert_eq!(x, TRIPS);
    per_trip
}

fn measure(name: &str, rt: Runtime) {
    let (mut spawn, mut trip) = (Vec::new(), Vec::new());
    for _ in 0..3 {
        // block_on runs the measuring future on the calling thread, so on a multi-thread runtime
        // spawn it: the measurement then runs on a worker, like the tasks it measures.
        let (s, p) = rt.block_on(async { tokio::spawn(async { (spawn_join().await, ping_pong().await) }).await.unwrap() });
        spawn.push(s);
        trip.push(p);
    }
    println!("{name:<30} spawn+join per task {spawn:>8.0?}   round trip {trip:>8.0?}");
}

fn main() {
    measure("current_thread", Builder::new_current_thread().build().unwrap());
    measure("multi_thread, 4 workers", Builder::new_multi_thread().worker_threads(4).build().unwrap());
    measure("multi_thread, 1 worker", Builder::new_multi_thread().worker_threads(1).build().unwrap());
    // For scale: an OS thread hand-off round trip, the same shape with std channels.
    let (to_b, b_rx) = std::sync::mpsc::sync_channel::<u32>(0);
    let (to_a, a_rx) = std::sync::mpsc::sync_channel::<u32>(0);
    let b = std::thread::spawn(move || {
        while let Ok(x) = b_rx.recv() {
            if to_a.send(x + 1).is_err() {
                break;
            }
        }
    });
    let t = Instant::now();
    let mut x = 0;
    for _ in 0..20_000 {
        to_b.send(x).unwrap();
        x = a_rx.recv().unwrap();
    }
    println!("{:<30} {:>40} round trip {:>8.0?}", "OS threads (std sync_channel)", "", t.elapsed() / 20_000);
    drop(to_b);
    b.join().unwrap();
}
