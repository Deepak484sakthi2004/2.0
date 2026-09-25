// verify: release ok
// verify: debug miri-ok
// A LongAdder-style striped counter: each thread adds to its own cache-padded cell; readers sum the cells.
// Writes scale with threads; sum() is NOT an atomic snapshot (as java.util.concurrent.atomic.LongAdder
// documents for its own sum()).
use crossbeam::utils::CachePadded;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::thread;
use std::time::Instant;

pub struct StripedCounter {
    cells: Box<[CachePadded<AtomicU64>]>,
}

static NEXT_THREAD: AtomicUsize = AtomicUsize::new(0);
thread_local! {
    // Each thread gets a small index once; LongAdder instead hashes a per-thread probe and grows its
    // cell array when CAS failures show contention.
    static THREAD_INDEX: usize = NEXT_THREAD.fetch_add(1, Relaxed);
}

impl StripedCounter {
    pub fn new(stripes: usize) -> Self {
        StripedCounter { cells: (0..stripes).map(|_| CachePadded::new(AtomicU64::new(0))).collect() }
    }
    pub fn add(&self, n: u64) {
        let i = THREAD_INDEX.with(|i| *i) % self.cells.len();
        self.cells[i].fetch_add(n, Relaxed);
    }
    /// Sums the cells one by one: concurrent adds may or may not be included.
    pub fn sum(&self) -> u64 {
        self.cells.iter().map(|c| c.load(Relaxed)).sum()
    }
}

const THREADS: u64 = 4;
const PER_THREAD: u64 = if cfg!(miri) { 20 } else { 5_000_000 };

fn main() {
    let single = AtomicU64::new(0);
    let t = Instant::now();
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| (0..PER_THREAD).for_each(|_| { single.fetch_add(1, Relaxed); }));
        }
    });
    let single_ms = t.elapsed().as_secs_f64() * 1e3;

    let striped = StripedCounter::new(8);
    let t = Instant::now();
    let mid_sums: Vec<u64> = thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| (0..PER_THREAD).for_each(|_| striped.add(1)));
        }
        // A metrics scrape while the writers run: each value is "somewhere in between".
        (0..3).map(|_| { thread::yield_now(); striped.sum() }).collect()
    });
    let striped_ms = t.elapsed().as_secs_f64() * 1e3;

    println!("single AtomicU64: {} in {single_ms:.0} ms", single.load(Relaxed));
    println!("striped (8 cells): {} in {striped_ms:.0} ms", striped.sum());
    println!("sums read during the run: {mid_sums:?}");
}
