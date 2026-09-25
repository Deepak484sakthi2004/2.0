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

use counting::measure;

fn main() {
    for word in ["straße", "İstanbul", "ΟΔΥΣΣΕΥΣ", "ﬁle"] {
        let lower = word.to_lowercase();
        let upper = word.to_uppercase();
        println!(
            "{word:>10} ({:>2} bytes) -> lower {lower:?} ({} bytes), upper {upper:?} ({} bytes)",
            word.len(),
            lower.len(),
            upper.len()
        );
    }

    // Case-insensitive comparison: three ways.
    let (a, b) = ("Content-Length", "content-length");
    let ((), allocs, _) = measure(|| {
        let eq = a.to_lowercase() == b.to_lowercase();
        std::hint::black_box(eq);
    });
    println!("to_lowercase() == to_lowercase(): {allocs} allocations");
    let ((), allocs, _) = measure(|| {
        let eq = a.eq_ignore_ascii_case(b);
        std::hint::black_box(eq);
    });
    println!("eq_ignore_ascii_case:              {allocs} allocations");

    let mut header = String::from("X-Request-ID");
    let ((), allocs, _) = measure(|| header.make_ascii_lowercase());
    println!("make_ascii_lowercase in place:     {allocs} allocations -> {header:?}");

    // ASCII-only helpers leave non-ASCII alone; that is a feature for protocol tokens, a bug for names.
    println!("\"STRASSE\".eq_ignore_ascii_case(\"straße\") = {}", "STRASSE".eq_ignore_ascii_case("straße"));
    println!("\"ÉCOLE\".to_ascii_lowercase() = {:?}", "ÉCOLE".to_ascii_lowercase());
}
