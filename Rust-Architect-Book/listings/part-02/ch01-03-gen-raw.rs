// verify: debug ok
fn main() {
    let r#gen = 5; // a raw identifier: 2024 code can still use a name that became a keyword
    println!("{}", r#gen);
}
