// verify: debug error:E0373
use std::thread;

fn main() {
    let tenant = String::from("acme");
    let handle = thread::spawn(|| {
        println!("warming cache for {tenant}");
    });
    handle.join().unwrap();
}
