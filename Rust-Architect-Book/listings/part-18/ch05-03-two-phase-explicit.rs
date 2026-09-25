// verify: debug error:E0502
// Two-phase borrows are created only for autoref'd method receivers (and overloaded compound
// assignment). Writing the `&mut v` argument yourself creates an ordinary mutable borrow.
fn main() {
    let mut v: Vec<usize> = vec![10, 20];
    v.push(v.len()); // method-call syntax: accepted (two-phase borrow)
    Vec::push(&mut v, v.len()); // explicit `&mut v` argument: no two-phase borrow
    println!("{v:?}");
}
