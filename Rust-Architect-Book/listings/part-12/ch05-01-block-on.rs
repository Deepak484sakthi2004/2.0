// verify: debug ok
// verify: debug miri-ok
//! The smallest real executor: block_on. Waking = unparking the thread that polls.
use std::future::Future;
use std::pin::{pin, Pin};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::Duration;

struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

static PARKS: AtomicU32 = AtomicU32::new(0);

fn block_on<F: Future>(fut: F) -> (F::Output, u32) {
    let mut fut = pin!(fut);
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut polls = 0;
    loop {
        polls += 1;
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return (v, polls),
            // Sleep until unpark(). If the wake already happened, park() returns at once: the
            // unpark token is what prevents the lost-wakeup race. Spurious returns just re-poll.
            Poll::Pending => {
                PARKS.fetch_add(1, Ordering::Relaxed);
                thread::park();
            }
        }
    }
}

/// Wakes itself before returning Pending: exercises the "wake before park" path.
struct YieldTimes(u32);
impl Future for YieldTimes {
    type Output = &'static str;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<&'static str> {
        if self.0 == 0 {
            return Poll::Ready("yielded");
        }
        self.0 -= 1;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// Completed by another thread: exercises the "park, then get unparked" path.
struct Delay {
    state: Arc<Mutex<(bool, Option<Waker>)>>,
    started: bool,
}
impl Future for Delay {
    type Output = &'static str;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<&'static str> {
        let mut st = self.state.lock().unwrap();
        if st.0 {
            return Poll::Ready("timer fired");
        }
        st.1 = Some(cx.waker().clone());
        drop(st);
        if !self.started {
            self.started = true;
            let state = self.state.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(20));
                let waker = {
                    let mut st = state.lock().unwrap();
                    st.0 = true;
                    st.1.take()
                };
                if let Some(w) = waker {
                    w.wake();
                }
            });
        }
        Poll::Pending
    }
}

fn main() {
    let (out, polls) = block_on(YieldTimes(3));
    println!("{out}: {polls} polls, {} parks", PARKS.swap(0, Ordering::Relaxed));
    let (out, polls) = block_on(Delay { state: Arc::default(), started: false });
    println!("{out}: {polls} polls, {} parks (at least 1)", PARKS.swap(0, Ordering::Relaxed));
}
