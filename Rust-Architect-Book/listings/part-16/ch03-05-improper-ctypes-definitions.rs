// verify: debug error:improper_ctypes_definitions
// An export that only works when the caller is Rust: `&str` is a (pointer, length) pair with no C
// equivalent. The lint (warn-by-default) says so at the definition; Meridian's crates deny it.
#![deny(improper_ctypes_definitions)]

#[unsafe(no_mangle)]
pub extern "C" fn meridian_merchant_known(merchant: &str) -> bool {
    merchant.starts_with("m-")
}

fn main() {
    println!("{}", meridian_merchant_known("m-42"));
}
