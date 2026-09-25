// verify: debug ok
// verify: release ok
// Part XVIII capstone: a small Meridian ledger client whose compiler artifacts you'll read.
use std::fmt;

macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !$cond {
            return Err($err);
        }
    };
}

#[derive(Debug)]
pub enum LedgerError {
    Overflow,
    Negative(i64),
    Closed,
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LedgerError::Overflow => write!(f, "balance overflow"),
            LedgerError::Negative(b) => write!(f, "would go negative: {b}"),
            LedgerError::Closed => write!(f, "account closed"),
        }
    }
}

pub trait Sink {
    fn write(&mut self, line: &str);
}

pub struct Stdout;

impl Sink for Stdout {
    fn write(&mut self, line: &str) {
        println!("  sink: {line}");
    }
}

pub struct Account {
    pub id: u64,
    pub balance: i64,
    pub open: bool,
}

pub struct Journal<'a> {
    sink: &'a mut dyn Sink,
    entries: u32,
}

impl Drop for Journal<'_> {
    fn drop(&mut self) {
        let line = format!("journal closed after {} entries", self.entries);
        self.sink.write(&line);
    }
}

#[inline(never)]
pub fn post(acct: &mut Account, delta: i64, journal: &mut Journal<'_>) -> Result<i64, LedgerError> {
    ensure!(acct.open, LedgerError::Closed);
    let next = acct.balance.checked_add(delta).ok_or(LedgerError::Overflow)?;
    ensure!(next >= 0, LedgerError::Negative(next));
    acct.balance = next;
    journal.entries += 1;
    journal.sink.write(&format!("acct {} -> {}", acct.id, next));
    Ok(next)
}

pub trait Currency {
    const MINOR: i64;
}
pub struct Eur;
pub struct Usd;
impl Currency for Eur {
    const MINOR: i64 = 100;
}
impl Currency for Usd {
    const MINOR: i64 = 100;
}

#[inline(never)]
pub fn to_major<C: Currency>(minor: i64) -> i64 {
    minor / C::MINOR
}

fn main() {
    let mut out = Stdout;
    let mut acct = Account { id: 7, balance: 1_000, open: true };
    {
        let mut j = Journal { sink: &mut out, entries: 0 };
        println!("{:?}", post(&mut acct, -300, &mut j));
        println!("{:?}", post(&mut acct, -900, &mut j));
    }
    out.write("done");
    let eur: fn(i64) -> i64 = to_major::<Eur>;
    let usd: fn(i64) -> i64 = to_major::<Usd>;
    println!("eur={} usd={} same fn? {}", eur(12_345), usd(12_345), std::ptr::fn_addr_eq(eur, usd));
}
