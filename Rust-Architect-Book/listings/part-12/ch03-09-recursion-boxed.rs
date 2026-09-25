// verify: debug ok
//! Async recursion with Box::pin: each level's state moves to the heap, one allocation per level.
//! Measured with the counting allocator from Part III.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator unchanged; only counts calls and requested bytes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Depth of a (simulated) merchant hierarchy: parent -> child -> ... n levels.
async fn depth(n: u32) -> u32 {
    if n == 0 {
        0
    } else {
        // Box::pin gives the recursive future a fixed size (a pointer), breaking the cycle.
        1 + Box::pin(depth(n - 1)).await
    }
}

fn main() {
    println!("future size: {} bytes", size_of_val(&depth(10)));
    futures::executor::block_on(depth(0)); // warm-up: block_on allocates once per thread
    for n in [1, 10, 100] {
        let (a0, b0) = (ALLOCS.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed));
        let d = futures::executor::block_on(depth(n));
        let (a, b) = (ALLOCS.load(Ordering::Relaxed) - a0, BYTES.load(Ordering::Relaxed) - b0);
        println!("depth({n:>3}) = {d:>3}: {a:>3} allocations, {b:>5} bytes");
    }
}
