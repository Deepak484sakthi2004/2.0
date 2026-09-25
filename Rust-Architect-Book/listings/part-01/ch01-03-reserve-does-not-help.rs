// verify: debug error:E0502
fn main() {
    let mut v = Vec::with_capacity(16);
    v.extend([1, 2, 3]);
    let first = &v[0];
    v.push(4); // capacity is 16, so no reallocation will happen...
    println!("first = {first}"); // ...but the borrow checker still rejects this
}
