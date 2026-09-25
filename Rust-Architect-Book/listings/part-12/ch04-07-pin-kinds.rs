// verify: debug ok
//! Three ways to pin, and what each costs: Pin::new (only for Unpin types, free),
//! pin! (on the stack, no allocation, can't outlive the scope), Box::pin (heap, can be moved around).
//! Also: while pinned, a local inside an async block keeps its address across polls.
use std::future::Future;
use std::pin::{pin, Pin};
use std::task::{Context, Poll, Waker};

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

/// Records the address of its local before and after a suspension.
async fn where_is_my_local() -> (usize, usize) {
    let local = [0u64; 4];
    let before = &local as *const _ as usize;
    YieldOnce(false).await;
    let after = &local as *const _ as usize;
    (before, after)
}

fn drive<F: Future>(mut fut: Pin<&mut F>) -> (F::Output, u32) {
    let mut cx = Context::from_waker(Waker::noop());
    let mut polls = 0;
    loop {
        polls += 1;
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return (v, polls);
        }
    }
}

fn main() {
    // 1. Unpin types can be pinned with Pin::new: pinning them promises nothing.
    let mut ready = std::future::ready(7);
    let (v, polls) = drive(Pin::new(&mut ready));
    println!("Pin::new(&mut Ready): {v} after {polls} poll");

    // 2. pin!: pinned on this stack frame. No allocation.
    let fut = pin!(where_is_my_local());
    let ((before, after), polls) = drive(fut);
    println!("pin!:     local at the same address after {polls} polls: {}", before == after);

    // 3. Box::pin: pinned on the heap; the Pin<Box<_>> itself can move freely.
    let boxed = Box::pin(where_is_my_local());
    let mut moved_around = vec![boxed]; // moving the Box doesn't move the future
    let ((before, after), polls) = drive(moved_around[0].as_mut());
    println!("Box::pin: local at the same address after {polls} polls: {}", before == after);
    println!("sizes: Pin<&mut F> {} B, Pin<Box<F>> {} B", size_of::<Pin<&mut u8>>(), size_of::<Pin<Box<u8>>>());
}
