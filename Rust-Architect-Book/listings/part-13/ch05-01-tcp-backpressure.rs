// verify: release ok
//! Backpressure below Tokio: when a peer stops reading, the kernel's socket buffers fill, the TCP
//! window closes, and the writer's write() stops completing. Tokio just parks the task (Pending).
//! Real loopback TCP; buffer sizes are the kernel's defaults on this machine.
use socket2::SockRef;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (resume_tx, resume_rx) = tokio::sync::oneshot::channel::<()>();

    // The slow consumer: accepts, then doesn't read until told to.
    let server = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let rcvbuf = SockRef::from(&sock).recv_buffer_size().unwrap();
        resume_rx.await.unwrap();
        let mut buf = vec![0u8; 1 << 16];
        let mut total = 0usize;
        loop {
            let n = sock.read(&mut buf).await.unwrap();
            if n == 0 {
                return (rcvbuf, total);
            }
            total += n;
        }
    });

    let mut client = TcpStream::connect(addr).await.unwrap();
    let sndbuf = SockRef::from(&client).send_buffer_size().unwrap();
    let chunk = vec![7u8; 64 * 1024];
    let mut accepted = 0usize;
    // Write until a write makes no progress for 200 ms.
    loop {
        match timeout(Duration::from_millis(200), client.write(&chunk)).await {
            Ok(Ok(n)) => accepted += n,
            Ok(Err(e)) => panic!("{e}"),
            Err(_) => break, // the kernel won't take more: this task is parked on the socket's write readiness
        }
    }
    println!("peer not reading: the kernel accepted {accepted} bytes ({:.1} MiB), then write() stopped completing", accepted as f64 / 1048576.0);
    println!("client SO_SNDBUF = {sndbuf} bytes");

    // Let the consumer read: the window reopens and the parked writer continues.
    resume_tx.send(()).unwrap();
    let more = 8 * 1024 * 1024;
    client.write_all(&vec![1u8; more]).await.unwrap();
    client.shutdown().await.unwrap();
    let (rcvbuf, total) = server.await.unwrap();
    println!("server SO_RCVBUF = {rcvbuf} bytes");
    println!("after the consumer resumed: {total} bytes received in total ({} + {more})", total - more);
}
