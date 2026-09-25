// verify: debug ok
// A `move` closure captures a Copy value BY COPY. Mutations happen to the closure's own copy.
fn with_retries<F: FnMut() -> Result<u32, &'static str>>(mut attempt: F, max: u32) -> Result<u32, &'static str> {
    let mut last = Err("never tried");
    for _ in 0..max {
        last = attempt();
        if last.is_ok() {
            break;
        }
    }
    last
}

fn main() {
    let mut attempts = 0u32; // meant to be reported in the access log
    let mut failures_left = 2; // the upstream fails twice, then succeeds
    let result = with_retries(
        move || {
            attempts += 1; // increments the CLOSURE's copy of `attempts`
            if failures_left > 0 {
                failures_left -= 1;
                Err("upstream timeout")
            } else {
                Ok(attempts)
            }
        },
        5,
    );
    println!("result = {result:?}, attempts logged = {attempts}");
}
