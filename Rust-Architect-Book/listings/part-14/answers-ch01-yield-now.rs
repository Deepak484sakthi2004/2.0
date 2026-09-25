// verify: release build
// Answer key, Chapter 14.1 debugging exercise: does a call to thread::yield_now() in the loop body stop the
// compiler from hoisting the read of a `static mut` flag? Inspect with:
//   tools/emit.ps1 listings/part-14/answers-ch01-yield-now.rs -Target asm -Mode release
// (The program is still a data race if another thread writes READY: yield_now changes the codegen, not the
// language rules.)
use std::sync::atomic::{AtomicBool, Ordering::{Acquire, Release}};

pub static mut READY: bool = false;

/// The exercise's loop, as written.
#[inline(never)]
pub fn wait_until_ready_static_mut() {
    // SAFETY: none if another thread writes READY concurrently (a data race). Kept to inspect codegen.
    while !unsafe { READY } {
        std::thread::yield_now();
    }
}

pub static READY_ATOMIC: AtomicBool = AtomicBool::new(false);

/// The fix: an atomic flag. Release/Acquire because init() publishes the loaded config along with the flag.
#[inline(never)]
pub fn wait_until_ready_atomic() {
    while !READY_ATOMIC.load(Acquire) {
        std::thread::yield_now();
    }
}

#[inline(never)]
pub fn mark_ready() {
    READY_ATOMIC.store(true, Release);
}
