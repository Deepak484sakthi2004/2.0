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
use std::collections::{HashMap, HashSet};
use std::mem::size_of;

fn main() {
    println!(
        "size_of HashMap<u64,u64> = {}, HashSet<u64> = {}, Vec<u64> = {}",
        size_of::<HashMap<u64, u64>>(),
        size_of::<HashSet<u64>>(),
        size_of::<Vec<u64>>()
    );

    let mut caps = Vec::with_capacity(32);
    let mut m: HashMap<u64, u64> = HashMap::new();
    caps.push(m.capacity());
    let ((), allocs, frees) = measure(|| {
        for k in 0..1000u64 {
            m.insert(k, k);
            if m.capacity() != *caps.last().unwrap() {
                caps.push(m.capacity());
            }
        }
    });
    println!("1000 inserts: capacities {caps:?}");
    println!("              {allocs} allocations, {frees} frees");

    let (pre, allocs, _) = measure(|| {
        let mut m: HashMap<u64, u64> = HashMap::with_capacity(1000);
        for k in 0..1000u64 {
            m.insert(k, k);
        }
        m
    });
    println!("with_capacity(1000): capacity {}, {allocs} allocation", pre.capacity());

    // clear() keeps the table; shrink_to_fit() gives it back.
    let mut big: HashMap<u64, u64> = (0..100_000u64).map(|k| (k, k)).collect();
    big.clear();
    println!("after clear(): len {}, capacity {}", big.len(), big.capacity());
    big.shrink_to_fit();
    println!("after shrink_to_fit(): capacity {}", big.capacity());

    // Iteration order: unspecified, and different for every RandomState.
    let keys: Vec<u64> = (0..12).collect();
    let a: HashSet<u64> = keys.iter().copied().collect();
    let b: HashSet<u64> = keys.iter().copied().collect();
    let order_a: Vec<u64> = a.iter().copied().collect();
    let order_b: Vec<u64> = b.iter().copied().collect();
    println!("set A iterates {order_a:?}");
    println!("set B iterates {order_b:?}");
    println!("same contents: {}, same order: {}", a == b, order_a == order_b);
}
