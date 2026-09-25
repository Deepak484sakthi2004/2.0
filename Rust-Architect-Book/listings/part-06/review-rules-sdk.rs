// verify: debug error:E0038
// verify: debug error:E0117
use std::fmt;

pub struct Txn {
    pub amount_cents: i64,
    pub country: &'static str,
}

/// Meridian fraud SDK v0.1: teams implement Rule; the engine loads rules chosen by config.
pub trait Rule: Clone {
    fn id(&self) -> &str;
    fn score(&self, txn: &Txn) -> u32;
    fn explain<W: fmt::Write>(&self, txn: &Txn, out: &mut W) -> fmt::Result;

    /// Scores at or above this block the payment.
    fn block_at(&self) -> u32 {
        0
    }
}

#[derive(Clone)]
pub struct LargeAmount {
    pub over_cents: i64,
}

impl Rule for LargeAmount {
    fn id(&self) -> &str {
        "large-amount"
    }
    fn score(&self, txn: &Txn) -> u32 {
        if txn.amount_cents > self.over_cents { 60 } else { 0 }
    }
    fn explain<W: fmt::Write>(&self, txn: &Txn, out: &mut W) -> fmt::Result {
        write!(out, "{} > {} in {}", txn.amount_cents, self.over_cents, txn.country)
    }
}

impl fmt::Display for Vec<Box<dyn Rule>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in self {
            write!(f, "{} ", r.id())?;
        }
        Ok(())
    }
}

pub struct Engine {
    rules: Vec<Box<dyn Rule>>,
}

impl Engine {
    pub fn blocked(&self, txn: &Txn) -> bool {
        self.rules.iter().any(|r| r.score(txn) >= r.block_at())
    }
}

fn main() {
    let engine = Engine { rules: vec![Box::new(LargeAmount { over_cents: 500_000 })] };
    let small = Txn { amount_cents: 1_200, country: "IE" };
    println!("rules: {}", engine.rules);
    println!("blocked a 12.00 payment? {}", engine.blocked(&small));
}
