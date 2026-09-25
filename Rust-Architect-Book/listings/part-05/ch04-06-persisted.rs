// verify: debug ok
mod payment {
    use std::marker::PhantomData;

    pub enum Pending {}
    pub enum Authorized {}
    pub enum Captured {}
    pub enum Refunded {}
    pub enum Failed {}

    pub struct Payment<S> {
        id: u64,
        cents: i64,
        _state: PhantomData<S>,
    }

    fn raw<S>(id: u64, cents: i64) -> Payment<S> {
        Payment { id, cents, _state: PhantomData }
    }

    fn retag<S, T>(p: Payment<S>) -> Payment<T> {
        Payment { id: p.id, cents: p.cents, _state: PhantomData }
    }

    impl Payment<Pending> {
        pub fn authorize(self) -> Payment<Authorized> { retag(self) }
        pub fn fail(self) -> Payment<Failed> { retag(self) }
    }
    impl Payment<Authorized> {
        pub fn capture(self) -> Payment<Captured> { retag(self) }
        pub fn fail(self) -> Payment<Failed> { retag(self) }
    }
    impl Payment<Captured> {
        pub fn refund(self) -> Payment<Refunded> { retag(self) }
    }

    /// The run-time view: what a database row or a message can hold.
    pub enum AnyPayment {
        Pending(Payment<Pending>),
        Authorized(Payment<Authorized>),
        Captured(Payment<Captured>),
        Refunded(Payment<Refunded>),
        Failed(Payment<Failed>),
    }

    #[derive(Debug, Clone, Copy)]
    pub enum Event {
        Authorize,
        Capture,
        Refund,
        Fail,
    }

    /// A row as stored: the state is a string column, validated here, once ("parse, don't validate").
    pub struct Row {
        pub id: u64,
        pub cents: i64,
        pub state: &'static str,
    }

    impl AnyPayment {
        pub fn from_row(r: Row) -> Result<AnyPayment, String> {
            Ok(match r.state {
                "PENDING" => AnyPayment::Pending(raw(r.id, r.cents)),
                "AUTHORIZED" => AnyPayment::Authorized(raw(r.id, r.cents)),
                "CAPTURED" => AnyPayment::Captured(raw(r.id, r.cents)),
                "REFUNDED" => AnyPayment::Refunded(raw(r.id, r.cents)),
                "FAILED" => AnyPayment::Failed(raw(r.id, r.cents)),
                other => return Err(format!("row {}: unknown state {other:?}", r.id)),
            })
        }

        pub fn id(&self) -> u64 {
            match self {
                AnyPayment::Pending(p) => p.id,
                AnyPayment::Authorized(p) => p.id,
                AnyPayment::Captured(p) => p.id,
                AnyPayment::Refunded(p) => p.id,
                AnyPayment::Failed(p) => p.id,
            }
        }

        pub fn state_column(&self) -> &'static str {
            match self {
                AnyPayment::Pending(_) => "PENDING",
                AnyPayment::Authorized(_) => "AUTHORIZED",
                AnyPayment::Captured(_) => "CAPTURED",
                AnyPayment::Refunded(_) => "REFUNDED",
                AnyPayment::Failed(_) => "FAILED",
            }
        }

        /// The dynamic edge: an event from a queue. Every legal pair delegates to a typed transition,
        /// so the transition *logic* exists once, in the typed API.
        pub fn apply(self, e: Event) -> Result<AnyPayment, (AnyPayment, String)> {
            use AnyPayment as A;
            use Event as E;
            Ok(match (self, e) {
                (A::Pending(p), E::Authorize) => A::Authorized(p.authorize()),
                (A::Pending(p), E::Fail) => A::Failed(p.fail()),
                (A::Authorized(p), E::Capture) => A::Captured(p.capture()),
                (A::Authorized(p), E::Fail) => A::Failed(p.fail()),
                (A::Captured(p), E::Refund) => A::Refunded(p.refund()),
                (s, e) => {
                    let msg = format!("invalid transition {} + {e:?}", s.state_column());
                    return Err((s, msg));
                }
            })
        }
    }
}

use payment::{AnyPayment, Event, Row};

fn main() {
    // (row as loaded from the database, event from the queue)
    let work = [
        (Row { id: 42, cents: 4_999, state: "AUTHORIZED" }, Event::Capture),
        (Row { id: 43, cents: 150, state: "PENDING" }, Event::Refund),
        (Row { id: 44, cents: 10, state: "SETTLED" }, Event::Capture),
        (Row { id: 45, cents: 800, state: "CAPTURED" }, Event::Refund),
        (Row { id: 46, cents: 75, state: "PENDING" }, Event::Fail),
        (Row { id: 47, cents: 99, state: "REFUNDED" }, Event::Authorize),
    ];
    for (row, event) in work {
        match AnyPayment::from_row(row) {
            Err(e) => println!("load error: {e}"),
            Ok(p) => {
                let before = p.state_column();
                match p.apply(event) {
                    Ok(p) => println!("#{} {before} --{event:?}--> {}", p.id(), p.state_column()),
                    Err((p, e)) => println!("#{} rejected: {e}; still {}", p.id(), p.state_column()),
                }
            }
        }
    }
    println!("size_of::<AnyPayment>() = {}", std::mem::size_of::<AnyPayment>());
}
