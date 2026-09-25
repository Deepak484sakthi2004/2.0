// verify: debug ok
// verify: debug miri-ok
use std::mem::size_of;

/// Wire encoding is part of the contract: explicit discriminants, explicit width.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Bid = 1,
    Ask = 2,
}

#[derive(Debug, PartialEq)]
struct BadSide(u8);

impl TryFrom<u8> for Side {
    type Error = BadSide;
    fn try_from(b: u8) -> Result<Side, BadSide> {
        match b {
            1 => Ok(Side::Bid),
            2 => Ok(Side::Ask),
            other => Err(BadSide(other)),
        }
    }
}

fn main() {
    println!("Side as u8: Bid={} Ask={}", Side::Bid as u8, Side::Ask as u8);
    for b in [1u8, 2, 0, 7] {
        println!("byte {b} -> {:?}", Side::try_from(b));
    }
    println!("size_of Side={} Option<Side>={}", size_of::<Side>(), size_of::<Option<Side>>());
    // What does rustc store for None? One of the byte values Side can never hold.
    let none: Option<Side> = None;
    // SAFETY: Option<Side> is 1 byte with no padding, so reading it as u8 is sound.
    let raw: u8 = unsafe { std::mem::transmute(none) };
    println!("bits of None::<Side> = {raw}");
    println!(
        "discriminant(Bid) == discriminant(Bid)? {}",
        std::mem::discriminant(&Side::Bid) == std::mem::discriminant(&Side::Bid)
    );
}
