// verify: debug build
//! A two-await async fn, compiled as a library so its MIR can be inspected:
//!   powershell -File tools\emit.ps1 listings\part-12\ch03-01-state-machine.rs -Target mir -Mode debug
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Pending once (waking itself), then Ready: a yield point.
pub struct YieldOnce(bool);

impl Future for YieldOnce {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

pub async fn step(x: u64) -> u64 {
    let a = x + 1;
    YieldOnce(false).await;
    let b = a * 2;
    YieldOnce(false).await;
    a + b
}

/// Makes rustc emit the state machine's poll function (it's reached through the vtable).
pub fn boxed_step(x: u64) -> Pin<Box<dyn Future<Output = u64>>> {
    Box::pin(step(x))
}
