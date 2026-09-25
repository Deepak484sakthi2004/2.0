// verify: debug error:E0499
// The racy counter of ch02-01, written in safe Rust: it doesn't compile.
use std::thread;

fn main() {
    let mut hits: u64 = 0;
    thread::scope(|s| {
        s.spawn(|| hits += 1);
        s.spawn(|| hits += 1);
    });
    println!("hits = {hits}");
}
