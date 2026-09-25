// verify: debug error:invalid_reference_casting
// Writing through a pointer derived from `&T` (no UnsafeCell): the obvious case is a compile error.
fn main() {
    let x = 1u32;
    let r = &x;
    let p = r as *const u32 as *mut u32;
    unsafe { *p = 2 };
    println!("{x}");
}
