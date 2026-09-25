// verify: debug error:E0080
// Constant evaluation runs MIR in the compiler's interpreter. A validation written as a const
// assertion turns a bad table into a BUILD failure instead of a production incident.
/// Fee tiers: (monthly volume threshold in cents, fee in basis points). Must be sorted by threshold.
const FEE_TIERS: [(u64, u32); 4] = [
    (0, 290),
    (1_000_000_00, 250),
    (500_000_00, 220), // BUG: out of order (a copy-paste from the old table)
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

fn main() {
    println!("{} tiers", FEE_TIERS.len());
}
