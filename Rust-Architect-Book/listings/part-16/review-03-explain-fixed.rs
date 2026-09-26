// verify: debug ok
// verify: debug miri-ok
// verify: debug test
// The explanation API after review: prefixed symbols, a handle instead of a global, repr(C) options
// with explicit integer encodings, (ptr, len) inputs checked before use, library-owned output with
// its own free function, catch_unwind at every entry, and no pointer into shared state.
#![deny(improper_ctypes_definitions)]
use std::ffi::{CStr, c_char};
use std::mem::{offset_of, size_of};
use std::panic::{self, AssertUnwindSafe};

pub const MERIDIAN_ABI_VERSION: u32 = 4;
pub const MERIDIAN_OK: i32 = 0;
pub const MERIDIAN_ERR_INVALID: i32 = -1;
pub const MERIDIAN_ERR_PANIC: i32 = -99;
pub const MERIDIAN_FORMAT_TEXT: u32 = 0;
pub const MERIDIAN_FORMAT_JSON: u32 = 1;

/// C: `typedef struct { uint32_t top; uint32_t format; uint8_t include_negative; } MeridianExplainOptions;`
#[repr(C)]
pub struct MeridianExplainOptions {
    pub top: u32,
    pub format: u32,           // MERIDIAN_FORMAT_*: validated, never transmuted into an enum
    pub include_negative: u8,  // 0 or 1: validated, not a Rust bool
}
const _: () = {
    assert!(size_of::<MeridianExplainOptions>() == 12);
    assert!(offset_of!(MeridianExplainOptions, include_negative) == 8);
};

/// Library-owned bytes, released only by `meridian_buf_free` (Chapter 16.4).
#[repr(C)]
pub struct MeridianBuf {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

/// Opaque to C. Immutable after creation, so it's Sync: no global lock on the scoring path.
pub struct MeridianExplainer {
    names: Vec<&'static str>,
    weights: Vec<f64>,
}
const _: () = {
    const fn sync<T: Sync>() {}
    sync::<MeridianExplainer>();
};

enum Format {
    Text,
    Json,
}

struct Invalid;

fn guard(f: impl FnOnce() -> Result<(), Invalid>) -> i32 {
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => MERIDIAN_OK,
        Ok(Err(Invalid)) => MERIDIAN_ERR_INVALID,
        Err(_) => MERIDIAN_ERR_PANIC,
    }
}

fn explain_text(e: &MeridianExplainer, features: &[f64], top: usize, negative: bool, format: Format) -> String {
    let mut parts: Vec<(&str, f64)> = e.names.iter().zip(features).zip(&e.weights).map(|((n, x), w)| (*n, x * w)).collect();
    parts.sort_by(|a, b| b.1.total_cmp(&a.1)); // a total order: no unwrap, no panic on NaN
    parts.retain(|p| negative || p.1 >= 0.0);
    parts.truncate(top);
    match format {
        Format::Text => parts.iter().map(|(n, v)| format!("{n} {v:+.2}")).collect::<Vec<_>>().join(", "),
        Format::Json => {
            let items: Vec<String> = parts.iter().map(|(n, v)| format!("{{\"f\":\"{n}\",\"v\":{v:.2}}}")).collect();
            format!("[{}]", items.join(","))
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn meridian_abi_version() -> u32 {
    MERIDIAN_ABI_VERSION
}

/// # Safety
/// `model` is NULL or NUL-terminated; `out` is NULL or writable. On MERIDIAN_OK, `*out` owns an
/// explainer to be released with `meridian_explainer_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_explainer_new(model: *const c_char, out: *mut *mut MeridianExplainer) -> i32 {
    guard(|| {
        if model.is_null() || out.is_null() {
            return Err(Invalid);
        }
        // SAFETY: non-null and NUL-terminated (the contract).
        let model = unsafe { CStr::from_ptr(model) }.to_str().map_err(|_| Invalid)?;
        if model.is_empty() {
            return Err(Invalid);
        }
        let e = MeridianExplainer { names: vec!["amount", "velocity", "country"], weights: vec![0.41, 0.22, -0.05] };
        // SAFETY: `out` is non-null and writable.
        unsafe { out.write(Box::into_raw(Box::new(e))) };
        Ok(())
    })
}

/// NULL is a no-op; otherwise each explainer is passed here exactly once.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_explainer_free(e: Option<Box<MeridianExplainer>>) {
    drop(e);
}

/// # Safety
/// `e` is a live explainer (shared use from many threads is fine); `features` points to `n` f64s
/// (NULL allowed when n == 0); `opts` and `out` are NULL or valid. On MERIDIAN_OK, `*out` owns a
/// buffer to be released with `meridian_buf_free`; on any error `*out` is left untouched.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_explain(
    e: *const MeridianExplainer,
    features: *const f64,
    n: usize,
    opts: *const MeridianExplainOptions,
    out: *mut MeridianBuf,
) -> i32 {
    guard(|| {
        // SAFETY: NULL or valid, per the contract.
        let (e, opts) = unsafe { (e.as_ref().ok_or(Invalid)?, opts.as_ref().ok_or(Invalid)?) };
        let features: &[f64] = match n {
            0 => &[],
            _ if features.is_null() || !features.is_aligned() || n > isize::MAX as usize / 8 => return Err(Invalid),
            // SAFETY: non-null, aligned, size in range; readable for `n` (the contract).
            _ => unsafe { std::slice::from_raw_parts(features, n) },
        };
        let format = match opts.format {
            MERIDIAN_FORMAT_TEXT => Format::Text,
            MERIDIAN_FORMAT_JSON => Format::Json,
            _ => return Err(Invalid),
        };
        let negative = match opts.include_negative {
            0 => false,
            1 => true,
            _ => return Err(Invalid),
        };
        if out.is_null() || features.len() != e.names.len() || !features.iter().all(|x| x.is_finite()) {
            return Err(Invalid);
        }
        let text = explain_text(e, features, opts.top as usize, negative, format);
        let (ptr, len, cap) = text.into_bytes().into_raw_parts();
        // SAFETY: `out` is non-null and writable.
        unsafe { out.write(MeridianBuf { ptr, len, cap }) };
        Ok(())
    })
}

