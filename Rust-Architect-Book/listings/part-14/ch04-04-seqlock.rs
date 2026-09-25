// verify: debug miri-ok
// verify: release ok
// A sequence lock (seqlock) for a small, frequently read, rarely written record: one writer, many readers,
// readers never block the writer and never write shared memory. The data words are ATOMICS (read with
// Relaxed) so that a reader racing with the writer is not a data race; the fences make the retry check sound.
// Pattern from H.-J. Boehm, "Can Seqlocks Get Along With Programming Language Memory Models?" (MSPC 2012).
use std::hint::spin_loop;
use std::sync::atomic::{fence, AtomicU64, Ordering::{Acquire, Relaxed, Release}};
use std::thread;

pub struct SeqLock {
    seq: AtomicU64, // even: stable; odd: write in progress
    limit: AtomicU64,
    used: AtomicU64,
}

impl SeqLock {
    pub const fn new() -> Self {
        SeqLock { seq: AtomicU64::new(0), limit: AtomicU64::new(0), used: AtomicU64::new(0) }
    }

    /// Single writer only (a second writer needs its own mutual exclusion around this).
    pub fn write(&self, limit: u64, used: u64) {
        let s = self.seq.load(Relaxed);
        self.seq.store(s + 1, Relaxed); // odd: readers that overlap us will retry
        fence(Release); // the odd seq is ordered before the data stores below (pairs with the reader's fence)
        self.limit.store(limit, Relaxed);
        self.used.store(used, Relaxed);
        self.seq.store(s + 2, Release); // even again: publishes the data stores above
    }

    /// Returns a consistent (limit, used) pair and how many times it had to retry.
    pub fn read(&self) -> ((u64, u64), u32) {
        let mut retries = 0;
        loop {
            let s1 = self.seq.load(Acquire); // sees an even value ⇒ the data of that write is visible
            if s1 & 1 == 0 {
                let limit = self.limit.load(Relaxed);
                let used = self.used.load(Relaxed);
                fence(Acquire); // if we read ANY value of a newer write, the load below sees its odd seq
                if self.seq.load(Relaxed) == s1 {
                    return ((limit, used), retries);
                }
            }
            retries += 1;
            spin_loop();
        }
    }
}

const WRITES: u64 = if cfg!(miri) { 30 } else { 2_000_000 };

fn main() {
    let lock = SeqLock::new();
    let (reads, retries) = thread::scope(|s| {
        s.spawn(|| {
            for i in 1..=WRITES {
                lock.write(10 * i, 3 * i); // invariant: used * 10 == limit * 3
            }
        });
        let readers: Vec<_> = (0..2)
            .map(|_| {
                s.spawn(|| {
                    let (mut reads, mut retries) = (0u64, 0u64);
                    loop {
                        let ((limit, used), r) = lock.read();
                        assert_eq!(used * 10, limit * 3, "torn read: limit={limit} used={used}");
                        reads += 1;
                        retries += r as u64;
                        if limit == 10 * WRITES { break (reads, retries); }
                    }
                })
            })
            .collect();
        readers.into_iter().map(|h| h.join().unwrap()).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
    });
    println!("{reads} consistent reads, {retries} retries, 0 torn reads");
}
