// verify: release ok
// Answer-key listing for Chapter 15.1's Advanced exercise: the same invalid bool (byte 2) through
// `if b { 10 } else { 20 }`. What comes out depends on the instructions LLVM picked for a VALID bool.
use std::hint::black_box;

#[inline(never)]
fn pick(b: bool) -> u32 {
    if b { 10 } else { 20 }
}

fn main() {
    let raw: u8 = black_box(2);
    let b: bool = unsafe { std::mem::transmute::<u8, bool>(raw) };
    println!("pick(b) = {}", pick(black_box(b)));
}
