// verify: debug ok
// Returning closures: one concrete type -> `impl Fn`; a choice among several -> Box<dyn Fn> (or an enum).
fn fee_calculator(bps: u64) -> impl Fn(u64) -> u64 {
    move |amount_cents| amount_cents * bps / 10_000
}

fn rounding_rule(kind: &str) -> Box<dyn Fn(u64) -> u64> {
    match kind {
        "floor10" => Box::new(|c| c / 10 * 10),
        "ceil10" => Box::new(|c| c.div_ceil(10) * 10),
        _ => Box::new(|c| c),
    }
}

fn main() {
    let fee = fee_calculator(290); // 2.90%
    let round = rounding_rule("ceil10");
    for amount in [1_000, 12_345, 99_999] {
        println!("amount {amount:>6} -> fee {:>4} -> rounded {:>4}", fee(amount), round(fee(amount)));
    }
    println!("size_of_val(fee) = {} B, size_of_val(round) = {} B",
        std::mem::size_of_val(&fee), std::mem::size_of_val(&round));
}
