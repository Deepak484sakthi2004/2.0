// verify: debug miri use-after-free
// `ptr::read` makes a bitwise copy: now TWO values own one heap buffer. Dropping both frees it twice.
use std::mem::ManuallyDrop;

fn main() {
    let mut original = ManuallyDrop::new(String::from("token"));
    let copy: String = unsafe { std::ptr::read(&*original) }; // a second owner of the same buffer
    drop(copy); // frees the buffer
    unsafe { ManuallyDrop::drop(&mut original) }; // BUG: frees it again
}
