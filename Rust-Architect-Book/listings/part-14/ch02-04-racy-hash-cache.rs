// verify: debug miri Data race detected
// Java's String.hashCode() caches the hash in a plain field: a "benign" data race that the Java memory model
// allows (every racing thread writes the same value). Ported literally to Rust, it is undefined behavior.
use std::cell::UnsafeCell;
use std::thread;

pub struct Symbol {
    text: String,
    hash: UnsafeCell<u64>, // 0 = not computed yet (like Java's `hash` field)
}

// SAFETY: WRONG. This claims shared access is fine, but hash() writes `hash` without synchronization.
unsafe impl Sync for Symbol {}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

impl Symbol {
    pub fn new(text: &str) -> Self {
        Symbol { text: text.to_string(), hash: UnsafeCell::new(0) }
    }

    #[inline(never)]
    pub fn hash(&self) -> u64 {
        let h = unsafe { *self.hash.get() }; // racy read
        if h != 0 {
            return h;
        }
        let h = fnv1a(&self.text);
        unsafe { *self.hash.get() = h }; // racy write: "benign" in Java, UB in Rust
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
