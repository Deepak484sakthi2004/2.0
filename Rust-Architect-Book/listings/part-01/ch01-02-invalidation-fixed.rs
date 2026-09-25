// verify: debug ok
fn main() {
    // Fix 1: copy the value out. `i32` is `Copy`, so no borrow survives.
    let mut v = vec![1, 2, 3];
    let first = v[0];
    v.push(4);
    println!("fix 1: first = {first}");

    // Fix 2: remember a position, not an address.
    let mut v = vec![1, 2, 3];
    let first_idx = 0;
    v.push(4);
    println!("fix 2: first = {}", v[first_idx]);

    // Fix 3: finish using the borrow before mutating.
    let mut v = vec![1, 2, 3];
    let first = &v[0];
    println!("fix 3: first = {first}");
    v.push(4);
    println!("fix 3: len = {}", v.len());
}
