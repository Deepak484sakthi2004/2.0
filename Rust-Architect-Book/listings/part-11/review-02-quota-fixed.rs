// verify: debug ok
// verify: debug test
// verify: release ok
//! Part XI review capstone: one redesign of the partner-quota PR (review-01-quota-pr.rs).
//!
//! - decide under ONE short lock, act after it (alerts, audit), never two locks at once;
//! - check-and-increment in one critical section (no TOCTOU);
//! - sharded per-partner windows with a keyed router; poisoning policy: recover, count, clear;
//! - limits published whole with ArcSwap; unknown partners are refused (fail closed);
//! - one owned writer thread for the audit volume behind a BOUNDED channel: "no audit, no admission";
//! - Drop / shutdown() closes the channel and joins the writer, so the last batch is never lost.

mod quota {
    use arc_swap::ArcSwap;
    use std::collections::HashMap;
    use std::hash::{BuildHasher, RandomState};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    #[derive(Debug, PartialEq)]
    pub enum Denied {
        UnknownPartner,
        QuotaExhausted,
        TooManyInFlight,
        AuditUnavailable,
    }

    #[derive(Default)]
    struct Window {
        used: u64,
        inflight: u64,
        alerted: bool,
    }

    enum Record {
        Admit(String),
        WindowClosed(Vec<(String, u64)>),
    }

    /// What the writer thread did, returned when it is joined.
    #[derive(Debug, Default)]
    pub struct WriterReport {
        pub admissions_written: u64,
        pub batches: u64,
        pub bytes_written: u64,
        pub window_summaries: Vec<String>,
    }

    #[derive(Default)]
    pub struct Stats {
        pub admitted: AtomicU64,
        pub denied: AtomicU64,
        pub alerts: AtomicU64,
        pub poison_recoveries: AtomicU64,
    }

    pub struct Config {
        pub shards: usize,
        pub max_inflight: u64,
        pub audit_capacity: usize,
        pub write_delay: Duration, // what one batch append + fsync takes
    }

    type Shard = Mutex<HashMap<String, Window>>;

    pub struct QuotaTracker {
        limits: ArcSwap<HashMap<String, u64>>,
        shards: Vec<Shard>,
        router: RandomState,
        max_inflight: u64,
        on_exceeded: Box<dyn Fn(&str) + Send + Sync>,
        writer: Option<(SyncSender<Record>, JoinHandle<WriterReport>)>,
        pub stats: Stats,
    }

