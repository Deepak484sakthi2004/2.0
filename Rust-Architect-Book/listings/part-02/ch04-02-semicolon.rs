// verify: debug error:E0308
fn total_cents(items: &[u64]) -> u64 {
    let mut sum = 0;
    for item in items {
        sum += item;
    }
    sum;
}

fn main() {
    println!("{}", total_cents(&[250, 199]));
}
