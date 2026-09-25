// verify: release ok
// False sharing, measured. Four threads each increment a counter PER_THREAD times with fetch_add(Relaxed).
// Only the placement of the counters changes. One run on a shared 4-vCPU machine: rough numbers.
use crossbeam::utils::CachePadded;
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::thread;
use std::time::Instant;

const THREADS: usize = 4;
const PER_THREAD: u64 = 5_000_000;

#[repr(align(64))]
struct Pad64(AtomicU64);

/// Four counters guaranteed to sit in ONE 64-byte cache line.
#[repr(align(64))]
struct OneLine([AtomicU64; THREADS]);

fn bench(name: &str, counter_for: impl Fn(usize) -> &'static AtomicU64 + Sync) {
    let t = Instant::now();
    thread::scope(|s| {
        for i in 0..THREADS {
            let c = counter_for(i);
            s.spawn(move || {
                for _ in 0..PER_THREAD {
                    c.fetch_add(1, Relaxed);
                }
            });
        }
    });
    let ns = t.elapsed().as_nanos() as f64 / PER_THREAD as f64;
    println!("{name:<50} {ns:>6.2} ns per increment per thread");
}

fn main() {
    let shared: &'static AtomicU64 = Box::leak(Box::new(AtomicU64::new(0)));
    let adjacent: &'static OneLine = Box::leak(Box::new(OneLine(std::array::from_fn(|_| AtomicU64::new(0)))));
    let pad64: &'static [Pad64; THREADS] = Box::leak(Box::new(std::array::from_fn(|_| Pad64(AtomicU64::new(0)))));
    let pad128: &'static [CachePadded<AtomicU64>; THREADS] =
        Box::leak(Box::new(std::array::from_fn(|_| CachePadded::new(AtomicU64::new(0)))));

    println!("size_of: AtomicU64 = {}, Pad64 = {}, CachePadded<AtomicU64> = {}",
        size_of::<AtomicU64>(), size_of::<Pad64>(), size_of::<CachePadded<AtomicU64>>());
    bench("one shared counter (true sharing)", |_| shared);
    bench("own counter, adjacent in one line (false sharing)", |i| &adjacent.0[i]);
    bench("own counter, 64-byte aligned", |i| &pad64[i].0);
    bench("own counter, CachePadded (128 bytes)", |i| &pad128[i]);

    // No sharing at all: count locally, publish once. black_box keeps the loop from being folded away.
    let total = AtomicU64::new(0);
    let t = Instant::now();
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                let mut local = 0u64;
                for _ in 0..PER_THREAD {
                    local = black_box(local + 1);
                }
                total.fetch_add(local, Relaxed);
            });
        }
    });
    let ns = t.elapsed().as_nanos() as f64 / PER_THREAD as f64;
    println!("{:<50} {ns:>6.2} ns per increment per thread", "thread-local count, one fetch_add at the end");
    let sum = shared.load(Relaxed) + adjacent.0.iter().chain(pad64.iter().map(|p| &p.0)).chain(pad128.iter().map(|p| &**p))
        .map(|c| c.load(Relaxed)).sum::<u64>() + total.load(Relaxed);
    println!("all increments accounted for: {}", sum == 5 * THREADS as u64 * PER_THREAD);
}
