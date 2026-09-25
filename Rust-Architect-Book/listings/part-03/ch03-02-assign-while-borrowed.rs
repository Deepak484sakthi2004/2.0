// verify: debug error:E0506
fn main() {
    let mut limit = 100;
    let r = &limit;
    limit = 200;
    println!("{r}");
}
