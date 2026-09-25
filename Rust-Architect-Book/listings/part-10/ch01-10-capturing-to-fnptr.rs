// verify: debug error:E0308
fn apply(f: fn(u64) -> u64, x: u64) -> u64 {
    f(x)
}

fn main() {
    let bps = 290;
    println!("{}", apply(|x| x + 1, 10)); // non-capturing closure: coerces to a fn pointer
    println!("{}", apply(|x| x * bps / 10_000, 10_000)); // captures `bps`: no fn pointer exists for it
}
