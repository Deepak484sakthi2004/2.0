// verify: release ok
// One run on a shared machine: the numbers are noisy. Compare ratios, not absolutes.
use std::collections::HashMap;
use std::hash::{BuildHasher, Hash};
use std::hint::black_box;
use std::time::{Duration, Instant};

fn bench<K: Hash + Eq + Clone, S: BuildHasher + Default>(keys: &[K]) -> (Duration, Duration) {
    let t = Instant::now();
    let mut m: HashMap<K, u32, S> = HashMap::with_capacity_and_hasher(keys.len(), S::default());
    for k in keys {
        *m.entry(k.clone()).or_insert(0) += 1;
    }
    let build = t.elapsed();

    let t = Instant::now();
    let mut hits = 0u64;
    for k in keys {
        hits += *m.get(black_box(k)).unwrap() as u64;
    }
    black_box(hits);
    (build, t.elapsed())
}

fn run<K: Hash + Eq + Clone>(label: &str, keys: &[K]) {
    println!("{label}");
    let rows: [(&str, (Duration, Duration)); 4] = [
        ("std RandomState (SipHash-1-3)", bench::<K, std::hash::RandomState>(keys)),
        ("ahash::RandomState", bench::<K, ahash::RandomState>(keys)),
        ("foldhash::fast::RandomState", bench::<K, foldhash::fast::RandomState>(keys)),
        ("fxhash::FxBuildHasher", bench::<K, fxhash::FxBuildHasher>(keys)),
    ];
    for (name, (build, lookup)) in rows {
        println!("  {name:<31} build {build:>10.2?}   lookup {lookup:>10.2?}");
    }
}

fn main() {
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let n = 1_000_000;
    let ints: Vec<u64> = (0..n).map(|_| next()).collect();
    let strings: Vec<String> = ints.iter().map(|k| format!("user:{}", k % 10_000_000)).collect();

    run(&format!("{n} random u64 keys"), &ints);
    run(&format!("{n} short String keys (\"user:NNNNNNN\")"), &strings);
}
