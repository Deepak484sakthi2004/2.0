// verify: debug ok
// verify: debug miri-ok
// A drop guard: if initialization panics, drop exactly the initialized prefix.
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};

static DROPS: AtomicUsize = AtomicUsize::new(0);

struct Conn(u32);
impl Drop for Conn {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::Relaxed);
    }
}

/// Owns the partially initialized array while it is being filled.
struct Guard<'a, T, const N: usize> {
    slots: &'a mut [MaybeUninit<T>; N],
    /// INVARIANT: slots[..init] are initialized.
    init: usize,
}

impl<T, const N: usize> Drop for Guard<'_, T, N> {
    fn drop(&mut self) {
        for slot in &mut self.slots[..self.init] {
            // SAFETY: slots[..init] are initialized (invariant) and dropped exactly once, here.
            unsafe { slot.assume_init_drop() };
        }
    }
}

fn open_all(fail_at: u32) -> [Conn; 4] {
    let mut slots: [MaybeUninit<Conn>; 4] = [const { MaybeUninit::uninit() }; 4];
    let mut guard = Guard { slots: &mut slots, init: 0 };
    for i in 0..4u32 {
        if i == fail_at {
            panic!("connect failed at {i}"); // the guard drops slots[..init] while unwinding
        }
        guard.slots[i as usize].write(Conn(i));
        guard.init += 1;
    }
    std::mem::forget(guard); // success: ownership passes to the array below, not to the guard
    // SAFETY: all 4 slots were written; the guard was forgotten, so nothing else will drop them.
    unsafe { std::mem::transmute::<[MaybeUninit<Conn>; 4], [Conn; 4]>(slots) }
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    let r = std::panic::catch_unwind(|| open_all(2));
    println!("panicked: {}, drops after unwinding: {}", r.is_err(), DROPS.load(Ordering::Relaxed));
    let ok = open_all(99);
    println!("opened: {:?}", ok.iter().map(|c| c.0).collect::<Vec<_>>());
    drop(ok);
    println!("drops after a successful open + drop: {}", DROPS.load(Ordering::Relaxed));
}
