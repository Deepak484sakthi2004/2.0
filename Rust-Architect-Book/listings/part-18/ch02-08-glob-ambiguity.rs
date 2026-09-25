// verify: debug error:E0659
// Two glob imports provide `Error`. The resolver records where each name came from, and only
// reports the ambiguity when the name is actually used.
mod ledger {
    #[derive(Debug)]
    pub struct Error;
}
mod gateway {
    #[derive(Debug)]
    pub struct Error;
}

use gateway::*;
use ledger::*;

fn main() {
    let e = Error; // which one?
    println!("{e:?}");
}
