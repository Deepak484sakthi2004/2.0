// verify: debug error:E0502
use std::collections::HashMap;

/// Returns the cached value, inserting a default first if the key is missing.
/// This is SOUND (the early return only happens when no insert follows), but the current
/// borrow checker (NLL) rejects it; the next-generation checker (Polonius) accepts it.
fn get_or_default(cache: &mut HashMap<u32, String>, key: u32) -> &String {
    if let Some(v) = cache.get(&key) {
        return v;
    }
    cache.insert(key, String::from("default"));
    cache.get(&key).unwrap()
}

fn main() {
    let mut cache = HashMap::new();
    println!("{}", get_or_default(&mut cache, 7));
}
