// verify: debug+nightly error:vtable
// Answer-key check for Chapter 18.6's beginner exercise: the vtable of a trait with a supertrait.
#![feature(rustc_attrs)]
#![allow(internal_features, dead_code)]
use std::fmt::Debug;

trait Shape: Debug {
    fn area(&self) -> f64;
}

#[derive(Debug)]
struct Circle {
    r: f64,
}

#[rustc_dump_vtable]
impl Shape for Circle {
    fn area(&self) -> f64 {
        3.14 * self.r * self.r
    }
}

fn main() {}
