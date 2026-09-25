// verify: debug ok
// The same program with std's Vec compiles: Vec's Drop promises (unsafely, via #[may_dangle])
// not to USE its elements, only to drop them, and dropping a `&String` does nothing.
fn main() {
    let mut names: Vec<&String> = Vec::new();
    let s = String::from("merchant-7"); // dropped before `names`, as before
    names.push(&s);
    println!("{}", names.len());
}
