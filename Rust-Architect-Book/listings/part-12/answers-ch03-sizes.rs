// verify: debug ok
// verify: release ok
//! Answer key, Chapter 12.3 beginner exercise: future sizes for a String + u32 held across one
//! .await, and a [u64; 4] held across two consecutive .awaits. Same YieldOnce as ch03-02.
use std::future::Future;
use std::mem::size_of_val;
use std::pin::Pin;
use std::task::{Context, Poll};

struct YieldOnce(bool);
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

async fn string_and_u32(name: String, n: u32) -> usize {
    YieldOnce(false).await; // both arguments are live across this await
    name.len() + n as usize
}

async fn array_two_awaits() -> u64 {
    let a = [1u64, 2, 3, 4];
    YieldOnce(false).await;
    YieldOnce(false).await; // `a` is live across both
    a.iter().sum()
}

async fn string_made_inside(n: u32) -> usize {
    let name = format!("merchant-{n}"); // a local, not an argument
    YieldOnce(false).await;
    name.len()
}

fn main() {
    println!("{:>3} B  String + u32 (arguments) across one await", size_of_val(&string_and_u32(String::new(), 1)));
    println!("{:>3} B  [u64; 4] across two consecutive awaits", size_of_val(&array_two_awaits()));
    println!("{:>3} B  u32 argument, String local across one await", size_of_val(&string_made_inside(1)));
}
