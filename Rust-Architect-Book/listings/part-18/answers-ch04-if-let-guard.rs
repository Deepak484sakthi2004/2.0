// verify: debug ok
// Answer-key check for Chapter 18.4's debugging exercise, Q3: in edition 2024, an `if let` scrutinee's
// temporaries are dropped before the ELSE block, not before the THEN block, so the guard is still held
// while `bump` runs. Copying the field out in its own statement fixes it.
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

fn process_if_let(m: &Mutex<State>) -> &'static str {
    if let 0 = m.lock().unwrap().status {
        bump(m);
        "retried"
    } else {
        "done"
    }
}

fn process_fixed(m: &Mutex<State>) -> &'static str {
    let status = m.lock().unwrap().status; // the guard is dropped at the end of this statement
    if status == 0 {
        bump(m);
        "retried"
    } else {
        "done"
    }
}

fn main() {
    let m = Mutex::new(State { status: 0, retries: 0 });
    println!("if let: {}", process_if_let(&m));
    println!("fixed:  {}", process_fixed(&m));
    println!("retries = {}", m.lock().unwrap().retries);
}
