// verify: debug error:E0384
fn main() {
    let retries = 3;
    if std::env::args().count() > 5 {
        retries = 5;
    }
    println!("{retries}");
}
