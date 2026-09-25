// verify: debug ok
// verify: debug miri-ok
// verify: debug+tree miri-ok
// The shape of std's split_at_mut: ONE raw pointer, both halves derived from it.
fn my_split_at_mut(s: &mut [u32], mid: usize) -> (&mut [u32], &mut [u32]) {
    let len = s.len();
    assert!(mid <= len, "mid > len");
    let p = s.as_mut_ptr();
    // SAFETY: [0, mid) and [mid, len) are in bounds (mid <= len) and do not overlap; both slices
    // derive from the single pointer `p` and borrow `s` for their whole lifetime.
    unsafe {
        (
            std::slice::from_raw_parts_mut(p, mid),
            std::slice::from_raw_parts_mut(p.add(mid), len - mid),
        )
    }
}

fn main() {
    let mut v = [1, 2, 3, 4];
    let (l, r) = my_split_at_mut(&mut v, 2);
    l[0] += 10;
    r[0] += 20;
    println!("{v:?}");
}
