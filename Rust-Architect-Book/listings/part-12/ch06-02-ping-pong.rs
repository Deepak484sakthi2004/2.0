// verify: release ok
//! A message round trip between two OS threads (each blocks in recv: the kernel switches threads)
//! vs between two async tasks on one thread (a switch is: return Pending, pop the next task).
//! One run on a shared machine: noisy. Compare orders of magnitude, not digits.
use futures::channel::mpsc as amsc;
use futures::{SinkExt, StreamExt};
use std::time::Instant;

const ROUNDS: u32 = 100_000;

fn threads() -> f64 {
    let (to_b, from_a) = std::sync::mpsc::channel::<u32>();
    let (to_a, from_b) = std::sync::mpsc::channel::<u32>();
    let b = std::thread::spawn(move || {
        while let Ok(v) = from_a.recv() {
            to_a.send(v + 1).unwrap();
        }
    });
    let start = Instant::now();
    let mut v = 0;
    for _ in 0..ROUNDS {
        to_b.send(v).unwrap();
        v = from_b.recv().unwrap();
    }
    let ns = start.elapsed().as_nanos() as f64 / ROUNDS as f64;
    drop(to_b);
    b.join().unwrap();
    assert_eq!(v, ROUNDS);
    ns
}

fn tasks() -> f64 {
    use futures::task::LocalSpawnExt;
    let mut pool = futures::executor::LocalPool::new();
    let (mut to_b, mut from_a) = amsc::channel::<u32>(1);
    let (mut to_a, mut from_b) = amsc::channel::<u32>(1);
    pool.spawner()
        .spawn_local(async move {
            while let Some(v) = from_a.next().await {
                to_a.send(v + 1).await.unwrap();
            }
        })
        .unwrap();
    let start = Instant::now();
    let v = pool.run_until(async move {
        let mut v = 0;
        for _ in 0..ROUNDS {
            to_b.send(v).await.unwrap();
            v = from_b.next().await.unwrap();
        }
        v
    });
    assert_eq!(v, ROUNDS);
    start.elapsed().as_nanos() as f64 / ROUNDS as f64
}

fn main() {
    println!("OS threads, std mpsc:          {:>8.0} ns per round trip", threads());
    println!("async tasks, one thread, mpsc: {:>8.0} ns per round trip", tasks());
}
