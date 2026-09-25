// verify: debug error:E0382
fn main() {
    let a = String::from("hello");
    let b = a;
    println!("{a} {b}");
}
