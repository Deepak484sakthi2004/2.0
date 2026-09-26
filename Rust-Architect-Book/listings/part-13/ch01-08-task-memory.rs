// verify: release ok
//! What one spawned task costs in memory: tokio allocates each task as ONE heap block holding the task header,
//! the future itself, and space for its output [LIB]. Measured with the counting allocator (Part III).
//! Spawning happens inside a worker of the multi-thread runtime, whose run queues are preallocated, so the
//! counts below are the tasks' own allocations.
use std::alloc::{GlobalAlloc, Layout, System};
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use tokio::sync::oneshot;

struct Counting;
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to System with the same arguments and only adds bookkeeping.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE_BYTES.fetch_add(l.size(), Relaxed);
        ALLOCS.fetch_add(1, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE_BYTES.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

const N: usize = 10_000;

async fn per_task<F, Fut>(label: &str, make: F)
where
    F: Fn(oneshot::Receiver<()>) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let (_tx, rx) = oneshot::channel();
    let future_size = std::mem::size_of_val(&make(rx));
    // Everything except the spawns is allocated before we start counting.
    let pairs: Vec<_> = (0..N).map(|_| oneshot::channel::<()>()).collect();
    let mut senders = Vec::with_capacity(N);
    let mut handles = Vec::with_capacity(N);
    let (bytes0, allocs0) = (LIVE_BYTES.load(Relaxed), ALLOCS.load(Relaxed));
    for (tx, rx) in pairs {
        senders.push(tx);
        handles.push(tokio::spawn(make(rx))); // each task parks on its oneshot until we send
    }
    tokio::task::yield_now().await;
    let bytes = (LIVE_BYTES.load(Relaxed) - bytes0) / N;
    let allocs = (ALLOCS.load(Relaxed) - allocs0) as f64 / N as f64;
    for tx in senders {
        let _ = tx.send(());
    }
    for h in handles {
        h.await.unwrap();
    }
    println!("{label:<42} future {future_size:>5} B   per task: {allocs:.2} allocations, {bytes:>5} B live");
}

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).build().unwrap();
    rt.block_on(async {
        tokio::spawn(async {
            per_task("tiny: await a oneshot", |rx| async move {
                let _ = rx.await;
            })
            .await;
            per_task("with a 1 KiB buffer alive across the await", |rx| async move {
                let buf = [7u8; 1024];
                let _ = rx.await;
                std::hint::black_box(&buf);
            })
            .await;
            per_task("with a 16 KiB buffer alive across the await", |rx| async move {
                let buf = [7u8; 16 * 1024];
                let _ = rx.await;
                std::hint::black_box(&buf);
            })
            .await;
        })
        .await
        .unwrap();
    });
}
