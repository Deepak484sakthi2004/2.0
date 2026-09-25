// verify: debug ok
// Two NON-capturing closures in if/else coerce to their common type: a fn pointer. It compiles,
// but every call through the result is now an indirect call through that pointer.
fn rounding_rule(ceil: bool) -> impl Fn(u64) -> u64 {
    if ceil {
        |c: u64| c.div_ceil(10) * 10
    } else {
        |c: u64| c / 10 * 10
    }
}

fn main() {
    let r = rounding_rule(true);
    println!("{} ({} B: a fn pointer)", r(12_345), std::mem::size_of_val(&r));
}
