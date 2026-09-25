// verify: debug error:E0308
use std::marker::PhantomData;

#[derive(Debug)]
pub struct Id<T> {
    raw: u64,
    _entity: PhantomData<fn() -> T>,
}
impl<T> Id<T> {
    pub fn new(raw: u64) -> Self {
        Id { raw, _entity: PhantomData }
    }
}

pub struct Tenant;
pub struct Order;

fn cancel_order(id: Id<Order>) -> u64 {
    id.raw
}

fn main() {
    let tenant = Id::<Tenant>::new(7);
    // Both are u64 underneath. With raw u64 parameters this call compiles and cancels order #7.
    println!("{}", cancel_order(tenant));
}
