// verify: debug error:E0080
// A layout assertion doing its job: someone added a field in the middle of a boundary struct.
#![allow(dead_code)]
use std::mem::{offset_of, size_of};

#[repr(C)]
pub struct MeridianTxn {
    pub flags: u8,
    pub amount_cents: i64,
    pub currency: u16,
    pub channel: u32, // new in this PR: "four bytes, what could it break?"
    pub merchant_id: u32,
}

const _: () = {
    assert!(size_of::<MeridianTxn>() == 24, "MeridianTxn size changed: bump MERIDIAN_ABI_VERSION");
    assert!(
        offset_of!(MeridianTxn, merchant_id) == 20,
        "MeridianTxn.merchant_id moved: update meridian_fraud.h and the Java MemoryLayout"
    );
};

fn main() {}
