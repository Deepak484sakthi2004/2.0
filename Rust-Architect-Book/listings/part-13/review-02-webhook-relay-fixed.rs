// verify: debug ok
// verify: debug test
//! Part XIII review capstone, the redesign. Same simulation as review-01: paused clock, 300 events at 100/s,
//! merchant m3's endpoint takes 2 s per call, a deploy at t = 5 s.
//! Per merchant: a bounded queue, at most 4 deliveries in flight, 500 ms per attempt, 3 attempts with backoff.
//! Nothing is lost silently: every event ends delivered, dead-lettered, spilled, or requeued.
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::{sleep, sleep_until, timeout, Instant};
use tokio_util::sync::CancellationToken;

const QUEUE_PER_MERCHANT: usize = 32;
const IN_FLIGHT_PER_MERCHANT: usize = 4;
const ATTEMPT_TIMEOUT: Duration = Duration::from_millis(500);
const ATTEMPTS: u32 = 3;
const DRAIN_DEADLINE: Duration = Duration::from_millis(1_000);

#[derive(Clone, Debug)]
struct Event {
    id: u32,
    merchant: u8,
    created: Instant,
}

async fn endpoint(ev: &Event) -> u16 {
    let ms = if ev.merchant == 3 { 2_000 } else { 20 };
    sleep(Duration::from_millis(ms)).await;
    200
}

/// Where every event ends up. The four lists together must account for every accepted event.
#[derive(Default, Debug)]
struct Outcomes {
    delivered: Vec<(u32, u8, Duration)>, // (event id, merchant, latency from intake)
    dead_letter: Vec<u32>, // all attempts failed: the retry job takes over
    spilled: Vec<u32>, // the merchant's queue was full at intake: written to the durable retry table instead
    requeued: Vec<u32>, // not finished at shutdown: back to the retry table (safe: receivers dedup on the id)
}

struct AuditLog {
    pending: Vec<String>,
    durable: Vec<String>,
}

impl AuditLog {
    fn record(&mut self, ev: &Event) {
        self.pending.push(format!("{} m{} 200", ev.id, ev.merchant));
        if self.pending.len() == 64 {
            self.flush();
        }
    }
    fn flush(&mut self) {
        let mut batch = std::mem::take(&mut self.pending);
        self.durable.append(&mut batch);
    }
}

#[derive(Clone)]
struct Shared {
    out: Arc<Mutex<Outcomes>>, // std Mutex: every critical section is a push, never an .await
    audit: Arc<Mutex<AuditLog>>,
    peak_in_flight: Arc<[AtomicU32; 4]>,
}

/// Retrying is safe only because the receiver deduplicates on `ev.id` (an idempotency key, Chapter 8.4).
async fn deliver_with_retries(ev: &Event) -> bool {
    for attempt in 0..ATTEMPTS {
        if let Ok(200) = timeout(ATTEMPT_TIMEOUT, endpoint(ev)).await {
            return true;
        }
        sleep(Duration::from_millis(100 << attempt)).await; // back off on the timer, never on the thread
    }
    false
}

/// One per merchant: owns that merchant's queue and deliveries. A slow merchant can only slow itself.
async fn dispatcher(mut queue: mpsc::Receiver<Event>, shared: Shared, stop: CancellationToken) {
    let permits = Arc::new(Semaphore::new(IN_FLIGHT_PER_MERCHANT));
    let in_flight_ids = Arc::new(Mutex::new(HashSet::new()));
    let mut deliveries = JoinSet::new();
    loop {
        while deliveries.try_join_next().is_some() {} // reap finished deliveries
        let ev = tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            ev = queue.recv() => match ev { Some(ev) => ev, None => break },
        };
        let permit = tokio::select! {
            biased;
            _ = stop.cancelled() => {
                shared.out.lock().unwrap().requeued.push(ev.id); // never attempted
                break;
            }
            p = Arc::clone(&permits).acquire_owned() => p.unwrap(),
        };
        let (shared, ids) = (shared.clone(), Arc::clone(&in_flight_ids));
        deliveries.spawn(async move {
            let _permit = permit;
            ids.lock().unwrap().insert(ev.id);
            let n = (IN_FLIGHT_PER_MERCHANT - _permit.semaphore().available_permits()) as u32;
            shared.peak_in_flight[ev.merchant as usize].fetch_max(n, Relaxed);
            let ok = deliver_with_retries(&ev).await;
            ids.lock().unwrap().remove(&ev.id);
            if ok {
                shared.audit.lock().unwrap().record(&ev);
                shared.out.lock().unwrap().delivered.push((ev.id, ev.merchant, ev.created.elapsed()));
            } else {
                shared.out.lock().unwrap().dead_letter.push(ev.id);
            }
        });
    }
    // Shutdown: whatever is still queued was never attempted: requeue it at once.
    queue.close();
    while let Ok(ev) = queue.try_recv() {
        shared.out.lock().unwrap().requeued.push(ev.id);
    }
    // In-flight deliveries get until the deadline. After that their outcome is unknown: requeue them too.
    let finished = timeout(DRAIN_DEADLINE, async { while deliveries.join_next().await.is_some() {} }).await;
    if finished.is_err() {
        deliveries.shutdown().await;
        let mut out = shared.out.lock().unwrap();
        out.requeued.extend(in_flight_ids.lock().unwrap().drain());
    }
}

