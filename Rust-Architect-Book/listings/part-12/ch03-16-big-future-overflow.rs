// verify: debug crash overflowed its stack
// verify: release crash overflowed its stack
//! A future is a value. If it holds a 1 MiB buffer across an .await, it IS more than 1 MiB, and
//! pinning it on a thread's stack needs that much stack. This thread has 512 KiB.
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
    let mut page = [0u8; 1 << 20]; // 1 MiB "render buffer", used after the await
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
            let fut = pin!(render_statement(7)); // the whole state machine lives on this stack
            futures::executor::block_on(fut)
        })
        .unwrap();
    println!("result: {}", worker.join().unwrap());
}
