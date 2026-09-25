// verify: release ok
// One run on a shared machine: noisy. The crossover between sizes is the point.
use std::collections::{BTreeMap, HashMap};
use std::hint::black_box;
use std::time::Instant;

fn run(n: usize, lookups: usize) {
    // Keys are account ids 0..n spread out by a stride (sparse), values are u32 limits.
    let keys: Vec<u64> = (0..n as u64).map(|i| i * 3 + 1).collect();
    let sorted: Vec<(u64, u32)> = keys.iter().map(|&k| (k, k as u32)).collect();
    let hm: HashMap<u64, u32> = sorted.iter().copied().collect();
    let bt: BTreeMap<u64, u32> = sorted.iter().copied().collect();
    let mut dense: Vec<Option<u32>> = vec![None; n * 3 + 2];
    for &(k, v) in &sorted {
        dense[k as usize] = Some(v);
    }

    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let probes: Vec<u64> = (0..lookups)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            keys[(x % n as u64) as usize]
        })
        .collect();

    let t = Instant::now();
    let s1: u64 = probes.iter().map(|k| sorted.binary_search_by_key(k, |e| e.0).map(|i| sorted[i].1 as u64).unwrap()).sum();
    let t_bs = t.elapsed();
    let t = Instant::now();
    let s2: u64 = probes.iter().map(|k| hm[black_box(k)] as u64).sum();
    let t_hm = t.elapsed();
    let t = Instant::now();
    let s3: u64 = probes.iter().map(|k| bt[black_box(k)] as u64).sum();
    let t_bt = t.elapsed();
    let t = Instant::now();
    let s4: u64 = probes.iter().map(|&k| dense[black_box(k) as usize].unwrap() as u64).sum();
    let t_d = t.elapsed();
    assert!(s1 == s2 && s2 == s3 && s3 == s4);
    println!(
        "n = {n:>9}: sorted Vec + binary_search {t_bs:>9.2?} | HashMap {t_hm:>9.2?} | BTreeMap {t_bt:>9.2?} | dense Vec index {t_d:>9.2?}"
    );
}

fn main() {
    println!("1,000,000 random lookups against n keys");
    for n in [1_000, 100_000, 1_000_000] {
        run(n, 1_000_000);
    }
}
