// verify: debug ok
// verify: debug miri-ok
// The Meridian fraud library's C API (the contract in meridian_fraud.h), exported from ordinary
// generic Rust. Every entry point: null/size checks, catch_unwind, a return code. `main` plays C.
use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};

pub const MERIDIAN_OK: i32 = 0;
pub const MERIDIAN_ERR_INVALID: i32 = -1;
pub const MERIDIAN_ERR_PANIC: i32 = -99;
pub const MERIDIAN_ABI_VERSION: u32 = 3;

/// Passed by pointer from C. repr(C): the header declares the same two fields in the same order.
#[repr(C)]
pub struct MeridianConfig {
    pub block_at: u32,
    pub review_at: u32,
}

// ---------- internals: ordinary, generic Rust; nothing here knows about C ----------

pub trait FeatureSource {
    fn features(&self, id: u64) -> [f64; 3];
}

pub struct InMemory {
    rows: HashMap<u64, [f64; 3]>,
}

impl FeatureSource for InMemory {
    fn features(&self, id: u64) -> [f64; 3] {
        self.rows[&id] // BUG kept on purpose: indexing panics for an unknown id (the -99 path)
    }
}

pub struct Model {
    weights: [f64; 3],
}

fn score_all<S: FeatureSource>(src: &S, model: &Model, ids: &[u64], out: &mut [i32]) {
    for (id, slot) in ids.iter().zip(out.iter_mut()) {
        let f = src.features(*id);
        let s: f64 = f.iter().zip(&model.weights).map(|(x, w)| x * w).sum();
        *slot = (s * 100.0).round() as i32;
    }
}

// ---------- the boundary ----------

/// Opaque to C (`typedef struct MeridianScorer MeridianScorer;`). Immutable after creation, and the
/// header promises it may be shared across threads, so it must be Sync. The compiler checks that:
pub struct MeridianScorer {
    model: Model,
    source: InMemory,
}
const _: () = {
    const fn assert_sync<T: Sync>() {}
    assert_sync::<MeridianScorer>();
};

struct Invalid;

/// One place that turns Result + panics into the header's codes.
fn ffi_guard(f: impl FnOnce() -> Result<(), Invalid>) -> i32 {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => MERIDIAN_OK,
        Ok(Err(Invalid)) => MERIDIAN_ERR_INVALID,
        Err(_) => MERIDIAN_ERR_PANIC,
    }
}

/// C's (pointer, count) as a slice, with the checks `slice::from_raw_parts` requires of us.
/// # Safety
/// If `n > 0` and `p` is non-null, `p` must point to `n` readable, initialized `T`s that stay valid
/// and unmodified for `'a`.
unsafe fn slice_in<'a, T>(p: *const T, n: usize) -> Result<&'a [T], Invalid> {
    if n == 0 {
        return Ok(&[]); // C may pass NULL with 0; from_raw_parts may not receive NULL
    }
    if p.is_null() || !p.is_aligned() || n > isize::MAX as usize / size_of::<T>() {
        return Err(Invalid);
    }
    // SAFETY: non-null, aligned, size in range (checked above); readable for `n` (the caller's contract).
    Ok(unsafe { std::slice::from_raw_parts(p, n) })
}

/// # Safety
/// As `slice_in`, but writable, and not accessed through any other pointer for `'a`.
unsafe fn slice_out<'a, T>(p: *mut T, n: usize) -> Result<&'a mut [T], Invalid> {
    if n == 0 {
        return Ok(&mut []);
    }
    if p.is_null() || !p.is_aligned() || n > isize::MAX as usize / size_of::<T>() {
        return Err(Invalid);
    }
    // SAFETY: as in slice_in, plus exclusivity (the caller's contract).
    Ok(unsafe { std::slice::from_raw_parts_mut(p, n) })
}

#[unsafe(no_mangle)]
pub extern "C" fn meridian_abi_version() -> u32 {
    MERIDIAN_ABI_VERSION
}

