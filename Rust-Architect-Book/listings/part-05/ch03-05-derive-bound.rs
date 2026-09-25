// verify: debug error:E0382
use std::marker::PhantomData;

// The first version of Id<T>: derive everything, as you would for a plain struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id<T> {
    raw: u64,
    _entity: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    pub fn new(raw: u64) -> Self {
        Id { raw, _entity: PhantomData }
    }
}

pub struct Order {
    pub cents: i64, // not Copy: owns heap data in the real system
    pub lines: Vec<String>,
}

fn audit(id: Id<Order>) {
    println!("audit {}", id.raw);
}

fn main() {
    let id = Id::<Order>::new(42);
    audit(id);
    audit(id); // Id<Order> is "just a u64"... isn't it Copy?
}