struct Report {
    accepted: u32,
    outcomes: Outcomes,
    audit_durable: usize,
    peak_in_flight: [u32; 4],
}

async fn simulate() -> Report {
    let shared = Shared {
        out: Arc::new(Mutex::new(Outcomes::default())),
        audit: Arc::new(Mutex::new(AuditLog { pending: Vec::new(), durable: Vec::new() })),
        peak_in_flight: Arc::new([AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)]),
    };
    let stop = CancellationToken::new();
    let mut queues = HashMap::new();
    let mut dispatchers = JoinSet::new();
    for m in 1..=3u8 {
        let (tx, rx) = mpsc::channel(QUEUE_PER_MERCHANT);
        queues.insert(m, tx);
        dispatchers.spawn(dispatcher(rx, shared.clone(), stop.clone()));
    }
    let start = Instant::now();
    let mut accepted = 0;
    for id in 0..300u32 {
        let ev = Event { id, merchant: (id % 3 + 1) as u8, created: Instant::now() };
        accepted += 1;
        if let Err(TrySendError::Full(ev) | TrySendError::Closed(ev)) = queues[&ev.merchant].try_send(ev) {
            shared.out.lock().unwrap().spilled.push(ev.id); // intake never waits on a slow merchant
        }
        sleep(Duration::from_millis(10)).await;
    }
    sleep_until(start + Duration::from_secs(5)).await; // the deploy
    stop.cancel();
    drop(queues);
    while dispatchers.join_next().await.is_some() {}
    shared.audit.lock().unwrap().flush(); // explicit: Drop can't be trusted with durable writes
    let audit_durable = shared.audit.lock().unwrap().durable.len();
    let outcomes = std::mem::take(&mut *shared.out.lock().unwrap());
    let peak_in_flight = [0, 1, 2, 3].map(|m| shared.peak_in_flight[m].load(Relaxed));
    Report { accepted, outcomes, audit_durable, peak_in_flight }
}

fn worst(latencies: &[(u32, u8, Duration)], m: u8) -> u128 {
    latencies.iter().filter(|(_, mm, _)| *mm == m).map(|(_, _, d)| d.as_millis()).max().unwrap_or(0)
}

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().start_paused(true).build().unwrap();
    let r = rt.block_on(simulate());
    let o = &r.outcomes;
    let per = |m: u8| o.delivered.iter().filter(|(_, mm, _)| *mm == m).count();
    println!("after the deploy: {} events accepted", r.accepted);
    println!("  delivered m1 {} / m2 {} / m3 {}; dead-lettered {}; spilled at intake {}; requeued at shutdown {}",
        per(1), per(2), per(3), o.dead_letter.len(), o.spilled.len(), o.requeued.len());
    println!("  accounted for: {} of {}", o.delivered.len() + o.dead_letter.len() + o.spilled.len() + o.requeued.len(), r.accepted);
    println!("  worst delivery latency: m1 {} ms, m2 {} ms; peak in flight per merchant {:?}",
        worst(&o.delivered, 1), worst(&o.delivered, 2), &r.peak_in_flight[1..]);
    println!("  audit lines on durable storage {} of {} deliveries", r.audit_durable, o.delivered.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> Report {
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().start_paused(true).build().unwrap();
        rt.block_on(simulate())
    }

    #[test]
    fn every_event_is_accounted_for_exactly_once() {
        let r = run();
        let o = &r.outcomes;
        let mut ids: Vec<u32> = o.delivered.iter().map(|(id, _, _)| *id).collect();
        ids.extend(&o.dead_letter);
        ids.extend(&o.spilled);
        ids.extend(&o.requeued);
        ids.sort_unstable();
        assert_eq!(ids, (0..r.accepted).collect::<Vec<u32>>()); // each id once: none lost, none twice
    }

    #[test]
    fn a_slow_merchant_does_not_delay_the_others() {
        let r = run();
        assert!(worst(&r.outcomes.delivered, 1) <= 40, "m1 worst {} ms", worst(&r.outcomes.delivered, 1));
        assert!(worst(&r.outcomes.delivered, 2) <= 40, "m2 worst {} ms", worst(&r.outcomes.delivered, 2));
    }

    #[test]
    fn in_flight_deliveries_per_merchant_are_bounded() {
        let r = run();
        assert!(r.peak_in_flight.iter().all(|&p| p as usize <= IN_FLIGHT_PER_MERCHANT), "{:?}", r.peak_in_flight);
    }

    #[test]
    fn the_audit_trail_matches_the_deliveries() {
        let r = run();
        assert_eq!(r.audit_durable, r.outcomes.delivered.len());
    }
}
