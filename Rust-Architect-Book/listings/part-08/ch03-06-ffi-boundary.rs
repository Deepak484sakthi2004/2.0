// verify: debug ok
// verify: debug miri-ok
use std::panic::{self, AssertUnwindSafe};

pub const OK: i32 = 0;
pub const ERR_INVALID: i32 = -1;
pub const ERR_PANIC: i32 = -99;

fn score_impl(amount_cents: i64) -> Result<i32, &'static str> {
    if amount_cents == 0 {
        return Err("zero amount");
    }
    if amount_cents < 0 {
        panic!("negative amount {amount_cents}"); // a bug, not a validation failure
    }
    Ok((amount_cents % 100) as i32)
}

/// The FFM-library pattern (Chapter 2.2): unwind + catch_unwind at every exported entry point.
/// Errors become codes; panics become a distinct code; nothing ever unwinds into the caller's frames.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_score(amount_cents: i64, out: *mut i32) -> i32 {
    let result = panic::catch_unwind(AssertUnwindSafe(|| score_impl(amount_cents)));
    match result {
        Ok(Ok(v)) => {
            // SAFETY: the caller's contract (documented in the C header) is that `out` is valid for writes.
            unsafe { out.write(v) };
            OK
        }
        Ok(Err(_)) => ERR_INVALID,
        Err(_) => ERR_PANIC,
    }
}

/// The other option (Rust 1.71+): declare that unwinding may cross the boundary, for callers that support it.
extern "C-unwind" fn score_may_unwind(amount_cents: i64) -> i32 {
    score_impl(amount_cents).unwrap_or(-1)
}

fn main() {
    panic::set_hook(Box::new(|_| {})); // keep stdout readable
    for amount in [250, 0, -5] {
        let mut out = 0;
        let rc = meridian_score(amount, &mut out);
        println!("meridian_score({amount}) -> rc={rc} out={out}");
    }
    let r = panic::catch_unwind(|| score_may_unwind(-5));
    println!("C-unwind: caught on the Rust side: {}", r.is_err());
}
