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
    // (GlobalAlloc's default `realloc` calls our `alloc` + copy + `dealloc`, so every
    // reallocation is counted as one allocation and one free.)
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

/// Pushes `n` default values into an empty Vec, recording every capacity it passes through.
fn growth<T: Default>(n: usize) -> (Vec<usize>, usize) {
    let mut caps = Vec::with_capacity(64); // allocated BEFORE measuring
    let mut v: Vec<T> = Vec::new();
    caps.push(v.capacity());
    let ((), allocs, _) = measure(|| {
        for _ in 0..n {
            v.push(T::default());
            if v.capacity() != *caps.last().unwrap() {
                caps.push(v.capacity());
            }
        }
    });
    (caps, allocs)
}

#[derive(Clone, Copy)]
struct Big([u8; 2048]);
impl Default for Big {
    fn default() -> Self {
        Big([0; 2048])
    }
}

fn main() {
    let (caps, allocs) = growth::<u8>(100);
    println!("Vec<u8>,   100 pushes: capacities {caps:?}, {allocs} allocator calls");
    let (caps, allocs) = growth::<u64>(100);
    println!("Vec<u64>,  100 pushes: capacities {caps:?}, {allocs} allocator calls");
    let (caps, allocs) = growth::<Big>(10);
    println!("Vec<Big>,   10 pushes: capacities {caps:?}, {allocs} allocator calls  (Big = 2048 bytes)");

    let ((), allocs, _) = measure(|| {
        let mut v: Vec<u64> = Vec::with_capacity(100);
        for i in 0..100 {
            v.push(i);
        }
        std::hint::black_box(&v);
    });
    println!("with_capacity(100) + 100 pushes: {allocs} allocation");

    let mut v: Vec<u64> = (0..10).collect();
    let before = v.capacity();
    v.reserve(5); // "room for at least 5 more": may round up
    let after_reserve = v.capacity();
    let mut w: Vec<u64> = (0..10).collect();
    w.reserve_exact(5); // "exactly 5 more" (the allocator may still give more)
    println!("reserve(5): {before} -> {after_reserve}; reserve_exact(5): 10 -> {}", w.capacity());

    let mut big: Vec<u64> = (0..1000).collect();
    big.truncate(10);
    let cap_after_truncate = big.capacity();
    let ((), allocs, frees) = measure(|| big.shrink_to_fit());
    println!(
        "truncate(10) keeps capacity {cap_after_truncate}; shrink_to_fit -> {} ({allocs} alloc, {frees} free)",
        big.capacity()
    );

    let ((), allocs, _) = measure(|| {
        let v: Vec<u64> = Vec::new();
        std::hint::black_box(&v);
    });
    println!("Vec::new(): {allocs} allocations");
}
