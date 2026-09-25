// verify: release ok
//! A nonblocking socket never parks the thread: read(2) returns EAGAIN ("WouldBlock") at once.
//! Polling it in a loop works, and burns a core doing nothing useful.
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut conn, _) = listener.accept().unwrap();
    conn.set_nonblocking(true).unwrap(); // fcntl(fd, F_SETFL, O_NONBLOCK)

    let mut buf = [0u8; 64];
    let e = conn.read(&mut buf).unwrap_err();
    println!("empty nonblocking socket: {:?} ({e})", e.kind());

    // The data arrives 20 ms from now. Busy-poll until it does.
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        client.write_all(b"data").unwrap();
        client
    });
    let start = Instant::now();
    let mut attempts = 0u64;
    let n = loop {
        attempts += 1;
        match conn.read(&mut buf) {
            Ok(n) => break n,
            Err(e) if e.kind() == ErrorKind::WouldBlock => continue, // not ready: ask again, immediately
            Err(e) => panic!("{e}"),
        }
    };
    let _client = writer.join().unwrap();
    println!("{attempts} read() calls in {:?} to receive {n} bytes", start.elapsed());
}
