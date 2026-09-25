// verify: release ok
// What Java's List<Long> costs, reproduced in Rust with Vec<Box<i64>>, next to Rust's Vec<i64>.
use std::hint::black_box;
use std::mem::size_of;

mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    static BYTES: AtomicUsize = AtomicUsize::new(0);
    pub struct Counting;
    // SAFETY: both methods forward their exact arguments to `System`, which upholds the
    // GlobalAlloc contract; the counters are plain atomics, so counting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size(), Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }
    #[global_allocator]
    static GLOBAL: Counting = Counting;
    /// Runs `f` and returns (result, allocations, bytes requested).
    pub fn measure<R>(f: impl FnOnce() -> R) -> (R, usize, usize) {
        let (a0, b0) = (ALLOCS.load(Relaxed), BYTES.load(Relaxed));
        let r = f();
        (r, ALLOCS.load(Relaxed) - a0, BYTES.load(Relaxed) - b0)
    }
}

/// One generic function; both calls below get their own machine code.
fn total<T: Copy + Into<i64>>(xs: &[T]) -> i64 {
    xs.iter().map(|&x| x.into()).sum()
}

const N: i64 = 1_000_000;

fn main() {
    // Rust generics: Vec<i64> stores the numbers themselves, contiguously.
    let (flat, allocs, bytes) = counting::measure(|| (0..N).collect::<Vec<i64>>());
    println!("Vec<i64>      : {allocs:>7} allocations, {bytes:>8} bytes requested");

    // Erasure-style: every element is a separate heap object, the Vec holds pointers.
    let (boxed, allocs, bytes) = counting::measure(|| (0..N).map(Box::new).collect::<Vec<Box<i64>>>());
    println!("Vec<Box<i64>> : {allocs:>7} allocations, {bytes:>8} bytes requested");

    let small: Vec<i32> = (0..1000).collect();
    println!("total::<i64> = {}", total(black_box(&flat)));
    println!("total::<i32> = {}", total(black_box(&small)));
    println!("sum via boxes = {}", boxed.iter().map(|b| **b).sum::<i64>());
    println!("size_of::<i64>() = {}, size_of::<Box<i64>>() = {}", size_of::<i64>(), size_of::<Box<i64>>());
}
