// verify: debug ok
//! Per-hop timeouts vs one propagated deadline. client -> gateway -> payments-core -> processor, client
//! budget 300 ms. Each hop runs as its own task, like a separate service: a caller that gives up does NOT
//! stop the callee. The processor's work takes 300 ms. Paused clock: exact virtual times.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::time::{sleep, timeout, timeout_at, Instant};

static WASTED_MS: AtomicU64 = AtomicU64::new(0);
const WORK: Duration = Duration::from_millis(300);

async fn processor(client_deadline: Instant, deadline: Option<Instant>) -> Result<&'static str, String> {
    if let Some(d) = deadline {
        let left = d.saturating_duration_since(Instant::now());
        if left < WORK {
            return Err(format!("processor refused at once: {left:?} left, needs {WORK:?}"));
        }
    }
    for _ in 0..30 {
        sleep(Duration::from_millis(10)).await; // the work, in 10 ms steps
        if Instant::now() > client_deadline {
            WASTED_MS.fetch_add(10, Ordering::SeqCst);
        }
    }
    Ok("charged")
}

/// Call a downstream service (spawned: it runs independently) and wait for it within a limit.
async fn call<F>(downstream: F, fixed: Duration, deadline: Option<Instant>, who: &str) -> Result<&'static str, String>
where
    F: std::future::Future<Output = Result<&'static str, String>> + Send + 'static,
{
    let handle = tokio::spawn(downstream);
    let waited = match deadline {
        Some(d) => timeout_at(d, handle).await,
        None => timeout(fixed, handle).await,
    };
    match waited {
        Ok(joined) => joined.unwrap(),
        Err(_) => Err(format!("{who}: gave up waiting")),
    }
}

async fn payments_core(client_deadline: Instant, deadline: Option<Instant>) -> Result<&'static str, String> {
    sleep(Duration::from_millis(60)).await; // fraud check
    call(processor(client_deadline, deadline), Duration::from_millis(250), deadline, "payments-core").await
}

async fn gateway(client_deadline: Instant, deadline: Option<Instant>) -> Result<&'static str, String> {
    sleep(Duration::from_millis(50)).await; // auth, routing
    call(payments_core(client_deadline, deadline), Duration::from_millis(280), deadline, "gateway").await
}

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    for propagate in [false, true] {
        WASTED_MS.store(0, Ordering::SeqCst);
        let start = Instant::now();
        let client_deadline = start + Duration::from_millis(300);
        let deadline = propagate.then_some(client_deadline);
        let result = call(gateway(client_deadline, deadline), Duration::from_millis(300), Some(client_deadline), "client").await;
        let answered_at = start.elapsed();
        sleep(Duration::from_millis(500)).await; // let every hop finish whatever it's still doing
        let mode = if propagate { "deadline propagated:" } else { "fixed per-hop timeouts:" };
        println!("{mode}");
        println!("  client got {result:?} after {answered_at:?}");
        println!("  processor work done after the client had given up: {} ms", WASTED_MS.load(Ordering::SeqCst));
    }
}
