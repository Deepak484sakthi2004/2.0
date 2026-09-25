// verify: debug ok
// verify: debug miri-ok
// The same intent, written safely: read the value (Copy) before mutating. Miri finds nothing.
fn main() {
    let mut v = vec![1, 2, 3];
    let first = v[0];
    v.push(4);
    println!("first = {first}");
}
