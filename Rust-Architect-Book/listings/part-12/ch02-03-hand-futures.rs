// verify: debug ok
//! Two hand-written futures and the one rule that comes with Poll::Pending:
//! before returning Pending, arrange for the waker to be called when progress is possible.
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

/// Completes on its (n+1)-th poll. Each Pending wakes itself at once: a cooperative "yield".
struct Countdown {
    left: u32,
    polls: u32,
}

impl Future for Countdown {
    type Output = u32;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u32> {
        self.polls += 1;
        if self.left == 0 {
            return Poll::Ready(self.polls);
        }
        self.left -= 1;
        cx.waker().wake_by_ref(); // the obligation: someone must call wake, here it's us
        Poll::Pending
    }
}

/// Completes when a timer thread says so. The waker is the only way back to the executor.
struct Delay {
    shared: Arc<Mutex<DelayState>>,
    duration: Duration,
    started: bool,
}

#[derive(Default)]
struct DelayState {
    done: bool,
    waker: Option<Waker>,
    polls: u32,
}

impl Delay {
    fn new(duration: Duration) -> Self {
        Delay { shared: Arc::default(), duration, started: false }
    }
}

impl Future for Delay {
    type Output = u32; // how many times we were polled
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u32> {
        let mut state = self.shared.lock().unwrap();
        state.polls += 1;
        if state.done {
            return Poll::Ready(state.polls);
        }
        state.waker = Some(cx.waker().clone()); // register BEFORE returning Pending
        drop(state);
        if !self.started {
            self.started = true; // lazy: the timer starts on the first poll, not at construction
            let (shared, duration) = (self.shared.clone(), self.duration);
            std::thread::spawn(move || {
                std::thread::sleep(duration);
                let mut state = shared.lock().unwrap();
                state.done = true;
                if let Some(waker) = state.waker.take() {
                    waker.wake(); // the executor learns it's worth polling again
                }
            });
        }
        Poll::Pending
    }
}

fn main() {
    let polls = futures::executor::block_on(Countdown { left: 3, polls: 0 });
    println!("Countdown(3) finished on poll {polls}");

    let start = Instant::now();
    let polls = futures::executor::block_on(Delay::new(Duration::from_millis(30)));
    let ms = start.elapsed().as_millis();
    println!("Delay(30 ms) finished after ~{} ms, polled {polls} times", ms / 10 * 10);
}
