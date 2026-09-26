// verify: release ok
//! Blocking inside async code stalls a worker thread and everything queued on it.
//! A heartbeat task sleeps 10 ms in a loop; we report its worst lateness while blocking work runs.
//! One run on a shared machine.
use std::time::{Duration, Instant};
use tokio::runtime::Builder;

async fn heartbeat(beats: u32) -> Duration {
    let mut worst = Duration::ZERO;
    for _ in 0..beats {
        let t = Instant::now();
        tokio::time::sleep(Duration::from_millis(10)).await;
        worst = worst.max(t.elapsed().saturating_sub(Duration::from_millis(10)));
    }
    worst
}

fn scenario(label: &str, workers: usize, blockers: usize, use_spawn_blocking: bool) {
    let rt = Builder::new_multi_thread().worker_threads(workers).enable_all().build().unwrap();
    let worst = rt.block_on(async move {
        let hb = tokio::spawn(heartbeat(30));
        tokio::time::sleep(Duration::from_millis(5)).await;
        let mut jobs = Vec::new();
        for _ in 0..blockers {
            jobs.push(if use_spawn_blocking {
                tokio::task::spawn_blocking(|| std::thread::sleep(Duration::from_millis(200)))
            } else {
                tokio::spawn(async { std::thread::sleep(Duration::from_millis(200)) }) // blocking call in async code
            });
        }
        for j in jobs {
            j.await.unwrap();
        }
        hb.await.unwrap()
    });
    println!("{label:<55} heartbeat worst lateness {worst:>8.1?}");
}

fn main() {
    scenario("4 workers, 1 task calls thread::sleep(200 ms)", 4, 1, false);
    scenario("4 workers, 4 tasks call thread::sleep(200 ms)", 4, 4, false);
    scenario("4 workers, 4 x spawn_blocking(thread::sleep(200 ms))", 4, 4, true);
}
