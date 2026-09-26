// verify: debug ok
// A logic bug, not a memory bug: errno read too late reports the WRONG error.
use std::ffi::CStr;
use std::io;

fn log_failure(what: &str) {
    // Looks harmless. It also calls into libc, which is allowed to set errno.
    let _ = std::fs::OpenOptions::new().append(true).open("/"); // e.g. probing a log target
    let _ = what;
}

/// Buggy: logs first, reads errno second.
pub fn open_model_buggy(path: &CStr) -> io::Result<i32> {
    // SAFETY: `path` is NUL-terminated and outlives the call.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        log_failure("model open failed");
        return Err(io::Error::last_os_error()); // errno now belongs to log_failure's call
    }
    Ok(fd)
}

/// Fixed: capture errno first, then do anything else.
pub fn open_model_fixed(path: &CStr) -> io::Result<i32> {
    // SAFETY: as above.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        let err = io::Error::last_os_error();
        log_failure("model open failed");
        return Err(err);
    }
    Ok(fd)
}

fn main() {
    let path = c"/etc/meridian/fraud-model.bin";
    println!("buggy: {:?}", open_model_buggy(path).map_err(|e| e.to_string()));
    println!("fixed: {:?}", open_model_fixed(path).map_err(|e| e.to_string()));
}
