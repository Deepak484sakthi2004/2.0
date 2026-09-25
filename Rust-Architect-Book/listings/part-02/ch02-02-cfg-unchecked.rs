// verify: debug ok
// Compiles, even though `record_latency` calls a function that does not exist:
// code under a false #[cfg] is parsed, then removed before name resolution and type checking.
#[cfg(feature = "metrics")]
fn record_latency(ms: u64) {
    metrics_backend::histogram("latency_ms", ms);
}

fn handle_request() -> u64 {
    let ms = 42;
    #[cfg(feature = "metrics")]
    record_latency(ms);
    ms
}

fn main() {
    println!("handled in {} ms", handle_request());
}
