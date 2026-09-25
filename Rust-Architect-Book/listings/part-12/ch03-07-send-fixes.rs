// verify: debug ok
//! The three !Send futures of ch03-04..06, fixed. Each future is created on the main thread and
//! moved to another one, which is what a multi-threaded executor does with a spawned task.
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

async fn flush() {}

// Fix 1: Arc instead of Rc (or: don't keep the Rc alive across the await).
async fn handler() -> usize {
    let counter = Arc::new(AtomicUsize::new(1));
    flush().await;
    counter.load(Ordering::Relaxed)
}

// Fix 2: end the guard's scope before the await. The lock is held for microseconds, not for
// as long as the task happens to be suspended.
static HITS: Mutex<u64> = Mutex::new(0);
async fn record() -> u64 {
    let now = {
        let mut hits = HITS.lock().unwrap();
        *hits += 1;
        *hits
    }; // guard dropped here
    flush().await;
    now
}

// Fix 3: a thread-safe error type.
async fn parse_amount(s: &str) -> Result<u32, Box<dyn Error + Send + Sync>> {
    let parsed = s.parse::<u32>().map_err(|e| e.into());
    flush().await;
    parsed
}

fn on_another_thread<F>(fut: F) -> F::Output
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    std::thread::spawn(move || futures::executor::block_on(fut)).join().unwrap()
}

fn main() {
    println!("handler -> {}", on_another_thread(handler()));
    println!("record  -> {}", on_another_thread(record()));
    println!("parse   -> {:?}", on_another_thread(parse_amount("42")));
    println!("parse   -> {:?}", on_another_thread(parse_amount("4x2")).map_err(|e| e.to_string()));
}
