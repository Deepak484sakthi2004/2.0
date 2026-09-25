// verify: release ok
//! Per-message cost of moving 1M u64 values from one thread to another. One run, noisy.
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

const N: u64 = 1_000_000;

fn measure(label: &str, f: impl FnOnce() -> u64) {
    let t = Instant::now();
    let sum = f();
    assert_eq!(sum, N * (N - 1) / 2);
    println!("{label:<44} {:>6.1} ns per item", t.elapsed().as_nanos() as f64 / N as f64);
}

fn main() {
    measure("std mpsc::channel (unbounded)", || {
        let (tx, rx) = mpsc::channel();
        let h = thread::spawn(move || rx.iter().sum::<u64>());
        (0..N).for_each(|i| tx.send(i).unwrap());
        drop(tx);
        h.join().unwrap()
    });
    measure("std mpsc::sync_channel(1024)", || {
        let (tx, rx) = mpsc::sync_channel(1024);
        let h = thread::spawn(move || rx.iter().sum::<u64>());
        (0..N).for_each(|i| tx.send(i).unwrap());
        drop(tx);
        h.join().unwrap()
    });
    measure("std mpsc::sync_channel(1): near-lockstep", || {
        let (tx, rx) = mpsc::sync_channel(1);
        let h = thread::spawn(move || rx.iter().sum::<u64>());
        (0..N).for_each(|i| tx.send(i).unwrap());
        drop(tx);
        h.join().unwrap()
    });
    measure("crossbeam bounded(1024)", || {
        let (tx, rx) = crossbeam_channel::bounded(1024);
        let h = thread::spawn(move || rx.iter().sum::<u64>());
        (0..N).for_each(|i| tx.send(i).unwrap());
        drop(tx);
        h.join().unwrap()
    });
    measure("std sync_channel(16) of Vec<u64> batches of 1024", || {
        let (tx, rx) = mpsc::sync_channel::<Vec<u64>>(16);
        let h = thread::spawn(move || rx.iter().map(|b| b.iter().sum::<u64>()).sum::<u64>());
        let mut batch = Vec::with_capacity(1024);
        for i in 0..N {
            batch.push(i);
            if batch.len() == 1024 {
                tx.send(std::mem::replace(&mut batch, Vec::with_capacity(1024))).unwrap();
            }
        }
        tx.send(batch).unwrap();
        drop(tx);
        h.join().unwrap()
    });
    measure("no channel: sum on the same thread", || (0..N).map(std::hint::black_box).sum());
}
