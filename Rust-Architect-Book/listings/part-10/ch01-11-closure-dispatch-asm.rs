// verify: release build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
// Three ways to pass "a function of u64" and what each costs per call.

#[inline(never)]
pub fn apply_generic<F: Fn(u64) -> u64>(f: F, xs: &[u64]) -> u64 {
    xs.iter().map(|&x| f(x)).sum()
}

#[inline(never)]
pub fn apply_dyn(f: &dyn Fn(u64) -> u64, xs: &[u64]) -> u64 {
    xs.iter().map(|&x| f(x)).sum()
}

#[inline(never)]
pub fn apply_ptr(f: fn(u64) -> u64, xs: &[u64]) -> u64 {
    xs.iter().map(|&x| f(x)).sum()
}

pub fn fee_generic(xs: &[u64], bps: u64) -> u64 {
    apply_generic(move |c| c * bps / 10_000, xs)
}

pub fn fee_dyn(xs: &[u64], bps: u64) -> u64 {
    apply_dyn(&move |c| c * bps / 10_000, xs)
}

pub fn double_ptr(xs: &[u64]) -> u64 {
    apply_ptr(|c| c * 2, xs)
}
