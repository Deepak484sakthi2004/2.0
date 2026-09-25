// verify: debug ok
#![allow(dead_code)]
use std::collections::HashMap;
use std::mem::size_of;
use std::num::NonZeroU64;

/// Version 1: "not looked up yet" = None, "looked up, no account" = Some(None).
type CacheV1 = HashMap<u64, Option<Option<NonZeroU64>>>;

/// Version 2: an explicit enum. Clearer, but it still needs TWO spare bit patterns.
enum Cached {
    Unknown,
    Absent,
    Present(NonZeroU64),
}

/// Version 3: "unknown" is expressed by the key being absent from the map.
type CacheV3 = HashMap<u64, Option<NonZeroU64>>;

fn main() {
    println!("value sizes: Option<Option<NonZeroU64>>={} Cached={} Option<NonZeroU64>={}",
        size_of::<Option<Option<NonZeroU64>>>(), size_of::<Cached>(), size_of::<Option<NonZeroU64>>());
    println!("entry sizes: v1 (u64, Option<Option<_>>)={} v3 (u64, Option<_>)={}",
        size_of::<(u64, Option<Option<NonZeroU64>>)>(), size_of::<(u64, Option<NonZeroU64>)>());

    let mut v3: CacheV3 = HashMap::new();
    v3.insert(7, NonZeroU64::new(1_000_007)); // looked up: account exists
    v3.insert(8, None); // looked up: no account
    for user in [7, 8, 9] {
        match v3.get(&user) {
            None => println!("user {user}: unknown, ask the database"),
            Some(None) => println!("user {user}: known to have no account"),
            Some(Some(acct)) => println!("user {user}: account {acct}"),
        }
    }
    let _unused: CacheV1 = HashMap::new();
}
