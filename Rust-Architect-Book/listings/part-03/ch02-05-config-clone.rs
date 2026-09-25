// verify: release ok

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
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
struct Config {
    routes: HashMap<String, String>, // path -> backend
}

fn handle_cloned(cfg: Config, path: &str) -> usize {
    cfg.routes.get(path).map_or(0, |b| b.len())
}

fn handle_shared(cfg: Arc<Config>, path: &str) -> usize {
    cfg.routes.get(path).map_or(0, |b| b.len())
}

fn handle_borrowed(cfg: &Config, path: &str) -> usize {
    cfg.routes.get(path).map_or(0, |b| b.len())
}

fn main() {
    let cfg = Config {
        routes: (0..1_000).map(|i| (format!("/api/r{i}"), format!("backend-{i}"))).collect(),
    };
    let shared = Arc::new(cfg.clone());

    let (_, cloned, _) = measure(|| handle_cloned(cfg.clone(), "/api/r7"));
    let (_, arc, _) = measure(|| handle_shared(Arc::clone(&shared), "/api/r7"));
    let (_, borrowed, _) = measure(|| handle_borrowed(&cfg, "/api/r7"));

    println!("allocations per request: clone {cloned}, Arc {arc}, borrow {borrowed}");
}
