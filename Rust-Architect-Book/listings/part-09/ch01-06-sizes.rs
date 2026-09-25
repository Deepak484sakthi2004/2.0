// verify: debug ok

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
use smallvec::SmallVec;
use std::collections::{BTreeMap, BinaryHeap, HashMap, LinkedList, VecDeque};
use std::mem::size_of;

macro_rules! show {
    ($($t:ty),* $(,)?) => { $( println!("{:>28} {:>3} bytes", stringify!($t), size_of::<$t>()); )* };
}

fn main() {
    println!("The part that lives inline (on the stack, or inside the parent struct):");
    show!([u64; 4], &[u64], Box<[u64]>, Vec<u64>, Option<Vec<u64>>, SmallVec<[u64; 4]>);
    show!(VecDeque<u64>, BinaryHeap<u64>, LinkedList<u64>, BTreeMap<u64, u64>, HashMap<u64, u64>);

    // A SmallVec stores up to N elements inline and spills to the heap after that.
    let ((), allocs, _) = measure(|| {
        let mut s: SmallVec<[u64; 4]> = SmallVec::new();
        for i in 0..4 {
            s.push(i);
        }
        std::hint::black_box(&s);
    });
    println!("SmallVec<[u64; 4]>, 4 pushes: {allocs} allocations");
    let ((), allocs, _) = measure(|| {
        let mut s: SmallVec<[u64; 4]> = SmallVec::new();
        for i in 0..5 {
            s.push(i);
        }
        std::hint::black_box(&s);
    });
    println!("SmallVec<[u64; 4]>, 5 pushes: {allocs} allocation (spilled: {})", {
        let s: SmallVec<[u64; 4]> = (0..5).collect();
        s.spilled()
    });

    // Freezing a Vec: into_boxed_slice drops the capacity word, shrinking the buffer if needed.
    let mut v: Vec<u64> = Vec::with_capacity(100);
    v.extend(0..10);
    let (b, allocs, frees) = measure(move || v.into_boxed_slice());
    println!("Vec (len 10, cap 100) -> Box<[u64]>: {allocs} alloc, {frees} free, len {}", b.len());
}
