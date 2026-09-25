// verify: debug error:E0277
use std::marker::PhantomData;

pub enum Pending {} // uninhabited state marker
pub enum Authorized {}

#[derive(Debug)]
pub struct Payment<S> {
    id: u64,
    cents: i64,
    _state: PhantomData<S>,
}

impl Payment<Pending> {
    pub fn new(id: u64, cents: i64) -> Self {
        Payment { id, cents, _state: PhantomData }
    }
    pub fn authorize(self) -> Payment<Authorized> {
        Payment { id: self.id, cents: self.cents, _state: PhantomData }
    }
}

fn main() {
    let p = Payment::new(42, 4_999);
    tracing_like_log(&p); // log the payment before authorizing it
    let _a = p.authorize();
}

fn tracing_like_log<T: std::fmt::Debug>(v: &T) {
    println!("{v:?}");
}
