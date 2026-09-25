// verify: debug ok
// verify: debug@2021 error:E0597
use std::sync::Mutex;

fn main() {
    let queue = Mutex::new(vec![1, 2, 3]);
    // This `if let` is the TAIL expression of main (no semicolon after it).
    // Edition 2021: temporaries in a tail expression live until AFTER main's locals are dropped,
    // so the MutexGuard would outlive `queue` itself: rejected.
    // Edition 2024: tail-expression temporaries are dropped before the locals: accepted.
    if let Some(&first) = queue.lock().unwrap().first() {
        println!("first = {first}");
    }
}
