// verify: debug ok
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static PANICS_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Replace the default "thread 'main' panicked at ..." text with one structured log line and a metric.
fn install_panic_hook() {
    panic::set_hook(Box::new(|info| {
        PANICS_TOTAL.fetch_add(1, Relaxed);
        let msg = info.payload_as_str().unwrap_or("<non-string payload>");
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let thread = std::thread::current();
        // A real service emits this through its logger (and captures a backtrace here, if enabled).
        println!("[panic-hook] level=ERROR code=INTERNAL_PANIC thread={} at={at} msg={msg:?}", thread.name().unwrap_or("?"));
    }));
}

#[derive(Default)]
struct Stats {
    served: u64,
    failed: u64,
}

static CATALOG: [&str; 3] = ["keyboard", "monitor", "dock"];

/// A handler with a bug: it trusts the id in the path.
fn handle(path: &str, stats: &mut Stats) -> Result<String, (u16, &'static str)> {
    let id: usize = path.strip_prefix("/items/").ok_or((404, "NOT_FOUND"))?.parse().map_err(|_| (400, "BAD_ID"))?;
    stats.served += 1;
    Ok(CATALOG[id].to_string()) // BUG: no bounds check → panic for id >= 3
}

/// The boundary: one request's panic becomes one 500, and the loop keeps serving.
fn serve(path: &str, stats: &mut Stats) -> (u16, String) {
    // `&mut Stats` is not UnwindSafe: after a panic it might be half-updated. We assert that is acceptable
    // here (counters only), which is exactly the judgement AssertUnwindSafe asks you to make.
    match panic::catch_unwind(AssertUnwindSafe(|| handle(path, stats))) {
        Ok(Ok(body)) => (200, body),
        Ok(Err((status, code))) => (status, code.to_string()),
        Err(_) => {
            stats.failed += 1;
            (500, "INTERNAL".to_string())
        }
    }
}

fn main() {
    install_panic_hook();
    let mut stats = Stats::default();
    for path in ["/items/0", "/items/7", "/items/x", "/users/1", "/items/2"] {
        let (status, body) = serve(path, &mut stats);
        println!("{path:<9} -> {status} {body}");
    }
    println!("served={} failed={} panics_total={}", stats.served, stats.failed, PANICS_TOTAL.load(Relaxed));
    let _ = panic::take_hook(); // restore the default hook
}
