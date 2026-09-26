// verify: debug ok
// verify: debug miri-ok
// Opaque handles: Box::into_raw hands ownership to C, the matching free function takes it back.
// `Option<Box<T>>` is ABI-identical to a nullable `T*`, so the free function needs no unsafe code.
// On the Rust-caller side, an RAII wrapper turns the create/destroy pair back into ownership.
use std::ptr::NonNull;

pub struct MeridianScorer {
    block_at: u32,
}

/// C: `MeridianScorer *meridian_scorer_create(uint32_t block_at);` NULL on invalid input.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_scorer_create(block_at: u32) -> Option<Box<MeridianScorer>> {
    (block_at <= 100).then(|| Box::new(MeridianScorer { block_at }))
}

/// C: `void meridian_scorer_destroy(MeridianScorer *s);` NULL is a no-op.
/// Taking `Option<Box<_>>` by value means Rust frees it when this function returns. The one rule
/// the type can't enforce, and the header must state: C passes each pointer here at most once.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_scorer_destroy(s: Option<Box<MeridianScorer>>) {
    drop(s);
}

/// C: `uint32_t meridian_scorer_block_at(const MeridianScorer *s);` `s` must be a live scorer.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_scorer_block_at(s: &MeridianScorer) -> u32 {
    s.block_at
}

/// A Rust caller of a C-style API (say, a Rust service linking the same library through its header)
/// wraps the pair so that ownership is back in the type system.
pub struct Scorer(NonNull<MeridianScorer>);

impl Scorer {
    pub fn new(block_at: u32) -> Option<Scorer> {
        let b = meridian_scorer_create(block_at)?;
        Some(Scorer(NonNull::from(Box::leak(b)))) // ownership now lives in `Scorer`
    }
    pub fn block_at(&self) -> u32 {
        // SAFETY: `self.0` is live until Drop (the invariant of `Scorer`).
        meridian_scorer_block_at(unsafe { self.0.as_ref() })
    }
}

impl Drop for Scorer {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from meridian_scorer_create and is destroyed once, here.
        meridian_scorer_destroy(Some(unsafe { Box::from_raw(self.0.as_ptr()) }));
    }
}

fn main() {
    println!("create(150) is NULL: {}", meridian_scorer_create(150).is_none());
    meridian_scorer_destroy(None); // destroy(NULL): nothing happens
    let s = Scorer::new(80).expect("valid threshold");
    println!("block_at = {}", s.block_at());
} // `s` dropped: meridian_scorer_destroy runs exactly once
