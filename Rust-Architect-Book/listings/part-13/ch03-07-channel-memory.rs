// verify: debug ok
//! A bounded channel's capacity is a limit, not a preallocation: tokio's mpsc allocates its queue in
//! blocks as messages arrive [LIB]. Measured with the counting allocator (Part III's instrument).
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to System with the same arguments and only adds bookkeeping.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE_BYTES.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE_BYTES.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn live() -> usize {
    LIVE_BYTES.load(Ordering::Relaxed)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let base = live();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<u64>(1_000_000);
    println!("channel(1_000_000) created:        {:>9} bytes live", live() - base);
    for i in 0..100 {
        tx.send(i).await.unwrap();
    }
    println!("after 100 messages queued:         {:>9} bytes live", live() - base);
    for i in 0..100_000 {
        tx.send(i).await.unwrap();
    }
    println!("after 100,100 messages queued:     {:>9} bytes live ({:.1} bytes per message)", live() - base, (live() - base) as f64 / 100_100.0);
    while let Ok(_) = rx.try_recv() {}
    println!("after draining the queue:          {:>9} bytes live", live() - base);

    // broadcast is different: its ring buffer of `capacity` slots is allocated when the channel is created.
    let base = live();
    let (btx, _brx) = tokio::sync::broadcast::channel::<u64>(1_000);
    println!("broadcast::channel(1_000) created: {:>9} bytes live", live() - base);
    drop(btx);
}
