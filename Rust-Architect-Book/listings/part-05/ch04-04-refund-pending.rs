// verify: debug error:E0599
use std::marker::PhantomData;

pub enum Pending {}
pub enum Authorized {}
pub enum Captured {}
pub enum Refunded {}

pub struct Payment<S> {
    id: u64,
    _state: PhantomData<S>,
}

impl Payment<Pending> {
    pub fn new(id: u64) -> Self {
        Payment { id, _state: PhantomData }
    }
    pub fn authorize(self) -> Payment<Authorized> {
        Payment { id: self.id, _state: PhantomData }
    }
}

impl Payment<Authorized> {
    pub fn capture(self) -> Payment<Captured> {
        Payment { id: self.id, _state: PhantomData }
    }
}

impl Payment<Captured> {
    pub fn refund(self) -> Payment<Refunded> {
        Payment { id: self.id, _state: PhantomData }
    }
}

fn main() {
    let p = Payment::new(42);
    // Chapter 2.5 returned Err("invalid transition Pending + Refund") at run time. Here:
    let r = p.refund();
    println!("{}", r.id);
}
