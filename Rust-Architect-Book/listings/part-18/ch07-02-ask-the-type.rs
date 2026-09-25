// verify: debug error:E0308
// The oldest trick for asking the type checker what it inferred: bind the value to a pattern of the
// wrong type and read the error. (Your editor's inlay hints ask the same question via rust-analyzer.)
fn foo() -> String {
    String::from("meridian")
}

fn main() {
    let () = foo();
}
