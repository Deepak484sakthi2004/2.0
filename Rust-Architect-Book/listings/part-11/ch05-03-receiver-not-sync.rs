// verify: debug error:E0277
//! std's Receiver is single-consumer: it is Send (move it to one thread) but not Sync (share it with many).
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

fn main() {
    let (tx, rx) = mpsc::channel::<u32>();
    let rx = Arc::new(rx);
    for _ in 0..3 {
        let rx = Arc::clone(&rx);
        thread::spawn(move || {
            while let Ok(job) = rx.recv() {
                println!("job {job}");
            }
        });
    }
    tx.send(1).unwrap();
}
