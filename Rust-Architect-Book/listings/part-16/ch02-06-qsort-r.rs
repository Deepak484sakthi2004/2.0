// verify: debug ok
// verify: release ok
// A C function that calls back into Rust: glibc's qsort_r, with an `extern "C"` comparator and a
// user-data pointer. The comparator is the wrapper's OWN code, which is what makes the wrapper safe.
use std::cmp::Ordering;
use std::ffi::{c_int, c_void};

/// The C type of the comparator: (a, b, user data) -> <0, 0, >0.
type Compar = unsafe extern "C" fn(*const c_void, *const c_void, *mut c_void) -> c_int;

unsafe extern "C" {
    /// glibc's signature. (BSD and macOS have a qsort_r with a DIFFERENT argument order: the libc
    /// crate declares each platform's version; a hand-written declaration is per-platform.)
    ///
    /// # Safety
    /// `base` must point to `nmemb` elements of `size` bytes each, valid for reads and writes;
    /// `compar` must define a consistent total order (C11 7.22.5: anything else is undefined);
    /// `arg` is passed through to every `compar` call unchanged.
    fn qsort_r(base: *mut c_void, nmemb: usize, size: usize, compar: Compar, arg: *mut c_void);
}

struct RankCtx<'a> {
    scores: &'a [f64],
}

/// Descending by score, ties broken by index: a total order, because `f64::total_cmp` is one
/// (NaN included) and indices are distinct.
unsafe extern "C" fn by_score_desc(a: *const c_void, b: *const c_void, arg: *mut c_void) -> c_int {
    // SAFETY: qsort_r passes back the `arg` given below: a `RankCtx` that outlives the sort.
    let ctx = unsafe { &*(arg as *const RankCtx<'_>) };
    // SAFETY: a and b point to elements of the u32 index array being sorted.
    let (ia, ib) = unsafe { (*(a as *const u32), *(b as *const u32)) };
    let (sa, sb) = (ctx.scores[ia as usize], ctx.scores[ib as usize]);
    match sb.total_cmp(&sa).then(ia.cmp(&ib)) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// Safe API: indices of `scores`, highest score first.
pub fn rank_by_score(scores: &[f64]) -> Vec<u32> {
    assert!(scores.len() <= u32::MAX as usize);
    let mut idx: Vec<u32> = (0..scores.len() as u32).collect();
    let ctx = RankCtx { scores };
    // SAFETY: `idx` holds `idx.len()` u32s, valid for reads and writes, and is not otherwise borrowed
    // during the call; the comparator is a total order over those indices (see above), and every
    // index is in bounds for `scores`; `ctx` lives until qsort_r returns, and C never keeps `arg`.
    unsafe {
        qsort_r(
            idx.as_mut_ptr().cast(),
            idx.len(),
            size_of::<u32>(),
            by_score_desc,
            (&ctx as *const RankCtx<'_>).cast_mut().cast(),
        )
    };
    idx
}

fn main() {
    let scores = [0.12, 0.97, 0.45, 0.97, f64::NAN, 0.03];
    println!("scores  = {scores:?}");
    println!("ranking = {:?}", rank_by_score(&scores));
}
