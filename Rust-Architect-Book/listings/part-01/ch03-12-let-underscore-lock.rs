// verify: debug error:let_underscore_lock
use std::sync::Mutex;

static AUDIT_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn critical_section() {
    let _ = AUDIT_LOG.lock().unwrap(); // guard dropped immediately: no exclusion
    // ... work that was supposed to be exclusive ...
}

fn main() {
    critical_section();
}
