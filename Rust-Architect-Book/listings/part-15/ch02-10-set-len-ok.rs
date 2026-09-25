// verify: debug ok
// verify: debug miri-ok
// Initialize first, THEN publish the new length.
fn fill_squares(n: usize) -> Vec<u64> {
    let mut v: Vec<u64> = Vec::with_capacity(n);
    for (i, slot) in v.spare_capacity_mut()[..n].iter_mut().enumerate() {
        slot.write((i as u64) * (i as u64)); // MaybeUninit::write: no read of the old contents
    }
    // SAFETY: n <= capacity (with_capacity(n)), and every slot in [0, n) was written above.
    unsafe { v.set_len(n) };
    v
}

fn main() {
    println!("{:?}", fill_squares(6));
}
