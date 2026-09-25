// verify: debug panic there is no such thing as a relaxed fence
use std::hint::black_box;
use std::sync::atomic::{fence, Ordering};

fn main() {
    let o = black_box(Ordering::Relaxed); // the lint can't see through a run-time value...
    fence(o); // ...so the same mistake is caught here, as a panic
}
