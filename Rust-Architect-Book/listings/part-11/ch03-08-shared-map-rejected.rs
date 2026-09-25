// verify: debug error:E0499
//! Java 7's HashMap could loop forever under concurrent puts; Go's runtime aborts with
//! "fatal error: concurrent map writes". In Rust the same program is a compile error.
use std::collections::HashMap;
use std::thread;

fn main() {
    let mut routes: HashMap<String, u32> = HashMap::new();
    thread::scope(|s| {
        s.spawn(|| routes.insert("/checkout".to_string(), 1));
        s.spawn(|| routes.insert("/refunds".to_string(), 2));
    });
    println!("{}", routes.len());
}
