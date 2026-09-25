// verify: debug ok
// verify: debug miri-ok
// verify: debug+tree miri-ok
use std::num::NonZeroUsize;
use std::ptr::NonNull;

/// A pointer to an 8-aligned T with a 3-bit tag packed into its low (always-zero) bits.
struct Tagged<T> {
    raw: NonNull<T>,
}
// Manual impls: #[derive(Clone, Copy)] would require T: Copy (Chapter 5.3).
impl<T> Clone for Tagged<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Tagged<T> {}

const TAG_MASK: usize = 0b111;

impl<T> Tagged<T> {
    fn new(p: NonNull<T>, tag: usize) -> Self {
        assert!(align_of::<T>() >= 8 && tag <= TAG_MASK);
        // map_addr keeps p's PROVENANCE and changes only its address (strict provenance, Rust 1.84).
        Tagged { raw: p.map_addr(|a| a | tag) }
    }
    fn tag(self) -> usize {
        self.raw.addr().get() & TAG_MASK
    }
    fn ptr(self) -> NonNull<T> {
        self.raw.map_addr(|a| {
            // SAFETY: the original address was non-zero and 8-aligned, so clearing the
            // three tag bits gives back that same non-zero address.
            unsafe { NonZeroUsize::new_unchecked(a.get() & !TAG_MASK) }
        })
    }
}

#[repr(align(8))]
struct Node {
    value: u64,
}

fn main() {
    let raw = NonNull::from(Box::leak(Box::new(Node { value: 7 })));
    let t = Tagged::new(raw, 5);
    // SAFETY: t.ptr() has the leaked Box's address AND provenance; the Node is live and unaliased.
    let v = unsafe { t.ptr().as_ref().value };
    println!("tag={} value={}", t.tag(), v);
    // SAFETY: rebuild the Box exactly once, from the pointer Box::leak produced, to free it.
    drop(unsafe { Box::from_raw(t.ptr().as_ptr()) });
}
