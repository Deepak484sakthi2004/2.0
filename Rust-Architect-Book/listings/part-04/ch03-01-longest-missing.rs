// verify: debug error:E0106
fn longest(a: &str, b: &str) -> &str {
    if a.len() >= b.len() { a } else { b }
}

fn main() {
    println!("{}", longest("gateway", "db"));
}
