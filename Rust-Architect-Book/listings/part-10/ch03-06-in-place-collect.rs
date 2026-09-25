// verify: debug ok
// verify: release ok
// [LIB] In-place collect: vec.into_iter().<adapters>.collect::<Vec<_>>() can reuse the source buffer.
// Measured with a counting allocator; "same buffer" = the output's pointer equals the source's.

// --- instrumentation: count heap allocations (GlobalAlloc is explained in Part XV) ---
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    // SAFETY: both methods forward their exact arguments to `System`, which upholds the
    // GlobalAlloc contract; the counter is a plain atomic, so counting never allocates.
    // (GlobalAlloc's default `realloc` calls our `alloc` + copy + `dealloc`, so a shrinking
    // reallocation is counted as one allocation.)
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn allocs() -> usize {
        ALLOCS.load(Relaxed)
    }
}

fn report<T, U>(label: &str, src: Vec<T>, f: impl FnOnce(std::vec::IntoIter<T>) -> Vec<U>) {
    let (src_ptr, src_cap) = (src.as_ptr() as usize, src.capacity());
    let before = counting::allocs();
    let out = f(src.into_iter());
    let allocs = counting::allocs() - before;
    let same = out.as_ptr() as usize == src_ptr;
    println!(
        "{label:<40} allocs={allocs}  same buffer={same:<5}  len={:<7} cap={:<7} (source cap {src_cap})",
        out.len(),
        out.capacity()
    );
}

fn main() {
    let n = 1_000_000;
    report("u64 -> u64 via map", (0..n as u64).collect(), |it| it.map(|x| x * 2).collect::<Vec<u64>>());
    report("u64 -> u64 via filter (keeps 1%)", (0..n as u64).collect(), |it| {
        it.filter(|x| x % 100 == 0).collect::<Vec<u64>>()
    });
    report("u64 -> u32 via map", (0..n as u64).collect(), |it| it.map(|x| x as u32).collect::<Vec<u32>>());
    report("(u32, u32) -> u32 via map", (0..n as u32).map(|x| (x, x)).collect(), |it| {
        it.map(|(a, b)| a + b).collect::<Vec<u32>>()
    });
    report("u32 -> u64 via map (grows)", (0..n as u32).collect(), |it| it.map(|x| x as u64).collect::<Vec<u64>>());
    report("String -> usize via map (len)", (0..1_000).map(|i| i.to_string()).collect(), |it| {
        it.map(|s| s.len()).collect::<Vec<usize>>()
    });
    report("u64 -> u64 via filter, shrink_to_fit", (0..n as u64).collect(), |it| {
        let mut v = it.filter(|x| x % 100 == 0).collect::<Vec<u64>>();
        v.shrink_to_fit();
        v
    });
}
