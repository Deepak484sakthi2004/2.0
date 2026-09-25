// verify: debug miri uninitialized
// `*p = value` on an uninitialized slot DROPS the old "value" first: here, a garbage String.
use std::mem::MaybeUninit;

fn main() {
    let mut slot: MaybeUninit<String> = MaybeUninit::uninit();
    let p = slot.as_mut_ptr();
    unsafe { *p = String::from("gateway") }; // BUG: assignment runs drop_in_place on uninitialized memory
    // Correct: `unsafe { p.write(String::from("gateway")) }` or `slot.write(...)`: no drop of the old bytes.
    let s = unsafe { slot.assume_init() };
    println!("{s}");
}
