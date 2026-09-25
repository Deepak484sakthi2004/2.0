// verify: debug test
// verify: debug ok
//! merchant-notify PR #412, after review. Runtime-agnostic: time comes in through a `Clock`, so the
//! tests run on futures' executor with a fake clock, and production plugs in Tokio's timer.
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use futures::future::{self, Either};

pub struct Delivery {
    pub merchant: u64,
    pub event_id: u64,
    pub payload: Vec<u8>,
}

// ---------- acks: register before sending, refresh wakers, clean up on every exit ----------

#[derive(Default)]
struct AckState {
    acked: bool,
    waker: Option<Waker>,
}

#[derive(Default)]
pub struct AckTracker {
    pending: Mutex<HashMap<u64, Arc<Mutex<AckState>>>>,
}

pub struct AckWait {
    state: Arc<Mutex<AckState>>,
    tracker: Arc<AckTracker>,
    event_id: u64,
}

impl AckTracker {
    pub fn ack(&self, event_id: u64) {
        let state = self.pending.lock().unwrap().remove(&event_id); // map lock released here
        let waker = state.and_then(|st| {
            let mut st = st.lock().unwrap();
            st.acked = true;
            st.waker.take()
        });
        if let Some(w) = waker {
            w.wake(); // no lock held while waking
        }
    }

    /// Register interest BEFORE the frame is sent, so an early ACK can't be missed.
    pub fn register(self: &Arc<Self>, event_id: u64) -> AckWait {
        let state = Arc::new(Mutex::new(AckState::default()));
        let previous = self.pending.lock().unwrap().insert(event_id, state.clone());
        assert!(previous.is_none(), "event {event_id} is already awaiting an ack");
        AckWait { state, tracker: self.clone(), event_id }
    }

    pub fn pending_len(&self) -> usize {
        self.pending.lock().unwrap().len()
    }
}

impl Future for AckWait {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut st = self.state.lock().unwrap();
        if st.acked {
            return Poll::Ready(());
        }
        match &mut st.waker {
            Some(w) if w.will_wake(cx.waker()) => {}
            slot => *slot = Some(cx.waker().clone()), // always the CURRENT task's waker
        }
        Poll::Pending
    }
}

impl Drop for AckWait {
    fn drop(&mut self) {
        // Timed out, cancelled, or finished: never leave an entry behind.
        self.tracker.pending.lock().unwrap().remove(&self.event_id);
    }
}

// ---------- the seams: connection and clock ----------

pub trait Connection {
    fn send(&mut self, frame: &[u8]) -> impl Future<Output = std::io::Result<()>> + Send;
}

pub trait Clock: Sync {
    fn sleep(&self, d: Duration) -> impl Future<Output = ()> + Send;
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Acked,
    NoAck,
    SendFailed,
}

pub struct Notifier<K> {
    pub tracker: Arc<AckTracker>,
    pub delivered: Mutex<Vec<u64>>,
    pub clock: K,
}

impl<K: Clock> Notifier<K> {
    /// At-least-once delivery. The event is recorded as delivered only after its ACK.
    /// Cancellation-safe: dropping this future at any .await leaves no pending entry and no false
    /// "delivered" record. `frame` is a per-connection buffer, reused across deliveries.
    pub async fn deliver<C: Connection>(&self, conn: &mut C, frame: &mut Vec<u8>, d: &Delivery) -> Outcome {
        encode(d, frame);
        let ack = self.tracker.register(d.event_id);
        let mut sent = false;
        for attempt in 0..3u32 {
            if conn.send(frame).await.is_ok() {
                sent = true;
                break;
            }
            self.clock.sleep(Duration::from_millis(100 << attempt)).await; // async backoff
        }
        if !sent {
            return Outcome::SendFailed; // `ack` dropped here: its entry is removed
        }
        match future::select(ack, Box::pin(self.clock.sleep(Duration::from_secs(5)))).await {
            Either::Left(((), _)) => {
                self.delivered.lock().unwrap().push(d.event_id); // guard never crosses an .await
                Outcome::Acked
            }
            Either::Right(((), _ack)) => Outcome::NoAck,
        }
    }
}

fn encode(d: &Delivery, frame: &mut Vec<u8>) {
    use std::io::Write;
    frame.clear(); // keeps the capacity; grows only for unusually large payloads
    write!(frame, "EVT {} {} {}\n", d.merchant, d.event_id, d.payload.len()).unwrap();
    frame.extend_from_slice(&d.payload);
}

// ---------- a demo run: an ACK that arrives while the send is still "on the wire" ----------

struct InstantClock;
impl Clock for InstantClock {
    fn sleep(&self, _d: Duration) -> impl Future<Output = ()> + Send {
        future::pending() // "never": no timeout fires in the demo
    }
}

struct AckingConn(Arc<AckTracker>, u64);
impl Connection for AckingConn {
    fn send(&mut self, _frame: &[u8]) -> impl Future<Output = std::io::Result<()>> + Send {
        self.0.ack(self.1); // the merchant's ACK beats our own send completion
        future::ready(Ok(()))
    }
}

