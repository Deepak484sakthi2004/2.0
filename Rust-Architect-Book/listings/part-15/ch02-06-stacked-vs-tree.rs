// verify: debug ok
// verify: debug miri Undefined
// verify: debug+tree miri-ok
// UB under Stacked Borrows, accepted under Tree Borrows: calling `as_mut_ptr()` twice.
fn split_twice(s: &mut [u32], mid: usize) -> (&mut [u32], &mut [u32]) {
    let len = s.len();
    assert!(mid <= len);
    unsafe {
        let left = std::slice::from_raw_parts_mut(s.as_mut_ptr(), mid);
        // The second `as_mut_ptr()` takes `&mut *s` again: a fresh unique reborrow of the WHOLE slice.
        let right = std::slice::from_raw_parts_mut(s.as_mut_ptr().add(mid), len - mid);
        (left, right)
    }
}

fn main() {
    let mut v = [1, 2, 3, 4];
    let (l, r) = split_twice(&mut v, 2);
    l[0] += 10;
    r[0] += 20;
    println!("{v:?}");
}
