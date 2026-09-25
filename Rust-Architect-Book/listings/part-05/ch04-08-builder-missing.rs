// verify: debug error:E0599
use std::marker::PhantomData;

pub struct Missing;
pub struct Set;

pub struct RefundBuilder<P, A> {
    payment_id: u64,
    cents: i64,
    _flags: PhantomData<(P, A)>,
}

impl RefundBuilder<Missing, Missing> {
    pub fn new() -> Self {
        RefundBuilder { payment_id: 0, cents: 0, _flags: PhantomData }
    }
}
impl<A> RefundBuilder<Missing, A> {
    pub fn payment(self, id: u64) -> RefundBuilder<Set, A> {
        RefundBuilder { payment_id: id, cents: self.cents, _flags: PhantomData }
    }
}
impl<P> RefundBuilder<P, Missing> {
    pub fn amount(self, cents: i64) -> RefundBuilder<P, Set> {
        RefundBuilder { payment_id: self.payment_id, cents, _flags: PhantomData }
    }
}
impl RefundBuilder<Set, Set> {
    pub fn build(self) -> (u64, i64) {
        (self.payment_id, self.cents)
    }
}

fn main() {
    // The amount was forgotten:
    let r = RefundBuilder::new().payment(42).build();
    println!("{r:?}");
}
