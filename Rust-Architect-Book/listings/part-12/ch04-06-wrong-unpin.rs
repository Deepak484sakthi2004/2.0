// verify: debug miri dangling
//! ch04-05 plus ONE line of safe code: `impl<F> Unpin for WithBudget<F> {}`.
//! The unsafe projection's SAFETY argument silently stops being true: safe code can now move a
//! WithBudget after its inner future has stored a pointer into itself. Miri reports the result.
//! (Not run natively: it's undefined behavior, and whatever it printed would mean nothing.)
use std::future::Future;
use std::pin::Pin;
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

async fn checksum() -> usize {
    let data = [1u8, 2, 3, 4];
    let view: &[u8] = &data;
    YieldOnce(false).await;
    view.iter().map(|&b| b as usize).sum()
}

struct WithBudget<F> {
    inner: F,
    polls_left: u32,
}

impl<F> Unpin for WithBudget<F> {} // BUG: a safe impl that contradicts the projection below

impl<F: Future> Future for WithBudget<F> {
    type Output = Option<F::Output>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: (claimed) same argument as ch04-05. False now: `Unpin` lets callers move us.
        let this = unsafe { self.get_unchecked_mut() };
        if this.polls_left == 0 {
            return Poll::Ready(None);
        }
        this.polls_left -= 1;
        let inner = unsafe { Pin::new_unchecked(&mut this.inner) };
        inner.poll(cx).map(Some)
    }
}

/// Moves the value out of the box and frees the box's allocation.
fn unbox<T>(b: Box<T>) -> T {
    *b
}

fn main() {
    let mut cx = Context::from_waker(Waker::noop());
    let mut boxed = Box::new(WithBudget { inner: checksum(), polls_left: 5 });
    let first = Pin::new(&mut *boxed).poll(&mut cx); // Pin::new is allowed: we claimed Unpin
    println!("first poll: {first:?} (the inner future now points into the box)");
    let mut moved = unbox(boxed); // safe code: move it out; the heap block is freed
    let second = Pin::new(&mut moved).poll(&mut cx); // inner reads through its stale pointer
    println!("second poll: {second:?}");
}
