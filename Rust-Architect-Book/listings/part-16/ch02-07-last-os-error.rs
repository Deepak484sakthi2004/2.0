// verify: debug ok
// C reports failure with a sentinel return value and a thread-local `errno`. The safe wrapper turns
// that pair into io::Result, and the returned descriptor into an owning type that closes itself.
use std::ffi::CStr;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

pub fn open_readonly(path: &CStr) -> io::Result<OwnedFd> {
    // SAFETY: `path` is NUL-terminated and outlives the call; O_RDONLY | O_CLOEXEC need no mode argument.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        // Read errno IMMEDIATELY: the next libc call on this thread may overwrite it.
        return Err(io::Error::last_os_error());
    }
    // SAFETY: open succeeded, so `fd` is a fresh descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn main() {
    match open_readonly(c"/etc/meridian/fraud-model.bin") {
        Ok(fd) => println!("opened fd {}", fd.as_raw_fd()),
        Err(e) => println!("error: {e}; kind={:?}; raw_os_error={:?}", e.kind(), e.raw_os_error()),
    }
    match open_readonly(c"/proc/self/stat") {
        Ok(fd) => println!("opened fd {} (closed when `fd` is dropped)", fd.as_raw_fd()),
        Err(e) => println!("error: {e}"),
    }
}
