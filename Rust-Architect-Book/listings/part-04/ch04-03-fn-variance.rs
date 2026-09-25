// verify: debug ok
fn print_any(s: &str) {
    // works for ANY lifetime: its type is for<'a> fn(&'a str)
    println!("any: {s}");
}

fn print_static(s: &'static str) {
    // demands a 'static argument
    println!("static: {s}");
}

fn main() {
    // A function that accepts MORE (any lifetime) can stand in for one that accepts LESS ('static only):
    // function parameters are CONTRAVARIANT.
    let f: fn(&'static str) = print_any;
    f("literal");

    let g: fn(&'static str) = print_static;
    g("literal");

    let owned = String::from("from a request");
    let h: fn(&str) = print_any;
    h(&owned);
}
