// verify: debug ok
// verify: release ok
//! What a future's size is made of: whatever is live across an .await, plus the state tag,
//! plus the child future being awaited. Sizes are computed by rustc [RUSTC], not guaranteed.
use std::future::Future;
use std::mem::size_of_val;
use std::pin::Pin;
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
fn yield_now() -> YieldOnce {
    YieldOnce(false)
}

async fn nothing() {}

async fn add_one(x: u64) -> u64 {
    x + 1
}

async fn step(x: u64) -> u64 {
    let a = x + 1;
    yield_now().await;
    let b = a * 2;
    yield_now().await;
    a + b
}

async fn buffer_across_await() -> u8 {
    let buf = [7u8; 1024];
    yield_now().await; // `buf` is used after this point, so it lives in the future
    buf[0]
}

async fn buffer_before_await() -> u32 {
    let sum: u32 = {
        let buf = [7u8; 1024];
        buf.iter().map(|&b| b as u32).sum()
    }; // `buf` is dead before the await: not stored
    yield_now().await;
    sum
}

async fn heap_buffer_across_await() -> u8 {
    let buf = vec![7u8; 1024]; // only the Vec header (24 bytes) lives in the future
    yield_now().await;
    buf[0]
}

async fn sequential() -> u8 {
    let a = buffer_across_await().await; // child futures awaited one after the other
    let b = buffer_across_await().await; // can share the same bytes
    a + b
}

async fn concurrent() -> u8 {
    let (a, b) = futures::join!(buffer_across_await(), buffer_across_await()); // both alive at once
    a + b
}

async fn boxed_child() -> u8 {
    Box::pin(buffer_across_await()).await // the child lives on the heap
}

fn main() {
    println!("{:>5} B  async fn nothing()", size_of_val(&nothing()));
    println!("{:>5} B  async fn add_one(x: u64)", size_of_val(&add_one(1)));
    println!("{:>5} B  async fn step(x: u64), two awaits", size_of_val(&step(1)));
    println!("{:>5} B  1 KiB array live across an await", size_of_val(&buffer_across_await()));
    println!("{:>5} B  1 KiB array dead before the await", size_of_val(&buffer_before_await()));
    println!("{:>5} B  Vec<u8> of 1 KiB across an await", size_of_val(&heap_buffer_across_await()));
    println!("{:>5} B  two 1 KiB children awaited in sequence", size_of_val(&sequential()));
    println!("{:>5} B  two 1 KiB children joined", size_of_val(&concurrent()));
    println!("{:>5} B  a 1 KiB child behind Box::pin", size_of_val(&boxed_child()));
    let erased: Pin<Box<dyn Future<Output = u8>>> = Box::pin(buffer_across_await());
    println!("{:>5} B  Pin<Box<dyn Future<Output = u8>>>", size_of_val(&erased));
}
