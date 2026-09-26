// verify: debug+nightly error:OnStack
// verify: release+nightly error:Cast
// Nightly-only: #[rustc_abi(debug)] prints the calling convention rustc computed for each function.
// Same types, two conventions: `conv: Rust` (unspecified, may change) vs `conv: C` (the platform's C ABI).
#![feature(rustc_attrs)]
#![allow(internal_features, dead_code)]

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Point {
    x: f64,
    y: f64,
} // 16 bytes: two SSE eightbytes in the System V classification

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Triple {
    a: u64,
    b: u64,
    c: u64,
} // 24 bytes: larger than 16, class MEMORY in the System V classification

#[rustc_abi(debug)]
fn rust_norm(p: Point) -> f64 {
    p.x * p.x + p.y * p.y
}

#[rustc_abi(debug)]
extern "C" fn c_norm(p: Point) -> f64 {
    p.x * p.x + p.y * p.y
}

#[rustc_abi(debug)]
fn rust_sum(t: Triple) -> u64 {
    t.a + t.b + t.c
}

#[rustc_abi(debug)]
extern "C" fn c_sum(t: Triple) -> u64 {
    t.a + t.b + t.c
}

#[rustc_abi(debug)]
extern "C-unwind" fn c_unwind_sum(t: Triple) -> u64 {
    t.a + t.b + t.c
}

fn main() {}
