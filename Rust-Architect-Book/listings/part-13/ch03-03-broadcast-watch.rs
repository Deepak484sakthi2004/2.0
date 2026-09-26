// verify: debug ok
//! broadcast: every receiver gets every message, unless it falls behind by more than the capacity (Lagged).
//! watch: receivers see only the latest value; intermediate values are skipped by design.
use tokio::sync::{broadcast, watch};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // broadcast with capacity 4; one fast receiver, one that doesn't read until 10 messages were sent.
    let (tx, mut fast) = broadcast::channel::<u32>(4);
    let mut slow = tx.subscribe();
    let mut fast_got = Vec::new();
    for i in 0..10 {
        tx.send(i).unwrap();
        fast_got.push(fast.recv().await.unwrap());
    }
    println!("fast receiver: {fast_got:?}");
    let mut slow_got = Vec::new();
    loop {
        match slow.try_recv() {
            Ok(v) => slow_got.push(format!("{v}")),
            Err(broadcast::error::TryRecvError::Lagged(n)) => slow_got.push(format!("Lagged({n})")),
            Err(broadcast::error::TryRecvError::Empty) => break,
            Err(e) => panic!("{e:?}"),
        }
    }
    println!("slow receiver: {slow_got:?}");

    // watch: a config value. The reader wakes on change and reads the latest version only.
    let (cfg_tx, mut cfg_rx) = watch::channel(("limits-v1", 100u32));
    for v in 2..=5 {
        cfg_tx.send_replace((["", "", "limits-v2", "limits-v3", "limits-v4", "limits-v5"][v], 100 * v as u32));
    }
    cfg_rx.changed().await.unwrap(); // at least one change since this receiver last looked
    println!("watch receiver after 4 updates sees: {:?}", *cfg_rx.borrow_and_update());
    println!("changed since then? {}", cfg_rx.has_changed().unwrap());
}
