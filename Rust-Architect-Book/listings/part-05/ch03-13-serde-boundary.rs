// verify: debug ok
use serde::{Deserialize, Serialize};

/// On the wire, just a number. In the program, not interchangeable with any other number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OrderId(u64);

/// Deserialization goes THROUGH the parser: an out-of-range value never becomes a BasisPoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct BasisPoints(u16);

impl TryFrom<u16> for BasisPoints {
    type Error = String;
    fn try_from(v: u16) -> Result<Self, String> {
        if v <= 10_000 { Ok(BasisPoints(v)) } else { Err(format!("{v} bps is more than 100%")) }
    }
}

impl From<BasisPoints> for u16 {
    fn from(b: BasisPoints) -> u16 {
        b.0
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscountRequest {
    pub order: OrderId,
    pub discount: BasisPoints,
}

fn main() {
    let ok: DiscountRequest = serde_json::from_str(r#"{"order": 42, "discount": 250}"#).unwrap();
    println!("parsed: {ok:?}");
    println!("serialized: {}", serde_json::to_string(&ok).unwrap());

    let bad = serde_json::from_str::<DiscountRequest>(r#"{"order": 43, "discount": 15000}"#);
    println!("rejected: {}", bad.unwrap_err());
}
