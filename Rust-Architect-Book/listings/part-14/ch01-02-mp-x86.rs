// verify: release ok
use std::hint::spin_loop;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::*};
use std::thread;

const N: usize = 200_000;

fn main() {
    // Message passing (MP): writer does data = 1; flag = 1. Reader does r1 = flag; r2 = data.
    // The "weak" outcome is r1 == 1 && r2 == 0: the flag is seen, the data it announces is not.
    let data: Vec<AtomicU32> = (0..N).map(|_| AtomicU32::new(0)).collect();
    let flag: Vec<AtomicU32> = (0..N).map(|_| AtomicU32::new(0)).collect();
    let arrived = AtomicUsize::new(0);
    let barrier = |i: usize| {
        arrived.fetch_add(1, SeqCst);
        while arrived.load(SeqCst) < 2 * (i + 1) {
            spin_loop();
        }
    };
    let (flag_seen, weak) = thread::scope(|s| {
        s.spawn(|| {
            for i in 0..N {
                barrier(i);
                data[i].store(1, Relaxed);
                flag[i].store(1, Relaxed);
            }
        });
        let reader = s.spawn(|| {
            let (mut flag_seen, mut weak) = (0, 0);
            for i in 0..N {
                barrier(i);
                // Vary the reader's delay (0-63 spins) so rounds land on every side of the race window.
                for _ in 0..(i % 64) { spin_loop(); }
                if flag[i].load(Relaxed) == 1 {
                    flag_seen += 1;
                    if data[i].load(Relaxed) == 0 { weak += 1; }
                }
            }
            (flag_seen, weak)
        });
        reader.join().unwrap()
    });
    println!("flag seen in {flag_seen} of {N} rounds; flag == 1 but data == 0 in {weak}");
}
