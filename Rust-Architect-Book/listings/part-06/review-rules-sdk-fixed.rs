// verify: debug ok
use std::fmt;
use std::sync::Arc;
use std::thread;

pub struct Txn {
    pub amount_cents: i64,
    pub country: &'static str,
}

/// Meridian fraud SDK v0.2. Dyn compatible, thread-safe, and policy-free: rules only SCORE.
pub trait Rule: Send + Sync {
    fn id(&self) -> &str;
    fn score(&self, txn: &Txn) -> u32;
    /// `&mut dyn fmt::Write` instead of a generic writer keeps the method in the vtable.
    fn explain(&self, txn: &Txn, out: &mut dyn fmt::Write) -> fmt::Result;
}

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
    fn explain(&self, txn: &Txn, out: &mut dyn fmt::Write) -> fmt::Result {
        write!(out, "{} > {}", txn.amount_cents, self.over_cents)
    }
}

pub struct RiskyCountry {
    pub countries: Vec<&'static str>,
}

impl Rule for RiskyCountry {
    fn id(&self) -> &str {
        "risky-country"
    }
    fn score(&self, txn: &Txn) -> u32 {
        if self.countries.contains(&txn.country) { 30 } else { 0 }
    }
    fn explain(&self, txn: &Txn, out: &mut dyn fmt::Write) -> fmt::Result {
        write!(out, "country {}", txn.country)
    }
}

/// The blocking threshold is POLICY: it belongs to the engine's config, not to each rule.
pub struct Engine {
    rules: Vec<Box<dyn Rule>>,
    block_at: u32,
}

/// Display for a LOCAL type instead of for Vec<Box<dyn Rule>>.
impl fmt::Display for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ids: Vec<&str> = self.rules.iter().map(|r| r.id()).collect();
        write!(f, "[{}] block_at={}", ids.join(", "), self.block_at)
    }
}

impl Engine {
    /// Returns the total score and, if blocked, an explanation.
    pub fn evaluate(&self, txn: &Txn) -> (u32, Option<String>) {
        let total: u32 = self.rules.iter().map(|r| r.score(txn)).sum();
        if total < self.block_at {
            return (total, None);
        }
        let mut why = String::new();
        for r in self.rules.iter().filter(|r| r.score(txn) > 0) {
            why.push_str(r.id());
            why.push_str(": ");
            r.explain(txn, &mut why).unwrap();
            why.push_str("; ");
        }
        (total, Some(why))
    }
}

fn main() {
    let engine = Arc::new(Engine {
        rules: vec![Box::new(LargeAmount { over_cents: 500_000 }), Box::new(RiskyCountry { countries: vec!["XX"] })],
        block_at: 80,
    });
    println!("engine: {engine}");

    let txns = [
        Txn { amount_cents: 1_200, country: "IE" },
        Txn { amount_cents: 750_000, country: "IE" },
        Txn { amount_cents: 750_000, country: "XX" },
    ];
    // The engine is shared by worker threads: `Rule: Send + Sync` makes Box<dyn Rule> shareable.
    let handles: Vec<_> = txns
        .into_iter()
        .map(|t| {
            let e = Arc::clone(&engine);
            thread::spawn(move || (t.amount_cents, t.country, e.evaluate(&t)))
        })
        .collect();
    for h in handles {
        let (amount, country, (score, blocked)) = h.join().unwrap();
        println!("{amount:>7} {country}: score {score:>3} -> {}", blocked.unwrap_or_else(|| "allow".into()));
    }
}
