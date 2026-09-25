// verify: debug error:E0308
fn print_static(s: &'static str) {
    println!("static: {s}");
}

fn main() {
    // The other direction: a function that accepts only 'static can NOT stand in for one that
    // must accept any lifetime.
    let h: fn(&str) = print_static;
    let owned = String::from("from a request");
    h(&owned);
}
