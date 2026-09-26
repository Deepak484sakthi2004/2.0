// verify: debug ok
// verify: debug miri-ok
// Rust strings -> C strings and back. Every conversion that can fail returns an error you must handle.
use std::ffi::{CStr, CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

fn main() {
    // String -> CString: allocates, appends the NUL, and rejects an interior NUL.
    let ok = CString::new("merchant-42").unwrap();
    println!("CString::new(\"merchant-42\") -> {} bytes with the NUL", ok.as_bytes_with_nul().len());
    println!(r#"CString::new("a\0b") -> {:?}"#, CString::new("a\0b").map_err(|e| e.nul_position()));

    // Literal C strings (Rust 1.77+): checked at compile time, stored with their NUL, no allocation.
    let lit: &CStr = c"fraud";
    println!("c\"fraud\" -> {:?}, count_bytes() = {}", lit, lit.count_bytes());

    // Bytes from C -> &CStr: the NUL must be exactly at the end, or use the "until NUL" variant.
    println!(r#"from_bytes_with_nul(b"ok\0")    -> {:?}"#, CStr::from_bytes_with_nul(b"ok\0"));
    println!(r#"from_bytes_with_nul(b"ok\0xx")  -> is_err: {:?}"#, CStr::from_bytes_with_nul(b"ok\0xx").is_err());
    println!(r#"from_bytes_until_nul(b"ok\0xx") -> {:?}"#, CStr::from_bytes_until_nul(b"ok\0xx"));

    // &CStr -> &str: only if the bytes are UTF-8. C promises bytes, not UTF-8.
    let latin1 = CStr::from_bytes_with_nul(b"Zo\xEB\0").unwrap(); // "Zoë" in Latin-1, not UTF-8
    println!("to_str() on Latin-1 bytes -> {:?}", latin1.to_str().map_err(|e| e.valid_up_to()));
    println!("to_string_lossy()         -> {:?}", latin1.to_string_lossy());

    // Paths and environment values are bytes on Unix: OsStr keeps them exact.
    let path = Path::new(OsStr::from_bytes(latin1.to_bytes()));
    println!("as a Path (exact bytes)   -> {:?}", path);
}
