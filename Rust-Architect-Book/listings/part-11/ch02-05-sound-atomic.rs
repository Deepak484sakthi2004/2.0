// verify: debug ok
// verify: debug miri-ok
//! The same counter with an atomic: Sync is derived automatically, no `unsafe`, and Miri is satisfied.
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

struct Stats {
    hits: AtomicU64,
}

fn main() {
    let stats = Stats { hits: AtomicU64::new(0) };
    thread::scope(|s| {
        for _ in 0..2 {
            s.spawn(|| {
                for _ in 0..100 {
                    // Relaxed is enough for a counter read after the scope joins (Chapter 1.3, Part XIV).
                    stats.hits.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    println!("hits = {}", stats.hits.load(Ordering::Relaxed));
}
