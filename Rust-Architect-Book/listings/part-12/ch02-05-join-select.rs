// verify: debug ok
//! join and select written by hand. join polls both children until both finish; select returns
//! the first to finish and DROPS the other: in Rust, cancellation is just drop.
//! (The `Unpin` bounds keep Pin projection out of this chapter; Chapter 12.4 removes them.)
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

/// A thread-backed timer future, eager for brevity (the thread starts at construction).
fn sleep_ms(ms: u64) -> Pin<Box<dyn Future<Output = u64> + Send>> {
    let (tx, rx) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        let _ = tx.send(()); // fails harmlessly if the receiver was dropped (cancelled)
    });
    Box::pin(async move {
        let _ = rx.await;
        ms
    })
}

struct Join2<A: Future, B: Future> {
    a: A,
    b: B,
    a_out: Option<A::Output>,
    b_out: Option<B::Output>,
}

impl<A, B> Future for Join2<A, B>
where
    A: Future + Unpin,
    B: Future + Unpin,
    A::Output: Unpin,
    B::Output: Unpin,
{
    type Output = (A::Output, B::Output);
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        if this.a_out.is_none() {
            if let Poll::Ready(v) = Pin::new(&mut this.a).poll(cx) {
                this.a_out = Some(v); // never poll a finished future again
            }
        }
        if this.b_out.is_none() {
            if let Poll::Ready(v) = Pin::new(&mut this.b).poll(cx) {
                this.b_out = Some(v);
            }
        }
        match (this.a_out.is_some(), this.b_out.is_some()) {
            (true, true) => Poll::Ready((this.a_out.take().unwrap(), this.b_out.take().unwrap())),
            _ => Poll::Pending, // each child that said Pending has registered cx's waker
        }
    }
}

#[derive(Debug)]
enum Either<L, R> {
    Left(L),
    Right(R),
}

struct Select2<A, B> {
    a: A,
    b: B,
}

impl<A: Future + Unpin, B: Future + Unpin> Future for Select2<A, B> {
    type Output = Either<A::Output, B::Output>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Poll::Ready(v) = Pin::new(&mut self.a).poll(cx) {
            return Poll::Ready(Either::Left(v));
        }
        if let Poll::Ready(v) = Pin::new(&mut self.b).poll(cx) {
            return Poll::Ready(Either::Right(v));
        }
        Poll::Pending
    }
}

/// Wraps a future and reports how it ended: completed, or dropped mid-flight.
struct Loud<F> {
    name: &'static str,
    inner: F,
    polls: u32,
    finished: bool,
}

impl<F: Future + Unpin> Future for Loud<F> {
    type Output = F::Output;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
        self.polls += 1;
        let out = Pin::new(&mut self.inner).poll(cx);
        self.finished = out.is_ready();
        out
    }
}

impl<F> Drop for Loud<F> {
    fn drop(&mut self) {
        if !self.finished {
            println!("   {} dropped after {} poll(s), unfinished: cancelled", self.name, self.polls);
        }
    }
}

fn loud<F>(name: &'static str, inner: F) -> Loud<F> {
    Loud { name, inner, polls: 0, finished: false }
}

fn main() {
    let start = Instant::now();
    let (a, b) = futures::executor::block_on(Join2 { a: sleep_ms(30), b: sleep_ms(20), a_out: None, b_out: None });
    println!("join(30 ms, 20 ms) -> ({a}, {b}) after ~{} ms", start.elapsed().as_millis() / 10 * 10);

    let start = Instant::now();
    let winner = futures::executor::block_on(Select2 { a: loud("fast", sleep_ms(10)), b: loud("slow", sleep_ms(50)) });
    println!("select(10 ms, 50 ms) -> {winner:?} after ~{} ms", start.elapsed().as_millis() / 10 * 10);
}
