// verify: debug ok
// verify: debug test
//! Part XI review capstone: the partner-quota PR, as submitted.
//!
//! It compiles, its author's test passes, and it works for one request at a time. `main` runs it the way the
//! gateway would: many worker threads admitting requests for the same partner at once.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Duration;

pub static ADMITTED_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static BILLING_EVENTS_UPLOADED: AtomicU64 = AtomicU64::new(0);

pub struct QuotaTracker {
    limits: RwLock<HashMap<String, u64>>, // requests per window, per partner
    usage: Mutex<HashMap<String, u64>>,   // requests admitted in the current window
    inflight: Mutex<HashMap<String, u64>>, // requests currently being served, per partner
    events: Sender<String>,               // one usage event per admitted request, for billing
    on_exceeded: Box<dyn Fn(&str) + Send + Sync>,
}

pub struct Slot<'a> {
    tracker: &'a QuotaTracker,
    partner: String,
}

impl QuotaTracker {
    pub fn new(limits: HashMap<String, u64>, on_exceeded: Box<dyn Fn(&str) + Send + Sync>) -> Arc<QuotaTracker> {
        let (tx, rx) = mpsc::channel::<String>();
        // The billing reporter: batches events and uploads every 50.
        thread::spawn(move || {
            let mut batch = Vec::new();
            for event in rx {
                batch.push(event);
                if batch.len() == 50 {
                    thread::sleep(Duration::from_millis(5)); // "upload"
                    BILLING_EVENTS_UPLOADED.fetch_add(50, Ordering::Relaxed);
                    batch.clear();
                }
            }
        });
        Arc::new(QuotaTracker {
            limits: RwLock::new(limits),
            usage: Mutex::new(HashMap::new()),
            inflight: Mutex::new(HashMap::new()),
            events: tx,
            on_exceeded,
        })
    }

    fn limit_for(&self, partner: &str) -> u64 {
        *self.limits.read().unwrap().get(partner).unwrap_or(&100)
    }

    /// Admit one request for `partner` if it is under its quota for this window.
    pub fn try_acquire(&self, partner: &str) -> bool {
        let used = *self.usage.lock().unwrap().get(partner).unwrap_or(&0);
        if used >= self.limit_for(partner) {
            let _usage = self.usage.lock().unwrap(); // a consistent view for the alert
            (self.on_exceeded)(partner);
            return false;
        }
        write_audit(&format!("admit {partner}")); // compliance: every admission is audited
        *self.usage.lock().unwrap().entry(partner.to_string()).or_insert(0) += 1;
        let n = ADMITTED_TOTAL.load(Ordering::Relaxed);
        ADMITTED_TOTAL.store(n + 1, Ordering::Relaxed);
        self.events.send(format!("{partner} +1")).unwrap();
        true
    }

    /// A concurrency slot: at most `max` requests of one partner in flight at once.
    pub fn acquire_slot(&self, partner: &str, max: u64) -> Option<Slot<'_>> {
        let mut inflight = self.inflight.lock().unwrap();
        let n = inflight.entry(partner.to_string()).or_insert(0);
        if *n >= max || self.usage_of(partner) >= self.limit_for(partner) {
            return None;
        }
        *n += 1;
        Some(Slot { tracker: self, partner: partner.to_string() })
    }

    pub fn usage_of(&self, partner: &str) -> u64 {
        *self.usage.lock().unwrap().get(partner).unwrap_or(&0)
    }

    /// Called by a timer thread at the end of each window.
    pub fn reset_window(&self) {
        let mut usage = self.usage.lock().unwrap();
        for (partner, used) in usage.iter() {
            let line = format!("window closed: {partner} used {used}");
            thread::spawn(move || write_audit(&line));
        }
        usage.clear();
        self.inflight.lock().unwrap().retain(|_, n| *n > 0);
    }
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        *self.tracker.inflight.lock().unwrap().get_mut(&self.partner).unwrap() -= 1;
    }
}

fn write_audit(_line: &str) {
    thread::sleep(Duration::from_millis(1)); // an append to the audit volume
}

fn main() {
    let limits = HashMap::from([("acme".to_string(), 20)]);
    let alerts = Arc::new(AtomicU64::new(0));
    let alerts_in_hook = Arc::clone(&alerts);
    let tracker = QuotaTracker::new(limits, Box::new(move |_| {
        alerts_in_hook.fetch_add(1, Ordering::Relaxed);
    }));

    // 8 gateway workers, 10 requests each, all for the same partner, whose quota is 20 per window.
    let admitted: u64 = thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|_| s.spawn(|| (0..10).filter(|_| tracker.try_acquire("acme")).count() as u64))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).sum()
    });

    println!("limit 20, attempts 80: admitted {admitted}, usage_of = {}", tracker.usage_of("acme"));
    println!("over-admitted: {}", admitted.saturating_sub(20));
    println!("quota alerts fired: {}", alerts.load(Ordering::Relaxed));
    println!("unknown partner 'zeta' admitted: {}", tracker.try_acquire("zeta"));
    let slot = tracker.acquire_slot("zeta", 2);
    println!("concurrency slot for 'zeta': {}", slot.is_some());
    drop(slot);
    tracker.reset_window();
    println!("after reset_window: usage_of(acme) = {}", tracker.usage_of("acme"));
    thread::sleep(Duration::from_millis(50)); // give the reporter time to "catch up"
    println!(
        "billing events uploaded: {} of {} admitted",
        BILLING_EVENTS_UPLOADED.load(Ordering::Relaxed),
        ADMITTED_TOTAL.load(Ordering::Relaxed)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admits_up_to_the_limit() {
        let t = QuotaTracker::new(HashMap::from([("p".to_string(), 3)]), Box::new(|_| {}));
        let results: Vec<bool> = (0..5).map(|_| t.try_acquire("p")).collect();
        assert_eq!(results, [true, true, true, false, false]);
        assert_eq!(t.usage_of("p"), 3);
    }
}
