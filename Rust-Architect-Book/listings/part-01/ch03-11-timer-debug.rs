// verify: debug ok
use std::thread;
use std::time::{Duration, Instant};

struct Timer {
    label: &'static str,
    start: Instant,
}

impl Timer {
    fn start(label: &'static str) -> Self {
        Timer { label, start: Instant::now() }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        println!("{} took {:?}", self.label, self.start.elapsed());
    }
}

fn slow_query() {
    thread::sleep(Duration::from_millis(200));
}

fn main() {
    let _ = Timer::start("slow_query");
    slow_query();
}
