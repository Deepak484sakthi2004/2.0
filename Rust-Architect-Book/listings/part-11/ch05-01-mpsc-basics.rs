// verify: debug ok
//! std::sync::mpsc: many producers, one consumer, and disconnection as a first-class signal.
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::Duration;

#[derive(Debug)]
struct Event {
    producer: u32,
    seq: u32,
}

fn main() {
    let (tx, rx) = mpsc::channel::<Event>();
    for p in 0..3 {
        let tx = tx.clone(); // each producer owns a Sender
        thread::spawn(move || {
            for seq in 0..3 {
                tx.send(Event { producer: p, seq }).unwrap();
            }
        }); // this producer's Sender is dropped here
    }
    drop(tx); // main's own Sender: forget this and the loop below never ends

    let mut per_producer = [0u32; 3];
    for e in &rx {
        per_producer[e.producer as usize] += 1;
        let _ = e.seq;
    }
    println!("received per producer {per_producer:?}; the loop ended because every Sender was dropped");
    println!("try_recv now: {:?}", rx.try_recv().unwrap_err());

    // The other direction: the receiver is gone, and send() hands the value back.
    let (tx, rx) = mpsc::channel::<String>();
    drop(rx);
    let err = tx.send("audit record #1".to_string()).unwrap_err();
    println!("send to a dropped receiver: Err(SendError({:?})): the value comes back to you", err.0);

    // Waiting with a deadline.
    let (tx, rx) = mpsc::channel::<u32>();
    println!("recv_timeout on an idle channel: {:?}", rx.recv_timeout(Duration::from_millis(10)).unwrap_err());
    println!("try_recv on an idle channel: {:?}", rx.try_recv().unwrap_err());
    drop(tx);
    assert_eq!(rx.recv_timeout(Duration::from_millis(10)), Err(RecvTimeoutError::Disconnected));
    assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
    println!("after the last Sender drops: Disconnected (not Timeout, not Empty)");
}
