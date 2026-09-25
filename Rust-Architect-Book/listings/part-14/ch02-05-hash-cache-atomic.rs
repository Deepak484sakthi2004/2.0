// verify: debug miri-ok
// verify: release ok
// The same cache, done right: an AtomicU64 with Relaxed ordering. No happens-before is needed, because the
// cached value is self-contained (it doesn't publish any other memory), and every racer writes the same value.
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::thread;

pub struct Symbol {
    text: String,
    hash: AtomicU64, // 0 = not computed yet
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

impl Symbol {
    pub fn new(text: &str) -> Self {
        Symbol { text: text.to_string(), hash: AtomicU64::new(0) }
    }

    #[inline(never)]
    pub fn hash(&self) -> u64 {
        let h = self.hash.load(Relaxed);
        if h != 0 {
            return h;
        }
        let h = fnv1a(&self.text);
        self.hash.store(h, Relaxed); // racing stores of the same value: fine, and no UB
        h
    }
}

fn main() {
    let sym = Symbol::new("merchant:4412");
    let (a, b) = thread::scope(|s| {
        let a = s.spawn(|| sym.hash());
        let b = s.spawn(|| sym.hash());
        (a.join().unwrap(), b.join().unwrap())
    });
    println!("{a:x} {b:x}");
}
