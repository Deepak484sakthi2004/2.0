// verify: debug ok
// verify: debug miri-ok
// Option<&T>, Option<NonNull<T>>, Option<Box<T>> and Option<extern "C" fn> are guaranteed to have the
// size and ABI of a C pointer, with None = NULL. With the lint denied, this file compiling is the
// compiler agreeing that every signature below is FFI-safe.
#![deny(improper_ctypes_definitions)]
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::NonNull;

#[repr(C)]
pub struct Limits {
    pub block_at: u32,
}

/// A typed handle: repr(transparent) gives it exactly the layout AND the calling convention of the
/// NonNull inside, so `Option<ScorerRef>` is still one nullable pointer.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct ScorerRef(NonNull<c_void>);

pub type Callback = extern "C" fn(u32) -> u32;

// `const Limits *limits` in C: may be NULL, and the type says so.
pub extern "C" fn block_threshold(limits: Option<&Limits>) -> u32 {
    limits.map_or(80, |l| l.block_at)
}

// `void (*on_block)(uint32_t)` in C: a nullable function pointer.
pub extern "C" fn notify(on_block: Option<Callback>, score: u32) -> u32 {
    match on_block {
        Some(f) => f(score),
        None => 0,
    }
}

// `MeridianScorer *` in C, passed back to us: NULL or a handle.
pub extern "C" fn is_open(h: Option<ScorerRef>) -> bool {
    h.is_some()
}

extern "C" fn double(x: u32) -> u32 {
    x * 2
}

fn main() {
    println!(
        "sizes: Option<&Limits>={} Option<NonNull<c_void>>={} Option<Box<Limits>>={} Option<Callback>={} Option<ScorerRef>={}",
        size_of::<Option<&Limits>>(),
        size_of::<Option<NonNull<c_void>>>(),
        size_of::<Option<Box<Limits>>>(),
        size_of::<Option<Callback>>(),
        size_of::<Option<ScorerRef>>()
    );
    let custom = Limits { block_at: 65 };
    println!("block_threshold(NULL)={} block_threshold(&65)={}", block_threshold(None), block_threshold(Some(&custom)));
    println!("notify(NULL, 40)={} notify(double, 40)={}", notify(None, 40), notify(Some(double), 40));
    let mut dummy = 0u8;
    let h = ScorerRef(NonNull::from(&mut dummy).cast());
    println!("is_open(NULL)={} is_open(h)={}", is_open(None), is_open(Some(h)));
}
