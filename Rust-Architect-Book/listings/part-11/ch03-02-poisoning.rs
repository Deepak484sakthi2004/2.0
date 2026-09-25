// verify: debug ok
//! A thread panicked while holding the lock, halfway through an update. Three policies for everyone else.
use std::panic;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

/// Invariant: the entries always sum to `total`.
#[derive(Debug)]
struct Ledger {
    entries: Vec<i64>,
    total: i64,
}

impl Ledger {
    fn consistent(&self) -> bool {
        self.entries.iter().sum::<i64>() == self.total
    }
}

fn main() {
    // Print panic messages on stdout, one line each, so the output shows the whole story in order.
    panic::set_hook(Box::new(|info| {
        println!("  [panic] {}", info.payload_as_str().unwrap_or("<non-string payload>"));
    }));

    let ledger = Arc::new(Mutex::new(Ledger { entries: vec![100], total: 100 }));

    let l = Arc::clone(&ledger);
    let _ = thread::spawn(move || {
        let mut g = l.lock().unwrap();
        g.entries.push(-30); // first half of the update...
        panic!("fee service timed out"); // ...and the second half (total -= 30) never happens
    })
    .join();

    println!("is_poisoned = {}", ledger.is_poisoned());

    // Policy 1: propagate. `lock().unwrap()` turns the poison into a panic in THIS thread too.
    let r = panic::catch_unwind(|| ledger.lock().unwrap().total);
    println!("policy 1 (unwrap):  {}", if r.is_err() { "this thread panicked as well" } else { "ok" });

    // Policy 2: ignore. Take the guard anyway, and trust that no invariant can be broken.
    let g = ledger.lock().unwrap_or_else(PoisonError::into_inner);
    println!("policy 2 (ignore):  {:?}, consistent = {}", *g, g.consistent());
    drop(g);

    // Policy 3: repair. Restore the invariant, then clear the poison flag (stable since 1.77).
    match ledger.lock() {
        Ok(_) => unreachable!("still poisoned"),
        Err(poisoned) => {
            let mut g = poisoned.into_inner();
            g.total = g.entries.iter().sum(); // or roll back: g.entries.pop()
            println!("policy 3 (repair):  {:?}, consistent = {}", *g, g.consistent());
        }
    }
    ledger.clear_poison();
    println!("after clear_poison: is_poisoned = {}, lock() is Ok = {}", ledger.is_poisoned(), ledger.lock().is_ok());
}
