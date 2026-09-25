// verify: debug error:lifetime
// The reproduction usually quoted from issue #25860 (2015): REJECTED by rustc 1.98.1.
// (The higher-ranked variant in ch01-09 still gets through.)
static UNIT: &'static &'static () = &&();

fn foo<'a, 'b, T>(_: &'a &'b (), v: &'b T) -> &'a T {
    v
}

fn bad<'a, T>(x: &'a T) -> &'static T {
    let f: fn(&'static &'a (), &'a T) -> &'static T = foo;
    f(UNIT, x)
}

fn main() {
    let s = String::from("freed");
    println!("{}", bad(&s).len());
}
