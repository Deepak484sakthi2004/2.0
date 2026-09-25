// verify: debug ok

// --- instrumentation: count heap allocations and frees (GlobalAlloc is explained in Part XV) ---
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);
    static FREES: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    // SAFETY: both methods forward their exact arguments to `System`, which upholds the
    // GlobalAlloc contract; the counters are plain atomics, so counting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            FREES.fetch_add(1, Relaxed);
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn measure<R>(f: impl FnOnce() -> R) -> (R, usize, usize) {
        let (a0, f0) = (ALLOCS.load(Relaxed), FREES.load(Relaxed));
        let r = f();
        (r, ALLOCS.load(Relaxed) - a0, FREES.load(Relaxed) - f0)
    }
}

use std::borrow::Cow;

/// HTTP header names are case-insensitive; normalize to lowercase. Most already are.
fn normalize_header(name: &str) -> Cow<'_, str> {
    if name.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(name.to_ascii_lowercase()) // pay for an allocation only when something changes
    } else {
        Cow::Borrowed(name) // zero-copy
    }
}

fn main() {
    let headers = ["content-type", "x-request-id", "Accept", "authorization", "X-Forwarded-For", "user-agent"];
    let (normalized, allocs, _) =
        counting::measure(|| headers.iter().map(|h| normalize_header(h)).collect::<Vec<_>>());
    for (original, n) in headers.iter().zip(&normalized) {
        let kind = if matches!(n, Cow::Borrowed(_)) { "borrowed" } else { "owned" };
        println!("{original:>16} -> {n:<16} ({kind})");
    }
    println!("allocations: {allocs} (one for the Vec, one per header that had to change)");
}
