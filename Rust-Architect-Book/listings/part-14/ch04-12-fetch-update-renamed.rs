// verify: debug ok
// verify: debug+nightly error:deprecated
// [VERSION] fetch_update is being renamed try_update (and an infallible `update` was added). On stable 1.98.1
// all three compile cleanly; on nightly, fetch_update is already deprecated, so with warnings denied (as in
// Meridian's CI, Chapter 8.1) the old name stops building as soon as that deprecation reaches stable.
#![deny(deprecated)]
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

fn main() {
    let max = AtomicU64::new(5);
    let _ = max.fetch_update(Relaxed, Relaxed, |cur| (9 > cur).then_some(9));
    let _ = max.try_update(Relaxed, Relaxed, |cur| (7 > cur).then_some(7)); // the new name
    let old = max.update(Relaxed, Relaxed, |x| x + 1); // the new infallible form
    println!("{old} {}", max.load(Relaxed));
}
