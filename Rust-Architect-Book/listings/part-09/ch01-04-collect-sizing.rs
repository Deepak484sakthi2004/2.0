// verify: debug ok
// verify: release ok

// --- instrumentation: count heap allocations and frees (GlobalAlloc is explained in Part XV) ---
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    static FREES: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    // SAFETY: both methods forward their exact arguments to `System`, which upholds the
    // GlobalAlloc contract; the counters are plain atomics, so counting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            FREES.fetch_add(1, Relaxed);
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn measure<R>(f: impl FnOnce() -> R) -> (R, usize, usize) {
        let (a0, f0) = (ALLOCS.load(Relaxed), FREES.load(Relaxed));
        let r = f();
        (r, ALLOCS.load(Relaxed) - a0, FREES.load(Relaxed) - f0)
    }
}

use counting::measure;

fn main() {
    // Exact size known up front (a Range): one allocation of exactly the right size.
    let (v, allocs, _) = measure(|| (0..10_000u64).collect::<Vec<_>>());
    println!("range.collect():                 {allocs} allocation(s), cap {}", v.capacity());

    // Unknown size (filter can drop anything): the Vec grows as it goes.
    let (w, allocs, _) = measure(|| (0..10_000u64).filter(|x| x % 3 == 0).collect::<Vec<_>>());
    println!("range.filter().collect():        {allocs} allocation(s), len {} cap {}", w.len(), w.capacity());

    // Pre-size with an upper bound you know, then extend.
    let (x, allocs, _) = measure(|| {
        let mut x = Vec::with_capacity(10_000 / 3 + 1);
        x.extend((0..10_000u64).filter(|x| x % 3 == 0));
        x
    });
    println!("with_capacity + extend(filter):  {allocs} allocation(s), len {} cap {}", x.len(), x.capacity());

    // Consuming a Vec and collecting the same element size back: std reuses the buffer.
    let (y, allocs, _) = measure(move || v.into_iter().map(|x| x * 2).collect::<Vec<u64>>());
    println!("vec.into_iter().map().collect(): {allocs} allocation(s), len {}", y.len());

    std::hint::black_box((w, x, y));
}
