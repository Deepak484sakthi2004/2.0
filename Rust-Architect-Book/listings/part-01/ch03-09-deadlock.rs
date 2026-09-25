// verify: debug build
// Compiles cleanly. Running it hangs forever (by design).
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() {
    let accounts = Arc::new((Mutex::new(100i64), Mutex::new(100i64)));

    let a = Arc::clone(&accounts);
    let t1 = thread::spawn(move || {
        let _from = a.0.lock().unwrap();
        thread::sleep(Duration::from_millis(50));
        let _to = a.1.lock().unwrap(); // waits for t2 to release account 1
    });

    let b = Arc::clone(&accounts);
    let t2 = thread::spawn(move || {
        let _from = b.1.lock().unwrap();
        thread::sleep(Duration::from_millis(50));
        let _to = b.0.lock().unwrap(); // waits for t1 to release account 0
    });

    t1.join().unwrap();
    t2.join().unwrap();
    println!("never printed");
}
