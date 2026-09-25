// verify: debug ok
use bytemuck::{Pod, Zeroable};
use std::mem::size_of;

// The same record with the padding made EXPLICIT and always zeroed: now every byte is defined.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct TradeRecord {
    id: u64,
    price_cents: i64,
    side: u8,
    _pad: [u8; 7],
}

impl TradeRecord {
    fn new(id: u64, price_cents: i64, side: u8) -> Self {
        TradeRecord { id, price_cents, side, _pad: [0; 7] }
    }
}

fn main() {
    let t = TradeRecord::new(1, 1999, 1);
    let bytes: &[u8] = bytemuck::bytes_of(&t);
    println!("size = {}, bytes = {:?}", size_of::<TradeRecord>(), bytes);
    let back: TradeRecord = bytemuck::pod_read_unaligned(bytes);
    println!("round trip: {back:?}");
}
