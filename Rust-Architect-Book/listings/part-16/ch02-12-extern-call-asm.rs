// verify: release build
// For tools/emit.ps1: what a call through an `unsafe extern` declaration compiles to.
use std::ffi::{CStr, c_char};

unsafe extern "C" {
    fn strlen(s: *const c_char) -> usize;
}

#[inline(never)]
pub fn c_len(s: &CStr) -> usize {
    // SAFETY: a &CStr is NUL-terminated and alive for the call.
    unsafe { strlen(s.as_ptr()) }
}

#[inline(never)]
pub fn c_len_plus_one(s: &CStr) -> usize {
    // SAFETY: as above.
    unsafe { strlen(s.as_ptr()) + 1 }
}
