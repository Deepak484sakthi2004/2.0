// verify: release ok
// No counting allocator here: we want the SYSTEM allocator's real realloc behavior.
fn main() {
    let mut v: Vec<u64> = Vec::new();
    let mut last_ptr = v.as_ptr() as usize;
    let mut last_cap = v.capacity();
    let mut events = Vec::with_capacity(64);
    for i in 0..4_000_000u64 {
        v.push(i);
        if v.capacity() != last_cap {
            let ptr = v.as_ptr() as usize;
            events.push((v.capacity(), last_cap == 0 || ptr != last_ptr));
            last_ptr = ptr;
            last_cap = v.capacity();
        }
    }
    let moved = events.iter().filter(|(_, m)| *m).count();
    println!("{} growth events, buffer address changed in {moved} of them", events.len());
    for (cap, moved) in &events {
        let bytes = cap * 8;
        println!("  cap {cap:>8} ({:>9} bytes): {}", bytes, if *moved { "moved" } else { "grew in place" });
    }
}
