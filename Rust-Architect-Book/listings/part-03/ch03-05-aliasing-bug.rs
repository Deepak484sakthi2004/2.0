// verify: debug error:E0502
fn main() {
    let mut prices = vec![30, 10, 20];
    let report_view = &prices; // the reporting component keeps a view (it relies on arrival order)
    prices.sort(); // the pricing component sorts in place
    println!("report sees {:?}", report_view);
}
