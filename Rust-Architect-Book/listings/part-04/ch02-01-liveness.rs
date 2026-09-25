// verify: debug ok
use std::collections::HashMap;

fn main() {
    let mut limits: HashMap<&str, u32> = HashMap::from([("gold", 1000), ("free", 10)]);
    let verbose = std::env::args().count() > 5; // false on the Playground

    let gold = &limits["gold"]; // a shared loan on `limits`...
    if verbose {
        println!("gold limit: {gold}"); // ...used on ONE path only
    }
    // On the path where `verbose` is false, `gold` is dead here, so the loan is not live:
    limits.insert("trial", 50); // allowed. On the verbose path the loan also ended at its last use.

    let mut total = 0;
    for (tier, limit) in &limits {
        total += limit;
        if *tier == "free" {
            // mutation here would conflict with the iteration loan
        }
    }
    limits.insert("enterprise", 10_000); // the iteration loan ended with the loop
    println!("tiers={} total before enterprise={total}", limits.len());
}
