// verify: debug error:E0308
fn rounding_rule(ceil: bool, step: u64) -> impl Fn(u64) -> u64 {
    if ceil {
        move |c: u64| c.div_ceil(step) * step
    } else {
        move |c: u64| c / step * step // a different closure is a different type, even with the same signature
    }
}

fn main() {
    println!("{}", rounding_rule(true, 10)(12_345));
}
