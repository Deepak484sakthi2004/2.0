// verify: debug ok
//! tokio::sync::Notify: notify_one() stores a permit if nobody is waiting; notify_waiters() doesn't.
//! The "check the condition, then wait" race, and the enable() pattern that closes it.
//! Everything runs on one thread in a fixed order, so the interleavings below are deterministic.
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::timeout;

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    // 1. notify_one before anyone waits: the permit is stored, the later wait completes at once.
    let n = Notify::new();
    n.notify_one();
    println!("1. notify_one, then wait:     {:?}", timeout(Duration::from_secs(1), n.notified()).await.map(|_| "woken"));

    // 2. notify_waiters before anyone waits: nothing is stored, the later wait sleeps until the timeout.
    let n = Notify::new();
    n.notify_waiters();
    println!("2. notify_waiters, then wait: {:?}", timeout(Duration::from_secs(1), n.notified()).await.map(|_| "woken"));

    // 3. The race with notify_waiters: check, [event happens here], then wait.
    let (ready, n) = (AtomicBool::new(false), Notify::new());
    let checked = ready.load(Ordering::SeqCst); // the waiter looks: not ready yet
    ready.store(true, Ordering::SeqCst); // the event, between the check and the wait
    n.notify_waiters();
    let r = if checked { Ok("ready") } else { timeout(Duration::from_secs(1), n.notified()).await.map(|_| "woken") };
    println!("3. check, event, wait:        {r:?}   <- lost wakeup");

    // 4. The fix: create and enable() the Notified future BEFORE checking. It is registered from that point on.
    let (ready, n) = (AtomicBool::new(false), Notify::new());
    let notified = n.notified();
    tokio::pin!(notified);
    notified.as_mut().enable(); // now on the waiter list
    let checked = ready.load(Ordering::SeqCst);
    ready.store(true, Ordering::SeqCst);
    n.notify_waiters();
    let r = if checked { Ok("ready") } else { timeout(Duration::from_secs(1), notified).await.map(|_| "woken") };
    println!("4. enable, check, event, wait: {r:?}");
}
