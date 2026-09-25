// verify: debug ok
//! The same ideas from the `futures` crate: join!, join_all (results in input order),
//! FuturesUnordered (results in completion order), and select (first wins, the rest are dropped).
use futures::future::{self, Either, FutureExt};
use futures::stream::{FuturesUnordered, StreamExt};
use std::future::Future;
use std::time::{Duration, Instant};

/// A quote from a (simulated) pricing upstream that answers after `ms` milliseconds.
fn fetch_quote(venue: &'static str, ms: u64) -> impl Future<Output = (&'static str, u64)> {
    let (tx, rx) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        let _ = tx.send(ms * 100);
    });
    rx.map(move |price| (venue, price.unwrap_or(0)))
}

fn main() {
    futures::executor::block_on(async {
        let start = Instant::now();
        let (a, b) = futures::join!(fetch_quote("A", 30), fetch_quote("B", 20));
        println!("join!:       {a:?} {b:?} after ~{} ms", start.elapsed().as_millis() / 10 * 10);

        let quotes = future::join_all(vec![fetch_quote("A", 30), fetch_quote("B", 10), fetch_quote("C", 20)]).await;
        println!("join_all:    {quotes:?} (input order)");

        let mut pending: FuturesUnordered<_> =
            [fetch_quote("A", 30), fetch_quote("B", 10), fetch_quote("C", 20)].into_iter().collect();
        let mut order = Vec::new();
        while let Some((venue, _)) = pending.next().await {
            order.push(venue);
        }
        println!("unordered:   {order:?} (completion order)");

        let slow = fetch_quote("SLOW", 200);
        let deadline = fetch_quote("deadline", 50);
        match future::select(Box::pin(slow), Box::pin(deadline)).await {
            Either::Left((quote, _)) => println!("select:      got {quote:?}"),
            Either::Right((_, _still_pending)) => println!("select:      deadline first; SLOW is dropped with the tuple"),
        }
    });
}
