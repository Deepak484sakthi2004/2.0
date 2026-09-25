// verify: debug ok
// verify: debug miri-ok
// The fix: every function in the module maintains the invariant, safe or not.
mod headers {
    use std::mem::MaybeUninit;

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

        /// Roll back to a checkpoint: can only SHRINK, and drops what it removes.
        pub fn restore(&mut self, checkpoint: usize) {
            while self.len > checkpoint {
                self.len -= 1; // shrink first: if a drop panics, the slot is already outside [..len]
                // SAFETY: slot `len` was initialized and is now outside [..len]; dropped exactly once.
                unsafe { self.slots[self.len].assume_init_drop() }
            }
        }
    }

    impl<const N: usize> Drop for HeaderBlock<N> {
        fn drop(&mut self) {
            self.restore(0);
        }
    }
}

fn main() {
    let mut h = headers::HeaderBlock::<8>::new();
    h.push("host", "api.meridian.example").unwrap();
    let checkpoint = h.len();
    h.push("x-request-id", "r-42").unwrap();
    h.push("x-tenant", "t-7").unwrap();
    h.restore(5); // a stale, larger checkpoint is now a no-op
    h.restore(checkpoint); // drops the two headers added after the checkpoint
    println!("len={} get(0)={:?} get(1)={:?}", h.len(), h.get(0), h.get(1));
}
