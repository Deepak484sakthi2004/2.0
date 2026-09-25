// verify: debug ok
// verify: release ok
// verify: debug miri-ok
// A lending iterator with a generic associated type (GAT, stable since 1.65): each item borrows from
// the iterator itself, so the buffer is reused and there is no allocation per line.

// --- instrumentation: count heap allocations (GlobalAlloc is explained in Part XV) ---
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

    static ALLOCS: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    // SAFETY: both methods forward their exact arguments to `System`, which upholds the
    // GlobalAlloc contract; the counter is a plain atomic, so counting never allocates.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn allocs() -> usize {
        ALLOCS.load(Relaxed)
    }
}

use std::io::BufRead;

trait LendingIterator {
    type Item<'a>
    where
        Self: 'a;
    fn next(&mut self) -> Option<Self::Item<'_>>;
}

struct Lines<R> {
    src: R,
    buf: Vec<u8>,
}

impl<R: BufRead> LendingIterator for Lines<R> {
    type Item<'a>
        = &'a [u8]
    where
        Self: 'a;

    fn next(&mut self) -> Option<&[u8]> {
        self.buf.clear(); // keeps the capacity: the same buffer serves every line
        match self.src.read_until(b'\n', &mut self.buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(&self.buf),
        }
    }
}

fn main() {
    // 10,000 log lines in memory (the input's own allocations happen before we start counting).
    // Under Miri (an interpreter, ~1000x slower) 500 lines exercise the same code paths.
    let n_lines = if cfg!(miri) { 500 } else { 10_000 };
    let mut input = String::new();
    for i in 0..n_lines {
        input.push_str(if i % 50 == 0 { "ERROR payment declined\n" } else { "INFO ok\n" });
    }
    let mut lines = Lines { src: input.as_bytes(), buf: Vec::with_capacity(256) };

    let before = counting::allocs();
    let (mut n, mut errors) = (0, 0);
    while let Some(line) = lines.next() {
        n += 1;
        if line.starts_with(b"ERROR") {
            errors += 1;
        }
    }
    let during = counting::allocs() - before;
    println!("lines={n} errors={errors} allocations while reading={during}");
}
