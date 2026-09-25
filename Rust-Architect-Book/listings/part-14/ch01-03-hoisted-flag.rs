// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
// Three ways to wait for a flag. Only the atomic one is a correct cross-thread wait.
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// A plain `&bool`: nothing says another thread may change it, so the compiler may read it ONCE.
#[inline(never)]
pub fn wait_plain(flag: &bool) {
    while !*flag {}
}

static mut STOP: bool = false;

/// A `static mut` read by value (allowed in edition 2024; references to it are not): the same problem.
#[inline(never)]
pub fn wait_static_mut() {
    // SAFETY: none, if another thread writes STOP concurrently: that's a data race (UB).
    while !unsafe { STOP } {}
}

/// An atomic, even with Relaxed ordering: every iteration performs a real load.
#[inline(never)]
pub fn wait_atomic(flag: &AtomicBool) {
    while !flag.load(Ordering::Relaxed) {}
}

#[inline(never)]
pub fn stop() {
    // SAFETY: see wait_static_mut.
    unsafe { STOP = true }
}

/// Progress reporting through a plain `&mut`: the compiler may keep `i` in a register and store once.
#[inline(never)]
pub fn copy_with_progress_plain(src: &[u64], dst: &mut [u64], progress: &mut usize) {
    for (i, (d, s)) in dst.iter_mut().zip(src).enumerate() {
        *d = *s;
        *progress = i + 1;
    }
}

/// Through an atomic (Relaxed): every store is kept, so another thread can watch progress.
#[inline(never)]
pub fn copy_with_progress_atomic(src: &[u64], dst: &mut [u64], progress: &AtomicUsize) {
    for (i, (d, s)) in dst.iter_mut().zip(src).enumerate() {
        *d = *s;
        progress.store(i + 1, Ordering::Relaxed);
    }
}
