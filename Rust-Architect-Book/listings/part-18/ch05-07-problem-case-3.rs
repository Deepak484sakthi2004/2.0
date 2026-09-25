// verify: debug error:E0502
// verify: debug+nightly ok
// NLL problem case #3 (Chapter 4.2). Rejected by stable 1.98.1 (and beta 1.99.0-beta.7), accepted
// by the nightly of 2026-09-24 (1.100.0-nightly) without any flag. [VERSION]
use std::collections::HashMap;

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