    /// Proof of admission. Dropping it ends the request: the in-flight count goes down.
    pub struct Admission<'a> {
        tracker: &'a QuotaTracker,
        partner: String,
    }

    impl QuotaTracker {
        pub fn new(limits: HashMap<String, u64>, config: Config, on_exceeded: Box<dyn Fn(&str) + Send + Sync>) -> Self {
            let (tx, rx) = mpsc::sync_channel(config.audit_capacity);
            let delay = config.write_delay;
            let handle = thread::Builder::new()
                .name("quota-audit-writer".into())
                .spawn(move || write_loop(rx, delay))
                .expect("spawn audit writer");
            QuotaTracker {
                limits: ArcSwap::from_pointee(limits),
                shards: (0..config.shards).map(|_| Mutex::new(HashMap::new())).collect(),
                router: RandomState::new(),
                max_inflight: config.max_inflight,
                on_exceeded,
                writer: Some((tx, handle)),
                stats: Stats::default(),
            }
        }

        pub fn reload_limits(&self, limits: HashMap<String, u64>) {
            self.limits.store(Arc::new(limits)); // readers keep the version they loaded
        }

        // Poisoning policy: RECOVER. Each critical section below changes independent per-partner counters with
        // statements that cannot panic halfway, so a map seen after a panic elsewhere is still consistent.
        fn lock_shard(&self, partner: &str) -> MutexGuard<'_, HashMap<String, Window>> {
            let i = (self.router.hash_one(partner) % self.shards.len() as u64) as usize;
            self.lock_index(i)
        }

        fn lock_index(&self, i: usize) -> MutexGuard<'_, HashMap<String, Window>> {
            self.shards[i].lock().unwrap_or_else(|poisoned| {
                self.stats.poison_recoveries.fetch_add(1, Ordering::Relaxed);
                self.shards[i].clear_poison();
                poisoned.into_inner()
            })
        }

        pub fn try_acquire(&self, partner: &str) -> Result<Admission<'_>, Denied> {
            let result = self.decide(partner);
            let counter = if result.is_ok() { &self.stats.admitted } else { &self.stats.denied };
            counter.fetch_add(1, Ordering::Relaxed);
            result
        }

        fn decide(&self, partner: &str) -> Result<Admission<'_>, Denied> {
            let Some(&limit) = self.limits.load().get(partner) else {
                return Err(Denied::UnknownPartner); // fail closed: no configured quota, no traffic
            };
            // 1. Decide under ONE short critical section: check AND increment together.
            let (decision, first_alert) = {
                let mut shard = self.lock_shard(partner);
                if !shard.contains_key(partner) {
                    shard.insert(partner.to_string(), Window::default()); // once per partner per window
                }
                let w = shard.get_mut(partner).expect("just inserted");
                if w.used >= limit {
                    let first = !w.alerted;
                    w.alerted = true;
                    (Err(Denied::QuotaExhausted), first)
                } else if w.inflight >= self.max_inflight {
                    (Err(Denied::TooManyInFlight), false)
                } else {
                    w.used += 1;
                    w.inflight += 1;
                    (Ok(()), false)
                }
            }; // the shard lock is released here
            // 2. Act after the lock: the hook may be slow, or may call back into the tracker.
            if first_alert {
                self.stats.alerts.fetch_add(1, Ordering::Relaxed);
                (self.on_exceeded)(partner);
            }
            decision?;
            // 3. No audit, no admission. A full channel means the audit volume is behind: refuse, don't queue.
            let tx = &self.writer.as_ref().expect("writer runs until drop").0;
            if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) = tx.try_send(Record::Admit(partner.to_string())) {
                // Compensate in a second short critical section. saturating_sub: the window may have closed meanwhile.
                let mut shard = self.lock_shard(partner);
                if let Some(w) = shard.get_mut(partner) {
                    w.used = w.used.saturating_sub(1);
                    w.inflight = w.inflight.saturating_sub(1);
                }
                return Err(Denied::AuditUnavailable);
            }
            Ok(Admission { tracker: self, partner: partner.to_string() })
        }

        pub fn usage_of(&self, partner: &str) -> u64 {
            self.lock_shard(partner).get(partner).map_or(0, |w| w.used)
        }

        /// End of a window: collect every shard's counts, one lock at a time, then report after unlocking.
        pub fn close_window(&self) -> Result<usize, &'static str> {
            let mut summary = Vec::new();
            for i in 0..self.shards.len() {
                let mut shard = self.lock_index(i);
                for (partner, w) in shard.iter_mut() {
                    if w.used > 0 {
                        summary.push((partner.clone(), w.used));
                    }
                    w.used = 0;
                    w.alerted = false; // in-flight requests keep their slots
                }
                shard.retain(|_, w| w.inflight > 0);
            }
            summary.sort(); // a deterministic report, whatever the shard layout
            let n = summary.len();
            let tx = &self.writer.as_ref().ok_or("shut down")?.0;
            tx.send(Record::WindowClosed(summary)).map_err(|_| "audit writer gone")?; // billing: block, never drop
            Ok(n)
        }

        /// Graceful shutdown: close the channel, let the writer drain it, join it.
        pub fn shutdown(mut self) -> WriterReport {
            self.stop_writer()
        }

        fn stop_writer(&mut self) -> WriterReport {
            match self.writer.take() {
                Some((tx, handle)) => {
                    drop(tx); // disconnection is the writer's "no more records" signal
                    handle.join().unwrap_or_default()
                }
                None => WriterReport::default(),
            }
        }

        #[cfg(test)]
        pub fn poison_shard_of(&self, partner: &str) {
            let i = (self.router.hash_one(partner) % self.shards.len() as u64) as usize;
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _guard = self.shards[i].lock().unwrap();
                panic!("simulated bug while holding a shard lock");
            }));
        }
    }

    impl Drop for QuotaTracker {
        fn drop(&mut self) {
            self.stop_writer(); // a no-op after shutdown()
        }
    }

    impl Drop for Admission<'_> {
        fn drop(&mut self) {
            // Never panics: lock_shard recovers from poisoning instead of unwrapping (Chapter 8.3's double panic).
            if let Some(w) = self.tracker.lock_shard(&self.partner).get_mut(&self.partner) {
                w.inflight = w.inflight.saturating_sub(1);
            }
        }
    }

    /// The only thread that touches the audit volume: batches records, one "append + fsync" per batch.
    fn write_loop(rx: Receiver<Record>, delay: Duration) -> WriterReport {
        let mut report = WriterReport::default();
        while let Ok(first) = rx.recv() {
            let mut batch = vec![first];
            batch.extend(rx.try_iter().take(255));
            thread::sleep(delay);
            report.batches += 1;
            for record in batch {
                match record {
                    Record::Admit(partner) => {
                        report.admissions_written += 1;
                        report.bytes_written += format!("admit {partner}\n").len() as u64;
                    }
                    Record::WindowClosed(rows) => {
                        report.window_summaries.extend(rows.into_iter().map(|(p, n)| format!("{p} used {n}")));
                    }
                }
            }
        }
        report // every Sender dropped and the queue drained
    }
}

use quota::{Config, QuotaTracker};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

fn config() -> Config {
    Config { shards: 16, max_inflight: 4, audit_capacity: 1024, write_delay: Duration::from_millis(1) }
}

