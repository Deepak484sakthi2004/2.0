// verify: debug ok
// Collecting into Result/Option short-circuits at the first failure: the rest is never parsed.
use std::cell::Cell;

fn main() {
    let parsed = Cell::new(0);
    let parse = |s: &str| {
        parsed.set(parsed.get() + 1);
        s.parse::<u64>().map_err(|e| format!("bad amount {s:?}: {e}"))
    };

    let good = ["1200", "550", "99"];
    let bad = ["1200", "5x0", "99", "7", "8"];

    let ok: Result<Vec<u64>, String> = good.iter().map(|s| parse(s)).collect();
    println!("{ok:?}  (parsed {})", parsed.replace(0));

    let err: Result<Vec<u64>, String> = bad.iter().map(|s| parse(s)).collect();
    println!("{err:?}  (parsed {} of {})", parsed.replace(0), bad.len());

    // sum / product also accept Result and Option items:
    let total: Result<u64, String> = good.iter().map(|s| parse(s)).sum();
    println!("sum = {total:?}");

    // Option: all present, or None at the first gap.
    let limits = [Some(500_u32), None, Some(100)];
    let all: Option<Vec<u32>> = limits.iter().copied().collect();
    println!("all limits present? {all:?}");
}
