// verify: release ok
//! std::sync::Mutex or tokio::sync::Mutex? Part 1: uncontended cost (real clock, one run).
//! Part 2: holding a tokio Mutex across an .await serializes every task behind the slowest await
//! (paused virtual clock, so the numbers are exact).
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn uncontended_costs() {
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let n = 1_000_000u32;
    let std_m = std::sync::Mutex::new(0u64);
    let t = Instant::now();
    for _ in 0..n {
        *std_m.lock().unwrap() += 1;
    }
    let std_each = t.elapsed() / n;
    let tokio_m = tokio::sync::Mutex::new(0u64);
    let tokio_each = rt.block_on(async {
        let t = Instant::now();
        for _ in 0..n {
            *tokio_m.lock().await += 1;
        }
        t.elapsed() / n
    });
    println!("uncontended lock+unlock: std::sync::Mutex {std_each:?}, tokio::sync::Mutex {tokio_each:?} (one run)");
}

async fn fetch_rate_from_upstream() -> u64 {
    tokio::time::sleep(Duration::from_millis(10)).await; // 10 ms network call
    150
}

fn convoy() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().start_paused(true).build().unwrap();
    rt.block_on(async {
        // Version A: hold the lock across the upstream call, "so two tasks don't both refresh".
        let cache = Arc::new(tokio::sync::Mutex::new(HashMap::<u32, u64>::new()));
        let t = tokio::time::Instant::now();
        let tasks: Vec<_> = (0..50u32)
            .map(|merchant| {
                let cache = Arc::clone(&cache);
                tokio::spawn(async move {
                    let mut guard = cache.lock().await;
                    let rate = fetch_rate_from_upstream().await; // the lock is held across this .await
                    guard.insert(merchant, rate);
                })
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }
        println!("A: 50 tasks, lock held across a 10 ms await: {:?} (virtual time)", t.elapsed());

        // Version B: await first, lock only to publish. A std Mutex is enough, since no guard crosses an .await.
        let cache = Arc::new(std::sync::Mutex::new(HashMap::<u32, u64>::new()));
        let t = tokio::time::Instant::now();
        let tasks: Vec<_> = (0..50u32)
            .map(|merchant| {
                let cache = Arc::clone(&cache);
                tokio::spawn(async move {
                    let rate = fetch_rate_from_upstream().await;
                    cache.lock().unwrap().insert(merchant, rate); // guard dropped at the end of the statement
                })
            })
            .collect();
        for task in tasks {
            task.await.unwrap();
        }
        println!("B: 50 tasks, await first, then lock:       {:?} (virtual time)", t.elapsed());
    });
}

fn main() {
    uncontended_costs();
    convoy();
}
