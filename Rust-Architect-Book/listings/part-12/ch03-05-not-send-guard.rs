// verify: debug error:future
//! A std MutexGuard held across an .await: the future is !Send (and the lock would be held
//! for as long as the task is suspended, which is the real bug).
use std::sync::Mutex;

static HITS: Mutex<u64> = Mutex::new(0);

async fn flush() {}

async fn record() {
    let mut hits = HITS.lock().unwrap();
    *hits += 1;
    flush().await; // the guard is still alive here
}

fn main() {
    let fut = record();
    std::thread::spawn(move || futures::executor::block_on(fut));
}
