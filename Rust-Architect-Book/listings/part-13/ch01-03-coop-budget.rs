// verify: release ok
//! Tokio's cooperative budget: Tokio resources (channels, sockets, timers) make a task yield after it has
//! done a budget's worth of operations, even if every one of them was ready. Futures outside Tokio don't.
//! A heartbeat task sleeps 10 ms in a loop and records its worst lateness while a busy task runs on the same
//! single-threaded runtime. One run on a shared machine: the orders of magnitude are the point.
use std::future::Future;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time;

const ITEMS: u64 = 5_000_000;
const SPINS: u64 = 200_000_000; // enough always-ready awaits to take a few hundred ms

async fn with_heartbeat<F: Future<Output = u64> + Send + 'static>(label: &str, busy: F) {
    let hb = tokio::spawn(async {
        let (mut worst, mut beats) = (Duration::ZERO, 0u32);
        loop {
            let t = Instant::now();
            time::sleep(Duration::from_millis(10)).await;
            worst = worst.max(t.elapsed().saturating_sub(Duration::from_millis(10)));
            beats += 1;
            if beats == 30 {
                return (worst, beats);
            }
        }
    });
    tokio::task::yield_now().await; // let the heartbeat start its first sleep
    let t = Instant::now();
    let sum = tokio::spawn(busy).await.unwrap();
    let busy_for = t.elapsed();
    let (worst, _) = hb.await.unwrap();
    println!("{label:<40} busy {busy_for:>9.1?}  heartbeat worst lateness {worst:>9.1?}  (sum {sum})");
}

fn filled_channel() -> mpsc::UnboundedReceiver<u64> {
    let (tx, rx) = mpsc::unbounded_channel();
    for i in 0..ITEMS {
        tx.send(i).unwrap();
    }
    rx // tx dropped here: recv() returns None after the last item
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // A: every recv() is ready, but each one spends budget; at zero, recv() returns Pending once.
    let mut rx = filled_channel();
    with_heartbeat("A: mpsc recv loop (budgeted)", async move {
        let mut sum = 0;
        while let Some(x) = rx.recv().await {
            sum += x;
        }
        sum
    })
    .await;

    // B: the same loop, with the budget switched off.
    let mut rx = filled_channel();
    with_heartbeat("B: same loop inside task::unconstrained", tokio::task::unconstrained(async move {
        let mut sum = 0;
        while let Some(x) = rx.recv().await {
            sum += x;
        }
        sum
    }))
    .await;

    // C: awaiting futures that are always ready and know nothing about Tokio's budget.
    with_heartbeat("C: loop over std::future::ready", async {
        let mut sum = 0;
        for i in 0..SPINS {
            sum += std::future::ready(std::hint::black_box(i)).await;
        }
        sum
    })
    .await;

    // D: C again, with an explicit yield every 10,000 iterations.
    with_heartbeat("D: C + yield_now every 10,000", async {
        let mut sum = 0;
        for i in 0..SPINS {
            sum += std::future::ready(std::hint::black_box(i)).await;
            if i % 10_000 == 0 {
                tokio::task::yield_now().await;
            }
        }
        sum
    })
    .await;
}
