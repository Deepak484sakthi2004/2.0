// verify: debug error:E0425
// A macro can't see the caller's local variables unless they are passed in: macro_rules! locals
// are resolved at the macro's definition site (hygiene).
macro_rules! log_request {
    () => {
        println!("request {}", request_id)
    };
}

fn main() {
    let request_id = 42;
    log_request!();
}
