// verify: debug ok
// Answer-key check for Chapter 2.6's "predict the size" exercise.
#![allow(dead_code)]
use std::mem::size_of;

enum E {
    A(u32),
    B(u16),
    C,
}

fn main() {
    println!("Option<Option<bool>> = {}", size_of::<Option<Option<bool>>>());
    println!("Result<u32, ()>      = {}", size_of::<Result<u32, ()>>());
    println!("Option<(u8, bool)>   = {}", size_of::<Option<(u8, bool)>>());
    println!("E {{ A(u32), B(u16), C }} = {}", size_of::<E>());
    println!("Option<Box<str>>     = {}", size_of::<Option<Box<str>>>());
}
