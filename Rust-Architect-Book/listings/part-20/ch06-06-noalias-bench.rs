// verify: release ok
// Chapter 15.2's Systems exercise: does `noalias` buy speed in a loop? y += a * x, 4,096 f32 (in L1/L2), three ways:
//   slices:            &mut [f32] and &[f32] (can't overlap: no runtime check)
//   raw, disjoint:     *mut f32 / *const f32 to separate buffers (runtime overlap check passes: vector path)
//   raw, overlapping:  x = y shifted by one element (the check fails: scalar fallback, and a different result)
use std::hint::black_box;
use std::time::Instant;

#[inline(never)]
fn saxpy(y: &mut [f32], x: &[f32], a: f32) {
    for (yi, xi) in y.iter_mut().zip(x) {
        *yi += a * xi;
    }
}

/// # Safety
/// `y` must be valid for reads and writes of `n` f32s and `x` for reads of `n` f32s. They may overlap.
#[inline(never)]
unsafe fn saxpy_raw(y: *mut f32, x: *const f32, n: usize, a: f32) {
    for i in 0..n {
        // SAFETY: the caller guarantees both ranges are valid for `n` elements.
        unsafe { *y.add(i) += a * *x.add(i) };
    }
}

fn ns_per_elem(n: usize, mut f: impl FnMut()) -> f64 {
    let reps = 20_000;
    (0..7)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..reps {
                f();
            }
            t.elapsed().as_nanos() as f64 / (reps * n) as f64
        })
        .fold(f64::MAX, f64::min)
}

fn main() {
    let n = 4096;
    let x = vec![0.5f32; n];
    let mut y = vec![1.0f32; n];
    let mut z = vec![1.0f32; n + 1];
    let a = black_box(1e-9f32);
    let s = ns_per_elem(n, || saxpy(black_box(&mut y), black_box(&x), a));
    // SAFETY: `y` and `x` are separate allocations of `n` elements each.
    let d = ns_per_elem(n, || unsafe { saxpy_raw(black_box(y.as_mut_ptr()), black_box(x.as_ptr()), n, a) });
    // SAFETY: `z` has n + 1 elements, so both z[1..] (written) and z[..n] (read) are in bounds; overlap is allowed.
    let o = ns_per_elem(n, || unsafe {
        let p = z.as_mut_ptr();
        saxpy_raw(black_box(p.add(1)), black_box(p.cast_const()), n, a)
    });
    println!("ns per element (best of 7):  slices {s:.3}   raw disjoint {d:.3}   raw overlapping {o:.3}");
}
