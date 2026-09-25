// verify: debug ok
// verify: release ok
// Batching a shared event stream with `by_ref().zip(0..size)`. When the counter runs out, Zip may pull
// one more item from the FIRST iterator and then drop it. Whether it does depends on the source type.
use std::sync::mpsc;

fn batches_zip(events: &mut impl Iterator<Item = u32>, size: usize) -> Vec<Vec<u32>> {
    let mut out = Vec::new();
    loop {
        let batch: Vec<u32> = events.by_ref().zip(0..size).map(|(e, _)| e).collect();
        if batch.is_empty() {
            return out;
        }
        out.push(batch);
    }
}

fn batches_take(events: &mut impl Iterator<Item = u32>, size: usize) -> Vec<Vec<u32>> {
    let mut out = Vec::new();
    loop {
        let batch: Vec<u32> = events.by_ref().take(size).collect(); // take checks its count BEFORE pulling
        if batch.is_empty() {
            return out;
        }
        out.push(batch);
    }
}

fn channel_of(n: u32) -> mpsc::Receiver<u32> {
    let (tx, rx) = mpsc::channel();
    for e in 1..=n {
        tx.send(e).unwrap();
    }
    rx // the sender is dropped here, so rx.iter() ends after the 10th event
}

fn report(label: &str, batches: &[Vec<u32>]) {
    let published: usize = batches.iter().map(Vec::len).sum();
    println!("{label:<22} {batches:?}  published {published}/10");
}

fn main() {
    // In the unit test, events come from a Vec:
    report("zip,  Vec source", &batches_zip(&mut (1..=10).collect::<Vec<u32>>().into_iter(), 3));
    // In production, events come from a channel:
    report("zip,  channel source", &batches_zip(&mut channel_of(10).iter(), 3));
    report("take, channel source", &batches_take(&mut channel_of(10).iter(), 3));
}
