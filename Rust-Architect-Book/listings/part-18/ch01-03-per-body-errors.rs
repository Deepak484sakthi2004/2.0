// verify: debug error:E0425
// verify: debug error:E0308
// verify: debug error:E0502
// Three bodies, three phases: name resolution (E0425), type checking (E0308), borrow checking
// (E0502). All three are reported: an error in one body doesn't stop other bodies being checked.
fn resolve_error() -> u32 {
    retries + 1 // E0425: no such name
}

fn type_error() -> u32 {
    "three" // E0308: wrong type
}

fn borrow_error() -> u32 {
    let mut v = vec![1u32];
    let first = &v[0];
    v.push(2); // E0502
    *first
}

fn main() {
    println!("{} {} {}", resolve_error(), type_error(), borrow_error());
}
