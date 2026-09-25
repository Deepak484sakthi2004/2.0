// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release -CrateType lib
use std::num::NonZeroU64;

#[inline(never)]
pub fn or_max_niche(x: Option<NonZeroU64>) -> u64 {
    x.map_or(u64::MAX, NonZeroU64::get)
}

#[inline(never)]
pub fn or_max_tagged(x: Option<u64>) -> u64 {
    x.unwrap_or(u64::MAX)
}

#[inline(never)]
pub fn deref_or_max(x: Option<&u64>) -> u64 {
    x.copied().unwrap_or(u64::MAX)
}

#[inline(never)]
pub fn tagged_in_memory(xs: &[Option<u64>]) -> u64 {
    xs.iter().flatten().sum()
}

#[inline(never)]
pub fn niche_in_memory(xs: &[Option<NonZeroU64>]) -> u64 {
    xs.iter().flatten().map(|n| n.get()).sum()
}
