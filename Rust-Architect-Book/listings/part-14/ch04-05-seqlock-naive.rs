// verify: debug miri Data race detected
// verify: release ok
// The "obvious" seqlock: plain (non-atomic) data, and "it's fine, torn reads are detected and retried".
// In Rust's (C++20's) memory model a racing non-atomic read is a data race: undefined behavior, even if the
// value would be thrown away. The compiler may assume it doesn't happen (e.g. re-read, or merge, the data).
use std::cell::UnsafeCell;
use std::hint::spin_loop;
use std::sync::atomic::{fence, AtomicU64, Ordering::{Acquire, Relaxed, Release}};
use std::thread;

pub struct NaiveSeqLock {
    seq: AtomicU64,
    data: UnsafeCell<(u64, u64)>, // (limit, used): NOT atomic
}

// SAFETY: WRONG. Readers read `data` while the writer may be writing it.
unsafe impl Sync for NaiveSeqLock {}

impl NaiveSeqLock {
    pub fn write(&self, limit: u64, used: u64) {
        let s = self.seq.load(Relaxed);
        self.seq.store(s + 1, Relaxed);
        fence(Release);
        unsafe { *self.data.get() = (limit, used) }; // non-atomic write
        self.seq.store(s + 2, Release);
    }

    pub fn read(&self) -> (u64, u64) {
        loop {
            let s1 = self.seq.load(Acquire);
            if s1 & 1 == 0 {
                let v = unsafe { *self.data.get() }; // non-atomic read, possibly racing with write()
                fence(Acquire);
                if self.seq.load(Relaxed) == s1 {
                    return v;
                }
            }
            spin_loop();
        }
    }
}

const WRITES: u64 = if cfg!(miri) { 30 } else { 2_000_000 };

fn main() {
    let lock = NaiveSeqLock { seq: AtomicU64::new(0), data: UnsafeCell::new((0, 0)) };
    let reads = thread::scope(|s| {
        s.spawn(|| (1..=WRITES).for_each(|i| lock.write(10 * i, 3 * i)));
        s.spawn(|| {
            let mut reads = 0u64;
            loop {
                let (limit, used) = lock.read();
                assert_eq!(used * 10, limit * 3);
                reads += 1;
                if limit == 10 * WRITES { break reads; }
            }
        })
        .join()
        .unwrap()
    });
    println!("{reads} reads, none torn (on this machine, this time)");
}
