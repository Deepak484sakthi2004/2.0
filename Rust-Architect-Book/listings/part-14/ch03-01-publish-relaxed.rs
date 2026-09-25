// verify: debug miri Data race detected
// verify: release ok
// "Publish a buffer via a counter" with Relaxed: the counter is atomic, the buffer is not,
// and nothing orders the buffer writes before the reader's buffer reads.
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::thread;

const CAP: usize = 8;

pub struct Batch {
    slots: [UnsafeCell<u64>; CAP],
    published: AtomicUsize, // slots[..published] are "ready"
}

// SAFETY (claimed): slot i is written only before `published` covers it, and read only after.
// That claim needs a happens-before edge from the write to the read, and Relaxed doesn't provide one.
unsafe impl Sync for Batch {}

impl Batch {
    pub fn new() -> Self {
        Batch { slots: std::array::from_fn(|_| UnsafeCell::new(0)), published: AtomicUsize::new(0) }
    }
    /// Single writer: fill slot i, then announce it.
    pub fn push(&self, i: usize, v: u64) {
        unsafe { *self.slots[i].get() = v };
        self.published.store(i + 1, Relaxed); // BUG: should be Release
    }
    /// Any reader: sum the announced prefix.
    pub fn sum_published(&self) -> (usize, u64) {
        let n = self.published.load(Relaxed); // BUG: should be Acquire
        let sum = (0..n).map(|i| unsafe { *self.slots[i].get() }).sum();
        (n, sum)
    }
}

fn main() {
    let batch = Batch::new();
    let (n, sum) = thread::scope(|s| {
        s.spawn(|| (0..CAP).for_each(|i| batch.push(i, 100 + i as u64)));
        loop {
            let (n, sum) = batch.sum_published();
            if n == CAP { break (n, sum); }
            std::hint::spin_loop();
        }
    });
    println!("read {n} slots, sum = {sum} (expected {})", (0..CAP as u64).map(|i| 100 + i).sum::<u64>());
}
