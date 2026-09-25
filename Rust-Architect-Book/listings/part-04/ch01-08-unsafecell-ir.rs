// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target llvm-ir -Mode release   (and -Target asm)
use std::cell::Cell;
use std::hint::black_box;

#[inline(never)]
pub fn read_twice(x: &i32) -> i32 {
    let a = *x;
    black_box(()); // an opaque call in between
    a + *x // `*x` can't have changed: &i32 is frozen
}

#[inline(never)]
pub fn read_twice_cell(x: &Cell<i32>) -> i32 {
    let a = x.get();
    black_box(()); // an opaque call in between
    a + x.get() // `x` MAY have changed: Cell allows mutation through a shared reference
}
