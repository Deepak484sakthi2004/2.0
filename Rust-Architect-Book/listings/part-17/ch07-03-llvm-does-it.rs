// verify: release build
// Listing 17.7-3: the optimizations of this chapter, done by LLVM on real Rust code.
// Emit release assembly with tools/emit.ps1 and read each function.

/// Constant folding with exact overflow semantics: `x + 1 < x` (wrapping) is true only for i64::MAX.
#[inline(never)]
pub fn wraps(x: i64) -> bool {
    x.wrapping_add(1) < x
}

/// Common subexpression elimination: `a * b` is computed once.
#[inline(never)]
pub fn cse(a: i64, b: i64) -> i64 {
    (a * b + 1) * (a * b + 2)
}

/// Loop-invariant code motion: `a * b` leaves the loop.
#[inline(never)]
pub fn scale_all(xs: &mut [i64], a: i64, b: i64) {
    for x in xs.iter_mut() {
        *x += a * b;
    }
}

/// Induction-variable analysis (scalar evolution): the loop becomes a formula.
#[inline(never)]
pub fn triangle(n: u64) -> u64 {
    let (mut s, mut i) = (0u64, 0u64);
    while i < n {
        i += 1;
        s = s.wrapping_add(i);
    }
    s
}

/// Dead-store elimination: the first store is never observable.
#[inline(never)]
pub fn overwrite(x: &mut i64) {
    *x = 1;
    *x = 2;
}

/// A trapping operation: Rust's `/` checks for zero (and for MIN / -1) before `idiv`.
#[inline(never)]
pub fn divide(a: i64, b: i64) -> i64 {
    a / b
}

/// The guarded, loop-invariant division of listing 17.7-2, in Rust.
#[inline(never)]
pub fn guarded_sum(n: u64, a: i64, b: i64) -> i64 {
    let mut acc = 0i64;
    for _ in 0..n {
        if a != 0 {
            acc = acc.wrapping_add(b / a);
        }
    }
    acc
}
