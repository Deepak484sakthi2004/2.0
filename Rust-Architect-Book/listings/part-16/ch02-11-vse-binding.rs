// verify: debug ok
// verify: debug miri-ok
// A vendor C library bound in two layers: `sys` (raw declarations, what bindgen would generate) and a
// safe `Engine` type. The vendor library is SIMULATED at the bottom of this file by Rust functions
// exported with the C ABI, so the whole binding runs on the Playground and under Miri.
use std::ffi::{CStr, CString, c_int};
use std::marker::PhantomData;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{self, NonNull};

/// Layer 1: the header, transcribed. Nothing here is safe to call casually.
mod sys {
    use std::ffi::{c_char, c_int};
    use std::marker::{PhantomData, PhantomPinned};

    /// `typedef struct vse_engine vse_engine;` an opaque C type: never constructed or read in Rust.
    #[repr(C)]
    pub struct VseEngine {
        _private: [u8; 0],
        _marker: PhantomData<(*mut u8, PhantomPinned)>, // !Send, !Sync, !Unpin: we know nothing about it
    }

    pub const VSE_OK: c_int = 0;
    pub const VSE_EINVAL: c_int = 1;
    pub const VSE_ENOMODEL: c_int = 2;
    pub const VSE_EWRONGTHREAD: c_int = 3;

    unsafe extern "C" {
        /// No preconditions at all, so it can be declared `safe` (edition 2024).
        pub safe fn vse_version() -> c_int;
        /// `path` NUL-terminated; `out` writable. On VSE_OK, `*out` is an engine owned by the caller.
        pub fn vse_open(path: *const c_char, out: *mut *mut VseEngine) -> c_int;
        /// `e` from vse_open, used on the thread that opened it; `features` readable for `n` f64s.
        pub fn vse_score(e: *mut VseEngine, features: *const f64, n: usize, out: *mut f64) -> c_int;
        /// Valid until the next call on `e`. Never NULL for a valid `e`.
        pub fn vse_last_error(e: *const VseEngine) -> *const c_char;
        /// Releases `e`; `e` must not be used afterwards.
        pub fn vse_close(e: *mut VseEngine);
    }
}

/// Layer 2: the safe API.
#[derive(Debug)]
pub enum VseError {
    PathContainsNul,
    Open { code: c_int },
    Score { code: c_int, message: String },
}

/// INVARIANT: `raw` came from a successful vse_open and has not been closed.
/// `PhantomData<*const ()>` makes Engine !Send and !Sync: the vendor requires one thread per engine.
pub struct Engine {
    raw: NonNull<sys::VseEngine>,
    _thread_affine: PhantomData<*const ()>,
}

impl Engine {
    pub fn open(model: &Path) -> Result<Engine, VseError> {
        let path = CString::new(model.as_os_str().as_bytes()).map_err(|_| VseError::PathContainsNul)?;
        let mut raw = ptr::null_mut();
        // SAFETY: `path` is NUL-terminated and lives across the call; `&mut raw` is writable.
        let rc = unsafe { sys::vse_open(path.as_ptr(), &mut raw) };
        if rc != sys::VSE_OK {
            return Err(VseError::Open { code: rc });
        }
        let raw = NonNull::new(raw).expect("vse_open returned VSE_OK and a NULL engine");
        Ok(Engine { raw, _thread_affine: PhantomData })
    }

    /// `&mut self`: one call at a time, which is also what keeps `vse_last_error`'s pointer valid
    /// until we've copied it.
    pub fn score(&mut self, features: &[f64]) -> Result<f64, VseError> {
        let mut out = 0.0;
        // SAFETY: the invariant (a live engine); Engine is !Send, so this is the opening thread;
        // `features` is readable for `len` f64s for the call; `&mut out` is writable.
        let rc = unsafe { sys::vse_score(self.raw.as_ptr(), features.as_ptr(), features.len(), &mut out) };
        if rc == sys::VSE_OK {
            return Ok(out);
        }
        // SAFETY: a live engine; the pointer is valid until the next call on it, and none can happen
        // before `to_string_lossy().into_owned()` has copied the text (we hold `&mut self`).
        let message = unsafe { CStr::from_ptr(sys::vse_last_error(self.raw.as_ptr())) }
            .to_string_lossy() // an error message is for humans: lossy is the right policy here
            .into_owned();
        Err(VseError::Score { code: rc, message })
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: the invariant; Drop runs once, and the engine is never used again.
        unsafe { sys::vse_close(self.raw.as_ptr()) }
    }
}

fn main() {
    println!("vse_version() = {} (a `safe` foreign item: no unsafe block)", sys::vse_version());
    println!("open(missing.bin): {:?}", Engine::open(Path::new("/models/missing.bin")).err());
    let mut engine = Engine::open(Path::new("/models/fraud-2026-09.vse")).expect("open");
    println!("score([0.9, 0.5, 0.1]) = {:?}", engine.score(&[0.9, 0.5, 0.1]));
    println!("score([0.9]) = {:?}", engine.score(&[0.9]));
} // engine dropped: vse_close runs here

// ---------------------------------------------------------------------------------------------
// SIMULATED VENDOR LIBRARY. In production this is C code in libvse.so; here it is Rust with the
// same C ABI and symbol names, so that `sys` above binds to it exactly as it would bind to C.
mod vendor {
    use super::sys::{VSE_EINVAL, VSE_ENOMODEL, VSE_EWRONGTHREAD, VSE_OK};
    use std::ffi::{CStr, CString, c_char, c_int};
    use std::thread::{self, ThreadId};

    pub struct EngineImpl {
        owner: ThreadId,
        weights: Vec<f64>,
        last_error: CString,
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn vse_version() -> c_int {
        402
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn vse_open(path: *const c_char, out: *mut *mut EngineImpl) -> c_int {
        // SAFETY: the header's contract: `path` is NUL-terminated.
        let path = unsafe { CStr::from_ptr(path) };
        if !path.to_bytes().ends_with(b".vse") {
            return VSE_ENOMODEL;
        }
        let e = Box::new(EngineImpl {
            owner: thread::current().id(),
            weights: vec![0.5, 0.3, 0.2],
            last_error: CString::default(),
        });
        // SAFETY: the header's contract: `out` is writable.
        unsafe { out.write(Box::into_raw(e)) };
        VSE_OK
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn vse_score(e: *mut EngineImpl, f: *const f64, n: usize, out: *mut f64) -> c_int {
        // SAFETY: the header's contract: `e` is a live engine, `f` readable for n, `out` writable.
        let (e, f) = unsafe { (&mut *e, std::slice::from_raw_parts(f, n)) };
        if e.owner != thread::current().id() {
            e.last_error = c"engine used from a thread other than the one that opened it".into();
            return VSE_EWRONGTHREAD;
        }
        if n != e.weights.len() {
            e.last_error = CString::new(format!("expected {} features, got {n}", e.weights.len())).unwrap();
            return VSE_EINVAL;
        }
        let s: f64 = f.iter().zip(&e.weights).map(|(x, w)| x * w).sum();
        // SAFETY: the header's contract: `out` is writable.
        unsafe { out.write(s) };
        VSE_OK
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn vse_last_error(e: *const EngineImpl) -> *const c_char {
        // SAFETY: the header's contract: `e` is a live engine.
        unsafe { (*e).last_error.as_ptr() }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn vse_close(e: *mut EngineImpl) {
        // SAFETY: the header's contract: `e` came from vse_open and is closed once.
        drop(unsafe { Box::from_raw(e) });
    }
}
