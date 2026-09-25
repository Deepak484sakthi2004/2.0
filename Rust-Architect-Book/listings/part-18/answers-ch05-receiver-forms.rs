// verify: debug error:E0502
// Answer-key check for Chapter 18.5's intermediate exercise: `(&mut v).push(v.len())` writes the
// `&mut v` explicitly, so it is an ordinary mutable borrow (not two-phase) and `v.len()` conflicts.
fn main() {
    let mut v: Vec<usize> = vec![10, 20];
    v.push(v.len()); // accepted: autoref'd receiver, two-phase
    (&mut v).push(v.len()); // rejected: the explicit borrow is not two-phase
    println!("{v:?}");
}
