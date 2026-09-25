// verify: release ok
//! What does a thread cost to create, compared with handing work to a thread that already exists?
//! One run on the shared Playground machine: the numbers are noisy; the ratios are the point.
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

const N: u32 = 2_000;

fn main() {
    // (a) spawn + join a thread that does nothing.
    let t = Instant::now();
    for i in 0..N {
        let h = thread::spawn(move || std::hint::black_box(i));
        h.join().unwrap();
    }
    let spawn_join = t.elapsed() / N;

    // (b) the same with a scoped thread.
    let t = Instant::now();
    for i in 0..N {
        thread::scope(|s| {
            s.spawn(|| std::hint::black_box(i));
        });
    }
    let scoped = t.elapsed() / N;

    // (c) hand a job to an existing worker and wait for the answer (a round trip over two channels).
    let (job_tx, job_rx) = mpsc::channel::<u32>();
    let (done_tx, done_rx) = mpsc::channel::<u32>();
    let worker = thread::spawn(move || {
        for j in job_rx {
            done_tx.send(std::hint::black_box(j)).unwrap();
        }
    });
    let t = Instant::now();
    for i in 0..N {
        job_tx.send(i).unwrap();
        done_rx.recv().unwrap();
    }
    let handoff = t.elapsed() / N;
    drop(job_tx);
    worker.join().unwrap();

    println!("spawn + join (std::thread::spawn): {spawn_join:>10.2?} per thread");
    println!("spawn + join (thread::scope):      {scoped:>10.2?} per thread");
    println!("hand-off to existing worker:       {handoff:>10.2?} per round trip");
}
