// verify: debug error:padding
use bytemuck::{Pod, Zeroable};

// A record we want to write to disk "as bytes". It has 7 bytes of padding after `side`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TradeRecord {
    id: u64,
    price_cents: i64,
    side: u8,
}

fn main() {
    let t = TradeRecord { id: 1, price_cents: 1999, side: 1 };
    println!("{:?}", bytemuck::bytes_of(&t));
}
