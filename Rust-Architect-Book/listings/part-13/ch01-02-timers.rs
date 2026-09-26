// verify: release ok
//! Tokio's timer: millisecond resolution, deadlines rounded UP, and many timers at almost no cost each.
//! Timings are from one run on a shared machine: read them as orders of magnitude.
use std::time::{Duration, Instant};
use tokio::time::{self, MissedTickBehavior};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // 1. Resolution: every sleep lasts at least one millisecond tick.
    for requested in [Duration::from_micros(1), Duration::from_micros(500), Duration::from_micros(1_500), Duration::from_millis(5)] {
        let t = Instant::now();
        for _ in 0..100 {
            time::sleep(requested).await;
        }
        let each = t.elapsed() / 100;
        println!("sleep({requested:>8?}) x100: {each:>10.3?} each");
    }

    // 2. Many timers at once: 100,000 tasks, each sleeping 50 ms, all registered in the wheel together.
    let t = Instant::now();
    let deadline = time::Instant::now() + Duration::from_millis(50);
    let handles: Vec<_> = (0..100_000)
        .map(|_| {
            tokio::spawn(async move {
                time::sleep_until(deadline).await;
                time::Instant::now().saturating_duration_since(deadline) // how late this timer fired
            })
        })
        .collect();
    let spawned = t.elapsed();
    let mut worst = Duration::ZERO;
    for h in handles {
        worst = worst.max(h.await.unwrap());
    }
    println!("100,000 sleeping tasks: spawned in {spawned:.1?}, all done after {:.1?}, worst lateness {worst:.1?}", t.elapsed());

    // 3. Interval ticks when the consumer falls behind: Burst (default) vs Skip.
    for behavior in [MissedTickBehavior::Burst, MissedTickBehavior::Skip] {
        let mut iv = time::interval(Duration::from_millis(10));
        iv.set_missed_tick_behavior(behavior);
        let start = time::Instant::now();
        let mut at = Vec::new();
        for i in 0..6 {
            iv.tick().await;
            at.push(start.elapsed().as_millis());
            if i == 1 {
                std::thread::sleep(Duration::from_millis(35)); // the consumer is late by 3.5 periods, once
            }
        }
        println!("{behavior:?}: ticks at {at:?} ms");
    }
}
