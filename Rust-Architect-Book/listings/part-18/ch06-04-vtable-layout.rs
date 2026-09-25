// verify: debug+nightly error:MetadataDropInPlace
// Nightly-only: the vtable rustc builds for `Circle as Shape` (Chapter 6.4 read it in LLVM IR),
// and the layout it computed for an enum (Chapter 5.2 measured it with size_of).
#![feature(rustc_attrs)]
#![allow(internal_features, dead_code)]

trait Shape {
    fn area(&self) -> f64;
    fn name(&self) -> &'static str {
        "shape"
    }
}

struct Circle {
    r: f64,
}

#[rustc_dump_vtable]
impl Shape for Circle {
    fn area(&self) -> f64 {
        3.14 * self.r * self.r
    }
}

#[rustc_dump_layout(debug)]
enum Reply {
    Ok(u8),
    Value(u32),
}

fn main() {}
