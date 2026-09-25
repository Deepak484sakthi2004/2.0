// verify: debug ok
// A third fix for Chapter 4.5's "closure returning a reference" error: an identity helper whose bound supplies
// the higher-ranked signature, so the closure can be stored in a variable and reused.
fn returns_borrow<F: Fn(&str) -> &str>(f: F) -> F {
    f // `Fn(&str) -> &str` in a bound means `for<'a> Fn(&'a str) -> &'a str` (elision in Fn sugar)
}

fn main() {
    let first_word = returns_borrow(|s| s.split(' ').next().unwrap_or(""));
    let methods: Vec<&str> = ["GET /health", "POST /pay"].iter().map(|l| first_word(l)).collect();
    println!("{methods:?}");
}
