// verify: debug ok
/// Fix 1: a function item gets normal elision (one input lifetime -> the output).
fn first_word(s: &str) -> &str {
    s.split(' ').next().unwrap_or("")
}

/// Fix 2: let an expected higher-ranked signature drive the closure's inference.
fn apply<F>(f: F, input: &str) -> &str
where
    F: for<'a> Fn(&'a str) -> &'a str,
{
    f(input)
}

fn main() {
    println!("{}", first_word("GET /health"));
    println!("{}", apply(|s| s.split(' ').nth(1).unwrap_or(""), "GET /health"));
}
