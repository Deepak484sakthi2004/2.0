// verify: debug ok
//! The same policies as reusable middleware: tower's load_shed, concurrency_limit, and timeout layers.
//! Layer order is policy: is time spent waiting for a permit part of the timeout or not? Paused clock.
use std::convert::Infallible;
use std::time::Duration;
use tower::{BoxError, Service, ServiceBuilder, ServiceExt};

/// The inner service: a processor call that takes `ms` milliseconds.
async fn processor(ms: u64) -> Result<u64, Infallible> {
    tokio::time::sleep(Duration::from_millis(ms)).await;
    Ok(ms)
}

fn describe(r: Result<u64, BoxError>) -> String {
    match r {
        Ok(ms) => format!("ok({ms}ms)"),
        Err(e) if e.is::<tower::load_shed::error::Overloaded>() => "shed".into(),
        Err(e) if e.is::<tower::timeout::error::Elapsed>() => "timeout".into(),
        Err(e) => format!("error: {e}"),
    }
}

async fn fire<S>(label: &str, svc: S)
where
    S: Service<u64, Response = u64, Error = BoxError> + Clone + Send + 'static,
    S::Future: Send,
{
    let start = tokio::time::Instant::now();
    let calls: Vec<_> = [30, 30, 30, 80, 30]
        .into_iter()
        .map(|ms| {
            let svc = svc.clone();
            tokio::spawn(async move {
                let r = svc.oneshot(ms).await;
                format!("{}@{}ms", describe(r), start.elapsed().as_millis())
            })
        })
        .collect();
    let mut out = Vec::new();
    for c in calls {
        out.push(c.await.unwrap());
    }
    println!("{label:<52} {out:?}");
}

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    // Shed when 2 calls are in flight; each admitted call has 50 ms.
    let shed_first = ServiceBuilder::new()
        .load_shed()
        .concurrency_limit(2)
        .timeout(Duration::from_millis(50))
        .service_fn(processor);
    fire("load_shed -> limit(2) -> timeout(50ms):", shed_first).await;

    // No shedding: callers wait for a permit in poll_ready. Timeout only times call(), so waiting is free.
    let timeout_outside = ServiceBuilder::new()
        .timeout(Duration::from_millis(50))
        .concurrency_limit(2)
        .service_fn(processor);
    fire("timeout(50ms) -> limit(2):", timeout_outside).await;

    // The other order behaves the same, for the same reason.
    let timeout_inside = ServiceBuilder::new()
        .concurrency_limit(2)
        .timeout(Duration::from_millis(50))
        .map_err(BoxError::from)
        .service_fn(processor);
    fire("limit(2) -> timeout(50ms):", timeout_inside).await;

    // A buffer makes poll_ready succeed at once and moves the wait for a permit INTO the call future,
    // where the outer timeout can see it: now queueing time counts against the 50 ms.
    let waiting_counts = ServiceBuilder::new()
        .timeout(Duration::from_millis(50))
        .buffer(16)
        .concurrency_limit(2)
        .service_fn(processor);
    fire("timeout(50ms) -> buffer(16) -> limit(2):", waiting_counts).await;
}
