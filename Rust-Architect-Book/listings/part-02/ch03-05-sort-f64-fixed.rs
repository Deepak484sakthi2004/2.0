// verify: debug ok
fn main() {
    let mut prices = vec![19.99, f64::NAN, 5.25, -0.0, 0.0, 12.0];
    prices.sort_by(|a, b| a.total_cmp(b)); // IEEE 754 totalOrder: every f64 has a place
    println!("{prices:?}");
    println!("NaN == NaN:       {}", f64::NAN == f64::NAN);
    println!("0.1 + 0.2 == 0.3: {}", 0.1 + 0.2 == 0.3);
}
