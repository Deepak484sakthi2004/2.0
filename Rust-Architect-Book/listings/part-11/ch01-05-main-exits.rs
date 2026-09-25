// verify: debug ok
//! When `main` returns, the process exits, and every other thread dies mid-flight.
//! (The JVM, by contrast, waits for all non-daemon threads.)
use std::thread;
use std::time::Duration;

fn main() {
    let _uploader = thread::spawn(|| {
        thread::sleep(Duration::from_millis(100)); // "upload the last batch file"
        println!("uploader: batch uploaded"); // never printed: the process is gone by then
    }); // the JoinHandle is dropped: the thread is detached, and nobody waits for it
    println!("main: done, returning");
}
