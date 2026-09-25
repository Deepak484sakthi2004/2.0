// verify: debug+nightly error:symbol
// Nightly-only: #[rustc_dump_symbol_name] prints the (v0-mangled) linker symbol rustc assigns.
#![feature(rustc_attrs)]
#![allow(internal_features, dead_code)]

#[rustc_dump_symbol_name]
pub fn fee_cents(amount: u64) -> u64 {
    amount / 100
}

pub struct Wrapper<T>(T);

impl<T> Wrapper<T> {
    #[rustc_dump_symbol_name]
    pub fn get(&self) -> &T {
        &self.0
    }
}

fn main() {}
