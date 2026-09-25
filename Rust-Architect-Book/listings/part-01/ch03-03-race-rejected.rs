// verify: debug error:E0499
use std::thread;

fn main() {
    let mut counter: u64 = 0;
    thread::scope(|s| {
        s.spawn(|| {
            for _ in 0..1_000_000 {
                counter += 1;
            }
        });
        s.spawn(|| {
            for _ in 0..1_000_000 {
                counter += 1;
            }
        });
    });
    println!("counter = {counter}");
}
