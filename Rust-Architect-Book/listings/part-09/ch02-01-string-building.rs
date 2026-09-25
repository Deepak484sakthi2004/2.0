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
use std::fmt::Write as _;
use std::time::Instant;

struct Row {
    id: u32,
    price: u64,
}

fn main() {
    let rows: Vec<Row> = (0..10_000).map(|i| Row { id: i, price: 100 + i as u64 }).collect();

    let report = |name: &str, (s, allocs, _): (String, usize, usize), t: std::time::Duration| {
        println!("{name:<34} {allocs:>6} allocs  {:>7} bytes  {t:?}", s.len());
    };

    // 1. The Java habit: s = s + ...  (String + &str reuses the left buffer, so this is NOT
    //    quadratic in Rust; but each format!() allocates a temporary.)
    let t = Instant::now();
    let r = measure(|| {
        let mut s = String::new();
        for r in &rows {
            s = s + &format!("{},{}\n", r.id, r.price);
        }
        s
    });
    report("s = s + &format!(..)", r, t.elapsed());

    // 2. The accidentally quadratic version: format! re-copies the whole prefix every time.
    let t = Instant::now();
    let r = measure(|| {
        let mut s = String::new();
        for r in &rows {
            s = format!("{s}{},{}\n", r.id, r.price);
        }
        s
    });
    report("s = format!(\"{s}..\")", r, t.elapsed());

    // 3. push_str of a format! temporary: one temporary per row.
    let t = Instant::now();
    let r = measure(|| {
        let mut s = String::new();
        for r in &rows {
            s.push_str(&format!("{},{}\n", r.id, r.price));
        }
        s
    });
    report("s.push_str(&format!(..))", r, t.elapsed());

    // 4. write! straight into the buffer: no temporaries, amortized growth only.
    let t = Instant::now();
    let r = measure(|| {
        let mut s = String::new();
        for r in &rows {
            writeln!(s, "{},{}", r.id, r.price).unwrap();
        }
        s
    });
    report("writeln!(s, ..)", r, t.elapsed());

    // 5. write! into a pre-sized buffer: one allocation total.
    let t = Instant::now();
    let r = measure(|| {
        let mut s = String::with_capacity(rows.len() * 16);
        for r in &rows {
            writeln!(s, "{},{}", r.id, r.price).unwrap();
        }
        s
    });
    report("with_capacity + writeln!", r, t.elapsed());

    // 6. Collect pieces then join: one String per row plus the Vec plus the result.
    let t = Instant::now();
    let r = measure(|| rows.iter().map(|r| format!("{},{}", r.id, r.price)).collect::<Vec<_>>().join("\n"));
    report("collect::<Vec<String>>().join", r, t.elapsed());
}
