// verify: release build
// What `add` vs `wrapping_add` promise to LLVM, and what a safe big-endian parse costs.
#[inline(never)]
pub fn nth(p: *const u32, i: usize) -> *const u32 {
    // SAFETY (caller's obligation): p + i stays within p's allocation (or one past its end).
    unsafe { p.add(i) }
}

#[inline(never)]
pub fn nth_wrapping(p: *const u32, i: usize) -> *const u32 {
    p.wrapping_add(i) // no promise: may point anywhere (dereferencing is another matter)
}

#[inline(never)]
pub fn body_len(buf: &[u8]) -> Option<u32> {
    let b: &[u8; 8] = buf.get(..8)?.try_into().ok()?;
    Some(u32::from_be_bytes([b[4], b[5], b[6], b[7]]))
}
