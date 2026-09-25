// verify: debug ok
// Answer-key check for Chapter 18.5's advanced exercise: two stable rewrites of problem case #3, with
// hash computations counted by a counting BuildHasher (the Part IX instrument).
use std::cell::Cell;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher, RandomState};

thread_local! { static HASHES: Cell<u32> = const { Cell::new(0) }; }

struct Counting(RandomState);
struct CountingHasher(std::hash::DefaultHasher);

impl BuildHasher for Counting {
    type Hasher = CountingHasher;
    fn build_hasher(&self) -> CountingHasher {
        CountingHasher(self.0.build_hasher())
    }
}
impl Hasher for CountingHasher {
    fn write(&mut self, bytes: &[u8]) {
        self.0.write(bytes)
    }
    fn finish(&self) -> u64 {
        HASHES.with(|h| h.set(h.get() + 1));
        self.0.finish()
    }
}

type Cache = HashMap<u32, String, Counting>;

fn with_contains_key(cache: &mut Cache, key: u32) -> &String {
    if !cache.contains_key(&key) {
        cache.insert(key, String::from("default"));
    }
    cache.get(&key).unwrap() // the only returned borrow is created after the last mutation
}

fn with_entry(cache: &mut Cache, key: u32) -> &String {
    cache.entry(key).or_insert_with(|| String::from("default"))
}

fn count(f: impl FnOnce()) -> u32 {
    HASHES.with(|h| h.set(0));
    f();
    HASHES.with(|h| h.get())
}

fn main() {
    let mut c: Cache = HashMap::with_hasher(Counting(RandomState::new()));
    c.insert(1, "one".to_string());
    println!("contains_key: hit {} hashes, miss {} hashes",
        count(|| { with_contains_key(&mut c, 1); }),
        count(|| { with_contains_key(&mut c, 2); }));
    println!("entry:        hit {} hashes, miss {} hashes",
        count(|| { with_entry(&mut c, 1); }),
        count(|| { with_entry(&mut c, 3); }));
}
