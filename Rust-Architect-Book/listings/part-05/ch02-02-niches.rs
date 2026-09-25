// verify: debug ok
#![allow(dead_code)]
use std::mem::size_of;
use std::num::{NonZeroU32, NonZeroU64};

enum Tri {
    Yes,
    No,
    Unknown,
}

enum Shape {
    Circle { r: f64 },
    Rect { w: f64, h: f64 },
    Empty,
}

enum Payload {
    Small(u8),
    Big(Box<[u8; 1024]>),
}

// Two other variants to encode, but a Box has only ONE invalid value (null).
enum Msg {
    Big(Box<u64>, u64),
    Small(u32),
    Empty,
}

// Only ONE other variant: the null pointer value is enough.
enum Msg2 {
    Big(Box<u64>, u64),
    Small(u32),
}

enum TwoU64 {
    A(u64),
    B(u64),
}

fn row(label: &str, size: usize) {
    println!("{label:<42} {size:>3}");
}

fn main() {
    row("u32", size_of::<u32>());
    row("Option<u32>", size_of::<Option<u32>>());
    row("NonZeroU32", size_of::<NonZeroU32>());
    row("Option<NonZeroU32>", size_of::<Option<NonZeroU32>>());
    row("Option<NonZeroU64>", size_of::<Option<NonZeroU64>>());
    row("Option<Option<NonZeroU64>>", size_of::<Option<Option<NonZeroU64>>>());
    row("char / Option<char>", size_of::<Option<char>>());
    row("Option<Option<Option<bool>>>", size_of::<Option<Option<Option<bool>>>>());
    row("Tri / Option<Tri>", size_of::<Option<Tri>>());
    row("Shape / Option<Shape>", size_of::<Option<Shape>>());
    row("Payload", size_of::<Payload>());
    row("Msg { Big(Box, u64), Small(u32), Empty }", size_of::<Msg>());
    row("Msg2 { Big(Box, u64), Small(u32) }", size_of::<Msg2>());
    row("TwoU64 { A(u64), B(u64) }", size_of::<TwoU64>());
    row("Result<u32, NonZeroU32>", size_of::<Result<u32, NonZeroU32>>());
    row("Result<(), Box<str>>", size_of::<Result<(), Box<str>>>());
    row("Option<Vec<u8>>", size_of::<Option<Vec<u8>>>());
    row("Option<String> vs String", size_of::<Option<String>>() - size_of::<String>());
}
