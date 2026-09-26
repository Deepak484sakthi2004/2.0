// verify: debug ok
// improper_ctypes_definitions checks what is passed BY VALUE. Behind a reference or a raw pointer it
// doesn't inspect the pointee's fields (a pointee may legitimately be opaque to C), so the same
// non-FFI-safe struct produces one warning here, not three. (Chapter 16.1's debugging exercise.)
#![allow(dead_code)]

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum ScoreMode {
    Normal = 0,
    Strict = 1,
}

#[repr(C)]
pub struct ScoreRequest {
    pub amount_cents: i64,
    pub mode: ScoreMode,
    pub override_block_at: Option<u32>,
}

#[unsafe(no_mangle)]
pub extern "C" fn meridian_score_req(req: ScoreRequest, out: &mut i32) -> i32 {
    *out = (req.amount_cents / 100) as i32;
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn meridian_score_req_ref(req: &ScoreRequest, out: &mut i32) -> i32 {
    *out = (req.amount_cents / 100) as i32;
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_score_req_ptr(req: *const ScoreRequest, out: *mut i32) -> i32 {
    // SAFETY: the caller's contract: both pointers valid (this listing only compiles it).
    unsafe { *out = ((*req).amount_cents / 100) as i32 };
    0
}

fn main() {
    let mut out = 0;
    let r = ScoreRequest { amount_cents: 25_000, mode: ScoreMode::Strict, override_block_at: None };
    meridian_score_req_ref(&r, &mut out);
    println!("by reference: {out}");
    meridian_score_req(r, &mut out);
    println!("by value: {out}");
}