fn main() {
    let tracker = Arc::new(AckTracker::default());
    let n = Notifier { tracker: tracker.clone(), delivered: Mutex::default(), clock: InstantClock };
    let d = Delivery { merchant: 7, event_id: 41, payload: b"payment.captured".to_vec() };
    let mut conn = AckingConn(tracker.clone(), 41);
    let mut frame = Vec::with_capacity(512);
    let fut = n.deliver(&mut conn, &mut frame, &d);
    println!("deliver future: {} bytes", size_of_val(&fut));
    let outcome = futures::executor::block_on(fut);
    println!("{outcome:?}; delivered {:?}; pending acks {}", n.delivered.lock().unwrap(), tracker.pending_len());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Every sleep completes at once and is recorded.
    #[derive(Default)]
    struct FakeClock(Mutex<Vec<Duration>>);
    impl Clock for FakeClock {
        fn sleep(&self, d: Duration) -> impl Future<Output = ()> + Send {
            self.0.lock().unwrap().push(d);
            future::ready(())
        }
    }

    struct FlakyConn(AtomicU32);
    impl Connection for FlakyConn {
        fn send(&mut self, _f: &[u8]) -> impl Future<Output = std::io::Result<()>> + Send {
            let failures_left = self.0.fetch_sub(1, Ordering::Relaxed);
            future::ready(if failures_left > 0 { Err(std::io::ErrorKind::BrokenPipe.into()) } else { Ok(()) })
        }
    }

    fn delivery(id: u64) -> Delivery {
        Delivery { merchant: 7, event_id: id, payload: b"x".to_vec() }
    }

    #[test]
    fn ack_reaches_the_task_that_polls_last() {
        let tracker = Arc::new(AckTracker::default());
        let mut wait = tracker.register(1);
        // First polled by some other task (e.g. inside a select elsewhere)...
        let _ = Pin::new(&mut wait).poll(&mut Context::from_waker(Waker::noop()));
        // ...then owned by a real executor on another thread.
        let t = std::thread::spawn(move || futures::executor::block_on(wait));
        std::thread::sleep(Duration::from_millis(20));
        tracker.ack(1);
        t.join().unwrap(); // hangs forever with the PR's register-once waker
    }

    #[test]
    fn no_ack_is_not_delivered_and_leaves_nothing_behind() {
        let tracker = Arc::new(AckTracker::default());
        let n = Notifier { tracker: tracker.clone(), delivered: Mutex::default(), clock: FakeClock::default() };
        let out = futures::executor::block_on(n.deliver(&mut FlakyConn(AtomicU32::new(0)), &mut Vec::new(), &delivery(2)));
        assert_eq!(out, Outcome::NoAck);
        assert!(n.delivered.lock().unwrap().is_empty());
        assert_eq!(tracker.pending_len(), 0);
    }

    #[test]
    fn retries_back_off_asynchronously() {
        let n = Notifier { tracker: Arc::default(), delivered: Mutex::default(), clock: FakeClock::default() };
        let out = futures::executor::block_on(n.deliver(&mut FlakyConn(AtomicU32::new(2)), &mut Vec::new(), &delivery(3)));
        assert_eq!(out, Outcome::NoAck); // sent on the third try, never acked (fake clock times out)
        let sleeps = n.clock.0.lock().unwrap().clone();
        assert_eq!(sleeps, [Duration::from_millis(100), Duration::from_millis(200), Duration::from_secs(5)]);
    }

    #[test]
    fn cancelled_delivery_leaves_nothing_behind() {
        let tracker = Arc::new(AckTracker::default());
        let n = Notifier { tracker: tracker.clone(), delivered: Mutex::default(), clock: InstantClock };
        let mut conn = FlakyConn(AtomicU32::new(0));
        let mut frame = Vec::new();
        let d = delivery(4);
        let mut fut = Box::pin(n.deliver(&mut conn, &mut frame, &d));
        assert!(fut.as_mut().poll(&mut Context::from_waker(Waker::noop())).is_pending()); // waiting for the ack
        assert_eq!(tracker.pending_len(), 1);
        drop(fut); // cancelled, e.g. the connection closed
        assert_eq!(tracker.pending_len(), 0);
        assert!(n.delivered.lock().unwrap().is_empty());
    }

    #[test]
    fn deliver_future_is_small_and_send() {
        fn assert_send<T: Send>(_: &T) {}
        let n = Notifier { tracker: Arc::default(), delivered: Mutex::default(), clock: FakeClock::default() };
        let mut conn = FlakyConn(AtomicU32::new(0));
        let mut frame = Vec::new();
        let d = delivery(5);
        let fut = n.deliver(&mut conn, &mut frame, &d);
        assert_send(&fut);
        assert!(size_of_val(&fut) < 512, "future is {} bytes", size_of_val(&fut));
    }
}
