// verify: debug ok
//! JoinHandle semantics: dropping it detaches the task; abort() cancels at the next .await;
//! a panic inside the task becomes a JoinError instead of crashing the process.
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    std::panic::set_hook(Box::new(|_| {})); // keep the demo's stderr quiet; JoinError still carries the payload
    let done = Arc::new(AtomicU32::new(0));

    // 1. Drop the handle: the task keeps running (detached).
    let d = Arc::clone(&done);
    drop(tokio::spawn(async move {
        sleep(Duration::from_millis(20)).await;
        d.fetch_add(1, Ordering::SeqCst);
    }));
    sleep(Duration::from_millis(50)).await;
    println!("1. handle dropped, task finished anyway: done = {}", done.load(Ordering::SeqCst));

    // 2. abort(): the task is dropped at its next .await; the handle reports cancellation.
    let d = Arc::clone(&done);
    let h = tokio::spawn(async move {
        sleep(Duration::from_secs(10)).await;
        d.fetch_add(100, Ordering::SeqCst); // never runs
    });
    h.abort();
    let e = h.await.unwrap_err();
    println!("2. aborted: is_cancelled = {}, is_panic = {}, done = {}", e.is_cancelled(), e.is_panic(), done.load(Ordering::SeqCst));

    // 3. A panic inside a task is caught by the runtime and returned through the handle.
    let h = tokio::spawn(async { panic!("refund amount overflow") });
    match h.await {
        Err(e) if e.is_panic() => {
            let payload = e.into_panic();
            let msg = payload.downcast_ref::<&str>().copied().unwrap_or("<non-&str payload>");
            println!("3. task panicked: {msg:?}; the runtime and this task are fine");
        }
        other => println!("3. unexpected: {other:?}"),
    }

    // 4. abort() after completion does nothing: the result is still there.
    let h = tokio::spawn(async { 7 });
    sleep(Duration::from_millis(10)).await;
    println!("4. is_finished before abort: {}", h.is_finished());
    h.abort();
    println!("4. result after abort: {:?}", h.await.map_err(|e| e.is_cancelled()));
}
