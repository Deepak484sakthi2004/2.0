// verify: debug miri null
// Creating a null reference is UB even if it is never read.
fn main() {
    let p: *const u64 = std::ptr::null();
    let r: &u64 = unsafe { &*p };
    println!("{:p}", r);
}
