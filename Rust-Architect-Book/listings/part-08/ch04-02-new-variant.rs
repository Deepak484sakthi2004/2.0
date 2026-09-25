// verify: debug error:E0004
#[derive(Debug)]
pub enum PaymentError {
    InvalidAmount(i64),
    Declined { reason: &'static str },
    ProcessorTimeout,
    FraudSuspected { score: u8 }, // added in a later release
}

impl PaymentError {
    pub fn http_status(&self) -> u16 {
        match self {
            Self::InvalidAmount(_) => 400,
            Self::Declined { .. } => 402,
            Self::ProcessorTimeout => 504,
        }
    }
}

fn main() {
    println!("{}", PaymentError::FraudSuspected { score: 97 }.http_status());
}
