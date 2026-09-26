// verify: debug ok
// verify: debug miri-ok
// Layout is half of an ABI. repr(C) pins field order and offsets; the default repr pins nothing.
#![allow(dead_code)]
use std::mem::{align_of, offset_of, size_of};

/// Rust's default representation: the compiler may reorder fields (and does, to cut padding).
struct TxnRust {
    flags: u8,
    amount_cents: i64,
    currency: u16,
    merchant_id: u32,
}

/// The same fields with the C layout: declaration order, each field aligned, size rounded up.
#[repr(C)]
pub struct MeridianTxn {
    pub flags: u8,         // offset 0, then 7 bytes of padding
    pub amount_cents: i64, // offset 8
    pub currency: u16,     // offset 16, then 2 bytes of padding
    pub merchant_id: u32,  // offset 20
} // size 24, align 8

/// Reordered by hand, largest first: still repr(C), still a fixed contract, now without holes.
/// (It is also a DIFFERENT contract: every C and Java caller must change with it. See 16.1 §10.)
#[repr(C)]
pub struct MeridianTxnV2 {
    pub amount_cents: i64, // offset 0
    pub merchant_id: u32,  // offset 8
    pub currency: u16,     // offset 12
    pub flags: u8,         // offset 14, then 1 byte of padding
} // size 16, align 8

// The contract as compile-time assertions: CI fails if anyone changes the layout by accident.
const _: () = {
    assert!(size_of::<MeridianTxn>() == 24);
    assert!(align_of::<MeridianTxn>() == 8);
    assert!(offset_of!(MeridianTxn, amount_cents) == 8);
    assert!(offset_of!(MeridianTxn, currency) == 16);
    assert!(offset_of!(MeridianTxn, merchant_id) == 20);
};

fn main() {
    println!(
        "default repr : size {:2}  flags@{:<2} amount@{:<2} currency@{:<2} merchant@{:<2}",
        size_of::<TxnRust>(),
        offset_of!(TxnRust, flags),
        offset_of!(TxnRust, amount_cents),
        offset_of!(TxnRust, currency),
        offset_of!(TxnRust, merchant_id)
    );
    println!(
        "repr(C)      : size {:2}  flags@{:<2} amount@{:<2} currency@{:<2} merchant@{:<2}",
        size_of::<MeridianTxn>(),
        offset_of!(MeridianTxn, flags),
        offset_of!(MeridianTxn, amount_cents),
        offset_of!(MeridianTxn, currency),
        offset_of!(MeridianTxn, merchant_id)
    );
    println!(
        "repr(C) v2   : size {:2}  flags@{:<2} amount@{:<2} currency@{:<2} merchant@{:<2}",
        size_of::<MeridianTxnV2>(),
        offset_of!(MeridianTxnV2, flags),
        offset_of!(MeridianTxnV2, amount_cents),
        offset_of!(MeridianTxnV2, currency),
        offset_of!(MeridianTxnV2, merchant_id)
    );
}