/// # Safety
/// `buf` came from `meridian_explain`, unmodified, and is freed once. A NULL `ptr` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_buf_free(buf: MeridianBuf) {
    if !buf.ptr.is_null() {
        // SAFETY: the triple came from Vec::into_raw_parts and returns exactly once.
        drop(unsafe { Vec::from_raw_parts(buf.ptr, buf.len, buf.cap) });
    }
}

/// A Rust test helper that plays the C caller.
fn call(e: *const MeridianExplainer, f: &[f64], format: u32, neg: u8) -> Result<String, i32> {
    let opts = MeridianExplainOptions { top: 2, format, include_negative: neg };
    let mut buf = MeridianBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 };
    // SAFETY: valid arguments per the contract; the buffer is read, then freed once.
    unsafe {
        let rc = meridian_explain(e, f.as_ptr(), f.len(), &opts, &mut buf);
        if rc != MERIDIAN_OK {
            return Err(rc);
        }
        let s = std::str::from_utf8(std::slice::from_raw_parts(buf.ptr, buf.len)).unwrap().to_owned();
        meridian_buf_free(buf);
        Ok(s)
    }
}

fn main() {
    let mut e: *mut MeridianExplainer = std::ptr::null_mut();
    // SAFETY: a NUL-terminated literal and a writable out-pointer.
    let rc = unsafe { meridian_explainer_new(c"fraud-2026-09".as_ptr(), &mut e) };
    println!("abi {} | explainer_new -> {rc}", meridian_abi_version());
    println!("text          -> {:?}", call(e, &[0.9, 0.5, 0.1], MERIDIAN_FORMAT_TEXT, 0));
    println!("json          -> {:?}", call(e, &[0.9, 0.5, 0.1], MERIDIAN_FORMAT_JSON, 1));
    println!("format 7      -> {:?}", call(e, &[0.9, 0.5, 0.1], 7, 0));
    println!("NaN feature   -> {:?}", call(e, &[f64::NAN, 0.5, 0.1], MERIDIAN_FORMAT_TEXT, 0));
    println!("2 features    -> {:?}", call(e, &[0.9, 0.5], MERIDIAN_FORMAT_TEXT, 0));
    // SAFETY: NULL features with n = 3 is rejected by the checks, not dereferenced.
    let rc = unsafe { meridian_explain(e, std::ptr::null(), 3, std::ptr::null(), std::ptr::null_mut()) };
    println!("NULL pointers -> {rc}");
    // SAFETY: `e` came from meridian_explainer_new; released once.
    meridian_explainer_free(Some(unsafe { Box::from_raw(e) }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explainer() -> *mut MeridianExplainer {
        let mut e = std::ptr::null_mut();
        assert_eq!(unsafe { meridian_explainer_new(c"m".as_ptr(), &mut e) }, MERIDIAN_OK);
        e
    }

    #[test]
    fn rejects_unknown_encodings_instead_of_trusting_them() {
        let e = explainer();
        assert_eq!(call(e, &[0.9, 0.5, 0.1], 2, 0), Err(MERIDIAN_ERR_INVALID));
        assert_eq!(call(e, &[0.9, 0.5, 0.1], MERIDIAN_FORMAT_TEXT, 2), Err(MERIDIAN_ERR_INVALID));
        meridian_explainer_free(Some(unsafe { Box::from_raw(e) }));
    }

    #[test]
    fn many_threads_share_one_explainer() {
        let e = explainer();
        // SAFETY: live until the free below, which runs after every scoped thread has joined.
        let shared: &MeridianExplainer = unsafe { &*e }; // &T is Send because MeridianExplainer: Sync
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(move || assert!(call(shared, &[0.9, 0.5, 0.1], 0, 0).is_ok()));
            }
        });
        meridian_explainer_free(Some(unsafe { Box::from_raw(e) }));
    }

    #[test]
    fn empty_model_name_is_invalid() {
        let mut e = std::ptr::null_mut();
        assert_eq!(unsafe { meridian_explainer_new(c"".as_ptr(), &mut e) }, MERIDIAN_ERR_INVALID);
        assert!(e.is_null());
    }
}
