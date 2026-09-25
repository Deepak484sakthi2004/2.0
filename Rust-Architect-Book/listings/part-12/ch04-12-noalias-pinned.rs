// verify: release build
//! Codegen: `&mut T` normally reaches LLVM as `noalias` (Chapter 3.3). For a !Unpin T, rustc
//! leaves `noalias` off [RUSTC], because a pinned value may legitimately be pointed to from
//! elsewhere (from itself, or from an intrusive list) while a `&mut` to it exists.
//! Inspect with: tools/emit.ps1 listings/part-12/ch04-12-noalias-pinned.rs -Target llvm-ir -Mode release
use std::marker::PhantomPinned;

pub struct Plain {
    pub x: u64,
}

pub struct Pinned {
    pub x: u64,
    _pin: PhantomPinned,
}

#[inline(never)]
pub fn bump_plain(p: &mut Plain, other: &u64) -> u64 {
    p.x += 1;
    p.x + *other
}

#[inline(never)]
pub fn bump_pinned(p: &mut Pinned, other: &u64) -> u64 {
    p.x += 1;
    p.x + *other
}
