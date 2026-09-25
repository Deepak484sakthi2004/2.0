// verify: debug miri uninitialized
// An integer made from uninitialized memory is invalid: "any bit pattern" does not include "no bit pattern".
use std::mem::MaybeUninit;

fn main() {
    let x: u32 = unsafe { MaybeUninit::<u32>::uninit().assume_init() };
    println!("{x}");
}
