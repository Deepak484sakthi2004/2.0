// verify: release ok
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

const THREADS: u64 = 4;
const PER_THREAD: u64 = 1_000_000;

// Fix A: make the shared counter atomic.
fn with_atomic() -> u64 {
    let counter = AtomicU64::new(0);
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                for _ in 0..PER_THREAD {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    counter.into_inner()
}

// Fix B: put the counter behind a lock.
fn with_mutex() -> u64 {
    let counter = Mutex::new(0u64);
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                for _ in 0..PER_THREAD {
                    *counter.lock().unwrap() += 1;
                }
            });
        }
    });
    counter.into_inner().unwrap()
}

// Fix C: don't share. Each thread owns a private count; combine at the end.
fn with_no_sharing() -> u64 {
    thread::scope(|s| {
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                s.spawn(|| {
                    let mut local = 0u64;
                    for _ in 0..PER_THREAD {
                        local += 1;
                    }
                    local
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).sum()
    })
}

fn main() {
    println!("atomic:     {}", with_atomic());
    println!("mutex:      {}", with_mutex());
    println!("no sharing: {}", with_no_sharing());
}
