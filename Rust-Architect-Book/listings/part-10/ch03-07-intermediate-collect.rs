// verify: debug ok
// A fused pipeline allocates only its result; each collect() in the middle materializes a whole stage.

// --- instrumentation: count heap allocations (GlobalAlloc is explained in Part XV) ---
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

    pub fn measure<R>(f: impl FnOnce() -> R) -> (R, usize, usize) {
        let (a0, b0) = (ALLOCS.load(Relaxed), BYTES.load(Relaxed));
        let r = f();
        (r, ALLOCS.load(Relaxed) - a0, BYTES.load(Relaxed) - b0)
    }
}

#[derive(Clone, Copy)]
struct Txn {
    merchant: u32,
    amount_cents: u64,
    settled: bool,
}

fn main() {
    let txns: Vec<Txn> = (0..100_000u32)
        .map(|i| Txn { merchant: i % 500, amount_cents: (i as u64 * 37) % 50_000, settled: i % 10 != 0 })
        .collect();

    // Java-stream habit ported literally: each step collects.
    let (total_a, allocs_a, bytes_a) = counting::measure(|| {
        let settled: Vec<&Txn> = txns.iter().filter(|t| t.settled).collect();
        let big: Vec<&Txn> = settled.into_iter().filter(|t| t.amount_cents >= 10_000).collect();
        let fees: Vec<u64> = big.iter().map(|t| t.amount_cents * 290 / 10_000).collect();
        fees.iter().sum::<u64>()
    });
    // One fused pipeline.
    let (total_b, allocs_b, bytes_b) = counting::measure(|| {
        txns.iter()
            .filter(|t| t.settled && t.amount_cents >= 10_000)
            .map(|t| t.amount_cents * 290 / 10_000)
            .sum::<u64>()
    });
    println!("collect per stage: total={total_a} allocations={allocs_a} bytes={bytes_a}");
    println!("fused pipeline:    total={total_b} allocations={allocs_b} bytes={bytes_b}");
    println!("(merchants in data: {})", txns.iter().map(|t| t.merchant).max().unwrap() + 1);
}
