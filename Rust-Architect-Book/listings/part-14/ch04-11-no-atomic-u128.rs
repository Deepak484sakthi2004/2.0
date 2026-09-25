// verify: debug error:E0432
// Double-width CAS (pointer + tag in one 128-bit word) needs `lock cmpxchg16b`, which is not in the baseline
// x86-64 target. std doesn't provide AtomicU128 for that target at all.
use std::sync::atomic::AtomicU128;

fn main() {
    let head = AtomicU128::new(0);
    let _ = head;
}
