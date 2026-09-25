// verify: debug error:E0382
fn consume(s: String) -> usize {
    s.len()
}

fn main() {
    let payload = String::from("payload");
    let mut total = 0;
    for _ in 0..3 {
        total += consume(payload);
    }
    println!("{total}");
}
