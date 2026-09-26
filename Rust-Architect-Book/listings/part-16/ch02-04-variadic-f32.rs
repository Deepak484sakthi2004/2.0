// verify: debug error:E0617
// C's default argument promotions: a variadic `float` is passed as `double`. Rust makes you say so.
use std::ffi::{c_char, c_int};

unsafe extern "C" {
    fn snprintf(buf: *mut c_char, size: usize, fmt: *const c_char, ...) -> c_int;
}

fn main() {
    let mut buf = [0u8; 32];
    let score: f32 = 0.87;
    // SAFETY (intended): buf is writable for 32 bytes; "%.2f" expects a double.
    let n = unsafe { snprintf(buf.as_mut_ptr().cast(), buf.len(), c"%.2f".as_ptr(), score) };
    println!("{n}");
}
