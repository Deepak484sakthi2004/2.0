// verify: debug ok
// `#[must_use]` does not protect a guard type from `let _ =`: explicitly discarding a value is
// "using" it, and the warning's own help text suggests `let _ = ...`.
#[must_use]
pub struct Lease;

pub fn acquire() -> Lease {
    Lease
}

fn main() {
    let _ = acquire(); // no warning: the value is dropped here, silently
    acquire(); // warning: unused `Lease` that must be used
}
