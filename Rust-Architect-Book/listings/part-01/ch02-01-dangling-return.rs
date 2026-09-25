// verify: debug error:E0515
fn make<'a>() -> &'a i32 {
    let x = 42;
    &x
}

fn main() {
    println!("{}", make());
}
