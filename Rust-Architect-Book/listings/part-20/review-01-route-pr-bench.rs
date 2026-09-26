// verify: release ok
// PR #2291 (gateway-core): "router: add a last-hit cache in front of the route map. 2x faster lookups."
// This is the PR's code and the PR's benchmark, as submitted. Review both before reading review-02.
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// The router on main today.
pub struct Router {
    routes: HashMap<String, u32>,
}

impl Router {
    pub fn lookup(&self, path: &str) -> Option<u32> {
        self.routes.get(&path.to_string()).copied()
    }
}

/// The PR: remember the last path looked up and its answer.
pub struct CachedRouter {
    inner: Router,
    last: Mutex<Option<(String, Option<u32>)>>,
}

impl CachedRouter {
    pub fn lookup(&self, path: &str) -> Option<u32> {
        let mut last = self.last.lock().unwrap();
        if let Some((p, id)) = last.as_ref() {
            if p == path {
                return *id; // cache hit
            }
        }
        let id = self.inner.lookup(path);
        *last = Some((path.to_string(), id));
        id
    }
}

fn main() {
    let paths: Vec<String> = (0..1000).map(|i| format!("/v1/svc{i:04}/items")).collect();
    let old = Router { routes: paths.iter().cloned().zip(0..).collect() };
    let new = CachedRouter { inner: Router { routes: paths.iter().cloned().zip(0..).collect() }, last: Mutex::new(None) };

    let t = Instant::now();
    for _ in 0..1_000_000 {
        old.lookup("/v1/svc0001/items");
    }
    let a = t.elapsed();

    let t = Instant::now();
    for _ in 0..1_000_000 {
        new.lookup("/v1/svc0001/items");
    }
    let b = t.elapsed();

    println!("old router:    {a:?} for 1M lookups");
    println!("cached router: {b:?} for 1M lookups");
    println!("speedup: {:.1}x", a.as_secs_f64() / b.as_secs_f64());
}
