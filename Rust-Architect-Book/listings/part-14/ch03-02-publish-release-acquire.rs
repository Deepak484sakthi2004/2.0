// verify: debug miri-ok
// verify: release ok
// "Publish a buffer via a counter", fixed: a Release store of the counter pairs with an Acquire load.
// Everything written before the Release store is visible after an Acquire load that reads its value.
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering::{Acquire, Release}};
use std::thread;

const CAP: usize = 8;

pub struct Batch {
    slots: [UnsafeCell<u64>; CAP],
    published: AtomicUsize, // slots[..published] are "ready"
}

// SAFETY (claimed): slot i is written only before `published` covers it, and read only after.
// The Release store / Acquire load pair provides the happens-before edge from each write to each read.
unsafe impl Sync for Batch {}

impl Batch {
    pub fn new() -> Self {
        Batch { slots: std::array::from_fn(|_| UnsafeCell::new(0)), published: AtomicUsize::new(0) }
    }
    /// Single writer: fill slot i, then announce it.
    pub fn push(&self, i: usize, v: u64) {
        unsafe { *self.slots[i].get() = v };
        self.published.store(i + 1, Release); // everything above is published with this store
    }
    /// Any reader: sum the announced prefix.
    pub fn sum_published(&self) -> (usize, u64) {
        let n = self.published.load(Acquire); // ...and visible to everything below this load
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
