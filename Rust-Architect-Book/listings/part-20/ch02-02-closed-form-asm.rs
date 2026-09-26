// verify: release build
// What the optimizer did to the "loop" benchmarks of ch02-01 (inspect with tools/emit.ps1 -Target asm -Mode release).
#[inline(never)]
pub fn sum_to(n: u64) -> u64 {
    (0..n).sum()
}

#[inline(never)]
pub fn sum_squares_to(n: u64) -> u64 {
    (0..n).map(|i| i.wrapping_mul(i)).fold(0u64, u64::wrapping_add)
}

#[inline(never)]
pub fn sum_slice(v: &[u64]) -> u64 {
    v.iter().sum()
}
