// verify: debug ok
// A struct holding a borrow, with no Drop impl: the loan ends at the struct's last use.
struct Span<'a> {
    name: &'a str,
}

fn main() {
    let mut name = String::from("checkout");
    let span = Span { name: &name };
    println!("span: {}", span.name);
    name.push_str("-v2"); // fine: span is dead here
    println!("{name}");
}
