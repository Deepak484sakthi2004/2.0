// verify: debug error:E0277
//! Futures from `async` blocks and fns are !Unpin: they may borrow their own locals.
//! An API that demands `Unpin` rejects them; `Box::pin(fut)` (or `pin!(fut)`) is the fix.
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

fn poll_once<F: Future + Unpin>(mut fut: F) -> Poll<F::Output> {
    Pin::new(&mut fut).poll(&mut Context::from_waker(Waker::noop()))
}

fn main() {
    let fut = async {
        let data = [1u8, 2, 3];
        let view = &data; // a borrow of a local inside the future
        async {}.await;
        view.len()
    };
    println!("{:?}", poll_once(fut));
}
