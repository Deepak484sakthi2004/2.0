// verify: debug ok
// verify: debug miri-ok
// The thread-affine vendor engine (Chapter 16.2's binding) behind an owner thread (Chapter 11.5):
// the !Send `Engine` never leaves the thread that opened it; every other thread sends requests.
use std::ffi::{CStr, CString, c_int};
use std::marker::PhantomData;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

mod sys {
    use std::ffi::{c_char, c_int};
    use std::marker::{PhantomData, PhantomPinned};
    #[repr(C)]
    pub struct VseEngine {
        _private: [u8; 0],
        _marker: PhantomData<(*mut u8, PhantomPinned)>,
    }
    pub const VSE_OK: c_int = 0;
    pub const VSE_EINVAL: c_int = 1;
    pub const VSE_ENOMODEL: c_int = 2;
    pub const VSE_EWRONGTHREAD: c_int = 3;
    unsafe extern "C" {
        pub fn vse_open(path: *const c_char, out: *mut *mut VseEngine) -> c_int;
        pub fn vse_score(e: *mut VseEngine, features: *const f64, n: usize, out: *mut f64) -> c_int;
        pub fn vse_last_error(e: *const VseEngine) -> *const c_char;
        pub fn vse_close(e: *mut VseEngine);
    }
}

#[derive(Debug)]
pub enum VseError {
    PathContainsNul,
    Open { code: c_int },
    Score { code: c_int, message: String },
}

/// INVARIANT: a live engine from vse_open. !Send + !Sync: it stays on the thread that opened it.
pub struct Engine {
    raw: NonNull<sys::VseEngine>,
    _thread_affine: PhantomData<*const ()>,
}

impl Engine {
    pub fn open(model: &Path) -> Result<Engine, VseError> {
        let path = CString::new(model.as_os_str().as_bytes()).map_err(|_| VseError::PathContainsNul)?;
        let mut raw = ptr::null_mut();
        // SAFETY: NUL-terminated path, writable out-pointer.
        let rc = unsafe { sys::vse_open(path.as_ptr(), &mut raw) };
        if rc != sys::VSE_OK {
            return Err(VseError::Open { code: rc });
        }
        Ok(Engine { raw: NonNull::new(raw).expect("VSE_OK with NULL"), _thread_affine: PhantomData })
    }

    pub fn score(&mut self, features: &[f64]) -> Result<f64, VseError> {
        let mut out = 0.0;
        // SAFETY: a live engine on its own thread (!Send); `features` readable; `out` writable.
        let rc = unsafe { sys::vse_score(self.raw.as_ptr(), features.as_ptr(), features.len(), &mut out) };
        if rc == sys::VSE_OK {
            return Ok(out);
        }
        // SAFETY: valid until the next call on this engine; copied before one can happen.
        let msg = unsafe { CStr::from_ptr(sys::vse_last_error(self.raw.as_ptr())) };
        Err(VseError::Score { code: rc, message: msg.to_string_lossy().into_owned() })
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: the invariant; closed once, on the owning thread (Engine can't have moved).
        unsafe { sys::vse_close(self.raw.as_ptr()) }
    }
}

// ---------------- the owner thread ----------------

#[derive(Debug)]
pub enum ServiceError {
    Engine(VseError),
    EngineDown, // the owner thread is gone (panicked or shut down)
}

struct Request {
    features: Vec<f64>,
    reply: mpsc::SyncSender<Result<f64, VseError>>, // a one-shot reply channel
}

/// Send + Sync: share it with `Arc` across every worker thread. The Engine itself never moves.
pub struct EngineService {
    tx: Option<mpsc::SyncSender<Request>>,
    owner: Option<JoinHandle<()>>,
}

impl EngineService {
    pub fn start(model: PathBuf, queue: usize) -> Result<EngineService, VseError> {
        let (tx, rx) = mpsc::sync_channel::<Request>(queue); // bounded: backpressure, Chapter 11.5
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let owner = thread::Builder::new()
            .name("vse-owner".into())
            .spawn(move || {
                let mut engine = match Engine::open(&model) {
                    Ok(e) => e, // opened HERE, so every vse_* call below happens on this thread
                    Err(e) => return drop(ready_tx.send(Err(e))),
                };
                let _ = ready_tx.send(Ok(()));
                for req in rx {
                    assert!(req.features.iter().all(|x| x.is_finite()), "non-finite feature reached the engine");
                    let _ = req.reply.send(engine.score(&req.features));
                }
            }) // loop ends when every sender is gone; `engine` drops (vse_close) on this thread
            .expect("spawn");
        ready_rx.recv().expect("owner thread reports")?;
        Ok(EngineService { tx: Some(tx), owner: Some(owner) })
    }

