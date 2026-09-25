// verify: debug ok
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug)]
struct Ledger {
    debits: i64,
    credits: i64,
}

fn main() {
    std::panic::set_hook(Box::new(|info| println!("[hook] {}", info.payload_as_str().unwrap_or("?"))));
    let ledger = Arc::new(Mutex::new(Ledger { debits: 0, credits: 0 }));

    let l = Arc::clone(&ledger);
    let worker = thread::spawn(move || {
        let mut g = l.lock().unwrap();
        g.debits += 100; // first half of a two-part update...
        panic!("processor client bug"); // ...second half never happens; the guard unlocks during unwinding
    });
    let joined = worker.join(); // a panicked thread's join returns Err(payload)
    println!("join is_err: {}", joined.is_err());

    match ledger.lock() {
        Ok(g) => println!("lock ok: {g:?}"),
        Err(poisoned) => {
            println!("lock poisoned: {}", poisoned);
            let g = poisoned.into_inner(); // you may still look, knowing an invariant may be broken
            println!("state seen through the poison: {g:?} (balanced: {})", g.debits == g.credits);
        }
    }
    ledger.clear_poison();
    println!("after clear_poison, is_poisoned = {}", ledger.is_poisoned());
}