/// # Safety
/// `cfg` is NULL or points to a valid MeridianConfig; `out` is NULL or writable.
/// On MERIDIAN_OK, `*out` owns a scorer that must be released with `meridian_scorer_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_scorer_new(cfg: *const MeridianConfig, out: *mut *mut MeridianScorer) -> i32 {
    ffi_guard(|| {
        // SAFETY: the contract: NULL or a valid, aligned MeridianConfig.
        let cfg = unsafe { cfg.as_ref() }.ok_or(Invalid)?;
        if out.is_null() || cfg.review_at > cfg.block_at || cfg.block_at > 100 {
            return Err(Invalid);
        }
        let rows = HashMap::from([(1001, [0.9, 0.8, 0.7]), (1002, [0.1, 0.2, 0.1]), (1003, [0.6, 0.5, 0.2])]);
        let scorer = Box::new(MeridianScorer {
            model: Model { weights: [0.5, 0.3, 0.2] },
            source: InMemory { rows },
        });
        // SAFETY: `out` is non-null (checked) and writable (the contract).
        unsafe { out.write(Box::into_raw(scorer)) };
        Ok(())
    })
}

/// # Safety
/// `s` is NULL (a no-op, like free(NULL)) or came from `meridian_scorer_new` and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_scorer_free(s: *mut MeridianScorer) {
    if !s.is_null() {
        // SAFETY: the contract: `s` came from Box::into_raw in meridian_scorer_new, released once.
        drop(unsafe { Box::from_raw(s) });
    }
}

/// Concrete signature over the generic `score_all`: C has no generics, so the boundary picks the types.
/// # Safety
/// `s` came from `meridian_scorer_new`; `ids` points to `n` u64s and `out` to room for `n` i32s
/// (either may be NULL when `n == 0`). On an error return the contents of `out` are unspecified.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_score_batch(s: *const MeridianScorer, ids: *const u64, n: usize, out: *mut i32) -> i32 {
    ffi_guard(|| {
        // SAFETY: the contract: NULL or a live scorer, shared (read-only) for the call.
        let s = unsafe { s.as_ref() }.ok_or(Invalid)?;
        // SAFETY: the contract on `ids` / `out`, checked for NULL, alignment and size inside.
        let (ids, out) = unsafe { (slice_in(ids, n)?, slice_out(out, n)?) };
        score_all(&s.source, &s.model, ids, out);
        Ok(())
    })
}

fn main() {
    panic::set_hook(Box::new(|_| {})); // production logs here; the demo keeps stdout readable
    println!("meridian_abi_version() = {}", meridian_abi_version());
    let cfg = MeridianConfig { block_at: 80, review_at: 50 };
    let mut scorer: *mut MeridianScorer = std::ptr::null_mut();
    // SAFETY (for all calls below): the arguments satisfy each function's documented contract.
    unsafe {
        println!("scorer_new(80/50)         -> rc={}", meridian_scorer_new(&cfg, &mut scorer));
        let ids = [1001u64, 1002, 1003];
        let mut out = [0i32; 3];
        let rc = meridian_score_batch(scorer, ids.as_ptr(), ids.len(), out.as_mut_ptr());
        println!("score_batch([1001, 1002, 1003]) -> rc={rc} scores={out:?}");
        let rc = meridian_score_batch(scorer, std::ptr::null(), 3, out.as_mut_ptr());
        println!("score_batch(NULL, n=3)    -> rc={rc}");
        let rc = meridian_score_batch(scorer, std::ptr::null(), 0, std::ptr::null_mut());
        println!("score_batch(NULL, n=0)    -> rc={rc}");
        let ids = [1001u64, 4242];
        let rc = meridian_score_batch(scorer, ids.as_ptr(), ids.len(), out.as_mut_ptr());
        println!("score_batch([1001, 4242]) -> rc={rc}  (a bug inside, contained)");
        let bad = MeridianConfig { block_at: 40, review_at: 60 };
        let mut other: *mut MeridianScorer = std::ptr::null_mut();
        println!("scorer_new(40/60)         -> rc={}", meridian_scorer_new(&bad, &mut other));
        meridian_scorer_free(scorer);
        meridian_scorer_free(std::ptr::null_mut()); // a no-op, like free(NULL)
    }
}
