// verify: debug miri-ok
// verify: release ok
// Double-checked locking (DCL), hand-written the way Java needs `volatile` for it, next to what you
// should actually write in Rust: OnceLock (or LazyLock).
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering::{Acquire, Release}};
use std::sync::{Mutex, OnceLock};
use std::thread;

pub struct Model {
    weights: Vec<f64>,
    version: u32,
}

fn load_model() -> Model {
    Model { weights: vec![0.25, 0.5, 0.25], version: 7 }
}

static MODEL: AtomicPtr<Model> = AtomicPtr::new(ptr::null_mut());
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Hand-rolled DCL. Acquire on the fast path pairs with the Release publication below.
pub fn model() -> &'static Model {
    let p = MODEL.load(Acquire); // first check, no lock
    if !p.is_null() {
        return unsafe { &*p }; // SAFETY: published with Release, never freed (lives for 'static)
    }
    let _guard = INIT_LOCK.lock().unwrap();
    let p = MODEL.load(Acquire); // second check, under the lock
    if !p.is_null() {
        return unsafe { &*p };
    }
    let p = Box::into_raw(Box::new(load_model())); // build it fully...
    MODEL.store(p, Release); // ...then publish the pointer: the writes above happen-before any Acquire that sees p
    unsafe { &*p }
}

/// The same thing, in one line of std.
pub fn model_once() -> &'static Model {
    static M: OnceLock<Model> = OnceLock::new();
    M.get_or_init(load_model)
}

fn main() {
    let (a, b) = thread::scope(|s| {
        let hs: Vec<_> = (0..4)
            .map(|_| s.spawn(|| (model().weights.iter().sum::<f64>(), model_once().version)))
            .collect();
        let rs: Vec<_> = hs.into_iter().map(|h| h.join().unwrap()).collect();
        (rs[0], rs[3])
    });
    println!("DCL sum = {}, OnceLock version = {}; all threads agree: {}", a.0, a.1, a == b);
}
