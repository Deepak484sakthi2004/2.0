// verify: debug ok
// verify: release ok
use std::mem::{size_of, size_of_val};

struct RouteConfig {
    name: String, // 24 bytes
    limit: u64,   //  8 bytes
}

fn add_one(x: u64) -> u64 {
    x + 1
}

fn show(what: &str, bytes: usize) {
    println!("{what:<28}{bytes:>3} B");
}

fn main() {
    let cfg = RouteConfig { name: "payments".to_string(), limit: 500 };
    let threshold = 100u64;
    let mut hits = 0u32;

    // A closure is an anonymous struct holding its captures. Its size is the size of that struct.
    let no_capture = |x: u64| x + 1;
    let by_ref = |x: u64| x > threshold; // captures &threshold
    let two_refs = |x: u64| x > threshold && x < cfg.limit; // &threshold + &cfg.limit (a field!)
    let by_move = move |x: u64| x > threshold; // captures threshold itself (a u64 copy)
    let mut by_mut = |x: u64| {
        if x > threshold {
            hits += 1; // captures &mut hits (and &threshold)
        }
    };
    by_mut(150);

    show("no captures", size_of_val(&no_capture));
    show("&threshold", size_of_val(&by_ref));
    show("&threshold, &cfg.limit", size_of_val(&two_refs));
    show("move threshold (u64)", size_of_val(&by_move));
    show("&threshold, &mut hits", size_of_val(&by_mut));

    let name_len = move || cfg.name.len(); // moves ONLY cfg.name (edition 2021+ disjoint capture)
    show("move cfg.name (String)", size_of_val(&name_len));

    // Function items, function pointers, and boxed closures, for comparison.
    let item = add_one; // the fn ITEM type: a zero-sized type naming exactly one function
    let ptr: fn(u64) -> u64 = add_one; // a fn POINTER: 8 bytes, any function with this signature
    let boxed: Box<dyn Fn(u64) -> u64> = Box::new(move |x| x + threshold);
    show("fn item `add_one`", size_of_val(&item));
    show("fn pointer", size_of_val(&ptr));
    show("Box<dyn Fn> (data + vtable)", size_of_val(&boxed));
    println!("hits = {hits}; cfg.limit still usable: {}", cfg.limit);
    assert_eq!(size_of::<fn(u64) -> u64>(), 8);
    let _ = (no_capture(1), by_ref(1), two_refs(1), by_move(1), name_len(), item(1), ptr(1), boxed(1));
}
