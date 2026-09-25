// verify: release build
pub trait Fee {
    fn fee(&self, amount: i64) -> i64;
}

#[derive(Clone, Copy)]
pub struct Percent {
    pub bps: i64,
}
#[derive(Clone, Copy)]
pub struct Flat {
    pub cents: i64,
}
#[derive(Clone, Copy)]
pub struct Tiered {
    pub threshold: i64,
    pub low_bps: i64,
    pub high_bps: i64,
}

impl Fee for Percent {
    fn fee(&self, amount: i64) -> i64 {
        amount * self.bps / 10_000
    }
}
impl Fee for Flat {
    fn fee(&self, _amount: i64) -> i64 {
        self.cents
    }
}
impl Fee for Tiered {
    fn fee(&self, amount: i64) -> i64 {
        let bps = if amount < self.threshold { self.low_bps } else { self.high_bps };
        amount * bps / 10_000
    }
}

#[derive(Clone, Copy)]
pub enum FeeKind {
    Percent(Percent),
    Flat(Flat),
    Tiered(Tiered),
}

impl Fee for FeeKind {
    fn fee(&self, amount: i64) -> i64 {
        match self {
            FeeKind::Percent(p) => p.fee(amount),
            FeeKind::Flat(f) => f.fee(amount),
            FeeKind::Tiered(t) => t.fee(amount),
        }
    }
}

/// Closed set: the match is inlined into the loop.
#[inline(never)]
pub fn sum_enum(fees: &[FeeKind], amount: i64) -> i64 {
    fees.iter().map(|f| f.fee(amount)).sum()
}

/// Open set: one indirect call per element.
#[inline(never)]
pub fn sum_dyn(fees: &[Box<dyn Fee>], amount: i64) -> i64 {
    fees.iter().map(|f| f.fee(amount)).sum()
}
