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
use std::borrow::Cow;
use std::mem::size_of;
use std::sync::Arc;

fn main() {
    println!(
        "size_of: String {}, &str {}, Box<str> {}, Arc<str> {}, Cow<str> {}, Option<String> {}",
        size_of::<String>(),
        size_of::<&str>(),
        size_of::<Box<str>>(),
        size_of::<Arc<str>>(),
        size_of::<Cow<str>>(),
        size_of::<Option<String>>()
    );

    let name = String::from("merchant-category:5411");
    let (copies, allocs, _) = measure(|| (0..1000).map(|_| name.clone()).collect::<Vec<String>>());
    println!("1000 x String::clone:  {allocs} allocations");
    drop(copies);

    let shared: Arc<str> = Arc::from(name.as_str()); // one allocation: refcounts + bytes
    let (copies, allocs, _) = measure(|| (0..1000).map(|_| Arc::clone(&shared)).collect::<Vec<Arc<str>>>());
    println!("1000 x Arc<str>::clone: {allocs} allocation (the Vec itself); strong_count = {}", Arc::strong_count(&shared));
    drop(copies);

    // String growth is Vec<u8> growth: minimum non-zero capacity 8, then doubling.
    let mut s = String::new();
    let mut caps = vec![s.capacity()];
    for _ in 0..40 {
        s.push('x');
        if s.capacity() != *caps.last().unwrap() {
            caps.push(s.capacity());
        }
    }
    println!("String capacities over 40 pushes: {caps:?}");
}
