// verify: release ok
//! Blocking inside async code stalls every other task on that executor thread.
//! A heartbeat task wants to run every 10 ms; another task makes a 200 ms blocking call
//! (think: a synchronous DNS lookup, a bcrypt hash, a JDBC-style driver).
//! Tokio's single-threaded runtime is used as a representative executor (Part XIII covers Tokio).
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

fn hash_password() -> u64 {
    std::thread::sleep(Duration::from_millis(200)); // stands in for 200 ms of blocking work
    42
}

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();

    let worst = rt.block_on(async {
        let hb = tokio::spawn(heartbeat(30));
        tokio::time::sleep(Duration::from_millis(25)).await;
        let _ = hash_password(); // BUG: blocks the only executor thread
        hb.await.unwrap()
    });
    println!("blocking call on the executor thread: worst heartbeat delay {} ms", worst.as_millis());

    let worst = rt.block_on(async {
        let hb = tokio::spawn(heartbeat(30));
        tokio::time::sleep(Duration::from_millis(25)).await;
        let _ = tokio::task::spawn_blocking(hash_password).await.unwrap(); // runs on another thread
        hb.await.unwrap()
    });
    println!("blocking call moved to spawn_blocking: worst heartbeat delay {} ms", worst.as_millis());
}
