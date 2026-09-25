// verify: debug error:E0277
fn main() {
    let mut prices = vec![19.99, 5.25, 12.0];
    prices.sort();
    println!("{prices:?}");
}
