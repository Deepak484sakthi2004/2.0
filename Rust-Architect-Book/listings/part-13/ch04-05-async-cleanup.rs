// verify: debug ok
//! Drop can't .await. A writer that buffers must be flushed and shut down explicitly; Drop can only
//! discard or hand the work to a task (if a runtime still exists). In-memory pipe: deterministic.
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufWriter};

async fn deliver(explicit_close: bool) -> usize {
    let (mut reader, writer) = tokio::io::duplex(1 << 20);
    {
        let mut out = BufWriter::new(writer);
        for i in 0..1_000 {
            out.write_all(format!("settlement line {i}\n").as_bytes()).await.unwrap();
        }
        if explicit_close {
            out.shutdown().await.unwrap(); // flush the buffer, then close the write side
        }
    } // BufWriter dropped here: whatever is still in its buffer is discarded
    let mut received = Vec::new();
    reader.read_to_end(&mut received).await.unwrap();
    received.len()
}

/// A Drop backstop that hands cleanup to the runtime, if there is one.
struct Session(u32);
impl Drop for Session {
    fn drop(&mut self) {
        let id = self.0;
        match tokio::runtime::Handle::try_current() {
            Ok(h) => {
                h.spawn(async move { println!("  session {id}: async cleanup ran in a spawned task") });
            }
            Err(e) => println!("  session {id}: no runtime in Drop ({e}); cleanup skipped"),
        }
    }
}

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    rt.block_on(async {
        println!("bytes written: {}", (0..1_000).map(|i: u32| format!("settlement line {i}\n").len()).sum::<usize>());
        println!("received, dropped without shutdown(): {}", deliver(false).await);
        println!("received, with shutdown().await:      {}", deliver(true).await);
        drop(Session(1));
        tokio::task::yield_now().await;
    });
    drop(Session(2)); // outside the runtime: Handle::try_current() fails
}
