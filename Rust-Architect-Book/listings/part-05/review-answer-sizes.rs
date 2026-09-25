// verify: debug ok
// Sizes quoted in Appendix A (Part V): exercises of Chapter 5.2 and interview-mode question 5.
#![allow(dead_code)]
use std::mem::{offset_of, size_of};
use std::num::NonZeroU32;

struct A {
    a: u8,
    b: u16,
    c: u8,
}

#[repr(C)]
struct AC {
    a: u8,
    b: u16,
    c: u8,
}

#[repr(C)]
struct Declared {
    flag: bool,
    ts: u64,
    code: u16,
    qty: u32,
    kind: u8,
}

#[repr(C)]
struct HandOrdered {
    ts: u64,
    qty: u32,
    code: u16,
    flag: bool,
    kind: u8,
}

struct Default5 {
    flag: bool,
    ts: u64,
    code: u16,
    qty: u32,
    kind: u8,
}

enum E {
    A(char),
    B,
    C,
    D,
}

fn main() {
    println!("A (default) = {}, offsets a={} b={} c={}", size_of::<A>(), offset_of!(A, a), offset_of!(A, b), offset_of!(A, c));
    println!("A (repr C)  = {}, offsets a={} b={} c={}", size_of::<AC>(), offset_of!(AC, a), offset_of!(AC, b), offset_of!(AC, c));
    println!("Declared (repr C) = {}, HandOrdered (repr C) = {}, default repr = {}",
        size_of::<Declared>(), size_of::<HandOrdered>(), size_of::<Default5>());
    println!("Option<(NonZeroU32, bool)> = {}", size_of::<Option<(NonZeroU32, bool)>>());
    println!("Result<&u8, u8> = {}", size_of::<Result<&u8, u8>>());
    println!("Option<Result<(), bool>> = {}", size_of::<Option<Result<(), bool>>>());
    println!("E {{ A(char), B, C, D }} = {}", size_of::<E>());
    println!("Option<&u8> = {}, Option<Option<&u8>> = {}", size_of::<Option<&u8>>(), size_of::<Option<Option<&u8>>>());
}
