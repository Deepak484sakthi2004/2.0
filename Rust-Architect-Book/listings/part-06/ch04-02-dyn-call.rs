// verify: release build
pub trait Shape {
    fn area(&self) -> f64;
}

pub struct Circle {
    pub r: f64,
}

pub struct Square {
    pub side: f64,
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        std::f64::consts::PI * self.r * self.r
    }
}

impl Shape for Square {
    fn area(&self) -> f64 {
        self.side * self.side
    }
}

/// Dynamic dispatch: ONE copy of this function serves every Shape type; each call goes through a vtable.
#[inline(never)]
pub fn total_area_dyn(shapes: &[Box<dyn Shape>]) -> f64 {
    let mut total = 0.0;
    for s in shapes {
        total += s.area();
    }
    total
}

/// Static dispatch: one copy PER concrete T, with area() inlined into the loop.
#[inline(never)]
pub fn total_area_static<T: Shape>(shapes: &[T]) -> f64 {
    let mut total = 0.0;
    for s in shapes {
        total += s.area();
    }
    total
}

pub fn circles(c: &[Circle]) -> f64 {
    total_area_static(c)
}

pub fn squares(s: &[Square]) -> f64 {
    total_area_static(s)
}
