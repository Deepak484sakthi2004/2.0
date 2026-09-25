// verify: debug ok
// A Java habit ported to Rust: erase everything to "Object" and cast back later.
use std::any::Any;
use std::collections::HashMap;

fn limit(settings: &HashMap<&str, Box<dyn Any>>, key: &str, default: u64) -> u64 {
    match settings.get(key).and_then(|v| v.downcast_ref::<u64>()) {
        Some(&v) => v,
        None => {
            println!("  {key}: missing or not a u64, using default {default}");
            default
        }
    }
}

fn main() {
    let mut settings: HashMap<&str, Box<dyn Any>> = HashMap::new();
    settings.insert("max_payout_cents", Box::new(500_000_u64));
    settings.insert("max_refund_cents", Box::new(20_000_u32)); // stored as u32 by another module
    println!("payout limit = {}", limit(&settings, "max_payout_cents", 0));
    println!("refund limit = {}", limit(&settings, "max_refund_cents", u64::MAX));
}
