// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
use std::ops::Add;

/// One source function. Each instantiation is optimized for its own T.
#[inline(never)]
pub fn sum<T: Copy + Default + Add<Output = T>>(xs: &[T]) -> T {
    let mut acc = T::default();
    for &x in xs {
        acc = acc + x;
    }
    acc
}

pub fn sum_u32(xs: &[u32]) -> u32 {
    sum(xs)
}
pub fn sum_f64(xs: &[f64]) -> f64 {
    sum(xs)
}
