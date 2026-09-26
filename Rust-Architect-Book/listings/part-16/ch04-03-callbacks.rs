// verify: debug ok
// verify: debug miri-ok
// Callbacks across the boundary: a C function pointer plus a `void *user` that the library passes
// back untouched. Data lent to the callback is valid only during the call. A panic in Rust code
// called from C is caught in the trampoline, carried across as a "stop" code, and resumed in Rust.
use std::any::Any;
use std::ffi::{CStr, c_char, c_void};
use std::ops::ControlFlow;
use std::panic::{self, AssertUnwindSafe};

pub const MERIDIAN_OK: i32 = 0;
pub const MERIDIAN_ERR_INVALID: i32 = -1;

/// C: `typedef int32_t (*meridian_feature_cb)(void *user, const char *name, double value);`
/// Return 0 to continue, anything else to stop (the value is returned to the caller).
pub type FeatureCb = unsafe extern "C" fn(user: *mut c_void, name: *const c_char, value: f64) -> i32;

// ---------------- library side (the exported C API) ----------------

/// C: `int32_t meridian_for_each_feature(uint64_t txn, meridian_feature_cb cb, void *user);`
/// `name` is valid only until the callback returns. `user` is passed through, never dereferenced.
/// # Safety
/// `cb`, if non-null, is safe to call with `user` and any valid NUL-terminated `name`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_for_each_feature(txn: u64, cb: Option<FeatureCb>, user: *mut c_void) -> i32 {
    let Some(cb) = cb else { return MERIDIAN_ERR_INVALID };
    for (name, weight) in [(c"amount", 0.41), (c"velocity", 0.22), (c"country", -0.05)] {
        let value = weight * (txn % 10) as f64;
        // SAFETY: the caller's contract on `cb`/`user`; `name` is a 'static C string here, but the
        // header only promises "valid during the call", so the library is free to change that.
        let rc = unsafe { cb(user, name.as_ptr(), value) };
        if rc != 0 {
            return rc;
        }
    }
    MERIDIAN_OK
}

// ---------------- caller side (a safe Rust wrapper over the C API) ----------------

struct Ctx<F> {
    f: F,
    panic: Option<Box<dyn Any + Send>>,
}

unsafe extern "C" fn trampoline<F>(user: *mut c_void, name: *const c_char, value: f64) -> i32
where
    F: FnMut(&CStr, f64) -> ControlFlow<()>,
{
    // SAFETY: `user` is the `&mut Ctx<F>` passed below, alive and unaliased for the whole call.
    let ctx = unsafe { &mut *(user as *mut Ctx<F>) };
    // SAFETY: the library promises a NUL-terminated `name`, valid until we return.
    let name = unsafe { CStr::from_ptr(name) };
    match panic::catch_unwind(AssertUnwindSafe(|| (ctx.f)(name, value))) {
        Ok(ControlFlow::Continue(())) => 0,
        Ok(ControlFlow::Break(())) => 1,
        Err(payload) => {
            ctx.panic = Some(payload); // can't unwind through C: park it, stop the iteration
            2
        }
    }
}

/// `f` gets `&CStr` for the duration of each call only (a higher-ranked borrow: it can't keep it).
pub fn for_each_feature<F>(txn: u64, f: F) -> Result<(), i32>
where
    F: FnMut(&CStr, f64) -> ControlFlow<()>,
{
    let mut ctx = Ctx { f, panic: None };
    // SAFETY: trampoline::<F> matches FeatureCb and expects exactly `&mut Ctx<F>` as `user`;
    // `ctx` outlives the call, and the library doesn't keep `user` after returning.
    let rc = unsafe { meridian_for_each_feature(txn, Some(trampoline::<F>), (&raw mut ctx).cast()) };
    if let Some(payload) = ctx.panic {
        panic::resume_unwind(payload); // back in Rust frames: continue the original panic
    }
    match rc {
        MERIDIAN_OK | 1 => Ok(()),
        other => Err(other),
    }
}

fn main() {
    let mut all = Vec::new();
    for_each_feature(7, |name, v| {
        all.push(format!("{}={v:.2}", name.to_str().unwrap())); // copy what you keep
        ControlFlow::Continue(())
    })
    .unwrap();
    println!("all features: {all:?}");

    let mut first_negative = None;
    for_each_feature(7, |name, v| {
        if v < 0.0 {
            first_negative = Some(name.to_owned());
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    })
    .unwrap();
    println!("first negative feature: {first_negative:?}");

    panic::set_hook(Box::new(|_| {}));
    let r = panic::catch_unwind(|| {
        let _ = for_each_feature(7, |name, _| {
            if name == c"velocity" {
                panic!("bad feature {name:?}");
            }
            ControlFlow::Continue(())
        });
    });
    let msg = r.unwrap_err().downcast::<String>().map(|s| *s).unwrap_or_default();
    println!("panic in the callback reached the Rust caller intact: {msg:?}");
}
