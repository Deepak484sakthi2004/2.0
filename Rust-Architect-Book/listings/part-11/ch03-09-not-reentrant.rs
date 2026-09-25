// verify: debug ok
//! Java's synchronized is reentrant; std::sync::Mutex is not. A straight port of a "synchronized method
//! calling another synchronized method" waits for itself. (try_lock reports it instead of hanging.)
use std::sync::{Mutex, TryLockError};

struct Batcher {
    pending: Mutex<Vec<u64>>,
}

impl Batcher {
    fn add(&self, id: u64) {
        let mut p = self.pending.lock().unwrap();
        p.push(id);
        if p.len() >= 3 {
            self.flush(); // Java: fine (same thread re-enters the monitor). Rust: the guard `p` is still alive.
        }
    }

    fn flush(&self) {
        match self.pending.try_lock() {
            Ok(mut p) => println!("flushed {:?}", std::mem::take(&mut *p)),
            Err(TryLockError::WouldBlock) => {
                println!("flush: WouldBlock: this thread already holds the lock; lock() would never return")
            }
            Err(TryLockError::Poisoned(_)) => println!("flush: poisoned"),
        }
    }

    /// The fix: decide under the lock, act after releasing it (or pass the guard's data along).
    fn add_fixed(&self, id: u64) {
        let batch = {
            let mut p = self.pending.lock().unwrap();
            p.push(id);
            if p.len() >= 3 { Some(std::mem::take(&mut *p)) } else { None }
        }; // guard dropped here
        if let Some(batch) = batch {
            println!("flushed {batch:?} (outside the lock)");
        }
    }
}

fn main() {
    let b = Batcher { pending: Mutex::new(Vec::new()) };
    for id in 1..=3 {
        b.add(id);
    }
    let b = Batcher { pending: Mutex::new(Vec::new()) };
    for id in 1..=3 {
        b.add_fixed(id);
    }
}
