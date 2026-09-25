// verify: debug ok
// verify: release ok
//! Chapter 3.1's route-table refresh, multi-threaded: readers never lock, never see a half-built table,
//! and the old table is freed on a dedicated dropper thread, not on a request thread.
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

struct RouteTable {
    version: u64,
    routes: HashMap<u32, (u64, u32)>, // route id -> (version stamp, upstream)
}

impl RouteTable {
    fn build(version: u64, n: u32) -> RouteTable {
        RouteTable { version, routes: (0..n).map(|id| (id, (version, id % 7))).collect() }
    }
}

static DROPPED_ON_DROPPER: AtomicU64 = AtomicU64::new(0);
static DROPPED_ELSEWHERE: AtomicU64 = AtomicU64::new(0);

impl Drop for RouteTable {
    fn drop(&mut self) {
        let counter = if thread::current().name() == Some("dropper") { &DROPPED_ON_DROPPER } else { &DROPPED_ELSEWHERE };
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

fn main() {
    const N: u32 = 10_000;
    let table = ArcSwap::from_pointee(RouteTable::build(0, N));
    let lookups = AtomicU64::new(0);
    let torn = AtomicU64::new(0);
    let max_version_seen = AtomicU64::new(0);

    // The dropper waits until it holds the LAST reference, so the free always happens here.
    let (old_tx, old_rx) = mpsc::channel::<Arc<RouteTable>>();
    let dropper = thread::Builder::new()
        .name("dropper".into())
        .spawn(move || {
            for mut old in old_rx {
                loop {
                    match Arc::try_unwrap(old) {
                        Ok(t) => break drop(t),
                        Err(still_shared) => {
                            old = still_shared; // a reader still holds it: wait for them to finish
                            thread::yield_now();
                        }
                    }
                }
            }
        })
        .unwrap();

    let stop = std::sync::atomic::AtomicBool::new(false);
    thread::scope(|s| {
        for r in 0..4u32 {
            let (table, lookups, torn, max_seen, stop) = (&table, &lookups, &torn, &max_version_seen, &stop);
            s.spawn(move || {
                let mut i = r;
                while !stop.load(Ordering::Relaxed) {
                    let t = table.load(); // no lock, and usually no refcount traffic either
                    // Every entry of one table carries that table's version: a torn read would show a mismatch.
                    let (stamp, _upstream) = t.routes[&(i % N)];
                    if stamp != t.version {
                        torn.fetch_add(1, Ordering::Relaxed);
                    }
                    max_seen.fetch_max(t.version, Ordering::Relaxed);
                    lookups.fetch_add(1, Ordering::Relaxed);
                    i = i.wrapping_add(7919);
                }
            });
        }
        // The writer: build each new table OFF to the side, publish with one atomic swap.
        for v in 1..=50 {
            let fresh = Arc::new(RouteTable::build(v, N));
            let old = table.swap(fresh);
            old_tx.send(old).unwrap();
        }
        stop.store(true, Ordering::Relaxed);
    });
    drop(old_tx);
    dropper.join().unwrap();

    println!("lookups > 0: {}", lookups.load(Ordering::Relaxed) > 0);
    println!("torn reads: {}", torn.load(Ordering::Relaxed));
    println!("current version: {}, highest version a reader saw: {}", table.load().version, max_version_seen.load(Ordering::Relaxed));
    println!(
        "old tables freed on the dropper thread: {}, elsewhere: {}",
        DROPPED_ON_DROPPER.load(Ordering::Relaxed),
        DROPPED_ELSEWHERE.load(Ordering::Relaxed)
    );
}
