// verify: debug error:E0277
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not marked Idempotent, so it must not be retried automatically",
    note = "retrying a non-idempotent request can double-apply it; add an idempotency key and implement `Idempotent`"
)]
pub trait Idempotent {}

pub trait Request {
    fn send(&self, attempt: u32) -> Result<String, String>;
}

pub struct Charge {
    pub cents: i64, // no idempotency key
}
impl Request for Charge {
    fn send(&self, _attempt: u32) -> Result<String, String> {
        Ok(format!("charged {}", self.cents))
    }
}

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
    println!("{:?}", with_retries(&Charge { cents: 4_999 }, 5));
}
