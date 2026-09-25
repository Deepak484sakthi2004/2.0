// verify: debug ok
// verify: debug@2021 ok
//! How long does a temporary guard live? The answer decides whether the second lock() deadlocks.
//! try_lock() stands in for lock() so the listing reports the deadlock instead of hanging.
use std::collections::HashMap;
use std::sync::{Mutex, TryLockError};

fn second_lock(cache: &Mutex<HashMap<u32, &'static str>>) -> &'static str {
    match cache.try_lock() {
        Ok(_) => "acquired",
        Err(TryLockError::WouldBlock) => "WouldBlock (lock() would deadlock here)",
        Err(TryLockError::Poisoned(_)) => "poisoned",
    }
}

fn main() {
    let cache = Mutex::new(HashMap::from([(1, "cached")]));

    // `if let ... else`: in edition 2024 the scrutinee's temporaries (the guard) are dropped before `else`.
    if let Some(v) = cache.lock().unwrap().get(&2) {
        println!("hit {v}");
    } else {
        println!("if-let else branch:  second lock {}", second_lock(&cache));
    }

    // `match`: the scrutinee's temporaries live until the end of the whole match, in every edition.
    match cache.lock().unwrap().get(&1) {
        Some(_) => println!("match arm:           second lock {}", second_lock(&cache)),
        None => {}
    }

    // Bind the value you need, and let the guard die at the end of the `let` statement.
    let v = cache.lock().unwrap().get(&1).copied();
    println!("after let-binding:   second lock {} (value {v:?})", second_lock(&cache));
}
