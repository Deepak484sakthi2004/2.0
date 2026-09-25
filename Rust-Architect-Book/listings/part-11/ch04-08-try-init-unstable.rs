// verify: debug error:E0658
//! Fallible one-time initialization would be `OnceLock::get_or_try_init`, still unstable on 1.98.1.
use std::sync::OnceLock;

fn main() {
    let config: OnceLock<u32> = OnceLock::new();
    let r: Result<&u32, String> = config.get_or_try_init(|| Err("REGION is not set".to_string()));
    println!("{r:?}");
}
