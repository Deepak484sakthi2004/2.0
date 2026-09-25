// verify: debug ok
// verify: debug miri-ok
//! epoll(7) by hand, through libc: one epoll instance watches many file descriptors, and
//! epoll_wait returns only the ready ones, each tagged with the token we registered.
//! Unix socketpairs stand in for TCP connections (Miri can run these; it can't run real TCP).

fn socketpair() -> [i32; 2] {
    let mut fds = [0i32; 2];
    // SAFETY: socketpair writes two fds into the 2-element array we pass.
    let rc = unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_NONBLOCK, 0, fds.as_mut_ptr()) };
    assert_eq!(rc, 0);
    fds
}

fn watch(ep: i32, fd: i32, token: u64, flags: u32) {
    let mut ev = libc::epoll_event { events: libc::EPOLLIN as u32 | flags, u64: token };
    // SAFETY: `ev` is a valid epoll_event; epoll_ctl copies it.
    assert_eq!(unsafe { libc::epoll_ctl(ep, libc::EPOLL_CTL_ADD, fd, &mut ev) }, 0);
}

/// Non-blocking epoll_wait (timeout 0): the tokens of the ready descriptors.
fn ready(ep: i32) -> Vec<u64> {
    let mut out = [libc::epoll_event { events: 0, u64: 0 }; 16];
    // SAFETY: `out` has room for the 16 events we tell the kernel it may write.
    let n = unsafe { libc::epoll_wait(ep, out.as_mut_ptr(), 16, 0) };
    assert!(n >= 0);
    out[..n as usize].iter().map(|e| e.u64).collect()
}

fn send(fd: i32, bytes: &[u8]) {
    // SAFETY: we pass a valid pointer and its length.
    let n = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
    assert_eq!(n, bytes.len() as isize);
}

fn recv(fd: i32, n: usize) -> usize {
    let mut buf = [0u8; 64];
    assert!(n <= buf.len());
    // SAFETY: `buf` has room for n <= 64 bytes.
    unsafe { libc::read(fd, buf.as_mut_ptr().cast(), n) as usize }
}

fn main() {
    // 1. One epoll instance, three "connections"; two of them receive data.
    // SAFETY: plain syscall, no pointers.
    let ep = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
    let conns: Vec<[i32; 2]> = (0..3).map(|_| socketpair()).collect();
    for (token, pair) in conns.iter().enumerate() {
        watch(ep, pair[1], token as u64, 0);
    }
    println!("ready before any data: {:?}", ready(ep));
    send(conns[0][0], b"GET a");
    send(conns[2][0], b"GET c");
    println!("ready after writes to 0 and 2: {:?}", ready(ep));

    // 2. Level-triggered vs edge-triggered: read only half the data, then ask again.
    for (name, flags) in [("level-triggered", 0u32), ("edge-triggered", libc::EPOLLET as u32)] {
        // SAFETY: plain syscall.
        let ep = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
        let pair = socketpair();
        watch(ep, pair[1], 7, flags);
        send(pair[0], b"12345678");
        let first = ready(ep);
        let got = recv(pair[1], 4);
        println!("{name}: ready {first:?}; read {got} of 8 bytes; ready again {:?}", ready(ep));
    }
}
