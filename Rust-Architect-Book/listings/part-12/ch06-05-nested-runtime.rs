// verify: debug panic Cannot start a runtime from within a runtime
//! The "fix" for ch06-04 that works in a unit test and fails in the service: block_on inside code
//! that is already running on the runtime.
async fn load_limits(merchant: u64) -> u64 {
    merchant * 1_000
}

/// A synchronous API over an async one, via a (global) runtime.
fn check_limit(merchant: u64, amount: u64) -> bool {
    let rt = tokio::runtime::Handle::current();
    amount <= rt.block_on(load_limits(merchant))
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // Deep inside a request handler, someone calls the synchronous helper:
    println!("{}", check_limit(7, 500));
}
