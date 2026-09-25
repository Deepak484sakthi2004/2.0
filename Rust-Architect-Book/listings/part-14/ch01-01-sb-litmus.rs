// verify: release ok
use std::hint::spin_loop;
use std::sync::atomic::{fence, AtomicU32, AtomicUsize, Ordering::*};
use std::thread;

const N: usize = 200_000;

#[derive(Clone, Copy)]
enum Variant { Relaxed, ReleaseAcquire, SeqCst, SeqCstFence }

/// One side of the store-buffering (SB) litmus test: write my flag, then read yours.
fn side(v: Variant, mine: &[AtomicU32], theirs: &[AtomicU32], arrived: &AtomicUsize) -> Vec<u32> {
    let mut seen = vec![0u32; N];
    for i in 0..N {
        // Two-thread spin barrier: both threads start round i at (almost) the same moment.
        arrived.fetch_add(1, SeqCst);
        while arrived.load(SeqCst) < 2 * (i + 1) {
            spin_loop();
        }
        seen[i] = match v {
            Variant::Relaxed => { mine[i].store(1, Relaxed); theirs[i].load(Relaxed) }
            Variant::ReleaseAcquire => { mine[i].store(1, Release); theirs[i].load(Acquire) }
            Variant::SeqCst => { mine[i].store(1, SeqCst); theirs[i].load(SeqCst) }
            Variant::SeqCstFence => { mine[i].store(1, Relaxed); fence(SeqCst); theirs[i].load(Relaxed) }
        };
    }
    seen
}

fn run(v: Variant) -> usize {
    // A fresh (x, y) pair per round, so nothing has to be reset between rounds.
    let xs: Vec<AtomicU32> = (0..N).map(|_| AtomicU32::new(0)).collect();
    let ys: Vec<AtomicU32> = (0..N).map(|_| AtomicU32::new(0)).collect();
    let arrived = AtomicUsize::new(0);
    let (r1, r2) = thread::scope(|s| {
        let a = s.spawn(|| side(v, &xs, &ys, &arrived)); // x = 1; r1 = y
        let b = s.spawn(|| side(v, &ys, &xs, &arrived)); // y = 1; r2 = x
        (a.join().unwrap(), b.join().unwrap())
    });
    r1.iter().zip(&r2).filter(|&(a, b)| *a == 0 && *b == 0).count()
}

fn main() {
    let variants = [
        ("Relaxed", Variant::Relaxed),
        ("Release/Acquire", Variant::ReleaseAcquire),
        ("SeqCst", Variant::SeqCst),
        ("Relaxed + fence(SeqCst)", Variant::SeqCstFence),
    ];
    for (name, v) in variants {
        let both_zero = run(v);
        println!("{name:<24} r1 == r2 == 0 in {both_zero:>6} of {N} rounds");
    }
}
