// verify: release ok
//! The read path of a shared, rarely-updated table, three ways, 4 reader threads. One run, noisy.
//! Every read must get a consistent snapshot it can use for a while (the table is replaced, never mutated).
use arc_swap::ArcSwap;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::Instant;

const READS: u32 = 1_000_000;

struct Table {
    upstreams: Vec<u32>,
}

fn run(label: &str, threads: usize, read: impl Fn(u32) -> u32 + Sync) {
    let t = Instant::now();
    thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut acc = 0u32;
                for i in 0..READS {
                    acc = acc.wrapping_add(read(i));
                }
                std::hint::black_box(acc);
            });
        }
    });
    println!("{label:<40} {:>6.1} ns per read", t.elapsed().as_nanos() as f64 / READS as f64);
}

fn main() {
    let threads = thread::available_parallelism().unwrap().get();
    let make = || Arc::new(Table { upstreams: (0..64).collect() });

    let mutex = Mutex::new(make());
    run("Mutex<Arc<Table>>: lock, clone Arc", threads, |i| {
        let t = Arc::clone(&mutex.lock().unwrap());
        t.upstreams[(i % 64) as usize]
    });

    let rwlock = RwLock::new(make());
    run("RwLock<Arc<Table>>: read lock, clone Arc", threads, |i| {
        let t = Arc::clone(&rwlock.read().unwrap());
        t.upstreams[(i % 64) as usize]
    });

    let swap = ArcSwap::new(make());
    run("ArcSwap<Table>: load()", threads, |i| {
        let t = swap.load();
        t.upstreams[(i % 64) as usize]
    });

    let plain = make();
    run("no sharing needed (baseline): &Table", threads, |i| plain.upstreams[(i % 64) as usize]);
    println!("({threads} reader threads, {READS} reads each; wall time / reads per thread)");
}
