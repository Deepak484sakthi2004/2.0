// verify: debug build
// verify: release build
// Answer-key check for Chapter 18.7's beginner and intermediate exercises: `log(x.as_str())` instead
// of `log(&x)`, and a `Box<str>` instead of a `String` (tools/emit.ps1 -Target asm -Mode release).
#[inline(never)]
pub fn foo() -> String {
    String::from("meridian")
}

#[inline(never)]
pub fn foo_boxed() -> Box<str> {
    Box::from("meridian")
}

#[inline(never)]
pub fn log(s: &str) {
    std::hint::black_box(s);
}

#[inline(never)]
pub fn caller_as_str() -> usize {
    let x = foo();
    log(x.as_str());
    x.len()
}

#[inline(never)]
pub fn caller_boxed() -> usize {
    let x = foo_boxed();
    log(&x);
    x.len()
}
