// verify: debug ok
// verify: debug miri-ok
// Two ways to return variable-size data to C, each with an unambiguous owner:
//  (1) the CALLER allocates: a (buffer, capacity) pair plus a size query, the snprintf protocol;
//  (2) the LIBRARY allocates: a repr(C) (ptr, len, cap) triple plus a matching free function.
use std::panic::{self, AssertUnwindSafe};

pub const MERIDIAN_OK: i32 = 0;
pub const MERIDIAN_ERR_INVALID: i32 = -1;
pub const MERIDIAN_ERR_TOO_SMALL: i32 = -2;
pub const MERIDIAN_ERR_PANIC: i32 = -99;

fn explain(txn_id: u64) -> Option<String> {
    (txn_id != 0).then(|| format!("txn {txn_id}: amount +0.41, velocity +0.22, country -0.05"))
}

fn guard(f: impl FnOnce() -> i32) -> i32 {
    panic::catch_unwind(AssertUnwindSafe(f)).unwrap_or(MERIDIAN_ERR_PANIC)
}

/// (1) C: `int32_t meridian_explain_into(uint64_t txn, uint8_t *buf, size_t cap, size_t *needed);`
/// Writes at most `cap` bytes (no NUL). If they don't fit, returns MERIDIAN_ERR_TOO_SMALL and sets
/// `*needed`; the caller retries with a bigger buffer. The library never allocates caller memory.
/// # Safety
/// `buf` is writable for `cap` bytes (may be NULL if cap == 0); `needed` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_explain_into(txn: u64, buf: *mut u8, cap: usize, needed: *mut usize) -> i32 {
    guard(|| {
        let Some(text) = explain(txn) else { return MERIDIAN_ERR_INVALID };
        if needed.is_null() || (cap > 0 && buf.is_null()) {
            return MERIDIAN_ERR_INVALID;
        }
        // SAFETY: `needed` is non-null and writable (the contract).
        unsafe { needed.write(text.len()) };
        if text.len() > cap {
            return MERIDIAN_ERR_TOO_SMALL;
        }
        // SAFETY: `buf` is writable for `cap >= text.len()` bytes, and can't overlap our String.
        unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), buf, text.len()) };
        MERIDIAN_OK
    })
}

/// (2) Library-owned bytes. C reads `ptr[0..len]`, must not change any field, and must hand the
/// struct back to `meridian_buf_free` exactly once. `cap` is there only so Rust can free it.
#[repr(C)]
pub struct MeridianBuf {
    pub ptr: *mut u8,
    pub len: usize,
    pub cap: usize,
}

/// # Safety
/// `out` is writable. On MERIDIAN_OK, `*out` owns a buffer to be released with `meridian_buf_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_explain(txn: u64, out: *mut MeridianBuf) -> i32 {
    guard(|| {
        let Some(text) = explain(txn) else { return MERIDIAN_ERR_INVALID };
        if out.is_null() {
            return MERIDIAN_ERR_INVALID;
        }
        let (ptr, len, cap) = text.into_bytes().into_raw_parts(); // no longer freed by Rust's Drop
        // SAFETY: `out` is non-null and writable (the contract).
        unsafe { out.write(MeridianBuf { ptr, len, cap }) };
        MERIDIAN_OK
    })
}

/// # Safety
/// `buf` was produced by `meridian_explain`, is unmodified, and is freed only once.
/// A zeroed MeridianBuf (ptr NULL) is accepted and ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_buf_free(buf: MeridianBuf) {
    if !buf.ptr.is_null() {
        // SAFETY: (ptr, len, cap) came from Vec::<u8>::into_raw_parts and come back exactly once,
        // so the Rust global allocator that made the buffer is the one that frees it.
        drop(unsafe { Vec::from_raw_parts(buf.ptr, buf.len, buf.cap) });
    }
}

fn main() {
    // SAFETY (all calls): arguments satisfy each function's documented contract.
    unsafe {
        // (1) caller-allocated: try a small stack buffer, learn the size, retry.
        let mut small = [0u8; 16];
        let mut needed = 0usize;
        let rc = meridian_explain_into(7001, small.as_mut_ptr(), small.len(), &mut needed);
        println!("explain_into(cap=16) -> rc={rc}, needed={needed}");
        let mut exact = vec![0u8; needed];
        let rc = meridian_explain_into(7001, exact.as_mut_ptr(), exact.len(), &mut needed);
        println!("explain_into(cap={}) -> rc={rc}, {:?}", exact.len(), std::str::from_utf8(&exact).unwrap());

        // (2) library-allocated: read it, then give it back to the library that allocated it.
        let mut buf = MeridianBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 };
        let rc = meridian_explain(7002, &mut buf);
        let text = std::str::from_utf8(std::slice::from_raw_parts(buf.ptr, buf.len)).unwrap();
        println!("explain -> rc={rc}, len={} cap={}, {:?}", buf.len, buf.cap, text);
        meridian_buf_free(buf);
        println!("explain(0) -> rc={}", meridian_explain(0, &mut MeridianBuf { ptr: std::ptr::null_mut(), len: 0, cap: 0 }));
    }
}
