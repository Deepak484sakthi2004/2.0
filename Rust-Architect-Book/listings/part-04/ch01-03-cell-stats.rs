// verify: debug ok
use std::cell::Cell;

/// Request-scoped statistics. Helpers only get `&RequestStats`, yet can still count:
/// Cell provides mutation through a shared reference (interior mutability).
#[derive(Default)]
struct RequestStats {
    cache_hits: Cell<u32>,
    cache_misses: Cell<u32>,
    db_queries: Cell<u32>,
}

fn lookup(key: &str, stats: &RequestStats) -> String {
    if key.starts_with("hot:") {
        stats.cache_hits.set(stats.cache_hits.get() + 1);
    } else {
        stats.cache_misses.set(stats.cache_misses.get() + 1);
        stats.db_queries.set(stats.db_queries.get() + 1);
    }
    format!("value-of-{key}")
}

fn main() {
    let stats = RequestStats::default();
    let a = &stats; // two shared references...
    let b = &stats; // ...both able to "mutate" the counters
    lookup("hot:user:1", a);
    lookup("cold:user:2", b);
    lookup("hot:user:3", a);
    println!(
        "hits={} misses={} db_queries={}",
        stats.cache_hits.get(),
        stats.cache_misses.get(),
        stats.db_queries.get()
    );
}
