// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release   (and -Target llvm-ir)
#[inline(never)]
pub fn add_twice(a: &mut i32, b: &i32) {
    *a += *b;
    *a += *b;
}

/// # Safety
/// `a` and `b` must be valid, aligned pointers to initialized i32s (they MAY point to the same i32).
#[inline(never)]
pub unsafe fn add_twice_raw(a: *mut i32, b: *const i32) {
    unsafe {
        *a += *b;
        *a += *b;
    }
}
