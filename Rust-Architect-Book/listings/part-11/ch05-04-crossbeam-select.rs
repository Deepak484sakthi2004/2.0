// verify: debug ok
//! crossbeam-channel: multi-producer AND multi-consumer, plus select! over several channels and a timer.
use crossbeam_channel::{bounded, select, tick, Receiver};
use std::thread;
use std::time::Duration;

fn worker(id: usize, jobs: Receiver<u32>, shutdown: Receiver<()>) -> (usize, u32, u32) {
    let heartbeat = tick(Duration::from_millis(5));
    let (mut done, mut beats) = (0, 0);
    loop {
        select! {
            recv(jobs) -> job => match job {
                Ok(j) => { std::hint::black_box(j); done += 1; }
                Err(_) => break, // every Sender dropped: no more work will ever come
            },
            recv(shutdown) -> _ => break, // a message OR disconnection: both mean stop
            recv(heartbeat) -> _ => beats += 1,
        }
    }
    (id, done, beats)
}

fn main() {
    let (job_tx, job_rx) = bounded::<u32>(64);
    let (stop_tx, stop_rx) = bounded::<()>(0);
    let workers: Vec<_> = (0..3)
        .map(|id| {
            let (jobs, stop) = (job_rx.clone(), stop_rx.clone()); // Receivers clone: MPMC
            thread::spawn(move || worker(id, jobs, stop))
        })
        .collect();
    for j in 0..3_000 {
        job_tx.send(j).unwrap();
    }
    thread::sleep(Duration::from_millis(20)); // idle for a while: heartbeats tick
    drop(stop_tx); // disconnecting the shutdown channel wakes every select! at once

    let mut total = 0;
    for w in workers {
        let (id, done, beats) = w.join().unwrap();
        total += done;
        println!("worker {id}: some jobs = {}, heartbeats seen = {}", done > 0, beats > 0);
    }
    println!("jobs processed in total: {total}");
}
