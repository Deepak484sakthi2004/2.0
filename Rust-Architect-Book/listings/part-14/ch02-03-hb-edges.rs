// verify: debug miri-ok
// verify: release ok
// Each function publishes a NON-atomic value from one thread to another through one std mechanism.
// Miri's data-race detector checks every one: a missing happens-before edge would be reported as UB.
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::{mpsc, Barrier, Mutex, OnceLock};
use std::thread;

/// Plain, non-atomic memory that we share across threads on purpose. The `unsafe impl Sync` is a promise
/// that every access is ordered by happens-before; Miri checks that promise.
struct Plain(UnsafeCell<u64>);
unsafe impl Sync for Plain {}
impl Plain {
    fn new() -> Self { Plain(UnsafeCell::new(0)) }
    // SAFETY (both): callers guarantee happens-before with every other access.
    unsafe fn write(&self, v: u64) { unsafe { *self.0.get() = v } }
    unsafe fn read(&self) -> u64 { unsafe { *self.0.get() } }
}

fn via_spawn_and_join() -> u64 {
    let p = Plain::new();
    unsafe { p.write(1) }; // before spawn → visible in the child
    thread::scope(|s| {
        s.spawn(|| unsafe { p.write(p.read() + 1) }); // child's writes → visible after join
    });
    unsafe { p.read() }
}

fn via_mutex() -> u64 {
    let (p, ready) = (Plain::new(), Mutex::new(false));
    thread::scope(|s| {
        s.spawn(|| {
            unsafe { p.write(10) };
            *ready.lock().unwrap() = true; // unlock (guard drop) is a Release
        });
        s.spawn(|| loop {
            if *ready.lock().unwrap() { // a lock that sees `true` is an Acquire of that unlock
                return unsafe { p.read() };
            }
            thread::yield_now();
        })
        .join()
        .unwrap()
    })
}

fn via_channel() -> u64 {
    let (p, (tx, rx)) = (Plain::new(), mpsc::channel::<()>());
    thread::scope(|s| {
        let p = &p;
        s.spawn(move || { unsafe { p.write(20) }; tx.send(()).unwrap(); }); // moves tx, borrows p
        rx.recv().unwrap(); // send happens-before the matching recv
        unsafe { p.read() }
    })
}

fn via_once_lock() -> u64 {
    let (p, once) = (Plain::new(), OnceLock::new());
    thread::scope(|s| {
        s.spawn(|| { unsafe { p.write(30) }; once.set(()).unwrap(); });
        while once.get().is_none() { thread::yield_now(); } // get() that sees the value is an Acquire
        unsafe { p.read() }
    })
}

fn via_barrier() -> u64 {
    let (p, b) = (Plain::new(), Barrier::new(2));
    thread::scope(|s| {
        s.spawn(|| { unsafe { p.write(40) }; b.wait(); });
        b.wait(); // everything before either wait() happens-before everything after both
        unsafe { p.read() }
    })
}

fn via_release_acquire() -> u64 {
    let (p, flag) = (Plain::new(), AtomicBool::new(false));
    thread::scope(|s| {
        s.spawn(|| { unsafe { p.write(50) }; flag.store(true, Release); });
        while !flag.load(Acquire) { std::hint::spin_loop(); }
        unsafe { p.read() }
    })
}

fn main() {
    println!("spawn + join:      {}", via_spawn_and_join());
    println!("Mutex:             {}", via_mutex());
    println!("channel:           {}", via_channel());
    println!("OnceLock:          {}", via_once_lock());
    println!("Barrier:           {}", via_barrier());
    println!("Release/Acquire:   {}", via_release_acquire());
}
