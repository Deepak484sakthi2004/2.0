// verify: debug ok
use std::cell::Cell;
use std::sync::Arc;
use std::thread;

trait Middleware {
    fn handle(&self, path: &str) -> bool;
}

struct Auth;

impl Middleware for Auth {
    fn handle(&self, path: &str) -> bool {
        path.starts_with("/v1/")
    }
}

/// Counts calls with a Cell: fine on one thread, but Cell is !Sync.
struct LocalStats {
    calls: Cell<u32>,
}

impl Middleware for LocalStats {
    fn handle(&self, _path: &str) -> bool {
        self.calls.set(self.calls.get() + 1);
        true
    }
}

/// Auto traits are part of the object type: `dyn Middleware + Send + Sync` is a different type.
type SharedPipeline = Arc<Vec<Box<dyn Middleware + Send + Sync>>>;

fn main() {
    let pipeline: SharedPipeline = Arc::new(vec![Box::new(Auth)]);
    let workers: Vec<_> = (0..3)
        .map(|i| {
            let p = Arc::clone(&pipeline);
            thread::spawn(move || {
                let path = if i == 2 { "/admin" } else { "/v1/payments" };
                p.iter().all(|m| m.handle(path))
            })
        })
        .collect();
    let results: Vec<bool> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    println!("{results:?}");

    // A !Sync middleware can still live in a single-threaded pipeline (plain dyn Middleware)...
    let local: Vec<Box<dyn Middleware>> = vec![Box::new(Auth), Box::new(LocalStats { calls: Cell::new(0) })];
    println!("local pipeline admitted: {}", local.iter().all(|m| m.handle("/v1/refunds")));
    // ...but `let bad: Box<dyn Middleware + Send + Sync> = Box::new(LocalStats { .. })` is rejected (E0277).
}
