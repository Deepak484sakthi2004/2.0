// verify: debug miri Data race detected
// Rust's read_volatile/write_volatile are NOT Java's `volatile`. They exist for memory-mapped I/O: they stop
// the compiler from eliding or merging the access, but they are not atomic and order nothing between threads.
use std::cell::UnsafeCell;
use std::ptr;
use std::thread;

struct Mailbox {
    ready: UnsafeCell<bool>,
    payload: UnsafeCell<u64>,
}

// SAFETY: WRONG. Nothing here synchronizes the two threads.
unsafe impl Sync for Mailbox {}

fn main() {
    let m = Mailbox { ready: UnsafeCell::new(false), payload: UnsafeCell::new(0) };
    let m = &m; // move closures below capture this reference (not the individual UnsafeCell fields)
    let got = thread::scope(|s| {
        s.spawn(move || unsafe {
            *m.payload.get() = 42;
            ptr::write_volatile(m.ready.get(), true); // "volatile store", Java-style... but not in Rust
        });
        s.spawn(move || unsafe {
            while !ptr::read_volatile(m.ready.get()) {
                std::hint::spin_loop();
            }
            *m.payload.get()
        })
        .join()
        .unwrap()
    });
    println!("payload = {got}");
}
