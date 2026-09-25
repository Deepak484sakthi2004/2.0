// verify: release ok
// One run on a shared machine: noisy. The shape (who wins which operation) is the point.
use std::collections::{BTreeMap, HashMap};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let n = 1_000_000;
    let keys: Vec<u64> = (0..n).map(|_| next()).collect();

    let t = Instant::now();
    let hm: HashMap<u64, u64> = keys.iter().map(|&k| (k, k)).collect();
    println!("build     HashMap {:>9.2?}", t.elapsed());
    let t = Instant::now();
    let bt: BTreeMap<u64, u64> = keys.iter().map(|&k| (k, k)).collect();
    println!("build    BTreeMap {:>9.2?}", t.elapsed());

    let probes: Vec<u64> = (0..n).map(|i| keys[(i * 7919) % n]).collect();
    let t = Instant::now();
    let s: u64 = probes.iter().map(|k| hm[black_box(k)]).fold(0, u64::wrapping_add);
    println!("1M gets   HashMap {:>9.2?}  ({s:x})", t.elapsed());
    let t = Instant::now();
    let s: u64 = probes.iter().map(|k| bt[black_box(k)]).fold(0, u64::wrapping_add);
    println!("1M gets  BTreeMap {:>9.2?}  ({s:x})", t.elapsed());

    // Ordered iteration: BTreeMap is already sorted; HashMap must collect and sort.
    let t = Instant::now();
    let first: Vec<u64> = bt.keys().take(10).copied().collect();
    let all_sorted = bt.keys().count();
    println!("sorted scan BTreeMap {:>9.2?}  ({all_sorted} keys)", t.elapsed());
    let t = Instant::now();
    let mut ks: Vec<u64> = hm.keys().copied().collect();
    ks.sort_unstable();
    println!("sorted scan HashMap  {:>9.2?}  (collect + sort_unstable)", t.elapsed());
    assert_eq!(&ks[..10], &first[..]);

    // Range query: the keys in a narrow band (~1000 of 1M).
    let lo = ks[500_000];
    let hi = ks[501_000];
    let t = Instant::now();
    let c1 = bt.range(lo..hi).count();
    println!("range     BTreeMap {:>9.2?}  ({c1} keys)", t.elapsed());
    let t = Instant::now();
    let c2 = hm.keys().filter(|&&k| k >= lo && k < hi).count();
    println!("range      HashMap {:>9.2?}  ({c2} keys, full scan)", t.elapsed());
}
