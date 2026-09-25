// verify: debug ok
//! A bounded channel is backpressure: once it's full, send() blocks, and the producer runs at the
//! consumer's pace. Timestamps are from one run (the consumer takes ~20 ms per item).
use std::sync::mpsc::{self, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    let start = Instant::now();
    let ms = move || start.elapsed().as_millis();
    let (tx, rx) = mpsc::sync_channel::<u32>(2); // room for 2 in flight

    let consumer = thread::spawn(move || {
        for item in rx {
            thread::sleep(Duration::from_millis(20)); // slow downstream (disk, network, ...)
            let _ = item;
        }
    });
    let mut sent_at = Vec::new();
    for item in 0..6 {
        tx.send(item).unwrap(); // blocks while 2 items are already waiting
        sent_at.push(ms());
    }
    drop(tx);
    consumer.join().unwrap();
    println!("producer: send() returned at ms {sent_at:?}");

    // Non-blocking variant: refuse instead of wait (load shedding).
    let (tx, _rx) = mpsc::sync_channel::<&str>(1);
    println!("try_send #1: {:?}", tx.try_send("a"));
    match tx.try_send("b") {
        Err(TrySendError::Full(v)) => println!("try_send #2: Err(Full({v:?})): refused, and the value comes back"),
        other => println!("try_send #2: {other:?}"),
    }

    // Capacity 0: a rendezvous. send() returns only when a receiver has taken the value.
    let (tx, rx) = mpsc::sync_channel::<u32>(0);
    let t0 = Instant::now();
    let h = thread::spawn(move || {
        thread::sleep(Duration::from_millis(30));
        rx.recv().unwrap()
    });
    tx.send(42).unwrap();
    let waited = t0.elapsed();
    println!("rendezvous: send() blocked until the receiver arrived (>= 30 ms: {}); it got {}", waited >= Duration::from_millis(30), h.join().unwrap());
}
