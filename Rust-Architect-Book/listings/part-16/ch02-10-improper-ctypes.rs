// verify: debug error:improper_ctypes
// A foreign declaration with a Rust-only type: `&str` is (pointer, length), and C's `puts` expects
// one pointer to a NUL-terminated string. The lint is warn-by-default; denied here.
#![deny(improper_ctypes)]
use std::ffi::c_int;

unsafe extern "C" {
    fn puts(s: &str) -> c_int;
}

fn main() {
    // SAFETY (intended): ...there is no way to write a true one for this declaration.
    unsafe { puts("hello") };
}
