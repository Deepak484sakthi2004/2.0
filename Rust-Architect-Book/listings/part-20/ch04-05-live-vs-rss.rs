// verify: release ok
// "Memory utilization" is two numbers: what your program holds (live heap) and what the OS has given it (RSS).
// Allocate 1,000,000 small objects, free every other one, then free the rest, then ask glibc to trim.
use std::hint::black_box;
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};

mod counting {
    use super::*;
    use std::alloc::{GlobalAlloc, Layout, System};
    pub static LIVE: AtomicIsize = AtomicIsize::new(0);
    pub struct Counting;
    // SAFETY: both methods forward their exact arguments to `System`, which upholds the GlobalAlloc contract;
    // the live-bytes counter is a plain atomic, so counting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            LIVE.fetch_add(layout.size() as isize, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            LIVE.fetch_sub(layout.size() as isize, Relaxed);
            unsafe { System.dealloc(ptr, layout) }
        }
    }
    #[global_allocator]
    static GLOBAL: Counting = Counting;
}

fn rss_mib() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kb: f64 = status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    kb / 1024.0
}

fn show(step: &str) {
    let live = counting::LIVE.load(Relaxed) as f64 / (1 << 20) as f64;
    println!("{step:<40} live heap {live:7.1} MiB   RSS {:7.1} MiB", rss_mib());
}

fn main() {
    show("start");
    let mut objs: Vec<Option<Box<[u8; 48]>>> = Vec::with_capacity(1_000_000);
    for i in 0..1_000_000u32 {
        objs.push(Some(Box::new([i as u8; 48])));
    }
    black_box(&objs);
    show("1,000,000 x 48-byte objects");
    for (i, o) in objs.iter_mut().enumerate() {
        if i % 2 == 0 {
            *o = None; // free every other object
        }
    }
    show("freed every other object");
    for o in objs.iter_mut() {
        *o = None;
    }
    show("freed all objects (Vec still held)");
    drop(objs);
    show("dropped the Vec too");
    // SAFETY: malloc_trim takes a padding size and has no other preconditions.
    let released = unsafe { libc::malloc_trim(0) };
    show(&format!("after malloc_trim(0) (returned {released})"));
}
