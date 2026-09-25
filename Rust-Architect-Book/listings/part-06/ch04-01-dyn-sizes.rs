// verify: debug ok
use std::mem::{align_of_val, size_of, size_of_val};
use std::rc::Rc;
use std::sync::Arc;

trait Shape {
    fn area(&self) -> f64;
    fn name(&self) -> &'static str;
}

struct Circle {
    r: f64,
}
struct Rect {
    w: f64,
    h: f64,
}
struct Tag(u8);

impl Shape for Circle {
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.r * self.r
    }
    fn name(&self) -> &'static str {
        "circle"
    }
}
impl Shape for Rect {
    fn area(&self) -> f64 {
        self.w * self.h
    }
    fn name(&self) -> &'static str {
        "rect"
    }
}
impl Shape for Tag {
    fn area(&self) -> f64 {
        0.0
    }
    fn name(&self) -> &'static str {
        if self.0 > 0 { "tag" } else { "empty-tag" }
    }
}

fn main() {
    println!("{:<30}{:>3} bytes", "&Circle", size_of::<&Circle>());
    println!("{:<30}{:>3} bytes", "&[u8]", size_of::<&[u8]>());
    println!("{:<30}{:>3} bytes", "&dyn Shape", size_of::<&dyn Shape>());
    println!("{:<30}{:>3} bytes", "&mut dyn Shape", size_of::<&mut dyn Shape>());
    println!("{:<30}{:>3} bytes", "*const dyn Shape", size_of::<*const dyn Shape>());
    println!("{:<30}{:>3} bytes", "Box<dyn Shape>", size_of::<Box<dyn Shape>>());
    println!("{:<30}{:>3} bytes", "Option<Box<dyn Shape>>", size_of::<Option<Box<dyn Shape>>>());
    println!("{:<30}{:>3} bytes", "Rc<dyn Shape>", size_of::<Rc<dyn Shape>>());
    println!("{:<30}{:>3} bytes", "Arc<dyn Shape + Send + Sync>", size_of::<Arc<dyn Shape + Send + Sync>>());

    // A fat pointer's first word is the ordinary data pointer.
    let c = Circle { r: 1.0 };
    let fat: &dyn Shape = &c;
    let data = fat as *const dyn Shape as *const ();
    println!("data half == &c: {}", data == &c as *const Circle as *const ());

    // size_of_val / align_of_val on a dyn value read the size and align entries of the vtable.
    let shapes: Vec<Box<dyn Shape>> =
        vec![Box::new(Circle { r: 1.0 }), Box::new(Rect { w: 2.0, h: 3.0 }), Box::new(Tag(7))];
    for s in &shapes {
        println!(
            "{:<7} area={:>6.3}  size_of_val={:>2}  align_of_val={}",
            s.name(),
            s.area(),
            size_of_val(&**s),
            align_of_val(&**s)
        );
    }
}
