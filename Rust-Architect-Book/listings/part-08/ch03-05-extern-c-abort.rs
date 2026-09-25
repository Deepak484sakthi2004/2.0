// verify: debug crash cannot unwind
use std::panic;

/// Imagine this is exported to C or Java (FFM). The "C" ABI promises the caller that no unwind comes out.
extern "C" fn score(amount_cents: i64) -> i32 {
    if amount_cents < 0 {
        panic!("negative amount {amount_cents}");
    }
    (amount_cents % 100) as i32
}

fn main() {
    println!("score(250) = {}", score(250));
    // catch_unwind can't help: the panic reaches the extern "C" boundary first, and the process aborts there.
    let r = panic::catch_unwind(|| score(-5));
    println!("never printed: {r:?}");
}
