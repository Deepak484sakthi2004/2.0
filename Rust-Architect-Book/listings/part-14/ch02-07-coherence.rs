// verify: debug miri-ok
// verify: release ok
// What Relaxed still guarantees: every atomic has ONE modification order that all threads agree on, and a
// thread's successive reads never go backwards in it (read-read coherence). A metrics scraper reading a
// Relaxed counter sees a monotonic series, possibly slightly behind, never out of order.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::thread;

const N: u64 = if cfg!(miri) { 200 } else { 5_000_000 };

fn main() {
    let requests = AtomicU64::new(0);
    let (reads, backwards) = thread::scope(|s| {
        s.spawn(|| (0..N).for_each(|_| { requests.fetch_add(1, Relaxed); }));
        let scraper = s.spawn(|| {
            let (mut last, mut reads, mut backwards) = (0, 0u64, 0u64);
            while last < N {
                let now = requests.load(Relaxed);
                if now < last { backwards += 1; }
                last = now;
                reads += 1;
            }
            (reads, backwards)
        });
        scraper.join().unwrap()
    });
    println!("{reads} scraper reads, {backwards} went backwards");
}
