// verify: debug build
//! merchant-notify PR #412, as submitted. It compiles, works on the happy path, and has at
//! least a dozen defects. (Part XII review capstone: find them before reading the fixed version.)
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

pub struct Delivery {
    pub merchant: u64,
    pub event_id: u64,
    pub payload: Vec<u8>,
}

#[derive(Default)]
pub struct AckState {
    acked: bool,
    waker: Option<Waker>,
}

#[derive(Default)]
pub struct AckTracker {
    pending: Mutex<HashMap<u64, Arc<Mutex<AckState>>>>,
}

pub struct AckWait(Arc<Mutex<AckState>>);

impl Future for AckWait {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut st = self.0.lock().unwrap();
        if st.acked {
            return Poll::Ready(());
        }
        if st.waker.is_none() {
            st.waker = Some(cx.waker().clone());
        }
        Poll::Pending
    }
}

impl AckTracker {
    /// Called by the connection reader when a merchant's ACK frame arrives.
    pub fn ack(&self, event_id: u64) {
        let pending = self.pending.lock().unwrap();
        if let Some(st) = pending.get(&event_id) {
            let mut st = st.lock().unwrap();
            st.acked = true;
            if let Some(w) = &st.waker {
                w.wake_by_ref();
            }
        }
    }

    pub fn wait(&self, event_id: u64) -> AckWait {
        let st = Arc::new(Mutex::new(AckState::default()));
        self.pending.lock().unwrap().insert(event_id, st.clone());
        AckWait(st)
    }
}

pub trait Connection {
    fn send(&mut self, frame: &[u8]) -> impl Future<Output = std::io::Result<()>>;
}

pub struct Notifier {
    pub tracker: Arc<AckTracker>,
    pub delivered: Mutex<Vec<u64>>,
}

impl Notifier {
    pub async fn deliver<C: Connection>(&self, conn: &mut C, d: Delivery) -> bool {
        let mut frame = [0u8; 65536];
        let n = encode(&d, &mut frame);
        let mut delivered = self.delivered.lock().unwrap();
        delivered.push(d.event_id);
        for attempt in 0..3 {
            if conn.send(&frame[..n]).await.is_ok() {
                let ack = self.tracker.wait(d.event_id);
                return with_timeout(ack, Duration::from_secs(5)).await;
            }
            std::thread::sleep(Duration::from_millis(100 << attempt));
        }
        false
    }
}

pub struct Timeout<F> {
    inner: F,
    deadline: Instant,
}

impl<F> Unpin for Timeout<F> {}

impl<F: Future> Future for Timeout<F> {
    type Output = Option<F::Output>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<F::Output>> {
        if Instant::now() >= self.deadline {
            return Poll::Ready(None);
        }
        // SAFETY: `inner` is never moved.
        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
        inner.poll(cx).map(Some)
    }
}

async fn with_timeout<F: Future>(f: F, d: Duration) -> bool {
    Timeout { inner: f, deadline: Instant::now() + d }.await.is_some()
}

fn encode(d: &Delivery, out: &mut [u8]) -> usize {
    let header = format!("EVT {} {} {}\n", d.merchant, d.event_id, d.payload.len());
    out[..header.len()].copy_from_slice(header.as_bytes());
    out[header.len()..header.len() + d.payload.len()].copy_from_slice(&d.payload);
    header.len() + d.payload.len()
}

/// For the admin API (called from its async handlers).
pub fn deliver_blocking<C: Connection>(n: &Notifier, conn: &mut C, d: Delivery) -> bool {
    futures::executor::block_on(n.deliver(conn, d))
}
