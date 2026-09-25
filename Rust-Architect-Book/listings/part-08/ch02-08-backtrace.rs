// verify: debug ok
use std::backtrace::{Backtrace, BacktraceStatus};
use std::fmt;
use std::time::Instant;

/// An error that records where it was created. Capturing is opt-in and not free.
#[derive(Debug)]
struct InvariantError {
    what: &'static str,
    backtrace: Backtrace,
}

impl fmt::Display for InvariantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invariant violated: {}", self.what)
    }
}

impl std::error::Error for InvariantError {}

#[inline(never)]
fn reconcile(debits: i64, credits: i64) -> Result<(), InvariantError> {
    if debits != credits {
        // capture() honours RUST_BACKTRACE / RUST_LIB_BACKTRACE; force_capture() always walks the stack.
        return Err(InvariantError { what: "debits != credits", backtrace: Backtrace::capture() });
    }
    Ok(())
}

fn main() {
    let e = reconcile(100, 90).unwrap_err();
    println!("{e}; capture() status: {:?}", e.backtrace.status());

    let t = Instant::now();
    let bt = Backtrace::force_capture();
    let captured = t.elapsed();
    let t = Instant::now();
    let text = bt.to_string(); // symbolication happens lazily, on first Display
    let rendered = t.elapsed();
    assert_eq!(bt.status(), BacktraceStatus::Captured);
    println!("force_capture: {} frame lines, mentions main: {}", text.lines().filter(|l| l.trim_start().starts_with(|c: char| c.is_ascii_digit())).count(), text.contains("main"));
    println!("capture took {captured:?}, first render took {rendered:?} (one run, noisy)");
}
