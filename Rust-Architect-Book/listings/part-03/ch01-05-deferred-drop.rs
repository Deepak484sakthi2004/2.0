// verify: release ok
use std::collections::HashMap;
use std::thread;
use std::time::Instant;

fn build() -> HashMap<u64, String> {
    (0..1_000_000u64).map(|i| (i, format!("session-{i}"))).collect()
}

fn main() {
    // Inline: the thread that drops the map pays for 1,000,001 frees.
    let map = build();
    let t = Instant::now();
    drop(map);
    let inline = t.elapsed();

    // Deferred: hand the map to another thread; this thread only pays for the handoff.
    let map = build();
    let t = Instant::now();
    let dropper = thread::spawn(move || drop(map));
    let handoff = t.elapsed();
    dropper.join().unwrap();

    println!("inline drop on this thread: {inline:?}");
    println!("handoff to a dropper thread: {handoff:?}");
}
