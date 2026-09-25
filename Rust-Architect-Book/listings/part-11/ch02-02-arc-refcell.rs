// verify: debug error:E0277
//! Arc makes ownership shareable across threads; it does not make the contents thread-safe.
//! Arc<T> is Send only if T is Send + Sync, and RefCell is not Sync.
use std::cell::RefCell;
use std::sync::Arc;
use std::thread;

fn main() {
    let seen = Arc::new(RefCell::new(Vec::<u32>::new()));
    let for_worker = Arc::clone(&seen);
    let h = thread::spawn(move || {
        for_worker.borrow_mut().push(1); // an unsynchronized borrow flag, touched from two threads
    });
    seen.borrow_mut().push(2);
    h.join().unwrap();
}
