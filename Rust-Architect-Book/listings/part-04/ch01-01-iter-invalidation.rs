// verify: debug error:E0502
fn main() {
    let mut orders = vec![1, 2, 3];
    for id in &orders {
        if *id == 2 {
            orders.push(4); // a follow-up order, added while iterating
        }
    }
    println!("{orders:?}");
}
