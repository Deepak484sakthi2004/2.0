// verify: debug miri enum tag
#![allow(dead_code)]
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
enum Side {
    Bid = 1,
    Ask = 2,
}

fn main() {
    let wire_byte: u8 = 7; // a corrupted or hostile byte from the network
    // "It's just a u8 underneath" -- no: 7 is not a valid Side.
    let side: Side = unsafe { std::mem::transmute(wire_byte) };
    println!("{side:?}");
}
