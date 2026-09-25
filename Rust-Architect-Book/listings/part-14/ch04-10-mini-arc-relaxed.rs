// verify: debug miri Data race detected
// verify: release ok
// MiniArc with Relaxed everywhere and no fence. It "works" on x86, but the last dropper's free is not
// ordered after the other threads' reads: a data race between a read and the deallocation.
use std::ops::Deref;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::thread;

struct Inner<T> {
    refs: AtomicUsize,
    data: T,
}

pub struct MiniArc<T> {
    ptr: NonNull<Inner<T>>,
}

// SAFETY: like Arc: shared &T across threads needs T: Sync; dropping T on any thread needs T: Send.
unsafe impl<T: Send + Sync> Send for MiniArc<T> {}
unsafe impl<T: Send + Sync> Sync for MiniArc<T> {}

impl<T> MiniArc<T> {
    pub fn new(data: T) -> Self {
        let b = Box::new(Inner { refs: AtomicUsize::new(1), data });
        MiniArc { ptr: NonNull::from(Box::leak(b)) }
    }
}

impl<T> Clone for MiniArc<T> {
    fn clone(&self) -> Self {
        // Relaxed: creating a new reference from an existing one needs no ordering; the count only has
        // to be atomic. (std also aborts on overflow; omitted here.)
        unsafe { self.ptr.as_ref() }.refs.fetch_add(1, Relaxed);
        MiniArc { ptr: self.ptr }
    }
}

impl<T> Deref for MiniArc<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &unsafe { self.ptr.as_ref() }.data // SAFETY: refs > 0 while we exist
    }
}

impl<T> Drop for MiniArc<T> {
    fn drop(&mut self) {
        // Release: this thread's uses of `data` happen-before whoever frees it.
        if unsafe { self.ptr.as_ref() }.refs.fetch_sub(1, Relaxed) != 1 { // BUG: not Release
            return;
        }
        // Acquire fence: synchronizes with every earlier Release decrement (they form a release sequence
        // on `refs`), so ALL other threads' uses happen-before the free below.
        // BUG: no fence(Acquire) here
        drop(unsafe { Box::from_raw(self.ptr.as_ptr()) }); // SAFETY: we were the last reference
    }
}

fn main() {
    let prices = MiniArc::new(vec![101u64, 102, 103, 104]);
    let sums: Vec<u64> = thread::scope(|s| {
        let hs: Vec<_> = (0..3)
            .map(|_| {
                let mine = prices.clone();
                s.spawn(move || mine.iter().sum::<u64>()) // reads, then drops `mine` on that thread
            })
            .collect();
        drop(prices); // main's reference may not be the last one
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    println!("sums = {sums:?}");
}
