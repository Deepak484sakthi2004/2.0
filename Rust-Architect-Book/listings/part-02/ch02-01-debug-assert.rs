// verify: debug panic discount over 100%
// verify: release ok
fn apply_discount(price_cents: u64, pct: u64) -> u64 {
    debug_assert!(pct <= 100, "discount over 100%: {pct}");
    price_cents - price_cents * pct / 100
}

fn main() {
    println!("debug_assertions = {}", cfg!(debug_assertions));
    println!("charged: {} cents", apply_discount(10_000, 150));
}
