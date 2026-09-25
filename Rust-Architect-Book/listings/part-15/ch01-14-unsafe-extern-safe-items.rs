// verify: debug ok
// Edition 2024: `unsafe extern` blocks, `safe` items, and `#[unsafe(...)]` attributes.
use std::ffi::{c_char, c_int};

unsafe extern "C" {
    // Declaring a signature is itself a promise (a wrong signature is UB at every call),
    // hence `unsafe extern`. Items with no preconditions may be declared `safe`:
    pub safe fn getpid() -> c_int;
    // strlen has preconditions (a valid NUL-terminated string), so it stays unsafe to call:
    pub fn strlen(s: *const c_char) -> usize;
}

// Exporting an unmangled symbol can collide with another symbol of the same name: an unsafe attribute.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_version() -> u32 {
    3
}

fn main() {
    println!("getpid() > 0: {}", getpid() > 0); // no unsafe block needed
    let s = c"meridian";
    // SAFETY: `s` is a valid NUL-terminated C string that outlives the call.
    let n = unsafe { strlen(s.as_ptr()) };
    println!("strlen = {n}, version = {}", meridian_version());
}
