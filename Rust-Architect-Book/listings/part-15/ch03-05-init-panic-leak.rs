// verify: debug ok
// A panic halfway through initialization: the initialized prefix LEAKS. Safe (leaks are), but a leak.
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};

static DROPS: AtomicUsize = AtomicUsize::new(0);

struct Conn(u32);
impl Drop for Conn {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::Relaxed);
    }
}

fn open_all(fail_at: u32) -> [Conn; 4] {
    let mut slots: [MaybeUninit<Conn>; 4] = [const { MaybeUninit::uninit() }; 4];
    for (i, slot) in slots.iter_mut().enumerate() {
        let i = i as u32;
        if i == fail_at {
            panic!("connect failed at {i}");
        }
        slot.write(Conn(i));
    }
    // SAFETY: all slots written (we only get here if no panic happened).
    unsafe { std::mem::transmute::<[MaybeUninit<Conn>; 4], [Conn; 4]>(slots) }
}

fn main() {
    std::panic::set_hook(Box::new(|_| {})); // keep the output short
    let r = std::panic::catch_unwind(|| open_all(2));
    println!("panicked: {}, drops after unwinding: {}", r.is_err(), DROPS.load(Ordering::Relaxed));
    let ok = open_all(99);
    println!("opened: {:?}", ok.iter().map(|c| c.0).collect::<Vec<_>>());
    drop(ok);
    println!("drops after a successful open + drop: {}", DROPS.load(Ordering::Relaxed));
}
