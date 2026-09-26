// verify: release ok
// A poor man's heap profiler (the idea behind dhat and heaptrack): a counting allocator that attributes every
// allocation to the "site" the current thread declared, and buckets sizes by power of two.
use std::cell::Cell;
use std::collections::HashMap;

mod profiler {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    pub const SITES: usize = 4;
    pub static COUNT: [AtomicU64; SITES] = [const { AtomicU64::new(0) }; SITES];
    pub static BYTES: [AtomicU64; SITES] = [const { AtomicU64::new(0) }; SITES];
    pub static SIZE_BUCKETS: [AtomicU64; 16] = [const { AtomicU64::new(0) }; 16];

    thread_local! {
        pub static SITE: Cell<usize> = const { Cell::new(0) };
    }

    pub struct Profiling;

    // SAFETY: both methods forward their exact arguments to `System`, which upholds the GlobalAlloc contract.
    // The bookkeeping uses only atomics and a const-initialized thread-local Cell, so it never allocates;
    // `try_with` avoids touching the thread-local after it has been destroyed.
    unsafe impl GlobalAlloc for Profiling {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let site = SITE.try_with(|s| s.get()).unwrap_or(0);
            COUNT[site].fetch_add(1, Relaxed);
            BYTES[site].fetch_add(layout.size() as u64, Relaxed);
            let bucket = (usize::BITS - layout.size().max(1).leading_zeros()).min(15) as usize;
            SIZE_BUCKETS[bucket].fetch_add(1, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Profiling = Profiling;
}

const SITE_NAMES: [&str; profiler::SITES] = ["(untagged)", "decode_order", "enrich_headers", "access_log"];

fn at_site<R>(site: usize, f: impl FnOnce() -> R) -> R {
    let prev = profiler::SITE.with(|s| s.replace(site));
    let r = f();
    profiler::SITE.with(|s: &Cell<usize>| s.set(prev));
    r
}

fn main() {
    let order_json = r#"{"order_id":"ord_91833","merchant":"m_48213","amount_minor":12999,"currency":"EUR",
        "items":[{"sku":"A-1","qty":2},{"sku":"B-7","qty":1}],"metadata":{"channel":"web","campaign":"autumn"}}"#;
    let config: HashMap<String, String> =
        (0..40).map(|i| (format!("header-{i}"), format!("value-{i}"))).collect();

    for _ in 0..10_000 {
        // Stage 1: decode into a dynamic JSON tree (every string and map allocates).
        let v: serde_json::Value = at_site(1, || serde_json::from_str(order_json).unwrap());
        // Stage 2: "enrich" by cloning the per-request config map (Chapter 3.2's 2,001-allocation pattern).
        let headers = at_site(2, || config.clone());
        // Stage 3: build an access-log line with format! (Chapter 9.2's pattern).
        let line = at_site(3, || {
            format!("{} {} {} {}", v["order_id"], v["merchant"], headers.len(), v["amount_minor"])
        });
        std::hint::black_box((v, headers, line));
    }

    println!("{:<16} {:>10} {:>12} {:>10}", "site", "allocs", "bytes", "per order");
    for (i, name) in SITE_NAMES.iter().enumerate() {
        let n = profiler::COUNT[i].load(std::sync::atomic::Ordering::Relaxed);
        let b = profiler::BYTES[i].load(std::sync::atomic::Ordering::Relaxed);
        if i > 0 {
            println!("{name:<16} {n:>10} {b:>12} {:>10.1}", n as f64 / 10_000.0);
        }
    }
    println!("allocation sizes (all sites, bytes <= bucket):");
    for (i, b) in profiler::SIZE_BUCKETS.iter().enumerate() {
        let n = b.load(std::sync::atomic::Ordering::Relaxed);
        if n > 0 {
            println!("  <= {:>6}: {n}", 1u64 << i);
        }
    }
}
