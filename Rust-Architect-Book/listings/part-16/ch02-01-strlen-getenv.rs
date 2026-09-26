// verify: debug ok
// verify: debug miri-ok
// Two C functions, declared by hand, each wrapped in a safe function whose TYPES carry the contract.
use std::ffi::{CStr, OsString, c_char};
use std::os::unix::ffi::OsStringExt;

mod sys {
    use std::ffi::c_char;

    unsafe extern "C" {
        /// # Safety
        /// `s` must point to a NUL-terminated byte string, readable up to and including the NUL.
        pub fn strlen(s: *const c_char) -> usize;

        /// # Safety
        /// `name` must be NUL-terminated. The result is NULL or points into the process environment,
        /// and it's only valid until the environment is next modified (setenv/putenv/unsetenv).
        pub fn getenv(name: *const c_char) -> *mut c_char;
    }
}

/// Safe: a `&CStr` is NUL-terminated by construction and stays alive for the whole call.
pub fn c_len(s: &CStr) -> usize {
    // SAFETY: `s.as_ptr()` points to a NUL-terminated string borrowed for the duration of the call.
    unsafe { sys::strlen(s.as_ptr()) }
}

/// Safe: copies the value out before returning, so no pointer into the environment escapes.
/// Contract kept by this program: nothing modifies the environment concurrently (in edition 2024
/// `std::env::set_var` is `unsafe` for exactly this reason).
pub fn env_var(name: &CStr) -> Option<OsString> {
    // SAFETY: `name` is NUL-terminated.
    let p: *mut c_char = unsafe { sys::getenv(name.as_ptr()) };
    if p.is_null() {
        return None;
    }
    // SAFETY: non-null means a NUL-terminated string inside the environment block, and it's still
    // valid because nothing has run since the call. `to_bytes().to_vec()` copies it immediately.
    let bytes = unsafe { CStr::from_ptr(p) }.to_bytes().to_vec();
    Some(OsString::from_vec(bytes)) // Unix environment values are bytes, not necessarily UTF-8
}

fn main() {
    println!("c_len(c\"meridian\") = {}", c_len(c"meridian"));
    println!("env_var(MERIDIAN_MODEL_DIR) = {:?}", env_var(c"MERIDIAN_MODEL_DIR"));
    println!("env_var(PATH) is set: {}", env_var(c"PATH").is_some());
    // std's own wrapper, for comparison: same answer, plus a lock shared with std's set_var.
    println!("std::env::var_os(MERIDIAN_MODEL_DIR) = {:?}", std::env::var_os("MERIDIAN_MODEL_DIR"));
}
