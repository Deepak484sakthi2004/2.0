// verify: debug miri-ok
use std::sync::atomic::{AtomicU32, Ordering::*};
use std::thread;

/// One MP round: returns (flag seen, data seen). Orderings are parameters.
fn mp_round(store_flag: std::sync::atomic::Ordering, load_flag: std::sync::atomic::Ordering) -> (u32, u32) {
    let data = AtomicU32::new(0);
    let flag = AtomicU32::new(0);
    thread::scope(|s| {
        s.spawn(|| {
            data.store(1, Relaxed);
            flag.store(1, store_flag);
        });
        s.spawn(|| {
            while flag.load(load_flag) == 0 {
                std::hint::spin_loop();
            }
            (1, data.load(Relaxed))
        })
        .join()
        .unwrap()
    })
}

fn sb_round(o: std::sync::atomic::Ordering) -> (u32, u32) {
    let (x, y) = (AtomicU32::new(0), AtomicU32::new(0));
    thread::scope(|s| {
        let a = s.spawn(|| { x.store(1, o); y.load(o) });
        let b = s.spawn(|| { y.store(1, o); x.load(o) });
        (a.join().unwrap(), b.join().unwrap())
    })
}

fn main() {
    const TRIALS: usize = 40;
    let stale = (0..TRIALS).filter(|_| mp_round(Relaxed, Relaxed).1 == 0).count();
    println!("MP  Relaxed flag:          data == 0 after flag == 1 in {stale} of {TRIALS} trials");
    let stale = (0..TRIALS).filter(|_| mp_round(Release, Acquire).1 == 0).count();
    println!("MP  Release/Acquire flag:  data == 0 after flag == 1 in {stale} of {TRIALS} trials");
    for (name, o) in [("Relaxed", Relaxed), ("SeqCst", SeqCst)] {
        let both = (0..TRIALS).filter(|_| sb_round(o) == (0, 0)).count();
        println!("SB  {name:<8} r1 == r2 == 0 in {both} of {TRIALS} trials");
    }
}
