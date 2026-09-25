// verify: release ok
// The deprecated `mem::uninitialized` today: it fills with 0x01 bytes instead of leaving memory uninitialized.
// Still UB for a u64 by the language's rules; the fill is a [RUSTC]/[LIB] mitigation, not a guarantee.
#![allow(deprecated, invalid_value)]

fn main() {
    let x: u64 = unsafe { std::mem::uninitialized() };
    let b: bool = unsafe { std::mem::uninitialized() };
    println!("u64 = {x:#018x}, bool = {b}");
}
