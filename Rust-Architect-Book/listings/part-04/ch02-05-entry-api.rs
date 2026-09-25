// verify: debug ok
use std::collections::HashMap;

/// The idiomatic shape: ONE lookup that returns a handle to the slot, occupied or vacant.
fn get_or_default(cache: &mut HashMap<u32, String>, key: u32) -> &String {
    cache.entry(key).or_insert_with(|| String::from("default"))
}

/// When the key must be computed or the value is expensive: or_insert_with only runs on a miss.
fn get_or_load<'c>(cache: &'c mut HashMap<u32, String>, key: u32, loads: &mut u32) -> &'c String {
    cache.entry(key).or_insert_with(|| {
        *loads += 1;
        format!("loaded-{key}")
    })
}

fn main() {
    let mut cache = HashMap::new();
    println!("{}", get_or_default(&mut cache, 7));
    let mut loads = 0;
    for key in [1, 2, 1, 1, 3] {
        get_or_load(&mut cache, key, &mut loads);
    }
    println!("entries={} loads={loads}", cache.len());
}
