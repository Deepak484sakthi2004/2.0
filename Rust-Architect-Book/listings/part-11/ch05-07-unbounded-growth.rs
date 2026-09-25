// verify: debug ok
//! An unbounded channel moves the overload from the producer's CPU to your heap.
//! A counting allocator (Part III pattern) reports live heap bytes while the consumer is stalled.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc;

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);

// SAFETY: every method forwards to the System allocator unchanged; we only keep a running byte count.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// An audit record: 64 bytes inline, no heap of its own.
#[allow(dead_code)]
struct Audit([u8; 64]);

fn live_kib() -> isize {
    LIVE.load(Ordering::Relaxed) / 1024
}

fn main() {
    let base = live_kib();

    let (tx, rx) = mpsc::channel::<Audit>();
    for n in 1..=100_000 {
        tx.send(Audit([0; 64])).unwrap(); // never blocks, never fails: the consumer is stalled
        if n % 25_000 == 0 {
            println!("unbounded: {n:>6} queued, live heap +{} KiB", live_kib() - base);
        }
    }
    drop(rx); // the backlog is freed only when the channel goes away
    drop(tx);
    println!("after dropping the channel: live heap +{} KiB", live_kib() - base);

    let (tx, rx) = mpsc::sync_channel::<Audit>(1_000);
    let mut refused = 0;
    for _ in 0..100_000 {
        if tx.try_send(Audit([0; 64])).is_err() {
            refused += 1; // the caller must decide: block, drop, sample, or fail the request
        }
    }
    println!("bounded(1000): accepted {}, refused {refused}, live heap +{} KiB", 100_000 - refused, live_kib() - base);
    drop(rx);
}
