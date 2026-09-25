// verify: debug error:E0277
fn main() {
    let port: u16 = "8080".parse()?;
    println!("{port}");
}
