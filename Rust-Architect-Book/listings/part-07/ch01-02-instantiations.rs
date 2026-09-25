// verify: debug build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release   (and -Target llvm-ir)
/// A generic function: no machine code exists for it until a concrete T is chosen.
#[inline(never)]
pub fn largest<T: PartialOrd + Copy>(xs: &[T]) -> Option<T> {
    let mut best = *xs.first()?;
    for &x in xs {
        if x > best {
            best = x;
        }
    }
    Some(best)
}

/// Generic over "anything that can be viewed as bytes". Owning and borrowing T get different code.
#[inline(never)]
pub fn byte_len<T: AsRef<[u8]>>(x: T) -> usize {
    x.as_ref().len()
} // for T = Vec<u8>, dropping `x` frees the buffer; for T = &[u8], there is nothing to drop

// Non-generic entry points: they force the compiler to instantiate the generic code.
pub fn largest_u8(xs: &[u8]) -> Option<u8> {
    largest(xs)
}
pub fn largest_i64(xs: &[i64]) -> Option<i64> {
    largest(xs)
}
pub fn largest_f64(xs: &[f64]) -> Option<f64> {
    largest(xs)
}
pub fn len_owned(v: Vec<u8>) -> usize {
    byte_len(v)
}
pub fn len_borrowed(v: &[u8]) -> usize {
    byte_len(v)
}
