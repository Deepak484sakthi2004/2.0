// verify: release ok
//! A burst of 1,000 simultaneous connects against the kernel's accept queue. When the queue is full, Linux drops
//! the handshake's final ACK: the CLIENT's connect() has already returned (it thinks it is connected), but the
//! SERVER accepts the connection only after it retransmits its SYN-ACK, 1 s, then 2 s, ... later.
//! Real loopback TCP; one run on a shared machine. Each burst is observed for at most 4.5 s.
use std::time::{Duration, Instant};
use tokio::net::{TcpSocket, TcpStream};

async fn burst(backlog: u32, clients: usize) {
    let socket = TcpSocket::new_v4().unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let listener = socket.listen(backlog).unwrap();
    let addr = listener.local_addr().unwrap();
    let start = Instant::now();
    // The accept loop records WHEN each connection was accepted, for at most 4.5 s.
    let acceptor = tokio::spawn(async move {
        let mut accepted_at = Vec::with_capacity(clients);
        let mut held = Vec::with_capacity(clients);
        let deadline = tokio::time::Instant::now() + Duration::from_millis(4_500);
        while accepted_at.len() < clients {
            match tokio::time::timeout_at(deadline, listener.accept()).await {
                Ok(Ok((s, _))) => {
                    accepted_at.push(start.elapsed());
                    held.push(s);
                }
                _ => break,
            }
        }
        accepted_at
    });
    let connects: Vec<_> = (0..clients)
        .map(|_| {
            tokio::spawn(async move {
                let t = Instant::now();
                let s = TcpStream::connect(addr).await.unwrap();
                (t.elapsed(), s)
            })
        })
        .collect();
    let mut connect_times = Vec::with_capacity(clients);
    let mut streams = Vec::with_capacity(clients);
    for c in connects {
        let (d, s) = c.await.unwrap();
        connect_times.push(d);
        streams.push(s);
    }
    connect_times.sort();
    let accepted = acceptor.await.unwrap();
    let by = |ms: u64| accepted.iter().filter(|d| **d <= Duration::from_millis(ms)).count();
    println!(
        "backlog {backlog:>5}: client view: all {clients} connect() calls returned, slowest {:>7.1?} | server view: accepted within 0.5 s {:>4}, 1.5 s {:>4}, 4.5 s {:>4}",
        connect_times[clients - 1],
        by(500),
        by(1_500),
        accepted.len()
    );
}

#[tokio::main]
async fn main() {
    // Each connection needs a descriptor on both ends: raise the soft limit.
    let mut lim = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: getrlimit/setrlimit only read and write the struct we pass; RLIMIT_NOFILE is a valid resource.
    unsafe {
        libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim);
        lim.rlim_cur = lim.rlim_max;
        libc::setrlimit(libc::RLIMIT_NOFILE, &lim);
    }
    burst(128, 1_000).await;
    burst(2_048, 1_000).await;
}
