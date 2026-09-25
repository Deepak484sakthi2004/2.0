// verify: release ok
//! Thread-per-connection: every accepted connection gets its own OS thread, blocked in read(2).
//! Open idle connections until the OS refuses to create another thread, and watch memory on the way.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

/// One field of /proc/self/status (Linux), e.g. "VmRSS" -> "7360 kB".
fn status(field: &str) -> String {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = s.lines().find(|l| l.starts_with(field)).unwrap_or_default();
    line.split_whitespace().skip(1).collect::<Vec<_>>().join(" ")
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (report_tx, report_rx) = mpsc::channel::<Result<usize, String>>();

    // The accept loop: one blocking handler thread per connection.
    thread::spawn(move || {
        for (i, stream) in listener.incoming().enumerate() {
            let mut stream = stream.unwrap();
            let spawned = thread::Builder::new().spawn(move || {
                let mut buf = [0u8; 512];
                // Parked in read(2) until the client sends something or hangs up.
                while let Ok(n) = stream.read(&mut buf) {
                    if n == 0 || stream.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
            });
            let report = spawned.map(|_| i + 1).map_err(|e| format!("connection {}: {e}", i + 1));
            let failed = report.is_err();
            report_tx.send(report).unwrap();
            if failed {
                break;
            }
        }
    });

    println!(
        "baseline:         threads {:>3} | VmSize {:>11} | VmRSS {}",
        status("Threads"),
        status("VmSize"),
        status("VmRSS")
    );
    let mut clients = Vec::new();
    loop {
        clients.push(TcpStream::connect(addr).unwrap()); // an idle client: connects, sends nothing
        match report_rx.recv().unwrap() {
            Ok(n) if [1, 100, 250, 500].contains(&n) => println!(
                "{n:>4} connections: threads {:>3} | VmSize {:>11} | VmRSS {}",
                status("Threads"),
                status("VmSize"),
                status("VmRSS")
            ),
            Ok(_) => {}
            Err(e) => {
                println!("thread spawn failed at {e}");
                break;
            }
        }
    }

    // The idle connections are real: the first one is still served by its own thread.
    clients[0].write_all(b"ping").unwrap();
    let mut reply = [0u8; 4];
    clients[0].read_exact(&mut reply).unwrap();
    println!("connection 1 still echoes: {}", String::from_utf8_lossy(&reply));
    std::process::exit(0); // the handler threads are blocked in read(2); don't wait for them
}
