// verify: release build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
// External iteration (a `for` loop calls next()) vs internal iteration (sum() calls fold()) over a Chain,
// plus the same sum through a trait object.

#[inline(never)]
pub fn chain_sum_external(a: &[u64], b: &[u64]) -> u64 {
    let mut total = 0;
    for x in a.iter().chain(b.iter()) {
        total += *x;
    }
    total
}

#[inline(never)]
pub fn chain_sum_internal(a: &[u64], b: &[u64]) -> u64 {
    a.iter().chain(b.iter()).sum()
}

#[inline(never)]
pub fn dyn_sum(it: &mut dyn Iterator<Item = u64>) -> u64 {
    let mut total = 0;
    for x in it {
        total += x;
    }
    total
}
