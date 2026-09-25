// verify: release ok
// verify: debug miri boolean
// A `bool` holding the byte 2 is an INVALID VALUE: undefined behavior the moment it is produced.
// Natively, the optimized `if b { 1 } else { 0 }` returns 2: a value neither branch can return.
use std::hint::black_box;

#[inline(never)]
fn to_u32(b: bool) -> u32 {
    if b { 1 } else { 0 }
}

fn main() {
    let raw: u8 = black_box(2);
    let b: bool = unsafe { std::mem::transmute::<u8, bool>(raw) };
    println!("to_u32(b) = {}", to_u32(black_box(b)));
}
