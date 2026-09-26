// verify: debug ok
// verify: debug miri-ok
// Two ways to keep a panic from crossing a boundary that can't take it:
// catch it inside (extern "C" + catch_unwind), or declare that unwinding may cross (extern "C-unwind").
use std::panic::{self, AssertUnwindSafe};

pub const MERIDIAN_OK: i32 = 0;
pub const MERIDIAN_ERR_PANIC: i32 = -99;

fn risky(x: u32) -> u32 {
    assert!(x != 0, "zero is a bug in the caller");
    100 / x
}

/// For callers that can't unwind (C, the JVM): the panic stops here and becomes a code.
pub extern "C" fn guarded(x: u32, out: &mut u32) -> i32 {
    match panic::catch_unwind(AssertUnwindSafe(|| risky(x))) {
        Ok(v) => {
            *out = v;
            MERIDIAN_OK
        }
        Err(_) => MERIDIAN_ERR_PANIC,
    }
}

/// For callers that CAN unwind (Rust, or C++ compiled with exceptions): the panic may cross.
pub extern "C-unwind" fn unguarded(x: u32) -> u32 {
    risky(x)
}

fn main() {
    panic::set_hook(Box::new(|_| {})); // keep the output readable
    let mut out = 0;
    println!("guarded(4)   -> rc={} out={out}", guarded(4, &mut out));
    println!("guarded(0)   -> rc={}", guarded(0, &mut out));
    let r = panic::catch_unwind(|| unguarded(0));
    println!("unguarded(0) -> the panic crossed the C-unwind boundary and was caught by the caller: {}", r.is_err());
}
