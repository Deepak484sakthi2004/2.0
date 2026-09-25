// verify: release ok
//! C10K on one thread: a mio (epoll) event loop serves 10,000 connections.
//! Compare with ch01-01, where one thread per connection hit the wall at about 500.
use mio::net::TcpListener;
use mio::{Events, Interest, Poll, Token};
use std::io::{ErrorKind, Read, Write};

const LISTENER: Token = Token(usize::MAX);
const N: usize = 10_000;

fn raise_fd_limit() {
    // Each in-process connection costs two file descriptors (client end and server end), and the
    // soft limit is 1024. (Not run under Miri: Miri doesn't model setrlimit.)
    // SAFETY: getrlimit/setrlimit only access the `rlimit` struct we pass by a valid pointer.
    unsafe {
        let mut r = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        assert_eq!(libc::getrlimit(libc::RLIMIT_NOFILE, &mut r), 0);
        r.rlim_cur = r.rlim_max.min(65_536);
        assert_eq!(libc::setrlimit(libc::RLIMIT_NOFILE, &r), 0);
    }
}

fn proc_field(path: &str, key: &str) -> String {
    let s = std::fs::read_to_string(path).unwrap();
    let line = s.lines().find(|l| l.starts_with(key)).unwrap_or_default();
    line[key.len()..].trim().to_string()
}

/// The whole server: one thread, one epoll instance, a slab of connections.
fn serve(mut listener: TcpListener) -> std::io::Result<(usize, u64)> {
    let mut poll = Poll::new()?;
    poll.registry().register(&mut listener, LISTENER, Interest::READABLE)?;
    let mut conns = slab::Slab::with_capacity(N);
    let mut events = Events::with_capacity(1024);
    let (mut echoed, mut wakeups) = (0usize, 0u64);
    while echoed < N {
        poll.poll(&mut events, None)?; // epoll_wait: park until something is ready
        wakeups += 1;
        for ev in events.iter() {
            if ev.token() == LISTENER {
                loop {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let slot = conns.vacant_entry();
                            poll.registry().register(&mut stream, Token(slot.key()), Interest::READABLE)?;
                            slot.insert(stream);
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }
            } else {
                let stream: &mut mio::net::TcpStream = &mut conns[ev.token().0];
                let mut buf = [0u8; 64];
                loop {
                    // mio registers edge-triggered: drain until WouldBlock, or we may never hear again.
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            stream.write_all(&buf[..n])?; // 4 bytes always fit in the send buffer
                            echoed += 1;
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }
            }
        }
    }
    Ok((conns.len(), wakeups))
}

fn main() -> std::io::Result<()> {
    raise_fd_limit();
    let listener = TcpListener::bind("127.0.0.1:0".parse().unwrap())?;
    let addr = listener.local_addr()?;
    let rss_before = proc_field("/proc/self/status", "VmRSS:");
    let server = std::thread::spawn(move || serve(listener));

    // Client side, same process, plain blocking sockets: open N idle connections...
    let mut clients = (0..N).map(|_| std::net::TcpStream::connect(addr)).collect::<Result<Vec<_>, _>>()?;
    println!("{N} connections open; threads in process: {}", proc_field("/proc/self/status", "Threads:"));
    println!("kernel TCP sockets: {}", proc_field("/proc/net/sockstat", "TCP:"));
    // ...then send one request on each and read every echo.
    for c in &mut clients {
        c.write_all(b"ping")?;
    }
    let mut echoed = 0;
    for c in &mut clients {
        let mut reply = [0u8; 4];
        c.read_exact(&mut reply)?;
        echoed += (&reply == b"ping") as usize;
    }
    let (served, wakeups) = server.join().unwrap()?;
    println!("{echoed} echoes from {served} connections: 1 server thread, {wakeups} epoll_wait wakeups");
    println!("VmRSS {rss_before} -> {}", proc_field("/proc/self/status", "VmRSS:"));
    Ok(())
}
