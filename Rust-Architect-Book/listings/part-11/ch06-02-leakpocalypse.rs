// verify: debug miri use-after-free
//! Why the pre-1.0 `thread::scoped` API was removed (2015, "Leakpocalypse"), rebuilt in miniature.
//! It relied on a guard's destructor to join the thread. `mem::forget` is SAFE, so the destructor may never
//! run, and the thread reads a borrow that no longer exists. Miri reports the use-after-free.
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

static DONE: AtomicBool = AtomicBool::new(false);

/// The old design: the thread may borrow for 'a because the guard's Drop joins it before 'a ends...
pub struct JoinGuard<'a> {
    handle: Option<thread::JoinHandle<()>>,
    _borrows: PhantomData<&'a ()>,
}

impl Drop for JoinGuard<'_> {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            h.join().unwrap();
        }
    }
}

pub fn scoped<'a, F: FnOnce() + Send + 'a>(f: F) -> JoinGuard<'a> {
    let job: Box<dyn FnOnce() + Send + 'a> = Box::new(f);
    // SAFETY (claimed): the closure can't outlive 'a, because JoinGuard<'a> joins in Drop.
    // That claim is FALSE: destructors are not guaranteed to run (mem::forget, Rc cycles).
    let job: Box<dyn FnOnce() + Send + 'static> = unsafe { std::mem::transmute(job) };
    JoinGuard { handle: Some(thread::spawn(job)), _borrows: PhantomData }
}

fn start_report() {
    let totals = vec![120u64, 45, 300];
    let guard = scoped(|| {
        thread::sleep(Duration::from_millis(50));
        let sum: u64 = totals.iter().sum(); // `totals` belongs to a stack frame that is gone
        println!("sum = {sum}");
        DONE.store(true, Ordering::Release);
    });
    std::mem::forget(guard); // safe code: no join, no error
} // `totals` is dropped here, while the thread still borrows it

fn main() {
    start_report();
    while !DONE.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(5));
    }
}
