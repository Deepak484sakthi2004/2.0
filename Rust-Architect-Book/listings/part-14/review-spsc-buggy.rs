// verify: debug miri Data race detected
// verify: release ok
// The PR under review: a single-producer/single-consumer ring for Meridian's market-data fan-out
// (feed-handler thread → fan-out thread). Its x86 CI is green. Find the defects before reading on.
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::thread;

pub struct Ring<T> {
    buf: Box<[UnsafeCell<MaybeUninit<T>>]>,
    head: AtomicUsize, // next slot to read
    tail: AtomicUsize, // next slot to write
}

// x86 is TSO, so Relaxed is fine here and saves the fences.
unsafe impl<T> Sync for Ring<T> {}

impl<T> Ring<T> {
    pub fn with_capacity(cap: usize) -> Self {
        let buf = (0..cap).map(|_| UnsafeCell::new(MaybeUninit::uninit())).collect();
        Ring { buf, head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }

    pub fn push(&self, v: T) -> Result<(), T> {
        let tail = self.tail.load(Relaxed);
        let head = self.head.load(Relaxed);
        if tail - head == self.buf.len() {
            return Err(v);
        }
        unsafe { (*self.buf[tail % self.buf.len()].get()).write(v) };
        self.tail.store(tail + 1, Relaxed);
        Ok(())
    }

    pub fn pop(&self) -> Option<T> {
        let head = self.head.load(Relaxed);
        let tail = self.tail.load(Relaxed);
        if head == tail {
            return None;
        }
        let v = unsafe { (*self.buf[head % self.buf.len()].get()).assume_init_read() };
        self.head.store(head + 1, Relaxed);
        Some(v)
    }
}

const N: u64 = if cfg!(miri) { 50 } else { 1_000_000 };

fn main() {
    let ring = Ring::with_capacity(64);
    let received = thread::scope(|s| {
        s.spawn(|| {
            for seq in 0..N {
                let mut quote = (seq, format!("MRDN,{}", 12_500 + seq % 100));
                while let Err(q) = ring.push(quote) {
                    quote = q;
                    std::hint::spin_loop();
                }
            }
        });
        let mut next = 0;
        while next < N {
            if let Some((seq, _text)) = ring.pop() {
                assert_eq!(seq, next, "out of order");
                next += 1;
            }
        }
        next
    });
    println!("received {received} quotes in order");
}
