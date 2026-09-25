// verify: debug ok
mod refund {
    use std::marker::PhantomData;

    // Two independent "has it been set?" flags, each a type parameter.
    pub struct Missing;
    pub struct Set;

    #[derive(Debug)]
    pub struct RefundRequest {
        pub payment_id: u64,
        pub cents: i64,
        pub reason: String,
    }

    pub struct RefundBuilder<P, A> {
        payment_id: u64,
        cents: i64,
        reason: String,
        _flags: PhantomData<(P, A)>,
    }

    impl RefundBuilder<Missing, Missing> {
        pub fn new() -> Self {
            RefundBuilder { payment_id: 0, cents: 0, reason: String::new(), _flags: PhantomData }
        }
    }

    impl<A> RefundBuilder<Missing, A> {
        pub fn payment(self, id: u64) -> RefundBuilder<Set, A> {
            RefundBuilder { payment_id: id, cents: self.cents, reason: self.reason, _flags: PhantomData }
        }
    }

    impl<P> RefundBuilder<P, Missing> {
        pub fn amount(self, cents: i64) -> RefundBuilder<P, Set> {
            RefundBuilder { payment_id: self.payment_id, cents, reason: self.reason, _flags: PhantomData }
        }
    }

    impl<P, A> RefundBuilder<P, A> {
        /// Optional field: allowed in any state, does not change the type.
        pub fn reason(mut self, r: &str) -> Self {
            self.reason = r.to_string();
            self
        }
    }

    impl RefundBuilder<Set, Set> {
        /// `build` exists only when both required fields have been provided.
        pub fn build(self) -> RefundRequest {
            RefundRequest { payment_id: self.payment_id, cents: self.cents, reason: self.reason }
        }
    }
}

use refund::RefundBuilder;

fn main() {
    let r = RefundBuilder::new().amount(1_200).reason("damaged item").payment(42).build();
    println!("refund {} cents on payment #{} ({:?})", r.cents, r.payment_id, r.reason);
    let r2 = RefundBuilder::new().payment(43).amount(150).build();
    println!("refund {} cents on payment #{} ({:?})", r2.cents, r2.payment_id, r2.reason);
}
