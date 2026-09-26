// verify: debug error:dangling_pointers_from_temporaries
// The most common C-string mistake, rejected at compile time once the lint is denied
// (it is warn-by-default; Meridian's crates deny it).
#![deny(dangling_pointers_from_temporaries)]
use std::ffi::{CString, c_char};

unsafe extern "C" {
    fn strlen(s: *const c_char) -> usize;
}

fn model_path_len(dir: &str) -> usize {
    let p = CString::new(format!("{dir}/model.bin")).unwrap().as_ptr();
    // SAFETY (intended): `p` is a NUL-terminated string... except the CString is already gone.
    unsafe { strlen(p) }
}

fn main() {
    println!("{}", model_path_len("/etc/meridian"));
}
