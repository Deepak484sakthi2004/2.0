// verify: debug ok
//! select! drops the losing branches' futures. If a losing future had already consumed input, that input
//! is gone. read_exact is not cancellation safe; a framed reader keeps its buffer in the reader, not in the
//! future, so it is. In-memory pipe (tokio::io::duplex) + paused clock: the interleaving is deterministic.
use futures::StreamExt;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::time::sleep;
use tokio_util::codec::{FramedRead, LengthDelimitedCodec};

/// The peer: three 8-byte frames (frame f = f*10 .. f*10+7). Each is written as 3 bytes, then the rest
/// 15 ms later; frames start 30 ms apart. Then the peer hangs up.
async fn peer(mut w: DuplexStream, length_prefixed: bool) {
    for f in 0..3u8 {
        let mut bytes = Vec::new();
        if length_prefixed {
            bytes.extend_from_slice(&8u32.to_be_bytes());
        }
        bytes.extend((0..8).map(|i| f * 10 + i));
        let (a, b) = bytes.split_at(3);
        w.write_all(a).await.unwrap();
        sleep(Duration::from_millis(15)).await;
        w.write_all(b).await.unwrap();
        sleep(Duration::from_millis(15)).await;
    }
}

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    // BROKEN: a fresh read_exact future each loop turn, raced against a 25 ms housekeeping tick.
    let (mut r, w) = tokio::io::duplex(64);
    tokio::spawn(peer(w, false));
    let (mut frames, mut ticks) = (Vec::new(), 0);
    loop {
        let mut buf = [0u8; 8];
        tokio::select! {
            res = r.read_exact(&mut buf) => match res {
                Ok(_) => frames.push(buf.to_vec()),
                Err(e) => { println!("read_exact in select!: stream ended: {e}"); break; }
            },
            _ = sleep(Duration::from_millis(25)) => { ticks += 1; } // housekeeping: flush metrics, check deadlines
        }
    }
    let got: usize = frames.iter().map(Vec::len).sum();
    println!("read_exact in select!: {frames:?}");
    println!("  {ticks} ticks; 24 bytes sent, {got} bytes delivered in frames, {} lost", 24 - got);

    // FIXED: FramedRead owns the partial frame between polls; its next() future is cancellation safe.
    let (r, w) = tokio::io::duplex(64);
    tokio::spawn(peer(w, true));
    let mut framed = FramedRead::new(r, LengthDelimitedCodec::new());
    let (mut frames, mut ticks) = (Vec::new(), 0);
    loop {
        tokio::select! {
            f = framed.next() => match f {
                Some(frame) => frames.push(frame.unwrap().to_vec()),
                None => break, // clean end of stream
            },
            _ = sleep(Duration::from_millis(25)) => { ticks += 1; }
        }
    }
    println!("FramedRead in select!: {frames:?}");
    println!("  {ticks} ticks; every frame intact");
}
