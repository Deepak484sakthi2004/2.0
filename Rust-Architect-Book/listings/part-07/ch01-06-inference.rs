// verify: debug error:E0284
fn main() {
    let port = "8080".parse().unwrap(); // parse::<F>() is generic over its RETURN type: F is unknown
    println!("{port}");
}
