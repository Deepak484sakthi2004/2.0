// verify: debug ok
//! Graceful shutdown with a CancellationToken (tell everyone to stop) and a TaskTracker (wait for them),
//! bounded by a deadline after which stragglers are aborted. Paused clock: exact timings.
use std::time::Duration;
use tokio::time::{sleep, timeout, Instant};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// A connection handler: serves requests until told to stop, but never abandons a request mid-way.
async fn connection(id: u32, request_ms: u64, stop: CancellationToken, start: Instant) -> String {
    let mut served = 0;
    loop {
        tokio::select! {
            _ = stop.cancelled() => {
                return format!("conn {id}: stopped cleanly at {:?} after {served} requests", start.elapsed());
            }
            _ = sleep(Duration::from_millis(30)) => {} // the next request arrives
        }
        sleep(Duration::from_millis(request_ms)).await; // serve it: deliberately NOT raced with `stop`
        served += 1;
    }
}

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    let start = Instant::now();
    let shutdown = CancellationToken::new();
    let tracker = TaskTracker::new();
    let mut handles = Vec::new();
    for (id, request_ms) in [(1, 5), (2, 50), (3, 5_000)] {
        let stop = shutdown.child_token();
        handles.push(tracker.spawn(async move { println!("{}", connection(id, request_ms, stop, start).await) }));
    }
    tracker.close(); // no more tasks will be added; wait() can now complete

    sleep(Duration::from_millis(100)).await;
    println!("at {:?}: shutdown requested", start.elapsed());
    shutdown.cancel();

    match timeout(Duration::from_secs(1), tracker.wait()).await {
        Ok(()) => println!("at {:?}: all connections drained", start.elapsed()),
        Err(_) => {
            let stragglers = handles.iter().filter(|h| !h.is_finished()).count();
            println!("at {:?}: drain deadline passed, aborting {stragglers} straggler(s)", start.elapsed());
            for h in &handles {
                h.abort();
            }
            tracker.wait().await;
            println!("at {:?}: done", start.elapsed());
        }
    }
}
