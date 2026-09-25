// verify: debug error:E0502
// The same code after the type gains a Drop impl. The drop at the end of scope is a USE of the
// loan (Drop::drop could read `self.name`), so the loan is live until then.
struct Span<'a> {
    name: &'a str,
}

impl Drop for Span<'_> {
    fn drop(&mut self) {}
}

fn main() {
    let mut name = String::from("checkout");
    let span = Span { name: &name };
    println!("span: {}", span.name);
    name.push_str("-v2"); // span's last *visible* use was above...
    println!("{name}");
}
