// verify: release ok
//! Chapter 12.5 failure scenario: an executor that queues a task on EVERY wake.
//! A fan-in task awaits N upstream responses. They arrive in two network batches (two
//! epoll_waits), so the same task is woken N/2 times before it runs, twice. Each extra queue
//! entry is a poll, and each poll of the fan-in re-polls every child that is still pending.
use std::collections::VecDeque;
use std::future::Future;
use std::pin::{pin, Pin};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

static CHILD_POLLS: AtomicUsize = AtomicUsize::new(0);

/// One upstream response: its own state, its own stored waker (like one socket registration).
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

/// Polls every unfinished child with the parent's waker (what small join_alls do).
struct JoinAll(Vec<Option<Response>>);
impl Future for JoinAll {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut pending = false;
        for child in self.0.iter_mut() {
            if let Some(r) = child {
                if Pin::new(r).poll(cx).is_ready() {
                    *child = None;
                } else {
                    pending = true;
                }
            }
        }
        if pending { Poll::Pending } else { Poll::Ready(()) }
    }
}

struct TaskWaker {
    queue: Mutex<VecDeque<()>>,
    queued: AtomicBool,
    dedup: bool,
}
impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        if self.dedup && self.queued.swap(true, Ordering::AcqRel) {
            return; // already in the run queue: one entry is enough
        }
        self.queue.lock().unwrap().push_back(());
    }
}

fn run(n: usize, dedup: bool) {
    let slots: Vec<Slot> = (0..n).map(|_| Arc::default()).collect();
    let tw = Arc::new(TaskWaker { queue: Mutex::new(VecDeque::from([()])), queued: AtomicBool::new(true), dedup });
    let waker = Waker::from(tw.clone());
    let mut task = pin!(JoinAll(slots.iter().map(|s| Some(Response(s.clone()))).collect()));
    let mut batches = vec![n / 2..n, 0..n / 2]; // popped from the back: first half, then second half
    CHILD_POLLS.store(0, Ordering::Relaxed);

    let (mut polls, mut finished_entries, mut done) = (0usize, 0usize, false);
    loop {
        let Some(()) = tw.queue.lock().unwrap().pop_front() else {
            // Run queue empty: the "reactor" delivers the next batch of responses, or we're done.
            let Some(batch) = batches.pop() else { break };
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
            continue;
        };
        tw.queued.store(false, Ordering::Release);
        if done {
            finished_entries += 1; // an entry for a task that has already finished
            continue;
        }
        polls += 1;
        done = task.as_mut().poll(&mut Context::from_waker(&waker)).is_ready();
    }
    println!(
        "n = {n:>5}, dedup {dedup:<5}: task polls {polls:>4}, child polls {:>7}, entries after completion {finished_entries:>3}",
        CHILD_POLLS.load(Ordering::Relaxed)
    );
}

fn main() {
    for n in [16, 1_000] {
        run(n, true);
        run(n, false);
    }
}
