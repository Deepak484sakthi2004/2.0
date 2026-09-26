// verify: release ok
//! The multi-thread scheduler seen from inside: where spawned tasks run.
//! One run on a shared 4-vCPU machine. Tokio's scheduling heuristics are implementation details [LIB].
use std::collections::HashMap;
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

fn busy(d: Duration) {
    let t = Instant::now();
    while t.elapsed() < d {
        std::hint::spin_loop();
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    // 1. Parent spawns a child and immediately awaits it. Where does the child run?
    let mut same = 0;
    for _ in 0..1_000 {
        let parent = tokio::spawn(async {
            let me = thread::current().id();
            let child = tokio::spawn(async { thread::current().id() }).await.unwrap();
            me == child
        });
        same += parent.await.unwrap() as u32;
    }
    println!("1. child ran on the parent's worker thread: {same} of 1,000");

    // 2. One task spawns 4,000 CPU-busy tasks (50 µs each). Who runs them?
    let t = Instant::now();
    let per_thread = tokio::spawn(async {
        let handles: Vec<_> = (0..4_000)
            .map(|_| {
                tokio::spawn(async {
                    busy(Duration::from_micros(50));
                    thread::current().id()
                })
            })
            .collect();
        let mut counts: HashMap<ThreadId, u32> = HashMap::new();
        for h in handles {
            *counts.entry(h.await.unwrap()).or_default() += 1;
        }
        let mut v: Vec<u32> = counts.into_values().collect();
        v.sort_unstable_by(|a, b| b.cmp(a));
        v
    })
    .await
    .unwrap();
    println!("2. 4,000 tasks spawned from one worker ran on {} threads: {per_thread:?} ({:.0?}; 200 ms of work)", per_thread.len(), t.elapsed());

    // 3. A task spawns a heartbeat, then blocks its worker for 300 ms without yielding. Three workers are idle.
    for spawn_second_task in [false, true] {
        let t = Instant::now();
        let first_beat = tokio::spawn(async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                let _ = tx.send(Instant::now()); // the "heartbeat": just report when it first ran
            });
            if spawn_second_task {
                tokio::spawn(async {}); // a second spawn pushes the heartbeat out of the LIFO slot
            }
            busy(Duration::from_millis(300)); // blocks this worker thread
            rx
        })
        .await
        .unwrap()
        .await
        .unwrap();
        let label = if spawn_second_task { "heartbeat, then another spawn" } else { "heartbeat only" };
        println!("3. {label:<30}: heartbeat first ran after {:.1?}", first_beat.duration_since(t));
    }
}
