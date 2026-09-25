// verify: release ok
// verify: debug miri-ok
// A "max latency since last scrape" gauge, three ways: a hand-written CAS loop, try_update, and fetch_max.
// Relaxed is enough everywhere: the gauge is one self-contained number that publishes no other memory.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::thread;

const THREADS: u64 = 4;
const PER_THREAD: u64 = if cfg!(miri) { 20 } else { 1_000_000 };

/// The classic CAS loop. Returns how many times the CAS lost a race and had to retry.
fn record_max_cas(max: &AtomicU64, v: u64) -> u64 {
    let mut retries = 0;
    let mut cur = max.load(Relaxed);
    while v > cur {
        match max.compare_exchange_weak(cur, v, Relaxed, Relaxed) {
            Ok(_) => break,
            Err(actual) => {
                retries += 1; // another thread changed it (or, on LL/SC CPUs, a spurious failure)
                cur = actual; // retry against the value we just learned
            }
        }
    }
    retries
}

fn record_max_update(max: &AtomicU64, v: u64) {
    // try_update (called fetch_update before it was renamed) is the same loop, written by std.
    // `None` means "no change needed".
    let _ = max.try_update(Relaxed, Relaxed, |cur| (v > cur).then_some(v));
}

fn run(name: &str, f: impl Fn(&AtomicU64, u64) -> u64 + Sync) {
    let max = AtomicU64::new(0);
    let t = std::time::Instant::now();
    let retries: u64 = thread::scope(|s| {
        let hs: Vec<_> = (0..THREADS)
            .map(|t| {
                let (max, f) = (&max, &f);
                // Interleaved, increasing values: every thread keeps raising the max, so CASes collide.
                s.spawn(move || (0..PER_THREAD).map(|i| f(max, i * THREADS + t)).sum::<u64>())
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).sum()
    });
    let ms = t.elapsed().as_secs_f64() * 1e3;
    println!("{name:<12} max = {:>7}  failed CAS attempts = {retries:>7}  ({ms:.1} ms)", max.load(Relaxed));
}

fn main() {
    run("CAS loop", record_max_cas);
    run("try_update", |m, v| { record_max_update(m, v); 0 });
    run("fetch_max", |m, v| { m.fetch_max(v, Relaxed); 0 });
    // Scrape: read-and-reset in ONE atomic step, so no update between "read" and "reset" is lost.
    let gauge = AtomicU64::new(0);
    record_max_cas(&gauge, 42);
    println!("scrape -> {}, after scrape -> {}", gauge.swap(0, Relaxed), gauge.load(Relaxed));
}
