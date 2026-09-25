// verify: release ok
//! futures' join_all under the same two-batch workload as ch05-06 (on a correct, deduplicating
//! executor: we poll by hand). Up to 30 children it re-polls every pending child on each poll;
//! from 31 on it switches to FuturesOrdered, where each child has its own waker [LIB].
use std::future::Future;
use std::pin::{pin, Pin};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

static CHILD_POLLS: AtomicUsize = AtomicUsize::new(0);

type Slot = Arc<Mutex<(bool, Option<Waker>)>>;

struct Response(Slot);

impl Future for Response {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        CHILD_POLLS.fetch_add(1, Ordering::Relaxed);
        let mut s = self.0.lock().unwrap();
        if s.0 {
            return Poll::Ready(());
        }
        s.1 = Some(cx.waker().clone());
        Poll::Pending
    }
}

fn run(n: usize) {
    let slots: Vec<Slot> = (0..n).map(|_| Arc::default()).collect();
    let mut fut = pin!(futures::future::join_all(slots.iter().map(|s| Response(s.clone()))));
    let mut cx = Context::from_waker(Waker::noop());
    CHILD_POLLS.store(0, Ordering::Relaxed);
    let mut polls = 0;
    for batch in [0..n / 2, n / 2..n] {
        polls += 1;
        assert!(fut.as_mut().poll(&mut cx).is_pending());
        for s in &slots[batch] {
            let w = {
                let mut st = s.lock().unwrap();
                st.0 = true;
                st.1.take()
            };
            if let Some(w) = w {
                w.wake();
            }
        }
    }
    polls += 1;
    assert!(fut.as_mut().poll(&mut cx).is_ready());
    println!("join_all n={n:>4}: {polls} polls, child polls {}", CHILD_POLLS.load(Ordering::Relaxed));
}

fn main() {
    for n in [16, 30, 31, 1000] {
        run(n);
    }
}
