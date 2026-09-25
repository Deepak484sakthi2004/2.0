// verify: release build
// Inspect with: tools\emit.ps1 <this file> -Target asm -Mode release
// Three ways to write out[i] = a[i] + b[i]. Which ones keep bounds checks, and which ones vectorize?

#[inline(never)]
pub fn add_indexed(out: &mut [u64], a: &[u64], b: &[u64]) {
    for i in 0..out.len() {
        out[i] = a[i] + b[i]; // a and b may be shorter than out: each access must be checked
    }
}

#[inline(never)]
pub fn add_resliced(out: &mut [u64], a: &[u64], b: &[u64]) {
    let n = out.len();
    let (a, b) = (&a[..n], &b[..n]); // check once, up front (panics if too short)
    for i in 0..n {
        out[i] = a[i] + b[i]; // now provably in bounds
    }
}

#[inline(never)]
pub fn add_zipped(out: &mut [u64], a: &[u64], b: &[u64]) {
    for ((o, x), y) in out.iter_mut().zip(a).zip(b) {
        *o = x + y; // stops at the shortest slice: no checks needed
    }
}
