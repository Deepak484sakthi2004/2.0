// verify: release build
pub trait Shape {
    fn area(&self) -> f64;
    fn name(&self) -> &str;
}

/// No destructor needed (no drop glue).
pub struct Circle {
    pub r: f64,
}

/// Owns a String, so dropping it must free memory (has drop glue).
pub struct Label {
    pub text: String,
    pub w: f64,
    pub h: f64,
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.r * self.r
    }
    fn name(&self) -> &str {
        "circle"
    }
}

impl Shape for Label {
    fn area(&self) -> f64 {
        self.w * self.h
    }
    fn name(&self) -> &str {
        &self.text
    }
}

/// Each unsizing coercion Box<Concrete> -> Box<dyn Shape> pairs the data pointer with a static vtable.
#[inline(never)]
pub fn circle(r: f64) -> Box<dyn Shape> {
    Box::new(Circle { r })
}

#[inline(never)]
pub fn label(text: String) -> Box<dyn Shape> {
    Box::new(Label { text, w: 2.0, h: 1.0 })
}
