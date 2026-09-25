// verify: debug miri Data race detected
// verify: release ok
// DCL ported from pre-Java-5 code, "without volatile": the pointer is atomic, but Relaxed, so a reader can
// see the pointer without the model's contents being ordered before its reads.
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering::Relaxed};
use std::sync::Mutex;
use std::thread;

pub struct Model {
    weights: Vec<f64>,
}

static MODEL: AtomicPtr<Model> = AtomicPtr::new(ptr::null_mut());
static INIT_LOCK: Mutex<()> = Mutex::new(());

pub fn try_model() -> Option<&'static Model> {
    let p = MODEL.load(Relaxed); // BUG: should be Acquire
    unsafe { p.as_ref() }
}

pub fn init_model() {
    let _guard = INIT_LOCK.lock().unwrap();
    if MODEL.load(Relaxed).is_null() {
        let p = Box::into_raw(Box::new(Model { weights: vec![0.25, 0.5, 0.25] }));
        MODEL.store(p, Relaxed); // BUG: should be Release
    }
}

fn main() {
    let sum = thread::scope(|s| {
        s.spawn(init_model);
        // A request thread that only takes the fast path: spin until the model "is there", then use it.
        s.spawn(|| loop {
            if let Some(m) = try_model() {
                break m.weights.iter().sum::<f64>();
            }
            std::hint::spin_loop();
        })
        .join()
        .unwrap()
    });
    println!("sum = {sum}");
}
