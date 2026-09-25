// verify: debug ok
//! payments-core's duplicate-request waiters (Chapter 12.2 production scenario).
//! The first request with an idempotency key does the work; duplicates that arrive while it's in
//! progress wait on a future that is woken when the first one finishes, and get the same result.
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

#[derive(Default)]
struct Slot {
    result: Option<String>,
    waiters: Vec<Waker>,
}

#[derive(Default)]
struct InFlight {
    slots: Mutex<HashMap<String, Arc<Mutex<Slot>>>>,
    processor_calls: Mutex<u32>,
}

/// Resolves when the slot has a result. Leaves its CURRENT waker in the list on every Pending.
struct WaitFor(Arc<Mutex<Slot>>);

impl Future for WaitFor {
    type Output = String;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<String> {
        let mut slot = self.0.lock().unwrap();
        if let Some(r) = &slot.result {
            return Poll::Ready(r.clone());
        }
        if !slot.waiters.iter().any(|w| w.will_wake(cx.waker())) {
            slot.waiters.push(cx.waker().clone());
        }
        Poll::Pending
    }
}

impl InFlight {
    async fn charge(&self, key: &str, request: u32) -> String {
        let (slot, first) = {
            let mut slots = self.slots.lock().unwrap(); // never held across an .await
            match slots.get(key) {
                Some(s) => (s.clone(), false),
                None => {
                    let s = Arc::new(Mutex::new(Slot::default()));
                    slots.insert(key.to_string(), s.clone());
                    (s, true)
                }
            }
        };
        if !first {
            let r = WaitFor(slot).await;
            return format!("request {request}: duplicate, got {r}");
        }
        let charge_id = self.call_processor().await;
        let waiters = {
            let mut s = slot.lock().unwrap();
            s.result = Some(charge_id.clone());
            std::mem::take(&mut s.waiters)
        };
        for w in waiters {
            w.wake(); // wake after releasing the lock
        }
        format!("request {request}: did the work, {charge_id}")
    }

    async fn call_processor(&self) -> String {
        let n = {
            let mut calls = self.processor_calls.lock().unwrap();
            *calls += 1;
            *calls
        };
        let (tx, rx) = futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30)); // the card processor is slow
            let _ = tx.send(());
        });
        let _ = rx.await;
        format!("ch_{n}")
    }
}

fn main() {
    let inflight = InFlight::default();
    let results = futures::executor::block_on(futures::future::join_all(
        (1..=4).map(|i| inflight.charge("idem-7f3a", i)),
    ));
    for r in results {
        println!("{r}");
    }
    println!("processor calls: {}", inflight.processor_calls.lock().unwrap());
}
