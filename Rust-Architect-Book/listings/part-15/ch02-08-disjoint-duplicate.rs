// verify: debug miri Undefined
// verify: debug+tree miri Undefined
// Bounds checked, distinctness forgotten: two `&mut` to the same element.
fn two_mut<T>(s: &mut [T], i: usize, j: usize) -> (&mut T, &mut T) {
    assert!(i < s.len() && j < s.len());
    let p = s.as_mut_ptr();
    unsafe { (&mut *p.add(i), &mut *p.add(j)) }
}

fn main() {
    let mut balances = [100i64, 50];
    let (a, b) = two_mut(&mut balances, 0, 0); // a transfer from an account to itself
    *a -= 30;
    *b += 30;
    println!("{balances:?}");
}
