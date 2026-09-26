// verify: debug ok
//! Semaphore: a concurrency limit you can wait on (acquire) or refuse with (try_acquire), and close for shutdown.
//! Paused virtual clock: timings are exact.
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Semaphore, TryAcquireError};
use tokio::time::{sleep, Instant};

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    // 1. At most 3 calls to the processor at once; 12 requests of 100 ms each.
    let permits = Arc::new(Semaphore::new(3));
    let (in_flight, peak) = (Arc::new(AtomicU32::new(0)), Arc::new(AtomicU32::new(0)));
    let t = Instant::now();
    let calls: Vec<_> = (0..12)
        .map(|_| {
            let (permits, in_flight, peak) = (Arc::clone(&permits), Arc::clone(&in_flight), Arc::clone(&peak));
            tokio::spawn(async move {
                let _permit = permits.acquire_owned().await.unwrap(); // released when dropped
                let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                sleep(Duration::from_millis(100)).await;
                in_flight.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect();
    for c in calls {
        c.await.unwrap();
    }
    println!("1. 12 x 100 ms through Semaphore(3): peak in flight {}, took {:?}", peak.load(Ordering::SeqCst), t.elapsed());

    // 2. Shedding instead of queueing: try_acquire fails fast when no permit is free.
    let permits = Semaphore::new(2);
    let a = permits.try_acquire().unwrap();
    let _b = permits.try_acquire().unwrap();
    match permits.try_acquire() {
        Err(TryAcquireError::NoPermits) => println!("2. third try_acquire: NoPermits -> reply 'busy' now, don't queue"),
        other => println!("2. unexpected {other:?}"),
    }
    drop(a);
    println!("2. after one permit is dropped: available = {}", permits.available_permits());

    // 3. close(): waiters and future acquires fail, which is how a shutdown stops admitting work.
    let permits = Arc::new(Semaphore::new(0));
    let waiter = tokio::spawn({
        let permits = Arc::clone(&permits);
        async move { permits.acquire().await.map(|_| ()).map_err(|e| e.to_string()) }
    });
    tokio::task::yield_now().await;
    permits.close();
    println!("3. waiter after close(): {:?}", waiter.await.unwrap());
}
