// verify: debug error:E0277
fn main() {
    let s = String::from("hello");
    let c = s[0];
    println!("{c}");
}
