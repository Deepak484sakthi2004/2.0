// verify: debug miri-ok
use std::sync::atomic::{AtomicU32, Ordering::{self, *}};
use std::thread;

/// IRIW: two independent writers, two readers reading in opposite orders.
/// Weak outcome: reader 1 sees x then not-yet y; reader 2 sees y then not-yet x (they disagree on the order).
fn iriw(st: Ordering, ld: Ordering) -> bool {
    let (x, y) = (AtomicU32::new(0), AtomicU32::new(0));
    thread::scope(|s| {
        s.spawn(|| x.store(1, st));
        s.spawn(|| y.store(1, st));
        let r1 = s.spawn(|| { let a = x.load(ld); let b = y.load(ld); (a, b) });
        let r2 = s.spawn(|| { let c = y.load(ld); let d = x.load(ld); (c, d) });
        let ((a, b), (c, d)) = (r1.join().unwrap(), r2.join().unwrap());
        a == 1 && b == 0 && c == 1 && d == 0
    })
}

fn main() {
    const TRIALS: usize = 40;
    for (name, st, ld) in [("Release/Acquire", Release, Acquire), ("SeqCst", SeqCst, SeqCst)] {
        let weak = (0..TRIALS).filter(|_| iriw(st, ld)).count();
        println!("IRIW {name:<16} readers disagree in {weak} of {TRIALS} trials");
    }
}
