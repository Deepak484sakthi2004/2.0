// verify: debug ok
// verify: debug miri-ok
// One process, two allocators that don't know about each other: Rust's global allocator (here a
// counting wrapper, like Meridian's mimalloc in production) and the C library's malloc/free.
// "Who allocates, frees" is a rule about WHICH allocator, not just about who calls free.
use std::ffi::{CString, c_char, c_void};

// --- instrumentation: count Rust heap allocations and frees (GlobalAlloc is explained in Part XV) ---
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

    pub fn counts() -> (usize, usize) {
        (ALLOCS.load(Relaxed), FREES.load(Relaxed))
    }
}

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(p: *mut c_void);
}

fn report(label: &str, before: (usize, usize)) {
    let (a, f) = counting::counts();
    println!("{label:<44} Rust allocator: +{} allocs, +{} frees", a - before.0, f - before.1);
}

fn main() {
    let t = counting::counts();
    let owned = CString::new("merchant-42").unwrap(); // Rust's allocator
    let raw: *mut c_char = owned.into_raw(); // ownership leaves the type system...
    report("CString::new + into_raw", t);

    let t = counting::counts();
    // SAFETY: `raw` came from CString::into_raw and returns exactly once, unmodified in length.
    drop(unsafe { CString::from_raw(raw) }); // ...and comes back to the allocator that made it
    report("CString::from_raw + drop", t);

    let t = counting::counts();
    // SAFETY: malloc has no preconditions; the result is checked and freed once, by free().
    unsafe {
        let p = malloc(64);
        assert!(!p.is_null());
        free(p);
    }
    report("malloc(64) + free", t);
}
