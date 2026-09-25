// verify: release panic capacity overflow
// verify: debug crash precondition
// verify: debug miri unreachable
// A non-UTF-8 `str` is a broken LIBRARY (safety) invariant: not UB the instant it exists,
// but std's code is written assuming UTF-8, and trusting it turns into language UB later.
use std::hint::black_box;

fn main() {
    let bytes: Vec<u8> = black_box(vec![b'o', b'k', 0xFF, 0xFE]);
    let s: &str = unsafe { std::str::from_utf8_unchecked(&bytes) };
    println!("len={} chars={}", s.len(), s.chars().count());
    println!("{:?}", s.chars().collect::<Vec<_>>());
}
