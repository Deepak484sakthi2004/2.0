// verify: release ok
// verify: debug miri-ok
// Publishing an immutable snapshot: build it completely, then publish the pointer. arc_swap::ArcSwap does the
// publication (and keeps old snapshots alive while readers still hold them), so readers never see a half-built table.
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;

/// Per-merchant limits. Invariant checked by readers: every entry's `daily` equals `per_txn * 20`.
pub struct Limits {
    version: u64,
    per_merchant: HashMap<u32, (u64, u64)>, // merchant → (per_txn, daily)
}

fn build(version: u64, merchants: u32) -> Limits {
    let per_merchant = (0..merchants).map(|m| {
        let per_txn = 1_000 * version + m as u64;
        (m, (per_txn, per_txn * 20))
    });
    Limits { version, per_merchant: per_merchant.collect() }
}

const VERSIONS: u64 = if cfg!(miri) { 3 } else { 200 };
const MERCHANTS: u32 = if cfg!(miri) { 4 } else { 1_000 };

fn main() {
    let current = ArcSwap::from_pointee(build(0, MERCHANTS));
    let (checked, last_seen) = thread::scope(|s| {
        s.spawn(|| {
            for v in 1..=VERSIONS {
                current.store(Arc::new(build(v, MERCHANTS))); // publish a fully built snapshot
            }
        });
        let reader = s.spawn(|| {
            let (mut checked, mut last) = (0u64, 0u64);
            while last < VERSIONS {
                let limits = current.load(); // a guard: the snapshot stays alive while we use it
                assert!(limits.version >= last, "versions went backwards");
                for (per_txn, daily) in limits.per_merchant.values() {
                    assert_eq!(*daily, per_txn * 20, "half-built snapshot");
                }
                assert_eq!(limits.per_merchant.len(), MERCHANTS as usize);
                last = limits.version;
                checked += 1;
            }
            (checked, last)
        });
        reader.join().unwrap()
    });
    println!("checked {checked} snapshots, last version {last_seen}, none half-built");
}
