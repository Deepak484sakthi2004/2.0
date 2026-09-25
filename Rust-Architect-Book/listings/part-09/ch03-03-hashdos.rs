// verify: release ok
// A deterministic, unkeyed hasher + attacker-chosen keys = every key in one probe chain.
use std::collections::HashMap;
use std::hash::BuildHasher;
use std::time::{Duration, Instant};

fn fill<S: BuildHasher + Default>(keys: impl Iterator<Item = u64>) -> Duration {
    let t = Instant::now();
    let mut m: HashMap<u64, u64, S> = HashMap::default();
    for k in keys {
        m.insert(k, k);
    }
    std::hint::black_box(&m);
    t.elapsed()
}

fn main() {
    for n in [10_000u64, 20_000, 40_000] {
        // FxHash of a u64 is `k * CONSTANT`: if k's low 32 bits are zero, so are the hash's.
        // hashbrown picks the starting bucket from the LOW bits, so every key starts at bucket 0.
        let fx_benign = fill::<fxhash::FxBuildHasher>(0..n);
        let fx_attack = fill::<fxhash::FxBuildHasher>((0..n).map(|i| i << 32));
        let sip_attack = fill::<std::hash::RandomState>((0..n).map(|i| i << 32));
        println!(
            "n = {n:>6}: FxHash benign {fx_benign:>10.2?} | FxHash crafted {fx_attack:>10.2?} | SipHash crafted {sip_attack:>10.2?}"
        );
    }
}
