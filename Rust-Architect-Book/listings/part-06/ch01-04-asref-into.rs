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
use std::path::Path;

struct Tenant {
    name: String,
}

impl Tenant {
    /// `impl Into<String>`: take ownership when the caller has a String, convert when it has a &str.
    fn new(name: impl Into<String>) -> Tenant {
        Tenant { name: name.into() }
    }
}

/// `impl AsRef<Path>`: accept &str, String, &Path, PathBuf... without forcing a conversion on the caller.
fn log_file_for(dir: impl AsRef<Path>, tenant: &Tenant) -> String {
    dir.as_ref().join(format!("{}.log", tenant.name)).display().to_string()
}

fn main() {
    let owned = String::from("acme");
    let (a, allocs_owned, _) = measure(|| Tenant::new(owned)); // moves the String in: no new allocation
    let (b, allocs_borrowed, _) = measure(|| Tenant::new("globex")); // &str -> String: one allocation
    println!("Tenant::new(String): {allocs_owned} allocation(s); Tenant::new(&str): {allocs_borrowed} allocation(s)");

    println!("{}", log_file_for("/var/log/meridian", &a));
    println!("{}", log_file_for(String::from("/tmp"), &b));
    println!("{}", log_file_for(Path::new("logs"), &a));
}
