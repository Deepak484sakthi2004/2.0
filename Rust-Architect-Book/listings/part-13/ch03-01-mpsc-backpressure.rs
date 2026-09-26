// verify: debug ok
//! A bounded mpsc channel is a backpressure device: send().await waits while the queue is full.
//! The clock is paused (tokio test-util), so the virtual timestamps below are exact and repeatable.
use std::time::Duration;
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio::time::{self, Instant};

#[tokio::main(flavor = "current_thread", start_paused = true)]
async fn main() {
    let start = Instant::now();
    let ms = move || start.elapsed().as_millis();

    // A producer that could send instantly, a consumer that needs 10 ms per item, capacity 4.
    let (tx, mut rx) = mpsc::channel::<u32>(4);
    let consumer = tokio::spawn(async move {
        let mut got = Vec::new();
        while let Some(x) = rx.recv().await {
            time::sleep(Duration::from_millis(10)).await; // process one item
            got.push(x);
        }
        got // recv() returned None: every sender is gone and the queue is drained
    });
    let mut sent_at = Vec::new();
    for i in 0..10 {
        tx.send(i).await.unwrap(); // waits for a free slot
        sent_at.push(ms());
    }
    println!("send() of items 0..10 completed at {sent_at:?} ms");
    drop(tx);
    println!("consumer received {:?}, finished at {} ms", consumer.await.unwrap(), ms());

    // try_send never waits: it hands the value back when the queue is full.
    let (tx, mut rx) = mpsc::channel::<&str>(2);
    for msg in ["a", "b", "c"] {
        match tx.try_send(msg) {
            Ok(()) => println!("try_send({msg:?}): queued"),
            Err(TrySendError::Full(v)) => println!("try_send({v:?}): Full, value handed back"),
            Err(TrySendError::Closed(v)) => println!("try_send({v:?}): Closed"),
        }
    }

    // reserve() takes a slot first and sends later: nothing is lost if the caller is cancelled in between.
    rx.recv().await;
    let permit = tx.reserve().await.unwrap();
    permit.send("d");
    println!("after reserve + send: queue holds {} of {}", tx.max_capacity() - tx.capacity(), tx.max_capacity());

    // Dropping the receiver closes the channel: send() fails and returns the value.
    drop(rx);
    let err = tx.send("e").await.unwrap_err();
    println!("send after the receiver is gone: Err(SendError({:?}))", err.0);
}
