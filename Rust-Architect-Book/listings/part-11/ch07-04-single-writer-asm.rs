// verify: release build
//! A counter with exactly one writer (other threads only read it) doesn't need an atomic read-modify-write.
//! Compare the release assembly of the two increments with `tools/emit.ps1 -Target asm -Mode release`.
use std::sync::atomic::{AtomicU64, Ordering};

/// Safe for concurrent readers; correct only if this is the ONLY thread that ever writes `c`.
#[inline(never)]
pub fn single_writer_inc(c: &AtomicU64) {
    c.store(c.load(Ordering::Relaxed) + 1, Ordering::Relaxed);
}

/// Correct with any number of writers.
#[inline(never)]
pub fn fetch_add_inc(c: &AtomicU64) {
    c.fetch_add(1, Ordering::Relaxed);
}
