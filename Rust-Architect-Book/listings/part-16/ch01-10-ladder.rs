// verify: debug ok
// Rust -> ABI -> C -> OS: the same question asked at three levels.
fn main() {
    let from_std = std::process::id(); // std: a safe wrapper over the C library on Linux
    // SAFETY: getpid has no preconditions (the libc crate declares every foreign function unsafe).
    let from_libc = unsafe { libc::getpid() }; // the C library's wrapper, called through the C ABI
    // The kernel's own convention: syscall number 39 on x86-64 Linux, via libc's generic entry point.
    // SAFETY: SYS_getpid takes no arguments and has no preconditions.
    let from_kernel = unsafe { libc::syscall(libc::SYS_getpid) };
    println!("std::process::id() = {from_std}");
    println!("libc::getpid()      = {from_libc}");
    println!("syscall(SYS_getpid={}) = {from_kernel}", libc::SYS_getpid);
    println!("all equal: {}", from_std as i64 == from_libc as i64 && from_libc as i64 == from_kernel);
}
