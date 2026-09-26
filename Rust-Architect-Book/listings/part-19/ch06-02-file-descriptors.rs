// verify: debug ok
// File descriptors: small integers indexing a per-process table. Which ones std marks close-on-exec, what a child
// process inherits, what the per-process limit does, and how a write to a closed pipe fails in a Rust program.
use std::io::Write;
use std::os::fd::AsRawFd;
use std::process::Command;

fn cloexec(fd: i32) -> bool {
    // SAFETY: F_GETFD on an fd we own; returns -1 on error.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    flags >= 0 && flags & libc::FD_CLOEXEC != 0
}

fn fds() -> String {
    let mut v: Vec<String> = std::fs::read_dir("/proc/self/fd").unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    v.sort_by_key(|s| s.parse::<i32>().unwrap_or(0));
    v.join(" ")
}

fn main() {
    println!("open fds at start: {}", fds());
    let file = std::fs::File::create("/tmp/fd-demo.txt").unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    // SAFETY: a plain open(2) with a NUL-terminated path, deliberately WITHOUT O_CLOEXEC.
    let raw = unsafe { libc::open(c"/tmp/fd-demo.txt".as_ptr(), libc::O_RDONLY) };
    assert!(raw >= 0);
    println!("std File        fd {} close-on-exec: {}", file.as_raw_fd(), cloexec(file.as_raw_fd()));
    println!("std TcpListener fd {} close-on-exec: {}", listener.as_raw_fd(), cloexec(listener.as_raw_fd()));
    println!("libc::open      fd {raw} close-on-exec: {}", cloexec(raw));

    let child = Command::new("sh").arg("-c").arg("ls /proc/$$/fd | sort -n | tr '\\n' ' '").output().unwrap();
    println!("fds the child process sees: {}", String::from_utf8_lossy(&child.stdout).trim());

    // The per-process limit (RLIMIT_NOFILE): lower the soft limit, then open until it fails.
    // SAFETY: getrlimit/setrlimit write/read the struct we pass.
    let mut lim = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) };
    println!("RLIMIT_NOFILE soft {} hard {}", lim.rlim_cur, lim.rlim_max);
    let saved = lim.rlim_cur;
    lim.rlim_cur = 16;
    unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &lim) };
    let mut held = Vec::new();
    let err = loop {
        match std::fs::File::open("/tmp/fd-demo.txt") {
            Ok(f) => held.push(f),
            Err(e) => break e,
        }
    };
    println!("with soft limit 16: opened {} more files, then: {err} (kind {:?}, raw {:?})", held.len(), err.kind(), err.raw_os_error());
    drop(held);
    lim.rlim_cur = saved;
    unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &lim) };

    // A pipe whose read end is closed: the kernel sends SIGPIPE, which Rust programs ignore, so write returns EPIPE.
    let (reader, mut writer) = std::io::pipe().unwrap();
    drop(reader);
    match writer.write_all(b"hello") {
        Ok(()) => println!("write to closed pipe: ok?"),
        Err(e) => println!("write to closed pipe: {e} (kind {:?}); the process is still running", e.kind()),
    }
    // SAFETY: closing the fd we opened with libc::open.
    unsafe { libc::close(raw) };
}
