// verify: debug ok
// `let _ = expr;` does not bind: the value is a temporary, dropped at the end of the statement.
// The debug MIR of pay_out_buggy (tools/emit.ps1 -Target mir -CrateType bin) shows the drop
// immediately after the call, before the payout work.
use std::cell::Cell;

/// A payout lease: while it's alive, this worker owns the merchant's payout run.
pub struct Lease<'a> {
    held: &'a Cell<bool>,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.held.set(false);
        println!("  lease released");
    }
}

pub fn acquire(held: &Cell<bool>) -> Lease<'_> {
    held.set(true);
    println!("  lease acquired");
    Lease { held }
}

#[inline(never)]
pub fn pay_out_buggy(held: &Cell<bool>) {
    let _ = acquire(held); // BUG: `_` is not a binding; the Lease is dropped at the end of this statement
    println!("  paying out (lease held: {})", held.get());
}

#[inline(never)]
pub fn pay_out_fixed(held: &Cell<bool>) {
    let _lease = acquire(held); // a binding: dropped at the end of the scope
    println!("  paying out (lease held: {})", held.get());
}

fn main() {
    let held = Cell::new(false);
    println!("buggy:");
    pay_out_buggy(&held);
    println!("fixed:");
    pay_out_fixed(&held);
}
