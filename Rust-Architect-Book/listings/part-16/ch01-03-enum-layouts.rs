// verify: debug ok
// verify: debug miri-ok
// RFC 2195: enums with fields get a DEFINED layout under repr(C, u32) and repr(u32),
// and the two layouts differ. Each is spelled out below as the struct/union it is defined to be.
#![allow(dead_code)]
use std::mem::{offset_of, size_of};

#[repr(C, u32)]
#[derive(Clone, Copy)]
pub enum DecisionC {
    Allow,
    Review(u8), // risk score
    Block(u64), // id of the rule that blocked
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum DecisionU32 {
    Allow,
    Review(u8),
    Block(u64),
}

// repr(C, u32) is defined as: a repr(C) struct { tag: u32, payload: repr(C) union of the variants }.
#[repr(C)]
struct DecisionCRepr {
    tag: u32,
    payload: DecisionCPayload, // the union is 8-aligned (it holds a u64), so it starts at offset 8
}
#[repr(C)]
#[derive(Clone, Copy)]
union DecisionCPayload {
    review: u8,
    block: u64,
}

// repr(u32) is defined as: a repr(C) union of repr(C) structs, each one starting with the u32 tag.
#[repr(C)]
struct ReviewU32 {
    tag: u32,
    risk: u8, // right after the tag: offset 4
}
#[repr(C)]
struct BlockU32 {
    tag: u32,
    rule: u64, // aligned to 8: offset 8
}

fn main() {
    println!("size: repr(C, u32) = {}, repr(u32) = {}", size_of::<DecisionC>(), size_of::<DecisionU32>());
    println!("repr(C, u32): Review payload at offset {}", offset_of!(DecisionCRepr, payload));
    println!("repr(u32)   : Review payload at offset {}", offset_of!(ReviewU32, risk));
    println!("repr(u32)   : Block  payload at offset {}", offset_of!(BlockU32, rule));

    // The equivalence is a language guarantee, so reading through the documented shape is defined.
    let d = DecisionC::Review(70);
    // SAFETY: RFC 2195 defines repr(C, u32) enums to have exactly DecisionCRepr's layout; tag 1 is
    // `Review`, so the `review` field of the union is the initialized one.
    let (tag, risk) = unsafe {
        let r = &*(&d as *const DecisionC as *const DecisionCRepr);
        (r.tag, r.payload.review)
    };
    println!("DecisionC::Review(70) seen as C: tag={tag} risk={risk}");

    let d = DecisionU32::Block(4_017);
    // SAFETY: repr(u32) defines the Block variant as BlockU32 (tag 2, then the u64 at offset 8).
    let (tag, rule) = unsafe {
        let r = &*(&d as *const DecisionU32 as *const BlockU32);
        (r.tag, r.rule)
    };
    println!("DecisionU32::Block(4017) seen as C: tag={tag} rule={rule}");
}
