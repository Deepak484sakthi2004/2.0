// verify: debug error:E0382
//! send() takes the value: the producer can't touch it afterwards. (A Java BlockingQueue passes a reference,
//! and the producer can keep mutating the object the consumer is reading.)
use std::sync::mpsc;
use std::thread;

struct Batch {
    ids: Vec<u64>,
}

fn main() {
    let (tx, rx) = mpsc::channel::<Batch>();
    let consumer = thread::spawn(move || rx.recv().unwrap().ids.len());

    let mut batch = Batch { ids: vec![1, 2, 3] };
    tx.send(batch).unwrap();
    batch.ids.push(4); // the consumer owns the batch now
    println!("{}", consumer.join().unwrap());
}
