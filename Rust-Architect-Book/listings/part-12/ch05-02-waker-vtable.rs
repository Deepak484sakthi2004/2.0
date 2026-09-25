// verify: debug ok
// verify: debug miri-ok
//! A Waker built by hand from a RawWaker: a data pointer plus a vtable of four functions.
//! Here the data pointer is an Arc turned into a raw pointer, so each Waker owns one strong count.
//! Miri checks the reference counting: a leaked or double-freed count would be reported.
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Arc;
use std::task::{RawWaker, RawWakerVTable, Waker};

#[derive(Default)]
struct Counters {
    wakes: AtomicUsize,
    clones: AtomicUsize,
    drops: AtomicUsize,
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop_waker);

unsafe fn clone(data: *const ()) -> RawWaker {
    // SAFETY: `data` came from Arc::into_raw in `counting_waker` (or here) and its count is live;
    // the new RawWaker owns one more strong count.
    unsafe { Arc::increment_strong_count(data as *const Counters) };
    // SAFETY: the Arc is alive (we hold at least one count), so the reference is valid.
    unsafe { &*(data as *const Counters) }.clones.fetch_add(1, Relaxed);
    RawWaker::new(data, &VTABLE)
}

unsafe fn wake(data: *const ()) {
    // SAFETY: `wake` consumes the waker: take back its strong count and release it at the end.
    let counters = unsafe { Arc::from_raw(data as *const Counters) };
    counters.wakes.fetch_add(1, Relaxed);
}

unsafe fn wake_by_ref(data: *const ()) {
    // SAFETY: the waker (and so its count) outlives this call; no ownership changes.
    unsafe { &*(data as *const Counters) }.wakes.fetch_add(1, Relaxed);
}

unsafe fn drop_waker(data: *const ()) {
    // SAFETY: dropping the waker releases the one strong count it owns.
    let counters = unsafe { Arc::from_raw(data as *const Counters) };
    counters.drops.fetch_add(1, Relaxed);
}

fn counting_waker(counters: Arc<Counters>) -> Waker {
    let raw = RawWaker::new(Arc::into_raw(counters) as *const (), &VTABLE);
    // SAFETY: the four vtable functions implement RawWaker's contract for this data pointer
    // (clone adds a count, wake and drop release one, wake_by_ref touches none), and Counters
    // is Send + Sync, so the waker may be used from any thread.
    unsafe { Waker::from_raw(raw) }
}

fn main() {
    println!("size_of::<Waker>() = {} bytes (data pointer + vtable pointer)", size_of::<Waker>());
    let counters = Arc::new(Counters::default());
    let w1 = counting_waker(counters.clone());
    let w2 = w1.clone();
    w2.wake_by_ref();
    let w3 = w2.clone();
    w3.wake(); // consumes w3: no drop call for it
    drop(w2);
    std::thread::spawn(move || w1.wake()).join().unwrap(); // wakers are Send: wake from another thread
    println!(
        "clones {}, wakes {}, drops {}, strong count now {}",
        counters.clones.load(Relaxed),
        counters.wakes.load(Relaxed),
        counters.drops.load(Relaxed),
        Arc::strong_count(&counters)
    );
}
