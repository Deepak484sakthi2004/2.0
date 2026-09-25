// verify: debug ok
// verify: debug miri-ok
//! A future combinator written by hand: `WithBudget` fails its inner future after N polls.
//! Polling the inner future needs Pin<&mut F> from Pin<&mut Self>: a "pin projection", done here
//! with `unsafe`, and checked by Miri with a self-referential inner future.
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

/// Self-referential once suspended: `view` borrows `data`, and both live inside the future.
async fn checksum() -> usize {
    let data = [1u8, 2, 3, 4];
    let view: &[u8] = &data;
    YieldOnce(false).await;
    view.iter().map(|&b| b as usize).sum()
}

#[derive(Debug)]
struct BudgetExceeded;

struct WithBudget<F> {
    inner: F,        // structurally pinned: if WithBudget is pinned, so is `inner`
    polls_left: u32, // not pinned: plain data, freely mutable
}

impl<F: Future> Future for WithBudget<F> {
    type Output = Result<F::Output, BudgetExceeded>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: we never move `inner` out of `self`: no mem::swap/replace on it, no
        // `impl Unpin for WithBudget<F>` without `F: Unpin` (the auto trait gets this right),
        // no Drop impl that moves it, no #[repr(packed)]. `polls_left` is not structurally pinned.
        let this = unsafe { self.get_unchecked_mut() };
        if this.polls_left == 0 {
            return Poll::Ready(Err(BudgetExceeded));
        }
        this.polls_left -= 1;
        // SAFETY: `this.inner` is pinned because `self` is, and it stays where it is until dropped.
        let inner = unsafe { Pin::new_unchecked(&mut this.inner) };
        inner.poll(cx).map(Ok)
    }
}

fn main() {
    let ok = futures::executor::block_on(WithBudget { inner: checksum(), polls_left: 5 });
    let too_slow = futures::executor::block_on(WithBudget { inner: checksum(), polls_left: 1 });
    println!("budget 5: {ok:?}");
    println!("budget 1: {too_slow:?}");
}
