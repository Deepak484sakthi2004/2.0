// verify: debug error:E0277
//! A MutexGuard must be dropped (unlocked) on the thread that locked it, so it is not Send.
use std::sync::Mutex;
use std::thread;

fn main() {
    let balance = Mutex::new(100i64);
    let guard = balance.lock().unwrap();
    thread::scope(|s| {
        s.spawn(move || {
            let mut g = guard; // unlock would happen on this other thread
            *g -= 10;
        });
    });
}
