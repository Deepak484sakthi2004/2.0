// verify: debug miri-ok
// verify: release ok
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::thread;

const THREADS: u64 = 4;
const PER_THREAD: u64 = if cfg!(miri) { 10 } else { 1_000_000 };

fn main() {
    let hits = AtomicU64::new(0);
    let before_spawn = String::from("config loaded"); // written BEFORE the threads exist
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                // spawn edge: everything the parent did before spawn() happens-before this line.
                assert_eq!(before_spawn, "config loaded");
                for _ in 0..PER_THREAD {
                    // Relaxed: atomic (no lost updates), but orders nothing else.
                    hits.fetch_add(1, Relaxed);
                }
            });
        }
    }); // join edge: every thread's last action happens-before scope() returns.
    // A Relaxed load is enough here: the joins already ordered all increments before it.
    println!("hits = {} (expected {})", hits.load(Relaxed), THREADS * PER_THREAD);
}
