// verify: debug ok
#![allow(dead_code)]
use std::mem::{align_of, offset_of, size_of};

struct Order {
    side: u8,
    qty: u32,
    id: u64,
    price_cents: i64,
    tif: u8,
}

#[repr(C)]
struct OrderC {
    side: u8,
    qty: u32,
    id: u64,
    price_cents: i64,
    tif: u8,
}

fn main() {
    println!("{:<18} size align", "type");
    println!("{:<18} {:>4} {:>5}", "u8", size_of::<u8>(), align_of::<u8>());
    println!("{:<18} {:>4} {:>5}", "u32", size_of::<u32>(), align_of::<u32>());
    println!("{:<18} {:>4} {:>5}", "u64", size_of::<u64>(), align_of::<u64>());
    println!("{:<18} {:>4} {:>5}", "u128", size_of::<u128>(), align_of::<u128>());
    println!("{:<18} {:>4} {:>5}", "(u8, u64)", size_of::<(u8, u64)>(), align_of::<(u8, u64)>());
    println!("{:<18} {:>4} {:>5}", "[u16; 3]", size_of::<[u16; 3]>(), align_of::<[u16; 3]>());
    println!("{:<18} {:>4} {:>5}", "Order (Rust)", size_of::<Order>(), align_of::<Order>());
    println!("{:<18} {:>4} {:>5}", "OrderC (repr C)", size_of::<OrderC>(), align_of::<OrderC>());

    println!("\nfield offsets        Order   OrderC");
    println!("side                 {:>5}   {:>6}", offset_of!(Order, side), offset_of!(OrderC, side));
    println!("qty                  {:>5}   {:>6}", offset_of!(Order, qty), offset_of!(OrderC, qty));
    println!("id                   {:>5}   {:>6}", offset_of!(Order, id), offset_of!(OrderC, id));
    println!("price_cents          {:>5}   {:>6}", offset_of!(Order, price_cents), offset_of!(OrderC, price_cents));
    println!("tif                  {:>5}   {:>6}", offset_of!(Order, tif), offset_of!(OrderC, tif));

    let n = 10_000_000usize;
    println!(
        "\n10M orders: {} MB (Rust layout) vs {} MB (repr C)",
        n * size_of::<Order>() / 1_000_000,
        n * size_of::<OrderC>() / 1_000_000
    );
}
