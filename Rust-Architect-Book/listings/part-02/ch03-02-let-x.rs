// verify: debug build
// Inspect with tools\emit.ps1: -Target mir / llvm-ir / asm, -Mode debug / release.
#[inline(never)]
pub fn constant() -> i32 {
    let x = 10;
    let y = x + 1;
    y * 2
}

#[inline(never)]
pub fn scale(a: i32) -> i32 {
    let x = a * 3;
    let y = x + 1;
    y
}
