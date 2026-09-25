// verify: debug ok
// verify: debug miri-ok
// Vec::into_raw_parts (stable on Rust 1.98): the safe half of the round trip; rebuilding is the unsafe half.
fn main() {
    let v = vec![1u32, 2, 3];
    let (ptr, len, cap) = v.into_raw_parts(); // nothing is freed: we own the buffer through `ptr` now
    // SAFETY: ptr/len/cap come from into_raw_parts of a Vec<u32> and are used exactly once, unchanged.
    let rebuilt = unsafe { Vec::from_raw_parts(ptr, len, cap) };
    println!("{rebuilt:?} (len {len}, cap {cap})");
}
