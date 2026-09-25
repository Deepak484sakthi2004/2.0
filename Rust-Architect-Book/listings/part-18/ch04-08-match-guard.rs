// verify: debug ok
// A `match` scrutinee's temporaries live until the END of the match (edition 2024 changed `if let`,
// not `match`). The MutexGuard from `lock()` is still alive inside the arm, so `bump` can't lock.
// (With `lock()` instead of `try_lock()` in `bump`, this program deadlocks.)
use std::sync::Mutex;

struct State {
    status: u8,
    retries: u32,
}

fn bump(m: &Mutex<State>) {
    match m.try_lock() {
        Ok(mut s) => s.retries += 1,
        Err(_) => println!("  bump: lock is still held"),
    }
}

#[inline(never)]
fn process(m: &Mutex<State>) -> &'static str {
    match m.lock().unwrap().status {
        0 => {
            bump(m);
            "retried"
        }
        _ => "done",
    }
}

fn main() {
    let m = Mutex::new(State { status: 0, retries: 0 });
    println!("{}", process(&m));
    println!("retries = {}", m.lock().unwrap().retries);
}
