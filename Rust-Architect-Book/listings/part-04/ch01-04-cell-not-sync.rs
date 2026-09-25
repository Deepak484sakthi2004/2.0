// verify: debug error:E0277
use std::cell::Cell;
use std::thread;

fn main() {
    let hits = Cell::new(0u32);
    thread::scope(|s| {
        s.spawn(|| hits.set(hits.get() + 1)); // two threads sharing &Cell: a data race waiting to happen
        s.spawn(|| hits.set(hits.get() + 1));
    });
    println!("{}", hits.get());
}
