// verify: debug ok
// verify: release ok
//! The fix for ch03-16: keep large buffers on the heap. The future shrinks to a few dozen bytes,
//! and the same 512 KiB thread runs it.
use std::future::Future;
use std::pin::{pin, Pin};
use std::task::{Context, Poll};

struct YieldOnce(bool);
impl Future for YieldOnce {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

async fn render_statement(seed: u8) -> u8 {
    let mut page = vec![0u8; 1 << 20]; // 1 MiB on the heap; the future holds a 24-byte Vec header
    page[0] = seed;
    std::hint::black_box(&mut page); // keep the optimizer from shrinking the buffer away
    YieldOnce(false).await;
    page[0].wrapping_add(page[1 << 19])
}

fn main() {
    println!("future size: {} bytes", size_of_val(&render_statement(1)));
    let worker = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let fut = pin!(render_statement(7));
            futures::executor::block_on(fut)
        })
        .unwrap();
    println!("result: {}", worker.join().unwrap());
}
