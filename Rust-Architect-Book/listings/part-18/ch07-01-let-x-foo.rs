// verify: debug build
// verify: release build
// The program Chapter 18.7 traces through every compiler stage (tools/emit.ps1 -Target
// expand | hir | mir | llvm-ir | asm, debug and release).
#[inline(never)]
pub fn foo() -> String {
    String::from("meridian")
}

#[inline(never)]
pub fn log(s: &str) {
    std::hint::black_box(s);
}

#[inline(never)]
pub fn caller() -> usize {
    let x = foo();
    log(&x);
    x.len()
}
