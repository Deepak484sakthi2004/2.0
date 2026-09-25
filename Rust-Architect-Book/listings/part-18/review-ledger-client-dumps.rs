// verify: release+nightly error:fn_abi_of
// Nightly-only companion to the capstone: the ABI rustc computes for `post` (release), and the
// vtable for `Stdout as Sink`.
#![feature(rustc_attrs)]
#![allow(internal_features, dead_code)]

pub trait Sink {
    fn write(&mut self, line: &str);
}

pub struct Stdout;

#[rustc_dump_vtable]
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

#[rustc_abi(debug)]
fn post(acct: &mut Account, delta: i64, journal: &mut Journal<'_>) -> Result<i64, ()> {
    acct.balance += delta;
    journal.entries += 1;
    Ok(acct.balance)
}

fn main() {}
