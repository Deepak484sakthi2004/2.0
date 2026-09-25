// verify: debug error:E0282
fn main() {
    let items = Vec::new();
    println!("{}", items.len());
}
