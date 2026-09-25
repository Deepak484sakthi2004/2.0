// verify: debug ok
//! `step` from ch03-01, written by hand as the enum rustc's coroutine transform is equivalent to.
//! (rustc doesn't generate this Rust source; it rewrites MIR. The shape is the same.)
use std::future::Future;
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

async fn step(x: u64) -> u64 {
    let a = x + 1;
    YieldOnce(false).await;
    let b = a * 2;
    YieldOnce(false).await;
    a + b
}

/// One variant per suspension point, holding exactly what is live there.
enum Step {
    Unresumed { x: u64 },
    Suspend0 { a: u64, awaitee: YieldOnce },
    Suspend1 { a: u64, b: u64, awaitee: YieldOnce },
    Returned,
}

impl Future for Step {
    type Output = u64;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u64> {
        loop {
            match &mut *self {
                Step::Unresumed { x } => {
                    let a = *x + 1;
                    *self = Step::Suspend0 { a, awaitee: YieldOnce(false) };
                }
                Step::Suspend0 { a, awaitee } => match Pin::new(awaitee).poll(cx) {
                    Poll::Pending => return Poll::Pending, // state is already saved in `self`
                    Poll::Ready(()) => {
                        let a = *a;
                        *self = Step::Suspend1 { a, b: a * 2, awaitee: YieldOnce(false) };
                    }
                },
                Step::Suspend1 { a, b, awaitee } => match Pin::new(awaitee).poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(()) => {
                        let out = *a + *b;
                        *self = Step::Returned;
                        return Poll::Ready(out);
                    }
                },
                Step::Returned => panic!("`Step` resumed after completion"),
            }
        }
    }
}

fn main() {
    let by_compiler = step(20);
    let by_hand = Step::Unresumed { x: 20 };
    println!("sizes: compiler {} B, hand-written {} B", size_of_val(&by_compiler), size_of_val(&by_hand));
    let a = futures::executor::block_on(by_compiler);
    let b = futures::executor::block_on(by_hand);
    println!("results: compiler {a}, hand-written {b}");
}
