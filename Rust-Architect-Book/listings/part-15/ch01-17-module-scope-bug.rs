// verify: debug miri uninitialized
// The scope of `unsafe` is the MODULE. A later PR added a SAFE method with no `unsafe` keyword in it,
// and it broke the invariant that the existing unsafe blocks rely on.
mod headers {
    use std::mem::MaybeUninit;

    /// Up to N (name, value) headers stored inline, with no heap allocation for the block itself.
    pub struct HeaderBlock<const N: usize> {
        slots: [MaybeUninit<(String, String)>; N],
        /// INVARIANT: len <= N, slots[..len] are initialized, slots[len..] are not.
        len: usize,
    }

    impl<const N: usize> HeaderBlock<N> {
        pub fn new() -> Self {
            HeaderBlock { slots: [const { MaybeUninit::uninit() }; N], len: 0 }
        }

        pub fn push(&mut self, name: &str, value: &str) -> Result<(), &'static str> {
            if self.len == N {
                return Err("header block full");
            }
            self.slots[self.len].write((name.to_owned(), value.to_owned()));
            self.len += 1;
            Ok(())
        }

        pub fn len(&self) -> usize {
            self.len
        }

        pub fn get(&self, i: usize) -> Option<(&str, &str)> {
            if i >= self.len {
                return None;
            }
            // SAFETY: i < len, and slots[..len] are initialized (invariant).
            let (n, v) = unsafe { self.slots[i].assume_init_ref() };
            Some((n, v))
        }

        /// Added in a later "no unsafe" PR: roll back to a checkpoint taken with `len()`.
        pub fn restore(&mut self, checkpoint: usize) {
            self.len = checkpoint; // no `unsafe` here... and the invariant is gone
        }
    }

    impl<const N: usize> Drop for HeaderBlock<N> {
        fn drop(&mut self) {
            for slot in &mut self.slots[..self.len] {
                // SAFETY: slots[..len] are initialized (invariant); each is dropped exactly once.
                unsafe { slot.assume_init_drop() }
            }
        }
    }
}

fn main() {
    let mut h = headers::HeaderBlock::<8>::new();
    h.push("host", "api.meridian.example").unwrap();
    h.push("x-request-id", "r-42").unwrap();
    h.push("x-tenant", "t-7").unwrap();
    // The retry path passed a checkpoint taken from a DIFFERENT request with more headers.
    h.restore(5);
    println!("len={} {:?}", h.len(), h.get(4)); // a slot that was never written
}
