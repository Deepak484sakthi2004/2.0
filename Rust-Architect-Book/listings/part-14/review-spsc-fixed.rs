// verify: debug miri-ok
// verify: release ok
// verify: debug test
// The SPSC ring after review: typed producer/consumer handles, Release/Acquire publication in both
// directions, head and tail on separate cache lines, T: Send, power-of-two capacity, and a Drop that
// drops unconsumed items.
use crossbeam::utils::CachePadded;
use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering::{Acquire, Relaxed, Release}};
use std::sync::Arc;
use std::thread;

struct Ring<T> {
    head: CachePadded<AtomicUsize>, // next slot to read; written only by the Consumer
    tail: CachePadded<AtomicUsize>, // next slot to write; written only by the Producer
    mask: usize,
    buf: Box<[UnsafeCell<MaybeUninit<T>>]>,
}

// SAFETY: each slot is accessed by one side at a time. The producer owns slot i until it publishes
// tail = i + 1 (Release); the consumer owns it after observing that tail (Acquire) until it publishes
// head = i + 1 (Release), after which the producer may reuse it (its Acquire load of head).
// Values cross from the producer's thread to the consumer's, hence T: Send.
unsafe impl<T: Send> Sync for Ring<T> {}

pub struct Producer<T> {
    ring: Arc<Ring<T>>,
}
pub struct Consumer<T> {
    ring: Arc<Ring<T>>,
}

/// The only way to get handles: exactly one Producer and one Consumer, neither Clone.
pub fn spsc<T>(capacity: usize) -> (Producer<T>, Consumer<T>) {
    assert!(capacity.is_power_of_two(), "capacity must be a power of two");
    let buf = (0..capacity).map(|_| UnsafeCell::new(MaybeUninit::uninit())).collect();
    let ring = Arc::new(Ring {
        head: CachePadded::new(AtomicUsize::new(0)),
        tail: CachePadded::new(AtomicUsize::new(0)),
        mask: capacity - 1,
        buf,
    });
    (Producer { ring: ring.clone() }, Consumer { ring })
}

impl<T> Producer<T> {
    pub fn push(&mut self, v: T) -> Result<(), T> {
        let r = &*self.ring;
        let tail = r.tail.load(Relaxed); // only this handle writes tail
        let head = r.head.load(Acquire); // consumer's reads of old slots happen-before our overwrite
        if tail.wrapping_sub(head) == r.buf.len() {
            return Err(v); // full
        }
        unsafe { (*r.buf[tail & r.mask].get()).write(v) }; // SAFETY: slot owned by the producer (see above)
        r.tail.store(tail.wrapping_add(1), Release); // publish the slot's contents
        Ok(())
    }
}

impl<T> Consumer<T> {
    pub fn pop(&mut self) -> Option<T> {
        let r = &*self.ring;
        let head = r.head.load(Relaxed); // only this handle writes head
        let tail = r.tail.load(Acquire); // producer's write of the slot happens-before our read
        if head == tail {
            return None; // empty
        }
        let v = unsafe { (*r.buf[head & r.mask].get()).assume_init_read() }; // SAFETY: published, owned by us
        r.head.store(head.wrapping_add(1), Release); // hand the slot back to the producer
        Some(v)
    }
}

impl<T> Drop for Ring<T> {
    fn drop(&mut self) {
        // &mut self: both handles are gone, so there is no concurrency left.
        let (mut i, tail) = (*self.head.get_mut(), *self.tail.get_mut());
        while i != tail {
            unsafe { self.buf[i & self.mask].get_mut().assume_init_drop() }; // SAFETY: published, unconsumed
            i = i.wrapping_add(1);
        }
    }
}

const N: u64 = if cfg!(miri) { 50 } else { 1_000_000 };

fn main() {
    let (mut tx, mut rx) = spsc(64);
    let received = thread::scope(|s| {
        s.spawn(move || {
            for seq in 0..N {
                let mut quote = (seq, format!("MRDN,{}", 12_500 + seq % 100));
                while let Err(q) = tx.push(quote) {
                    quote = q;
                    std::hint::spin_loop();
                }
            }
        });
        let mut next = 0;
        while next < N {
            if let Some((seq, _text)) = rx.pop() {
                assert_eq!(seq, next, "out of order");
                next += 1;
            }
        }
        next
    });
    println!("received {received} quotes in order");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct Counted;
    impl Drop for Counted {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Relaxed);
        }
    }

    #[test]
    fn full_empty_and_unconsumed_items_are_dropped() {
        let (mut tx, mut rx) = spsc(4);
        for _ in 0..4 {
            assert!(tx.push(Counted).is_ok());
        }
        assert!(tx.push(Counted).is_err()); // full: the rejected item comes back and is dropped here
        assert_eq!(DROPS.load(Relaxed), 1);
        drop(rx.pop()); // consumed and dropped
        assert_eq!(DROPS.load(Relaxed), 2);
        drop((tx, rx)); // 3 unconsumed items dropped by Ring::drop
        assert_eq!(DROPS.load(Relaxed), 5);
    }

    #[test]
    fn wraps_around_many_times() {
        let (mut tx, mut rx) = spsc(8);
        for i in 0..1000u32 {
            tx.push(i).unwrap();
            assert_eq!(rx.pop(), Some(i));
        }
        assert_eq!(rx.pop(), None);
    }
}
