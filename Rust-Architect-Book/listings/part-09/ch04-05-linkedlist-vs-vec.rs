// verify: release ok
// One run on a shared machine: noisy. Allocation counts are exact; times are indicative.

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
use std::collections::{LinkedList, VecDeque};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let n = 1_000_000u64;

    let (v, allocs, _) = measure(|| (0..n).collect::<Vec<u64>>());
    println!("Vec<u64>:        {allocs:>7} allocations for {n} elements");
    let (dq, allocs, _) = measure(|| (0..n).collect::<VecDeque<u64>>());
    println!("VecDeque<u64>:   {allocs:>7} allocations");
    let (fresh, allocs, _) = measure(|| (0..n).collect::<LinkedList<u64>>());
    println!("LinkedList<u64>: {allocs:>7} allocations (one node = prev + next + value = 24 bytes each)");

    // An "aged" list: nodes interleaved with other live allocations, as in a long-running server.
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut aged = LinkedList::new();
    let mut ballast: Vec<Box<[u8]>> = Vec::with_capacity(n as usize);
    for i in 0..n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        ballast.push(vec![0u8; 16 + (x % 200) as usize].into_boxed_slice());
        aged.push_back(i);
    }

    for round in 0..2 {
        let t = Instant::now();
        let s1: u64 = black_box(&v).iter().sum();
        let tv = t.elapsed();
        let t = Instant::now();
        let s2: u64 = black_box(&dq).iter().sum();
        let tq = t.elapsed();
        let t = Instant::now();
        let s3: u64 = black_box(&fresh).iter().sum();
        let tf = t.elapsed();
        let t = Instant::now();
        let s4: u64 = black_box(&aged).iter().sum();
        let ta = t.elapsed();
        assert!(s1 == s2 && s2 == s3 && s3 == s4);
        println!(
            "round {round}: sum Vec {tv:>9.2?} | VecDeque {tq:>9.2?} | LinkedList fresh {tf:>9.2?} | LinkedList aged {ta:>9.2?}"
        );
    }
    black_box(ballast);
}
