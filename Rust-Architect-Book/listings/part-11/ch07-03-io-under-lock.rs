// verify: release ok
//! The most common production lock bug: slow work (I/O) inside the critical section.
//! 4 threads x 100 requests; each request updates shared state (fast) and writes an audit record (~1 ms,
//! simulated with sleep). One run.
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

const THREADS: u32 = 4;
const REQUESTS: u32 = 100;

fn write_audit_record(_line: &str) {
    thread::sleep(Duration::from_millis(1)); // stands in for a network call or an fsync
}

fn run(label: &str, handle: impl Fn(u32) + Sync) {
    let t = Instant::now();
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| (0..REQUESTS).for_each(&handle));
        }
    });
    let secs = t.elapsed().as_secs_f64();
    println!("{label:<34} {:>6.0} requests/s  ({:.2} s)", (THREADS * REQUESTS) as f64 / secs, secs);
}

fn main() {
    let balances = Mutex::new(vec![0i64; 64]);

    run("audit write INSIDE the lock", |i| {
        let mut b = balances.lock().unwrap();
        b[(i % 64) as usize] += 1;
        write_audit_record(&format!("credit {i}")); // every other thread waits for this sleep
    });

    run("audit write AFTER the lock", |i| {
        let line = {
            let mut b = balances.lock().unwrap();
            b[(i % 64) as usize] += 1;
            format!("credit {i}")
        }; // guard dropped: the lock is held for nanoseconds
        write_audit_record(&line);
    });

    println!("total credits: {}", balances.lock().unwrap().iter().sum::<i64>());
}
