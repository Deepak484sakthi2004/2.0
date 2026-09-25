// verify: debug ok
//! Chapter 4.1's request statistics, grown up: Cell counters inside a request (one thread, plain loads and
//! stores), atomics for process-wide totals (touched once per request, not once per event).
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

/// Per request, owned by one thread: Cell is enough, and it's free.
#[derive(Default)]
struct RequestStats {
    cache_lookups: Cell<u32>,
    cache_hits: Cell<u32>,
    upstream_calls: Cell<u32>,
}

/// Process-wide: shared by every worker thread.
#[derive(Default)]
struct GlobalStats {
    requests: AtomicU64,
    cache_lookups: AtomicU64,
    cache_hits: AtomicU64,
    upstream_calls: AtomicU64,
}

struct RequestContext<'g> {
    stats: RequestStats,
    global: &'g GlobalStats,
}

fn lookup_cache(ctx: &RequestContext, key: u32) -> bool {
    ctx.stats.cache_lookups.set(ctx.stats.cache_lookups.get() + 1);
    let hit = key % 3 != 0;
    if hit {
        ctx.stats.cache_hits.set(ctx.stats.cache_hits.get() + 1);
    }
    hit
}

fn call_upstream(ctx: &RequestContext) {
    ctx.stats.upstream_calls.set(ctx.stats.upstream_calls.get() + 1);
}

fn handle(ctx: &RequestContext, request_id: u32) {
    for k in request_id..request_id + 4 {
        if !lookup_cache(ctx, k) {
            call_upstream(ctx);
        }
    }
}

impl Drop for RequestContext<'_> {
    /// Fold the request's counters into the global ones: 4 atomic adds per request, whatever happened inside.
    fn drop(&mut self) {
        let g = self.global;
        g.requests.fetch_add(1, Ordering::Relaxed);
        g.cache_lookups.fetch_add(self.stats.cache_lookups.get().into(), Ordering::Relaxed);
        g.cache_hits.fetch_add(self.stats.cache_hits.get().into(), Ordering::Relaxed);
        g.upstream_calls.fetch_add(self.stats.upstream_calls.get().into(), Ordering::Relaxed);
    }
}

fn main() {
    let global = GlobalStats::default();
    thread::scope(|s| {
        for w in 0..4u32 {
            let global = &global;
            s.spawn(move || {
                for r in 0..2_500 {
                    let ctx = RequestContext { stats: RequestStats::default(), global };
                    handle(&ctx, w * 10_000 + r);
                } // ctx dropped: counters folded in
            });
        }
    });
    let g = &global;
    println!(
        "requests={} lookups={} hits={} upstream={}",
        g.requests.load(Ordering::Relaxed),
        g.cache_lookups.load(Ordering::Relaxed),
        g.cache_hits.load(Ordering::Relaxed),
        g.upstream_calls.load(Ordering::Relaxed)
    );
}
