// verify: debug ok
// "I dropped it, why is RSS still high?" Freed memory goes back to the allocator, and only sometimes to the OS.
// glibc malloc: large blocks are separate mmaps (returned on free), small blocks live in the heap (brk) and arenas.
fn rss_mib() -> f64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap();
    let kib: f64 = s.lines().find(|l| l.starts_with("VmRSS")).unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    kib / 1024.0
}

fn trim() -> i32 {
    // SAFETY: malloc_trim has no preconditions; it returns 1 if it released memory to the OS.
    unsafe { libc::malloc_trim(0) }
}

fn main() {
    println!("{:<60} {:>8}", "step", "RSS MiB");
    println!("{:<60} {:>8.1}", "start", rss_mib());

    let big = vec![1u8; 128 << 20];
    println!("{:<60} {:>8.1}", "one Vec<u8> of 128 MiB (a single mmap'd block)", rss_mib());
    drop(big);
    println!("{:<60} {:>8.1}", "  dropped", rss_mib());

    let many: Vec<Box<[u8; 64]>> = (0..1_000_000).map(|_| Box::new([1u8; 64])).collect();
    println!("{:<60} {:>8.1}", "1,000,000 Box<[u8; 64]> (small blocks in the heap)", rss_mib());
    drop(many);
    println!("{:<60} {:>8.1}", "  dropped", rss_mib());
    println!("{:<60} {:>8.1}", format!("  malloc_trim(0) -> {}", trim()), rss_mib());

    let mut many: Vec<Option<Box<[u8; 64]>>> = (0..1_000_000).map(|_| Some(Box::new([1u8; 64]))).collect();
    let mut survivors = 0;
    for (i, slot) in many.iter_mut().enumerate() {
        if i % 64 != 0 {
            *slot = None; // free 63 of every 64 blocks: the heap is now full of holes
        } else {
            survivors += 1;
        }
    }
    println!("{:<60} {:>8.1}", format!("again, then free all but every 64th ({survivors} survive)"), rss_mib());
    println!("{:<60} {:>8.1}", format!("  malloc_trim(0) -> {}", trim()), rss_mib());
    drop(many);
    println!("{:<60} {:>8.1}", format!("  drop the rest, malloc_trim(0) -> {}", trim()), rss_mib());
}
