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

// --- instrumentation: a BuildHasher that counts how many hashes are computed ---
mod hashcount {
    use std::cell::Cell;
    use std::hash::{BuildHasher, DefaultHasher, Hasher, RandomState};

    thread_local! { static HASHES: Cell<usize> = const { Cell::new(0) }; }

    #[derive(Default, Clone)]
    pub struct CountingState(RandomState);
    pub struct CountingHasher(DefaultHasher);

    impl Hasher for CountingHasher {
        fn finish(&self) -> u64 {
            HASHES.with(|c| c.set(c.get() + 1));
            self.0.finish()
        }
        fn write(&mut self, bytes: &[u8]) {
            self.0.write(bytes)
        }
    }
    impl BuildHasher for CountingState {
        type Hasher = CountingHasher;
        fn build_hasher(&self) -> CountingHasher {
            CountingHasher(self.0.build_hasher())
        }
    }
    pub fn hashes() -> usize {
        HASHES.with(|c| c.get())
    }
}

use counting::measure;
use hashcount::{hashes, CountingState};
use std::collections::HashMap;

fn main() {
    // 10_000 words, 100 distinct: the shape of logstat's per-status counting.
    let words: Vec<String> = (0..10_000).map(|i| format!("w{}", i % 100)).collect();
    let words: Vec<&str> = words.iter().map(|s| s.as_str()).collect();

    let mut m: HashMap<String, u64, CountingState> = HashMap::with_capacity_and_hasher(128, CountingState::default());
    let h0 = hashes();
    let ((), allocs, _) = measure(|| {
        for w in &words {
            if let Some(c) = m.get_mut(*w) {
                *c += 1;
            } else {
                m.insert(w.to_string(), 1);
            }
        }
    });
    println!("get_mut + insert:        {:>6} hashes, {allocs:>6} allocations", hashes() - h0);

    let mut m: HashMap<String, u64, CountingState> = HashMap::with_capacity_and_hasher(128, CountingState::default());
    let h0 = hashes();
    let ((), allocs, _) = measure(|| {
        for w in &words {
            *m.entry(w.to_string()).or_insert(0) += 1;
        }
    });
    println!("entry(w.to_string()):    {:>6} hashes, {allocs:>6} allocations", hashes() - h0);

    let mut m: hashbrown::HashMap<String, u64, CountingState> =
        hashbrown::HashMap::with_capacity_and_hasher(128, CountingState::default());
    let h0 = hashes();
    let ((), allocs, _) = measure(|| {
        for w in &words {
            *m.entry_ref(*w).or_insert(0) += 1;
        }
    });
    println!("hashbrown entry_ref(w):  {:>6} hashes, {allocs:>6} allocations", hashes() - h0);

    // Growth: every resize re-hashes every key already in the table (hashbrown stores no hashes).
    let mut g: HashMap<u64, u64, CountingState> = HashMap::default();
    let h0 = hashes();
    for k in 0..1000u64 {
        g.insert(k, k);
    }
    println!("1000 inserts from empty: {:>6} hashes (1000 for the inserts, the rest are resize rehashes)", hashes() - h0);
    let mut g: HashMap<u64, u64, CountingState> = HashMap::with_capacity_and_hasher(1000, CountingState::default());
    let h0 = hashes();
    for k in 0..1000u64 {
        g.insert(k, k);
    }
    println!("1000 inserts, presized:  {:>6} hashes", hashes() - h0);
}
