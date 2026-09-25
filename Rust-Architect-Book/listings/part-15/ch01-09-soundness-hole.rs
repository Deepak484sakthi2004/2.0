// verify: debug ok
// verify: debug miri dangling
// rust-lang/rust issue 25860 (open since 2015): SAFE code, no `unsafe` anywhere, that the compiler
// accepts and that produces a dangling reference. This higher-ranked variant compiles on rustc 1.98.1.
const STATIC_UNIT: &&() = &&();

fn translate<'a, 'b, T: ?Sized>(_witness: &'a &'b (), v: &'b T) -> &'a T {
    v
}

fn expand<'a, 'b, T: ?Sized>(x: &'a T) -> &'b T {
    let f: for<'x> fn(_, &'x T) -> &'b T = translate;
    f(STATIC_UNIT, x)
}

fn main() {
    let dangling: &'static String = {
        let s = String::from("freed");
        expand(&s)
    };
    println!("{}", dangling.len());
}
