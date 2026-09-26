// verify: release ok
//! UDP through Tokio: datagram boundaries are kept, oversized datagrams are truncated, and there is no
//! backpressure. A sender never waits for a receiver; when the receiver's socket buffer is full, the kernel
//! drops datagrams silently. Real loopback UDP; buffer sizes are this machine's kernel defaults.
use socket2::SockRef;
use std::time::Duration;
use tokio::net::UdpSocket;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let rx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let tx = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    tx.connect(rx.local_addr().unwrap()).await.unwrap();

    // 1. One send = one datagram = one recv. A datagram bigger than the buffer is cut, not split.
    for size in [5usize, 1_500, 9_000] {
        tx.send(&vec![b'x'; size]).await.unwrap();
    }
    let mut buf = [0u8; 2_048];
    let mut sizes = Vec::new();
    for _ in 0..3 {
        let (n, _) = rx.recv_from(&mut buf).await.unwrap();
        sizes.push(n);
    }
    println!("sent datagrams of [5, 1500, 9000] bytes; recv into a 2048-byte buffer returned {sizes:?}");

    // 2. No backpressure: send 20,000 datagrams of 1,000 bytes while the receiver isn't reading.
    let rcvbuf = SockRef::from(&rx).recv_buffer_size().unwrap();
    let payload = vec![7u8; 1_000];
    let mut sent = 0u32;
    for _ in 0..20_000 {
        if tx.send(&payload).await.is_ok() {
            sent += 1; // send completes as soon as the kernel has taken the datagram
        }
    }
    let mut received = 0u32;
    // Drain whatever the kernel kept; stop when nothing arrives for 50 ms.
    while let Ok(Ok(_)) = tokio::time::timeout(Duration::from_millis(50), rx.recv_from(&mut buf)).await {
        received += 1;
    }
    println!("receiver SO_RCVBUF = {rcvbuf} bytes");
    println!("sent {sent} datagrams without waiting; received {received}; dropped silently {}", sent - received);
}
