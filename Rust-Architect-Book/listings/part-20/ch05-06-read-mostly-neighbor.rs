// verify: release ok
// False sharing hurts readers too. Three threads only READ a config value; one thread increments a counter.
// Layout A: the config value and the counter share a cache line. Layout B: they are 128 bytes apart.
// One Playground run, noisy.
use crossbeam::utils::CachePadded;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::thread;
use std::time::{Duration, Instant};

struct SameLine {
    config_version: AtomicU64,
    counter: AtomicU64,
}

struct Apart {
    config_version: CachePadded<AtomicU64>,
    counter: CachePadded<AtomicU64>,
}

fn run(config: &'static AtomicU64, counter: &'static AtomicU64) -> f64 {
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let stop = stop.clone();
        thread::spawn(move || {
            while !stop.load(Relaxed) {
                counter.fetch_add(1, Relaxed);
            }
        })
    };
    let readers: Vec<_> = (0..3)
        .map(|_| {
            thread::spawn(move || {
                let reads = 20_000_000u64;
                let t = Instant::now();
                let mut acc = 0u64;
                for _ in 0..reads {
                    acc = acc.wrapping_add(black_box(config).load(Relaxed));
                }
                black_box(acc);
                t.elapsed().as_nanos() as f64 / reads as f64
            })
        })
        .collect();
    let ns: Vec<f64> = readers.into_iter().map(|h| h.join().unwrap()).collect();
    stop.store(true, Relaxed);
    writer.join().unwrap();
    thread::sleep(Duration::from_millis(10));
    ns.iter().sum::<f64>() / ns.len() as f64
}

fn main() {
    let same: &'static SameLine = Box::leak(Box::new(SameLine { config_version: AtomicU64::new(7), counter: AtomicU64::new(0) }));
    let apart: &'static Apart = Box::leak(Box::new(Apart {
        config_version: CachePadded::new(AtomicU64::new(7)),
        counter: CachePadded::new(AtomicU64::new(0)),
    }));
    let a = (&same.config_version as *const _ as usize, &same.counter as *const _ as usize);
    let b = (&*apart.config_version as *const _ as usize, &*apart.counter as *const _ as usize);
    println!("same line: fields {} bytes apart; padded: {} bytes apart", a.1.abs_diff(a.0), b.1.abs_diff(b.0));
    println!("ns per config read, mean of 3 reader threads (1 writer incrementing the counter):");
    println!("  config and counter in one line   {:6.2}", run(&same.config_version, &same.counter));
    println!("  config and counter padded apart  {:6.2}", run(&apart.config_version, &apart.counter));
}
