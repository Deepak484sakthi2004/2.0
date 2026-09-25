// verify: debug error:E0308
// The same borrow error as ch01-03, but now in a body that ALSO has a type error. Only E0308 is
// reported: a body whose type check failed is not borrow-checked.
fn both_in_one_body() -> u32 {
    let mut v = vec![1u32];
    let first = &v[0];
    v.push(2); // would be E0502...
    let label: u32 = "three"; // ...but this body also has E0308
    *first + label
}

fn main() {
    println!("{}", both_in_one_body());
}
