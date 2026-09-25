// verify: debug ok
#![allow(dead_code)]
use std::mem::{align_of, size_of};

struct Unordered {
    a: u8,
    b: u64,
    c: u8,
} // default repr: the compiler may reorder fields

#[repr(C)]
struct COrdered {
    a: u8,
    b: u64,
    c: u8,
} // repr(C): declaration order, C padding rules

enum Shape {
    Circle { r: f64 },
    Rect { w: f64, h: f64 },
    Empty,
}

enum Message {
    Ping,
    Data([u8; 4096]),
}

enum MessageBoxed {
    Ping,
    Data(Box<[u8; 4096]>),
}

fn main() {
    println!("Unordered {{u8,u64,u8}}:     size {:>4}, align {}", size_of::<Unordered>(), align_of::<Unordered>());
    println!("repr(C)   {{u8,u64,u8}}:     size {:>4}, align {}", size_of::<COrdered>(), align_of::<COrdered>());
    println!("Shape (largest payload 16): size {:>4}", size_of::<Shape>());
    println!("Option<Shape>:              size {:>4}", size_of::<Option<Shape>>());
    println!("bool / Option<bool>:        size {:>4} / {}", size_of::<bool>(), size_of::<Option<bool>>());
    println!("Message (Data is 4 KiB):    size {:>4}", size_of::<Message>());
    println!("MessageBoxed:               size {:>4}", size_of::<MessageBoxed>());
    println!("Option<MessageBoxed>:       size {:>4}", size_of::<Option<MessageBoxed>>());
}
