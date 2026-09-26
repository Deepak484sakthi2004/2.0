// verify: release build
// For tools/emit.ps1 (asm and llvm-ir, release): the same two structs under both conventions,
// plus a caller for each, to see who copies what.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Triple {
    pub a: u64,
    pub b: u64,
    pub c: u64,
}

#[inline(never)]
pub fn rust_norm(p: Point) -> f64 {
    p.x * p.x + p.y * p.y
}

#[inline(never)]
pub extern "C" fn c_norm(p: Point) -> f64 {
    p.x * p.x + p.y * p.y
}

#[inline(never)]
pub fn rust_sum(t: Triple) -> u64 {
    t.a + t.b + t.c
}

#[inline(never)]
pub extern "C" fn c_sum(t: Triple) -> u64 {
    t.a + t.b + t.c
}

pub fn call_rust_sum(t: &Triple) -> u64 {
    rust_sum(*t)
}

pub fn call_c_sum(t: &Triple) -> u64 {
    c_sum(*t)
}
