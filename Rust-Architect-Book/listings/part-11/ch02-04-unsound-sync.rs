// verify: debug miri Data race
//! `unsafe impl Sync` is a promise the compiler cannot check. Break it, and two threads race on a Cell:
//! the program compiles and runs, and Miri reports the data race as Undefined Behavior.
use std::cell::Cell;
use std::thread;

struct Stats {
    hits: Cell<u64>,
}

// WRONG: Cell has no synchronization. This impl claims `&Stats` may be shared across threads anyway.
// SAFETY: (none: this is the bug the listing demonstrates)
unsafe impl Sync for Stats {}

fn main() {
    let stats = Stats { hits: Cell::new(0) };
    let shared = &stats; // capture the reference itself: a `move` closure stops at the deref, so the
                         // bound checked is `Stats: Sync` (the unsafe impl), not `Cell<u64>: Sync`
    thread::scope(|s| {
        for _ in 0..2 {
            s.spawn(move || {
                for _ in 0..100 {
                    shared.hits.set(shared.hits.get() + 1); // read-modify-write, unsynchronized
                }
            });
        }
    });
    println!("hits = {}", stats.hits.get());
}
