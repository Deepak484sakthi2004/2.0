// verify: debug error:invalid_atomic_ordering
// Orderings that make no sense for an operation are rejected at compile time when written as literals
// (the deny-by-default `invalid_atomic_ordering` lint). Four such mistakes, four errors.
use std::sync::atomic::{fence, AtomicU32, Ordering};

fn main() {
    let a = AtomicU32::new(0);
    let _ = a.load(Ordering::Release); // a load can't release anything
    a.store(1, Ordering::Acquire); // a store can't acquire anything
    let _ = a.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Release); // a failed CAS doesn't write
    fence(Ordering::Relaxed); // a fence that orders nothing
}
