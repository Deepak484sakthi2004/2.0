// verify: debug ok
// The corrected table: the same const assertion passes at compile time and costs nothing at run time.
const FEE_TIERS: [(u64, u32); 4] = [
    (0, 290),
    (500_000_00, 250),
    (1_000_000_00, 220),
    (10_000_000_00, 180),
];

const fn tiers_sorted(t: &[(u64, u32)]) -> bool {
    let mut i = 1;
    while i < t.len() {
        if t[i - 1].0 >= t[i].0 {
            return false;
        }
        i += 1;
    }
    true
}

const _: () = assert!(tiers_sorted(&FEE_TIERS), "FEE_TIERS must be sorted by threshold");

fn fee_bps(monthly_volume_cents: u64) -> u32 {
    let mut bps = FEE_TIERS[0].1;
    for &(threshold, tier_bps) in FEE_TIERS.iter() {
        if monthly_volume_cents >= threshold {
            bps = tier_bps;
        }
    }
    bps
}

fn main() {
    for v in [10_000_00u64, 700_000_00, 2_000_000_00, 50_000_000_00] {
        println!("volume {v:>13} cents -> {} bps", fee_bps(v));
    }
}
