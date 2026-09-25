// verify: debug error:E0391
// Capstone ticket 1: "the config crate won't compile after a refactor." A type alias can't refer
// to itself: an alias is expanded (by a query) where it's used, so expansion never terminates.
use std::collections::HashMap;

type Config = HashMap<String, Config>;

fn main() {
    let c: Config = HashMap::new();
    println!("{}", c.len());
}
