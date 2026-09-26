// verify: debug ok
//! Part XIII review capstone, the PR as submitted: `webhook-relay` delivers payout events to merchant webhooks.
//! Simulated in one process on a paused (virtual) clock, so every number printed is exact and repeatable.
//! During the incident window merchant m3's endpoint takes 2 s per call; m1 and m2 answer in 20 ms.
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep, sleep_until, Instant};

#[derive(Clone, Debug)]
struct Event {
    id: u32,
    merchant: u8,
    created: Instant,
}

/// The merchant's webhook endpoint (stands in for an HTTPS POST).
async fn endpoint(ev: &Event) -> u16 {
    let ms = if ev.merchant == 3 { 2_000 } else { 20 };
    sleep(Duration::from_millis(ms)).await;
    200
}

/// Buffered audit trail: lines reach durable storage 64 at a time.
struct AuditLog {
    pending: Vec<String>,
    durable: Arc<std::sync::Mutex<Vec<String>>>,
}

impl AuditLog {
    fn record(&mut self, ev: &Event, status: u16) {
        self.pending.push(format!("{} m{} {status}", ev.id, ev.merchant));
        if self.pending.len() == 64 {
            self.durable.lock().unwrap().append(&mut self.pending);
        }
    }
}

static DELIVERED: [AtomicU32; 4] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
static ALIVE: AtomicU32 = AtomicU32::new(0);
static PEAK_ALIVE: AtomicU32 = AtomicU32::new(0);
static M1_WORST_MS: AtomicU32 = AtomicU32::new(0);

// ------------------------------------------------ the PR ------------------------------------------------
async fn run_relay(mut events: mpsc::UnboundedReceiver<Event>, log: Arc<tokio::sync::Mutex<AuditLog>>) {
    while let Some(ev) = events.recv().await {
        let log = Arc::clone(&log);
        tokio::spawn(async move {
            let n = ALIVE.fetch_add(1, Relaxed) + 1;
            PEAK_ALIVE.fetch_max(n, Relaxed);
            let mut log = log.lock().await; // hold the log while delivering, "so audit lines stay in order"
            let status = endpoint(&ev).await;
            log.record(&ev, status);
            DELIVERED[ev.merchant as usize].fetch_add(1, Relaxed);
            if ev.merchant == 1 {
                M1_WORST_MS.fetch_max(ev.created.elapsed().as_millis() as u32, Relaxed);
            }
            ALIVE.fetch_sub(1, Relaxed);
        });
    }
}
// ---------------------------------------------------------------------------------------------------------

fn main() {
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().start_paused(true).build().unwrap();
    let durable = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::new(tokio::sync::Mutex::new(AuditLog { pending: Vec::new(), durable: Arc::clone(&durable) }));
    rt.block_on(async {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(run_relay(rx, Arc::clone(&log)));
        let start = Instant::now();
        for id in 0..300u32 {
            // intake: 100 events/s for 3 s, merchants m1, m2, m3 in turn
            tx.send(Event { id, merchant: (id % 3 + 1) as u8, created: Instant::now() }).unwrap();
            sleep(Duration::from_millis(10)).await;
        }
        sleep_until(start + Duration::from_secs(5)).await; // t = 5 s: a deploy sends SIGTERM; main returns
        let d = |m: usize| DELIVERED[m].load(Relaxed);
        println!("at the deploy (t = 5 s): 300 events accepted, delivered m1 {} / m2 {} / m3 {} (of 100 each)", d(1), d(2), d(3));
        println!("  tasks alive {} (peak {}), m1's worst delivery latency so far {} ms (its endpoint answers in 20 ms)",
            ALIVE.load(Relaxed), PEAK_ALIVE.load(Relaxed), M1_WORST_MS.load(Relaxed));
    });
    drop(rt); // what returning from #[tokio::main] does: every task still alive is dropped where it waits
    let delivered: u32 = (1..=3).map(|m| DELIVERED[m].load(Relaxed)).sum();
    println!("after main returned: {} events never delivered, and nothing records which", 300 - delivered);
    println!("  audit lines on durable storage {} of {} deliveries (the rest were in the buffer)", durable.lock().unwrap().len(), delivered);
}
