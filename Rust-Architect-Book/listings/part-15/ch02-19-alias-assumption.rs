// verify: debug ok
// verify: release ok
// verify: debug miri Undefined
// Why two `&mut` to one place is UB, not just "surprising": the optimizer may assume they differ.
// `transfer` returns the new `from` balance; with `from` and `to` aliasing, debug and release disagree.

#[inline(never)]
fn transfer(from: &mut i64, to: &mut i64, amount: i64) -> i64 {
    *from -= amount;
    *to += amount;
    *from // `from` and `to` are both `&mut` (noalias): the compiler may reuse the value it just computed
}

fn two_mut<T>(s: &mut [T], i: usize, j: usize) -> (&mut T, &mut T) {
    assert!(i < s.len() && j < s.len()); // bounds checked; distinctness forgotten
    let p = s.as_mut_ptr();
    unsafe { (&mut *p.add(i), &mut *p.add(j)) }
}

fn main() {
    let mut balances = [100i64, 50];
    let (a, b) = two_mut(&mut balances, 0, 0); // a transfer from an account to itself
    let reported = transfer(a, b, 30);
    println!("reported balance = {reported}, stored balance = {}", balances[0]);
}
