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

#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    x: f64,
    y: f64,
} // Copy: plain data; a bitwise duplicate IS a complete, independent copy

fn total_len(names: Vec<String>) -> usize {
    // takes ownership
    names.iter().map(|n| n.len()).sum()
} // `names` is dropped here

fn main() {
    // Copy: the original stays usable.
    let p = Point { x: 1.0, y: 2.0 };
    let q = p;
    println!("p = {p:?}, q = {q:?}");

    // Move: ownership transfers; nothing on the heap is touched.
    let names: Vec<String> = (0..1_000).map(|i| format!("name-{i}")).collect();
    let (moved, allocs, _) = measure(|| {
        let moved = names;
        moved
    });
    println!("move a Vec<String> of 1000:  {allocs} allocation(s)");

    // Clone: an explicit deep copy.
    let (cloned, allocs, _) = measure(|| moved.clone());
    println!("clone it:                    {allocs} allocation(s)");

    // Give the clone away; it dies inside the function.
    let (len, allocs, frees) = measure(|| total_len(cloned));
    println!("total_len(cloned) = {len}: {allocs} allocation(s), {frees} free(s)");
    println!("the original is untouched: {} names", moved.len());
}
