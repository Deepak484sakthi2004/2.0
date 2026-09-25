// verify: debug ok
// The lint that catches the payout-lease bug: `let_underscore_drop` is allow-by-default; turned on,
// it warns on `let _ = <value with a destructor>;` and suggests the two honest spellings.
#![warn(let_underscore_drop)]
use std::cell::Cell;

pub struct Lease<'a> {
    held: &'a Cell<bool>,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.held.set(false);
    }
}

pub fn acquire(held: &Cell<bool>) -> Lease<'_> {
    held.set(true);
    Lease { held }
}

fn main() {
    let held = Cell::new(false);
    let _ = acquire(&held);
    println!("lease held after `let _ =`: {}", held.get());
}
