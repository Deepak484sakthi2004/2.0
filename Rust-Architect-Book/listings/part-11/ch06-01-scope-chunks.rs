// verify: debug ok
//! Scoped threads borrow local data: no Arc, no 'static, no clone. The scope joins every thread before
//! it returns, so every borrow provably ends in time.
use std::thread;

fn main() {
    let mut latencies_ms: Vec<u32> = (0..1_000_000u32).map(|i| i.wrapping_mul(7919) % 500).collect();
    let threads = thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let chunk = latencies_ms.len().div_ceil(threads);

    // Phase 1: shared borrows. Each thread reads its own chunk of the same Vec.
    let maxima: Vec<u32> = thread::scope(|s| {
        let handles: Vec<_> = latencies_ms
            .chunks(chunk)
            .map(|part| s.spawn(move || *part.iter().max().unwrap()))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let max = *maxima.iter().max().unwrap();

    // Phase 2: exclusive borrows. chunks_mut hands each thread a disjoint &mut [u32].
    thread::scope(|s| {
        for part in latencies_ms.chunks_mut(chunk) {
            s.spawn(move || {
                for x in part {
                    *x = *x * 1000 / max; // normalize to 0..=1000 in place
                }
            });
        }
    }); // all threads joined here: `latencies_ms` is ours again

    println!("{threads} threads; per-chunk maxima {maxima:?}; after normalizing, max = {}", latencies_ms.iter().max().unwrap());
}
