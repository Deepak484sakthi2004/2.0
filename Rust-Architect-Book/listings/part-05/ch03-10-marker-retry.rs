// verify: debug ok
use std::cell::Cell;

/// Marker trait: "sending this request twice has the same effect as sending it once".
/// No methods. Implementing it is a claim that reviewers must check.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not marked Idempotent, so it must not be retried automatically",
    note = "retrying a non-idempotent request can double-apply it; add an idempotency key and implement `Idempotent`"
)]
pub trait Idempotent {}

pub trait Request {
    fn send(&self, attempt: u32) -> Result<String, String>;
}

pub struct GetBalance {
    pub account: u64,
}
impl Request for GetBalance {
    fn send(&self, attempt: u32) -> Result<String, String> {
        if attempt < 3 { Err(format!("timeout (attempt {attempt})")) } else { Ok(format!("balance of {} = 1200", self.account)) }
    }
}
impl Idempotent for GetBalance {}

/// A charge becomes idempotent only when it carries an idempotency key the server deduplicates on.
pub struct ChargeWithKey {
    pub cents: i64,
    pub key: &'static str,
    pub applied: Cell<u32>,
}
impl Request for ChargeWithKey {
    fn send(&self, attempt: u32) -> Result<String, String> {
        if self.applied.get() == 0 {
            self.applied.set(1); // the server applies the charge once and remembers the key
        }
        if attempt < 2 { Err(format!("timeout (attempt {attempt})")) } else { Ok(format!("charged {} once (key {})", self.cents, self.key)) }
    }
}
impl Idempotent for ChargeWithKey {}

/// Only idempotent requests may be retried: the bound is the policy.
pub fn with_retries<R: Request + Idempotent>(req: &R, max: u32) -> Result<String, String> {
    let mut last = String::new();
    for attempt in 1..=max {
        match req.send(attempt) {
            Ok(v) => return Ok(v),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn main() {
    println!("{:?}", with_retries(&GetBalance { account: 7 }, 5));
    let charge = ChargeWithKey { cents: 4_999, key: "ord-42", applied: Cell::new(0) };
    println!("{:?} (applied {} time)", with_retries(&charge, 5), charge.applied.get());
    println!("size_of::<GetBalance>() = {}", std::mem::size_of::<GetBalance>());
}