fn main() {
    let limits = HashMap::from([("acme".to_string(), 20), ("beta".to_string(), 100)]);
    let tracker = QuotaTracker::new(limits, config(), Box::new(|partner| eprintln!("[alert] {partner} exhausted its quota")));

    // The same load as the PR: 8 workers x 10 requests for one partner whose quota is 20.
    let admitted: u64 = thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|_| s.spawn(|| (0..10).filter(|_| tracker.try_acquire("acme").is_ok()).count() as u64))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).sum()
    });
    println!("limit 20, attempts 80: admitted {admitted}, usage_of = {}", tracker.usage_of("acme"));
    println!("quota alerts fired: {}", tracker.stats.alerts.load(Ordering::Relaxed));
    println!("unknown partner 'zeta': {:?}", tracker.try_acquire("zeta").err());

    // Concurrency slots: 4 in flight for 'beta'; the 5th is refused until one finishes.
    let held: Vec<_> = (0..4).map(|_| tracker.try_acquire("beta").unwrap()).collect();
    println!("5th concurrent 'beta' request: {:?}", tracker.try_acquire("beta").err());
    drop(held);
    println!("after the 4 finish: admitted = {}", tracker.try_acquire("beta").is_ok());

    println!("partners reported at window close: {:?}", tracker.close_window());
    println!("usage_of(acme) after close: {}", tracker.usage_of("acme"));

    // A config reload publishes a whole new limits map; requests already past the check keep the old one.
    tracker.reload_limits(HashMap::from([("acme".to_string(), 30), ("beta".to_string(), 100)]));
    println!("new window, new limits: acme admitted = {}", tracker.try_acquire("acme").is_ok());

    let report = tracker.shutdown();
    println!(
        "writer on shutdown: {} admissions ({} bytes) written in {} batches; summaries {:?}",
        report.admissions_written, report.bytes_written, report.batches, report.window_summaries
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::quota::Denied;

    fn tracker(limit: u64, cfg: Config) -> QuotaTracker {
        QuotaTracker::new(HashMap::from([("p".to_string(), limit)]), cfg, Box::new(|_| {}))
    }

    #[test]
    fn exact_under_concurrency() {
        let t = tracker(50, config());
        let admitted: u64 = thread::scope(|s| {
            let hs: Vec<_> = (0..8).map(|_| s.spawn(|| (0..25).filter(|_| t.try_acquire("p").is_ok()).count() as u64)).collect();
            hs.into_iter().map(|h| h.join().unwrap()).sum()
        });
        assert_eq!(admitted, 50);
        assert_eq!(t.usage_of("p"), 50);
        assert_eq!(t.stats.alerts.load(Ordering::Relaxed), 1);
        assert_eq!(t.shutdown().admissions_written, 50);
    }

    #[test]
    fn unknown_partners_are_refused() {
        let t = tracker(5, config());
        assert_eq!(t.try_acquire("nobody").err(), Some(Denied::UnknownPartner));
    }

    #[test]
    fn inflight_slots_are_released_on_drop() {
        let t = tracker(100, config());
        let a: Vec<_> = (0..4).map(|_| t.try_acquire("p").unwrap()).collect();
        assert_eq!(t.try_acquire("p").err(), Some(Denied::TooManyInFlight));
        drop(a);
        assert!(t.try_acquire("p").is_ok());
    }

    #[test]
    fn audit_backpressure_refuses_and_rolls_back() {
        let slow = Config { shards: 4, max_inflight: 1_000, audit_capacity: 1, write_delay: Duration::from_millis(30) };
        let t = tracker(1_000, slow);
        let results: Vec<_> = (0..20).map(|_| t.try_acquire("p").map(drop)).collect();
        let refused = results.iter().filter(|r| **r == Err(Denied::AuditUnavailable)).count() as u64;
        assert!(refused > 0, "a 1-slot channel behind a 30 ms writer must refuse some");
        let admitted = t.stats.admitted.load(Ordering::Relaxed);
        assert_eq!(t.usage_of("p"), admitted); // every refusal was rolled back
        assert_eq!(t.shutdown().admissions_written, admitted); // and every admission was audited
    }

    #[test]
    fn a_poisoned_shard_is_recovered() {
        std::panic::set_hook(Box::new(|_| {}));
        let t = tracker(10, config());
        t.poison_shard_of("p");
        assert!(t.try_acquire("p").is_ok());
        assert_eq!(t.stats.poison_recoveries.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn close_window_reports_and_resets() {
        let t = tracker(10, config());
        for _ in 0..3 {
            t.try_acquire("p").unwrap();
        }
        assert_eq!(t.close_window(), Ok(1));
        assert_eq!(t.usage_of("p"), 0);
        assert_eq!(t.shutdown().window_summaries, vec!["p used 3".to_string()]);
    }
}
