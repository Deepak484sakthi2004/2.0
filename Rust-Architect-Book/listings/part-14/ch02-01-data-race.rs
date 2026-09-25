// verify: debug miri Data race detected
// verify: debug ok
// verify: release ok
use std::thread;

/// A raw pointer that we (falsely) promise is safe to send to another thread.
/// This `unsafe impl` is the only reason the program below compiles.
#[derive(Clone, Copy)]
struct SendPtr(*mut u64);
unsafe impl Send for SendPtr {}

const THREADS: u64 = 4;
const PER_THREAD: u64 = if cfg!(miri) { 10 } else { 1_000_000 };

fn main() {
    let mut hits: u64 = 0;
    let p = SendPtr(&mut hits);
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(move || {
                let p = p; // capture the whole SendPtr (edition 2021+ would otherwise capture only p.0)
                for _ in 0..PER_THREAD {
                    // SAFETY: NONE. Four threads do this at once: a data race, which is undefined behavior.
                    unsafe { *p.0 += 1 };
                }
            });
        }
    });
    println!("hits = {hits} (expected {})", THREADS * PER_THREAD);
}
