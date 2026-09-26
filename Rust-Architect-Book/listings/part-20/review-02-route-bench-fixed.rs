// verify: release ok
// verify: debug test
// The corrected benchmark for PR #2291. Changes, one per defect found in review:
//   - realistic input: 4,096 request paths over 1,000 routes, skewed like production (a few hot routes, a long tail),
//     2% unknown paths; the PR's benchmark asked for ONE path a million times (a 100% cache hit rate)
//   - one change at a time: main as it is, main with the real fix (no String per lookup), the PR's cache
//   - results and inputs through black_box; warm-up; 31 interleaved samples; medians with a spread
//   - the gateway's real shape: 4 worker threads sharing one router (the PR's Mutex is on every lookup)
//   - allocations counted per lookup; a test that every router gives the same answers
use std::collections::HashMap;
use std::hint::black_box;
use std::sync::Mutex;
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

pub struct Router {
    routes: HashMap<String, u32>,
}

impl Router {
    /// On main: builds a String for every lookup.
    pub fn lookup(&self, path: &str) -> Option<u32> {
        self.routes.get(&path.to_string()).copied()
    }
    /// The one-line fix: a `HashMap<String, _>` can be queried with `&str` (String: Borrow<str>, Chapter 9.3).
    pub fn lookup_borrowed(&self, path: &str) -> Option<u32> {
        self.routes.get(path).copied()
    }
}

/// The PR's cache, unchanged.
pub struct CachedRouter {
    inner: Router,
    last: Mutex<Option<(String, Option<u32>)>>,
}

impl CachedRouter {
    pub fn lookup(&self, path: &str) -> Option<u32> {
        let mut last = self.last.lock().unwrap();
        if let Some((p, id)) = last.as_ref() {
            if p == path {
                return *id;
            }
        }
        let id = self.inner.lookup(path);
        *last = Some((path.to_string(), id));
        id
    }
}

fn build() -> (Vec<String>, Router, CachedRouter) {
    let paths: Vec<String> = (0..1000).map(|i| format!("/v1/svc{i:04}/items")).collect();
    let r = Router { routes: paths.iter().cloned().zip(0..).collect() };
    let c = CachedRouter { inner: Router { routes: paths.iter().cloned().zip(0..).collect() }, last: Mutex::new(None) };
    (paths, r, c)
}

/// 4,096 request paths: log-uniform over route rank (a few hot routes, a long tail), 2% unknown paths.
fn request_mix(paths: &[String], seed: u64) -> Vec<String> {
    let mut x = seed;
    (0..4096)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            if x % 50 == 0 {
                return format!("/v1/unknown{}/items", x % 997);
            }
            let u = (x >> 11) as f64 / (1u64 << 53) as f64;
            let rank = (1000f64.powf(u) as usize).clamp(1, 1000) - 1;
            paths[rank].clone()
        })
        .collect()
}

static SINK: AtomicUsize = AtomicUsize::new(0);

/// ns per lookup for `threads` threads, each running its own request mix through `f`; median of 31 samples.
fn measure(threads: usize, mixes: &[Vec<String>], f: &(dyn Fn(&str) -> Option<u32> + Sync)) -> (f64, f64, f64) {
    let mut samples: Vec<f64> = (0..31)
        .map(|_| {
            let t = Instant::now();
            thread::scope(|s| {
                for mix in &mixes[..threads] {
                    s.spawn(move || {
                        let mut hits = 0usize;
                        for p in black_box(mix) {
                            hits += black_box(f(black_box(p.as_str()))).is_some() as usize;
                        }
                        SINK.fetch_add(hits, Relaxed);
                    });
                }
            });
            t.elapsed().as_nanos() as f64 / mixes[0].len() as f64
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    (samples[3], samples[15], samples[27])
}

fn main() {
    let (paths, main_router, pr_router) = build();
    let mixes: Vec<Vec<String>> = (0..4).map(|t| request_mix(&paths, 0x2545_F491_4F6C_DD1D + t)).collect();
    let hit_rate = mixes[0].windows(2).filter(|w| w[0] == w[1]).count() as f64 / (mixes[0].len() - 1) as f64;
    println!("last-hit cache hit rate on the realistic mix: {:.1}%", 100.0 * hit_rate);

    let cands: [(&str, &(dyn Fn(&str) -> Option<u32> + Sync)); 3] = [
        ("main (String per lookup)", &|p| main_router.lookup(p)),
        ("main + borrowed lookup", &|p| main_router.lookup_borrowed(p)),
        ("PR: last-hit cache", &|p| pr_router.lookup(p)),
    ];
    for (_, f) in &cands {
        measure(1, &mixes, *f); // warm-up round
    }
    println!("wall ns per lookup (per thread's stream), median of 31 [p10 .. p90]; release; one Playground run, noisy");
    for threads in [1usize, 4] {
        for (name, f) in &cands {
            let a0 = counting::ALLOCS.load(Relaxed);
            let (p10, m, p90) = measure(threads, &mixes, *f);
            let lookups = 31 * threads * mixes[0].len();
            let allocs = (counting::ALLOCS.load(Relaxed) - a0) as f64 / lookups as f64;
            println!("  {threads} thread(s)  {name:<26} {m:7.1}  [{p10:.1} .. {p90:.1}]   allocs/lookup {allocs:.2}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_router_gives_the_same_answers() {
        let (paths, r, c) = build();
        for p in request_mix(&paths, 7).iter().chain(paths.iter()) {
            let expected = r.lookup(p);
            assert_eq!(r.lookup_borrowed(p), expected, "{p}");
            assert_eq!(c.lookup(p), expected, "{p}");
            assert_eq!(c.lookup(p), expected, "{p} (second lookup, cache hit)");
        }
        assert_eq!(c.lookup("/v1/svc9999/items"), None);
    }
}
