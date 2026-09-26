// verify: release build
// What the auto-vectorizer does and doesn't do (inspect with tools/emit.ps1 -Target asm -Mode release).
// The Playground builds for the x86-64 baseline (SSE2), so vector code uses 128-bit xmm registers.

/// Integer sum: reassociation is legal (wrapping add is associative), so LLVM vectorizes and unrolls.
#[inline(never)]
pub fn sum_u32(v: &[u32]) -> u32 {
    v.iter().fold(0u32, |a, &x| a.wrapping_add(x))
}

/// f64 sum in source order: IEEE addition isn't associative, so LLVM must keep one sequential chain.
#[inline(never)]
pub fn sum_f64(v: &[f64]) -> f64 {
    v.iter().sum()
}

/// f64 sum with 8 independent accumulators: we chose a different (still deterministic) order, so LLVM may vectorize.
#[inline(never)]
pub fn sum_f64_lanes(v: &[f64]) -> f64 {
    let mut acc = [0.0f64; 8];
    let chunks = v.chunks_exact(8);
    let tail = chunks.remainder();
    for c in chunks {
        for i in 0..8 {
            acc[i] += c[i];
        }
    }
    acc.iter().sum::<f64>() + tail.iter().sum::<f64>()
}

/// y += a * x over slices: `&mut [f32]` and `&[f32]` can't overlap, so no runtime overlap check is needed.
#[inline(never)]
pub fn saxpy(y: &mut [f32], x: &[f32], a: f32) {
    for (yi, xi) in y.iter_mut().zip(x) {
        *yi += a * xi;
    }
}

/// The same loop over raw pointers: they may overlap, so LLVM versions the loop behind a runtime overlap check.
///
/// # Safety
/// `y` must be valid for reads and writes of `n` f32s and `x` for reads of `n` f32s.
#[inline(never)]
pub unsafe fn saxpy_raw(y: *mut f32, x: *const f32, n: usize, a: f32) {
    for i in 0..n {
        // SAFETY: the caller guarantees both ranges are valid for `n` elements.
        unsafe { *y.add(i) += a * *x.add(i) };
    }
}

/// A search loop with an early exit.
#[inline(never)]
pub fn position_of(v: &[u32], needle: u32) -> Option<usize> {
    v.iter().position(|&x| x == needle)
}
