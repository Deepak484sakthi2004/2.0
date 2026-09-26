// verify: debug ok
// verify: release ok
// A variadic C function, wrapped safely: the format string is fixed by the wrapper, every argument
// type matches its conversion, and truncation is detected from the return value and retried.
use std::ffi::{CStr, c_char, c_int, c_ulonglong};

unsafe extern "C" {
    /// # Safety
    /// `buf` must be writable for `size` bytes; `fmt` must be NUL-terminated; each variadic argument
    /// must have exactly the type its conversion specifier expects (C checks none of this).
    fn snprintf(buf: *mut c_char, size: usize, fmt: *const c_char, ...) -> c_int;
}

#[derive(Debug)]
pub struct FormatError;

/// "EUR 1234.05" from minor units, formatted by the C library.
pub fn format_amount(currency: &CStr, cents: i64) -> Result<String, FormatError> {
    let sign: &CStr = if cents < 0 { c"-" } else { c"" };
    let abs = cents.unsigned_abs();
    let (major, minor) = (abs / 100, abs % 100);
    let mut buf = vec![0u8; 8]; // deliberately small: the first attempt truncates for large amounts
    loop {
        // SAFETY: `buf` is writable for `buf.len()` bytes. The format is a literal, and each argument
        // matches its conversion: %s <- NUL-terminated *const c_char (x2), %llu <- c_ulonglong (x2).
        let n = unsafe {
            snprintf(
                buf.as_mut_ptr().cast::<c_char>(),
                buf.len(),
                c"%s %s%llu.%02llu".as_ptr(),
                currency.as_ptr(),
                sign.as_ptr(),
                major as c_ulonglong,
                minor as c_ulonglong,
            )
        };
        if n < 0 {
            return Err(FormatError); // an encoding error: C's only failure signal here
        }
        let needed = n as usize; // the length it WANTED to write, excluding the NUL
        if needed < buf.len() {
            buf.truncate(needed); // drop the NUL and the unused tail
            return String::from_utf8(buf).map_err(|_| FormatError);
        }
        println!("  (buffer of {} bytes was too small: snprintf needs {} + NUL; retrying)", buf.len(), needed);
        buf.resize(needed + 1, 0);
    }
}

fn main() {
    println!("{:?}", format_amount(c"EUR", 1_050));
    println!("{:?}", format_amount(c"USD", -123_456_789));
}