    pub fn score(&self, features: Vec<f64>) -> Result<f64, ServiceError> {
        let (reply, answer) = mpsc::sync_channel(1);
        let tx = self.tx.as_ref().expect("present until Drop");
        tx.send(Request { features, reply }).map_err(|_| ServiceError::EngineDown)?;
        answer.recv().map_err(|_| ServiceError::EngineDown)?.map_err(ServiceError::Engine)
    }
}

impl Drop for EngineService {
    fn drop(&mut self) {
        drop(self.tx.take()); // closes the queue: the owner loop ends and closes the engine
        if let Some(h) = self.owner.take() {
            let _ = h.join();
        }
    }
}

const _: () = {
    const fn shareable<T: Send + Sync>() {}
    shareable::<EngineService>();
};

fn main() {
    std::panic::set_hook(Box::new(|_| {})); // keep stdout readable
    let service = Arc::new(EngineService::start("/models/fraud-2026-09.vse".into(), 64).expect("start"));
    let workers: Vec<_> = (0..4)
        .map(|w| {
            let s = Arc::clone(&service);
            thread::spawn(move || (0..3).map(|i| s.score(vec![0.1 * w as f64, 0.5, 0.1 * i as f64]).is_ok()).filter(|ok| *ok).count())
        })
        .collect();
    let ok: usize = workers.into_iter().map(|h| h.join().unwrap()).sum();
    println!("12 requests from 4 threads -> {ok} scored on the owner thread");
    println!("wrong feature count -> {:?}", service.score(vec![0.9]));
    println!("NaN feature (a bug) -> {:?}", service.score(vec![f64::NAN, 0.5, 0.1]));
    println!("after the owner thread died -> {:?}", service.score(vec![0.9, 0.5, 0.1]));
}

// ---------------------------------------------------------------------------------------------
// SIMULATED VENDOR LIBRARY (as in ch02-11): Rust with the C ABI, standing in for libvse.so.
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
    pub unsafe extern "C" fn vse_open(path: *const c_char, out: *mut *mut EngineImpl) -> c_int {
        // SAFETY: the header's contract: `path` is NUL-terminated, `out` writable.
        let path = unsafe { CStr::from_ptr(path) };
        if !path.to_bytes().ends_with(b".vse") {
            return VSE_ENOMODEL;
        }
        let e = EngineImpl { owner: thread::current().id(), weights: vec![0.5, 0.3, 0.2], last_error: CString::default() };
        unsafe { out.write(Box::into_raw(Box::new(e))) };
        VSE_OK
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn vse_score(e: *mut EngineImpl, f: *const f64, n: usize, out: *mut f64) -> c_int {
        // SAFETY: the header's contract: live engine, `f` readable for n, `out` writable.
        let (e, f) = unsafe { (&mut *e, std::slice::from_raw_parts(f, n)) };
        if e.owner != thread::current().id() {
            e.last_error = c"engine used from a thread other than the one that opened it".into();
            return VSE_EWRONGTHREAD;
        }
        if n != e.weights.len() {
            e.last_error = CString::new(format!("expected {} features, got {n}", e.weights.len())).unwrap();
            return VSE_EINVAL;
        }
        unsafe { out.write(f.iter().zip(&e.weights).map(|(x, w)| x * w).sum()) };
        VSE_OK
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn vse_last_error(e: *const EngineImpl) -> *const c_char {
        // SAFETY: the header's contract: a live engine.
        unsafe { (*e).last_error.as_ptr() }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn vse_close(e: *mut EngineImpl) {
        // SAFETY: the header's contract: from vse_open, closed once.
        drop(unsafe { Box::from_raw(e) });
    }
}
