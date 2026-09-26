// verify: release ok
// Chapter 9.2's promise: does removing the ~5 allocations per access-log line change latency, and at which
// percentile? Build 400,000 lines per thread, timing each batch of 16 lines, on 1 and on 4 threads.
use hdrhistogram::Histogram;
use std::fmt::Write as _;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::thread;
use std::time::Instant;

mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    pub static ALLOCS: AtomicUsize = AtomicUsize::new(0);
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
}

struct Req<'a> {
    ts: u64,
    method: &'a str,
    path: &'a str,
    status: u16,
    latency_us: u32,
    partner: Option<&'a str>,
    route: Option<u32>,
}

fn line_format(r: &Req) -> String {
    // The first prototype: one format! for the core, one per optional field, joined at the end.
    let core = format!("{} {} {} {} {}", r.ts, r.method, r.path, r.status, r.latency_us);
    let partner = r.partner.map(|p| format!(" partner={p}")).unwrap_or_default();
    let route = r.route.map(|x| format!(" route={x}")).unwrap_or_default();
    [core, partner, route].concat()
}

fn line_reuse(r: &Req, buf: &mut String) {
    buf.clear();
    let _ = write!(buf, "{} {} {} {} {}", r.ts, r.method, r.path, r.status, r.latency_us);
    if let Some(p) = r.partner {
        let _ = write!(buf, " partner={p}");
    }
    if let Some(x) = r.route {
        let _ = write!(buf, " route={x}");
    }
}

static SINK: AtomicUsize = AtomicUsize::new(0);

fn run(threads: usize, reuse: bool) -> (Histogram<u64>, f64) {
    let a0 = counting::ALLOCS.load(Relaxed);
    let handles: Vec<_> = (0..threads)
        .map(|t| {
            thread::spawn(move || {
                let mut h = Histogram::<u64>::new_with_bounds(1, 100_000_000, 3).unwrap();
                let mut buf = String::with_capacity(256);
                let mut total = 0usize;
                for batch in 0..25_000u64 {
                    let t0 = Instant::now();
                    for i in 0..16u64 {
                        let r = Req {
                            ts: 1_700_000_000_000 + batch * 16 + i,
                            method: "POST",
                            path: "/v1/payments/pay_123/refunds",
                            status: 201,
                            latency_us: (i * 37 + t as u64) as u32,
                            partner: Some("acme-travel"),
                            route: Some(250),
                        };
                        if reuse {
                            line_reuse(&r, &mut buf);
                            total += black_box(buf.len());
                        } else {
                            total += black_box(line_format(&r)).len();
                        }
                    }
                    h.record(t0.elapsed().as_nanos() as u64 / 16).unwrap();
                }
                SINK.fetch_add(total, Relaxed);
                h
            })
        })
        .collect();
    let mut all = Histogram::<u64>::new_with_bounds(1, 100_000_000, 3).unwrap();
    for h in handles {
        all.add(h.join().unwrap()).unwrap();
    }
    let lines = threads as f64 * 400_000.0;
    // Histogram buffers and thread spawns allocate a little too; per-line allocations dominate.
    (all, (counting::ALLOCS.load(Relaxed) - a0) as f64 / lines)
}

fn main() {
    println!("ns per line (batches of 16), release; one Playground run, noisy");
    println!("{:<30} {:>7} {:>7} {:>7} {:>9} {:>12}", "variant", "p50", "p99", "p99.9", "max", "allocs/line");
    for threads in [1usize, 4] {
        for reuse in [false, true] {
            let (h, allocs) = run(threads, reuse);
            let label = format!("{} thread(s), {}", threads, if reuse { "reused String" } else { "format! per field" });
            println!(
                "{label:<30} {:>7} {:>7} {:>7} {:>9} {:>12.2}",
                h.value_at_percentile(50.0),
                h.value_at_percentile(99.0),
                h.value_at_percentile(99.9),
                h.max(),
                allocs
            );
        }
    }
}
