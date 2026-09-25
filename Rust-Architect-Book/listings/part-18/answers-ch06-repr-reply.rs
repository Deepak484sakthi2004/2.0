// verify: debug+nightly error:layout_of
// Answer-key check for Chapter 18.6's debugging exercise: with #[repr(C, u32)] the tag is a u32 at
// offset 0 and every payload starts at offset 4, a layout a C++ reader can declare.
#![feature(rustc_attrs)]
#![allow(internal_features, dead_code)]

#[rustc_dump_layout(debug)]
#[repr(C, u32)]
enum Reply {
    Ok(u8),
    Value(u32),
}

fn main() {}
