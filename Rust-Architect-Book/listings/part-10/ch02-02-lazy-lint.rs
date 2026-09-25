// verify: debug error:lazy
#![deny(unused_must_use)]
fn main() {
    let ids = vec![7_u64, 8, 9];
    // Intended: log each id. Actually: builds a Map adapter and drops it; nothing is printed.
    ids.iter().map(|id| println!("processing {id}"));
}
