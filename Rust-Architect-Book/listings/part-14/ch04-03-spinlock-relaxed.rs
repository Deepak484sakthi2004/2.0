// verify: debug miri Data race detected
// verify: release ok
// The same spinlock with Relaxed everywhere. Mutual exclusion still holds (the CAS is atomic), but
// nothing orders one critical section before the next: the protected data is raced on.
use std::cell::UnsafeCell;
use std::hint::spin_loop;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::thread;

pub struct SpinLock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: the lock hands out &mut T to one thread at a time, and the Acquire/Release pair orders each
// critical section after the previous one. T must be Send because the value is accessed from many threads.
unsafe impl<T: Send> Sync for SpinLock<T> {}

pub struct Guard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        SpinLock { locked: AtomicBool::new(false), value: UnsafeCell::new(value) }
    }

    pub fn lock(&self) -> Guard<'_, T> {
        let mut backoff = 1u32;
        loop {
            // Acquire on success: the previous holder's writes (before its Release) are visible to us.
            if self.locked.compare_exchange_weak(false, true, Relaxed, Relaxed).is_ok() { // BUG: not Acquire
                return Guard { lock: self };
            }
            // "Test" before "test-and-set": spin on a plain load, which keeps the cache line Shared
            // instead of bouncing it between cores with failed read-modify-writes.
            while self.locked.load(Relaxed) {
                for _ in 0..backoff {
                    spin_loop(); // x86 `pause`: tells the core this is a spin-wait
                }
                backoff = (backoff * 2).min(64);
            }
        }
    }
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.value.get() } // SAFETY: we hold the lock
    }
}
impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.value.get() } // SAFETY: we hold the lock, exclusively
    }
}
impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        // Release: our writes inside the critical section happen-before the next Acquire that sees `false`.
        self.lock.locked.store(false, Relaxed); // BUG: not Release
    }
}

const THREADS: u64 = 4;
const PER_THREAD: u64 = if cfg!(miri) { 25 } else { 200_000 };

fn main() {
    let counter = SpinLock::new(0u64); // a plain u64: only the lock makes this safe
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                for _ in 0..PER_THREAD {
                    *counter.lock() += 1;
                }
            });
        }
    });
    println!("counter = {} (expected {})", *counter.lock(), THREADS * PER_THREAD);
}
