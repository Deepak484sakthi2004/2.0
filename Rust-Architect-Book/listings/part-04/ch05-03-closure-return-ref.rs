// verify: debug error:lifetime
fn main() {
    // A closure that returns (part of) its argument. Closures don't get fn-style lifetime elision,
    // so the compiler infers two unrelated lifetimes for the parameter and the return value.
    let first_word = |s: &str| -> &str { s.split(' ').next().unwrap_or("") };
    println!("{}", first_word("GET /health"));
}
