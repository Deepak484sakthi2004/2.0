// verify: debug ok
// verify: debug miri dangling reference
// "Silencing" E0502 with a raw-pointer round trip. It compiles, it runs, it may even print 1.
// It is Undefined Behavior: push() reallocates (capacity was 3), and `first` dangles.
fn main() {
    let mut v = vec![1, 2, 3];
    let first: &i32 = unsafe { &*(&v[0] as *const i32) }; // the borrow checker can't see through this
    v.push(4);
    println!("first = {first}");
}
