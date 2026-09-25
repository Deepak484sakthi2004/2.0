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

/// An API that demands `&'static str` (say, a metrics label registry).
fn register_label(label: &'static str) -> usize {
    label.len()
}

/// BUG: "fixing" the 'static requirement with Box::leak, once per request.
fn handle_request_leaky(tenant: &str) -> usize {
    let label: &'static str = Box::leak(format!("tenant.{tenant}").into_boxed_str());
    register_label(label)
}

/// Owned data instead: the value is freed when the request is done.
fn handle_request_owned(tenant: &str) -> usize {
    let label = format!("tenant.{tenant}");
    label.len()
}

fn main() {
    let ((), leaky_allocs, leaky_frees) = counting::measure(|| {
        for i in 0..1_000 {
            handle_request_leaky(&format!("t{i}"));
        }
    });
    let ((), owned_allocs, owned_frees) = counting::measure(|| {
        for i in 0..1_000 {
            handle_request_owned(&format!("t{i}"));
        }
    });
    println!("leaky: {leaky_allocs} allocations, {leaky_frees} frees  -> {} blocks leaked", leaky_allocs - leaky_frees);
    println!("owned: {owned_allocs} allocations, {owned_frees} frees  -> {} blocks leaked", owned_allocs - owned_frees);
}
