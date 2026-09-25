// verify: debug error:E0382
use std::marker::PhantomData;

pub enum Authorized {}
pub enum Captured {}

pub struct Payment<S> {
    id: u64,
    _state: PhantomData<S>,
}

impl Payment<Authorized> {
    pub fn capture(self) -> Payment<Captured> {
        Payment { id: self.id, _state: PhantomData }
    }
}

fn main() {
    let auth: Payment<Authorized> = Payment { id: 42, _state: PhantomData };
    let first = auth.capture();
    // A retry loop that captures the same authorization again (a double charge):
    let second = auth.capture();
    println!("{} {}", first.id, second.id);
}
