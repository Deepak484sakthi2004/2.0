// verify: release ok
// One run on a shared machine: noisy. Compare ratios.
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let n = 4_000_000usize;
    let flat: Vec<u64> = (0..n as u64).collect();
    // Java's ArrayList<Long>: an array of references to separately allocated boxes.
    let boxed: Vec<Box<u64>> = (0..n as u64).map(Box::new).collect();
    // Same boxes, shuffled: the references no longer follow allocation order.
    let mut shuffled: Vec<Box<u64>> = (0..n as u64).map(Box::new).collect();
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    for i in (1..n).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        shuffled.swap(i, (x % (i as u64 + 1)) as usize);
    }
    let gap = (&*boxed[1] as *const u64 as usize).wrapping_sub(&*boxed[0] as *const u64 as usize);
    println!("adjacent Box<u64> allocations are {gap} bytes apart (for 8 bytes of payload)");

    for round in 0..3 {
        let t = Instant::now();
        let a: u64 = black_box(&flat).iter().sum();
        let t1 = t.elapsed();
        let t = Instant::now();
        let b: u64 = black_box(&boxed).iter().map(|b| **b).sum();
        let t2 = t.elapsed();
        let t = Instant::now();
        let c: u64 = black_box(&shuffled).iter().map(|b| **b).sum();
        let t3 = t.elapsed();
        assert!(a == b && b == c);
        println!("round {round}: Vec<u64> {t1:>9.2?} | Vec<Box<u64>> in order {t2:>9.2?} | shuffled {t3:>9.2?}");
    }
}
